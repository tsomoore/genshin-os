// MemoryService - Memory and Storage Management Service
//
// 曾国藩曰：
// "仓储管理，当井井有条。"
// 内存服务管理内存分配、分页和交换，确保系统高效运行。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use crossbeam_channel::{Receiver, Sender};
use crate::messaging::{
    KernelMsg, MemoryRequest, Pid, VirtAddr, PhysAddr, MemProt, AccessType,
    MessageBus, Response, ResponseData, ServiceError as MessagingServiceError,
};
use crate::messaging::bus::Envelope;
use crate::{GenshinError, GenshinResult, ServiceError};
use crate::hardware::{PhysicalMemory, MMU, VirtualDisk};

// Import memory service components
use super::alloc::{FrameAllocator, Frame, PhysicalMemoryManager, MemoryUsage};
use super::paging::{PageTable, PageTableManager, PageTableEntry, PageFlags, PageError};
use super::swap::{SwapManager, SwapConfig, SwapPolicy, SwapResult};

/// Memory Service - Main storage management service
///
/// 曾国藩曰：
/// "统管钱粮，当知其入出。"
/// 内存服务统筹内存管理、页表维护和交换策略。
pub struct MemoryService {
    /// Message bus
    bus: Arc<dyn MessageBus>,

    /// Receiver for message bus
    receiver: Receiver<Envelope>,

    /// Physical memory manager
    memory_manager: Arc<Mutex<PhysicalMemoryManager>>,

    /// Page table manager
    page_tables: Arc<Mutex<PageTableManager>>,

    /// Swap manager
    swap_manager: Arc<Mutex<SwapManager>>,

    /// Hardware memory reference
    hardware_memory: Arc<Mutex<PhysicalMemory>>,
    mmu: Arc<MMU>,
}

impl MemoryService {
    /// Helper function to lock mutex and convert poison errors
    fn lock_mutex<T>(mutex: &Mutex<T>) -> GenshinResult<std::sync::MutexGuard<T>> {
        mutex.lock().map_err(|e| {
            GenshinError::Service(ServiceError::InvalidArguments {
                param: "mutex".to_string(),
                reason: format!("Mutex poisoned: {}", e)
            })
        })
    }

    /// Create a new memory service
    pub fn new(bus: Arc<dyn MessageBus>, hw: PhysicalMemory, mmu: Arc<MMU>) -> Self {
        let receiver = bus.subscribe();
        let size = hw.size();
        let memory_manager = Arc::new(Mutex::new(PhysicalMemoryManager::new(size, 4096)));
        let page_tables = Arc::new(Mutex::new(PageTableManager::new(4096, 256)));
        let swap_disk = VirtualDisk::new(256); // 256 sectors for swap (128KB)
        let swap_manager = Arc::new(Mutex::new(SwapManager::new(SwapConfig::default(), swap_disk)));
        let hardware_memory = Arc::new(Mutex::new(hw));
        Self { bus, receiver, memory_manager, page_tables, swap_manager, hardware_memory, mmu }
    }

    /// Run the memory service (main loop)
    pub fn run(&self) {
        println!("MemoryService starting...");

        loop {
            match self.receiver.recv() {
                Ok(envelope) => {
                    if let Err(e) = self.handle_envelope(envelope) {
                        eprintln!("MemoryService error: {}", e);
                    }
                }
                Err(_) => {
                    eprintln!("Message bus disconnected");
                    break;
                }
            }
        }
    }

    /// Handle incoming envelope
    fn handle_envelope(&self, envelope: Envelope) -> GenshinResult<()> {
        // Handle the message based on envelope type
        let result = match &envelope.message {
            KernelMsg::Memory(req) => {
                if envelope.expects_response() {
                    self.handle_memory_request_with_response(req.clone(), &envelope)
                } else {
                    self.handle_memory_request(req.clone())
                }
            }
            KernelMsg::Interrupt(int) => self.handle_interrupt(int.clone()),
            _ => {
                // Ignore other messages
                Ok(())
            }
        };

        // Log errors but don't fail the service
        if let Err(e) = result {
            eprintln!("MemoryService error handling message: {}", e);

            // If this was a request, send error response
            if envelope.expects_response() {
                let _ = envelope.respond_error(MessagingServiceError::Other {
                    code: 1,
                    msg: e.to_string(),
                });
            }
        }

        Ok(())
    }

    /// Handle memory service request
    fn handle_memory_request(&self, req: MemoryRequest) -> GenshinResult<()> {
        match req {
            // ========== Frame Allocation ==========
            MemoryRequest::AllocFrame { count } => {
                self.handle_alloc_frame(count)?;
            }

            MemoryRequest::FreeFrame { paddr } => {
                self.handle_free_frame(paddr)?;
            }

            // ========== Page Table Management ==========
            MemoryRequest::MapPage { pid, virt, phys, prot } => {
                self.handle_map_page(pid, virt, phys, prot)?;
            }

            MemoryRequest::UnmapPage { pid, virt } => {
                self.handle_unmap_page(pid, virt)?;
            }

            // ========== Page Fault Handling ==========
            MemoryRequest::PageFaultHandler { pid, faulting_addr, access_type } => {
                self.handle_page_fault(pid, faulting_addr, access_type)?;
            }

            // ========== Swap Management ==========
            MemoryRequest::SwapOut { pid, virt } => {
                self.handle_swap_out(pid, virt)?;
            }

            MemoryRequest::SwapIn { pid, virt } => {
                self.handle_swap_in(pid, virt)?;
            }
        }

        Ok(())
    }

    /// Handle memory service request with response
    fn handle_memory_request_with_response(&self, req: MemoryRequest, envelope: &Envelope) -> GenshinResult<()> {
        match req {
            // ========== Frame Allocation ==========
            MemoryRequest::AllocFrame { count } => {
                self.handle_alloc_frame_with_response(count, envelope)?;
            }

            MemoryRequest::FreeFrame { paddr } => {
                self.handle_free_frame_with_response(paddr, envelope)?;
            }

            // ========== Page Table Management ==========
            MemoryRequest::MapPage { pid, virt, phys, prot } => {
                self.handle_map_page_with_response(pid, virt, phys, prot, envelope)?;
            }

            MemoryRequest::UnmapPage { pid, virt } => {
                self.handle_unmap_page_with_response(pid, virt, envelope)?;
            }

            // ========== Page Fault Handling ==========
            MemoryRequest::PageFaultHandler { pid, faulting_addr, access_type } => {
                self.handle_page_fault_with_response(pid, faulting_addr, access_type, envelope)?;
            }

            // ========== Swap Management ==========
            MemoryRequest::SwapOut { pid, virt } => {
                self.handle_swap_out_with_response(pid, virt, envelope)?;
            }

            MemoryRequest::SwapIn { pid, virt } => {
                self.handle_swap_in_with_response(pid, virt, envelope)?;
            }
        }

        Ok(())
    }

    /// Handle hardware interrupt
    fn handle_interrupt(&self, interrupt: crate::messaging::Interrupt) -> GenshinResult<()> {
        match interrupt {
            crate::messaging::Interrupt::HardwareFailure { component } => {
                eprintln!("MemoryService: Hardware failure in {}", component);
            }
            _ => {
                println!("MemoryService: Received interrupt {:?}", interrupt);
            }
        }
        Ok(())
    }

    // ========== Frame Allocation Handlers ==========

    fn handle_alloc_frame(&self, count: usize) -> GenshinResult<()> {
        let mut memory = Self::lock_mutex(&self.memory_manager)?;

        // For now, we'll allocate to PID 0 (kernel)
        let frames = memory.allocate_frames(0, count);

        if frames.len() != count {
            return Err(GenshinError::Service(ServiceError::ResourceExhausted {
                resource: "Physical memory frames".to_string(),
                available: frames.len(),
                requested: count,
            }));
        }

        println!("MemoryService: Allocated {} frames", count);

        // In real implementation, would return frame addresses via response channel
        Ok(())
    }

    fn handle_alloc_frame_with_response(&self, count: usize, envelope: &Envelope) -> GenshinResult<()> {
        let mut memory = Self::lock_mutex(&self.memory_manager)?;

        // For now, we'll allocate to PID 0 (kernel)
        let frames = memory.allocate_frames(0, count);

        if frames.len() != count {
            let _ = envelope.respond_error(MessagingServiceError::ResourceExhausted {
                resource: "Physical memory frames".to_string(),
            });
            return Err(GenshinError::Service(ServiceError::ResourceExhausted {
                resource: "Physical memory frames".to_string(),
                available: frames.len(),
                requested: count,
            }));
        }

        println!("MemoryService: Allocated {} frames", count);

        // Return the first frame address as response
        let first_frame_addr = frames.first().map(|f| f.address).unwrap_or(0);
        let _ = envelope.respond_success(ResponseData::PhysicalAddr(first_frame_addr));

        Ok(())
    }

    fn handle_free_frame(&self, paddr: PhysAddr) -> GenshinResult<()> {
        let frame_size = 4096; // TODO: Get from configuration
        let frame_num = paddr / frame_size as u64;

        let mut memory = Self::lock_mutex(&self.memory_manager)?;
        let freed = memory.free_frame(0, frame_num);

        if freed {
            println!("MemoryService: Freed frame at {:#x}", paddr);
            Ok(())
        } else {
            Err(GenshinError::Service(ServiceError::NotFound {
                resource_type: "Frame".to_string(),
                id: format!("{:#x}", paddr),
            }))
        }
    }

    fn handle_free_frame_with_response(&self, paddr: PhysAddr, envelope: &Envelope) -> GenshinResult<()> {
        let frame_size = 4096; // TODO: Get from configuration
        let frame_num = paddr / frame_size as u64;

        let mut memory = Self::lock_mutex(&self.memory_manager)?;
        let freed = memory.free_frame(0, frame_num);

        if freed {
            println!("MemoryService: Freed frame at {:#x}", paddr);
            envelope.respond_success(ResponseData::Void)?;
            Ok(())
        } else {
            let _ = envelope.respond_error(MessagingServiceError::NotFound {
                resource: "Frame".to_string(),
                id: format!("{:#x}", paddr),
            });
            Err(GenshinError::Service(ServiceError::NotFound {
                resource_type: "Frame".to_string(),
                id: format!("{:#x}", paddr),
            }))
        }
    }

    // ========== Page Table Management Handlers ==========

    fn handle_map_page(&self, pid: Pid, virt: VirtAddr, phys: PhysAddr, prot: MemProt) -> GenshinResult<()> {
        let page_tables = Self::lock_mutex(&self.page_tables)?;

        // Get or create page table for process
        let table = if let Some(table) = page_tables.get_table(pid) {
            table
        } else {
            drop(page_tables);
            let mut page_tables = Self::lock_mutex(&self.page_tables)?;
            let table = page_tables.create_table(pid);
            drop(page_tables);
            table
        };

        let mut table = Self::lock_mutex(&table)?;
        let result = table.map(virt, phys, prot);

        match result {
            Ok(_) => {
                println!("MemoryService: Mapped {:#x} -> {:#x} for pid {}", virt, phys, pid);
                Ok(())
            }
            Err(PageError::AlreadyMapped { vpn }) => {
                Err(GenshinError::Service(ServiceError::InvalidArguments {
                    param: "virtual_address".to_string(),
                    reason: format!("VPN {} already mapped", vpn),
                }))
            }
            _ => {
                Err(GenshinError::Service(ServiceError::Other {
                    code: 1,
                    msg: "Page mapping failed".to_string(),
                }))
            }
        }
    }

    fn handle_unmap_page(&self, pid: Pid, virt: VirtAddr) -> GenshinResult<()> {
        self.mmu.unmap_page(pid, virt).map_err(|e| {
            GenshinError::Service(ServiceError::Other { code: 2, msg: format!("{:?}", e) })
        })
    }

    fn handle_map_page_with_response(&self, pid: Pid, virt: VirtAddr, phys: PhysAddr, prot: MemProt, envelope: &Envelope) -> GenshinResult<()> {
        use crate::hardware::PageFlags;
        let flags = PageFlags {
            present: true,
            writable: prot.writable,
            user_accessible: true,
        };
        match self.mmu.map_page(pid, virt, phys, flags) {
            Ok(_) => {
                println!("MemoryService: Mapped {:#x} -> {:#x} for pid {}", virt, phys, pid);
                let _ = envelope.respond_success(ResponseData::Void);
                Ok(())
            }
            Err(e) => {
                let _ = envelope.respond_error(MessagingServiceError::Other { code: 1, msg: format!("{:?}", e) });
                Err(GenshinError::Service(ServiceError::Other { code: 1, msg: format!("{:?}", e) }))
            }
        }
    }

    fn handle_unmap_page_with_response(&self, pid: Pid, virt: VirtAddr, envelope: &Envelope) -> GenshinResult<()> {
        match self.mmu.unmap_page(pid, virt) {
            Ok(_) => {
                println!("MemoryService: Unmapped {:#x} for pid {}", virt, pid);
                let _ = envelope.respond_success(ResponseData::Void);
                Ok(())
            }
            Err(e) => {
                let _ = envelope.respond_error(MessagingServiceError::Other { code: 2, msg: format!("{:?}", e) });
                Err(GenshinError::Service(ServiceError::Other { code: 2, msg: format!("{:?}", e) }))
            }
        }
    }

    // ========== Page Fault Handling ==========

    fn handle_page_fault(&self, pid: Pid, faulting_addr: VirtAddr, access_type: AccessType) -> GenshinResult<()> {
        println!("MemoryService: Page fault for pid {} at {:#x} ({:?})", pid, faulting_addr, access_type);

        // Get page table
        let page_tables = Self::lock_mutex(&self.page_tables)?;
        let table = page_tables.get_table(pid)
            .ok_or_else(|| GenshinError::Service(ServiceError::NotFound {
                resource_type: "Page table".to_string(),
                id: pid.to_string(),
            }))?;

        let mut table = Self::lock_mutex(&table)?;

        // Check if page is mapped
        if let Some(entry) = table.lookup(faulting_addr) {
            if entry.is_present() {
                // Page is present but access denied
                return Err(GenshinError::Service(ServiceError::PermissionDenied {
                    operation: format!("{:?}", access_type),
                    reason: "Access permission denied".to_string(),
                }));
            }
        }

        drop(table);

        // Page not present - need to allocate frame and map it
        // This is a simplified implementation
        let mut memory = Self::lock_mutex(&self.memory_manager)?;
        let frames = memory.allocate_frames(pid, 1);

        if frames.is_empty() {
            // No physical memory available - try to swap
            drop(memory);
            let mut swap = Self::lock_mutex(&self.swap_manager)?;

            if !swap.is_enabled() || !swap.has_space() {
                return Err(GenshinError::Service(ServiceError::ResourceExhausted {
                    resource: "Memory".to_string(),
                    available: 0,
                    requested: 1,
                }));
            }

            // TODO: Implement actual swap out logic
            println!("MemoryService: Would swap out page (not implemented)");
        } else {
            let frame = frames[0];
            drop(memory);

            // Map the page
            let prot = match access_type {
                AccessType::Read => MemProt::read_only(),
                AccessType::Write => MemProt::read_write(),
                AccessType::Execute => MemProt::execute(),
            };

            drop(page_tables);
            self.handle_map_page(pid, faulting_addr, frame.address, prot)?;
        }

        Ok(())
    }

    fn handle_page_fault_with_response(&self, pid: Pid, faulting_addr: VirtAddr, access_type: AccessType, envelope: &Envelope) -> GenshinResult<()> {
        println!("MemoryService: Page fault for pid {} at {:#x} ({:?})", pid, faulting_addr, access_type);

        // Get page table
        let page_tables = Self::lock_mutex(&self.page_tables)?;
        let table = page_tables.get_table(pid)
            .ok_or_else(|| {
                envelope.respond_error(MessagingServiceError::NotFound {
                    resource: "Page table".to_string(),
                    id: pid.to_string(),
                }).ok();
                GenshinError::Service(ServiceError::NotFound {
                    resource_type: "Page table".to_string(),
                    id: pid.to_string(),
                })
            })?;

        let mut table = Self::lock_mutex(&table)?;

        // Check if page is mapped
        if let Some(entry) = table.lookup(faulting_addr) {
            if entry.is_present() {
                // Page is present but access denied
                envelope.respond_error(MessagingServiceError::PermissionDenied {
                    operation: format!("{:?}", access_type),
                })?;
                return Err(GenshinError::Service(ServiceError::PermissionDenied {
                    operation: format!("{:?}", access_type),
                    reason: "Access permission denied".to_string(),
                }));
            }
        }

        drop(table);

        // Page not present - need to allocate frame and map it
        // This is a simplified implementation
        let mut memory = Self::lock_mutex(&self.memory_manager)?;
        let frames = memory.allocate_frames(pid, 1);

        if frames.is_empty() {
            // No physical memory available - try to swap
            drop(memory);
            let mut swap = Self::lock_mutex(&self.swap_manager)?;

            if !swap.is_enabled() || !swap.has_space() {
                let _ = envelope.respond_error(MessagingServiceError::ResourceExhausted {
                    resource: "Memory".to_string(),
                });
                return Err(GenshinError::Service(ServiceError::ResourceExhausted {
                    resource: "Memory".to_string(),
                    available: 0,
                    requested: 1,
                }));
            }

            // TODO: Implement actual swap out logic
            println!("MemoryService: Would swap out page (not implemented)");
        } else {
            let frame = frames[0];
            drop(memory);

            // Map the page
            let prot = match access_type {
                AccessType::Read => MemProt::read_only(),
                AccessType::Write => MemProt::read_write(),
                AccessType::Execute => MemProt::execute(),
            };

            drop(page_tables);
            self.handle_map_page(pid, faulting_addr, frame.address, prot)?;
        }

        envelope.respond_success(ResponseData::Void)?;
        Ok(())
    }

    // ========== Swap Management Handlers ==========

    fn handle_swap_out(&self, pid: Pid, virt: VirtAddr) -> GenshinResult<()> {
        let mut swap = Self::lock_mutex(&self.swap_manager)?;

        if !swap.is_enabled() {
            return Err(GenshinError::Service(ServiceError::NotImplemented {
                feature: "Swap".to_string(),
            }));
        }

        let vpn = virt / 4096; // TODO: Get from page size

        // Allocate swap slot
        let slot = swap.allocate_slot(pid, vpn)
            .ok_or_else(|| GenshinError::Service(ServiceError::ResourceExhausted {
                resource: "Swap space".to_string(),
                available: 0,
                requested: 1,
            }))?;

        println!("MemoryService: Swapped out pid {} page {:#x} to swap slot {}", pid, virt, slot.number);

        // TODO: Actually write to swap device
        Ok(())
    }

    fn handle_swap_out_with_response(&self, pid: Pid, virt: VirtAddr, envelope: &Envelope) -> GenshinResult<()> {
        let mut swap = Self::lock_mutex(&self.swap_manager)?;

        if !swap.is_enabled() {
            envelope.respond_error(MessagingServiceError::NotImplemented {
                feature: "Swap".to_string(),
            })?;
            return Err(GenshinError::Service(ServiceError::NotImplemented {
                feature: "Swap".to_string(),
            }));
        }

        let vpn = virt / 4096; // TODO: Get from page size

        // Allocate swap slot
        let slot = swap.allocate_slot(pid, vpn)
            .ok_or_else(|| {
                envelope.respond_error(MessagingServiceError::ResourceExhausted {
                    resource: "Swap space".to_string(),
                }).ok();
                GenshinError::Service(ServiceError::ResourceExhausted {
                    resource: "Swap space".to_string(),
                    available: 0,
                    requested: 1,
                })
            })?;

        // Read frame data from physical memory via MMU
        let mut frame_data = vec![0u8; 4096];
        if let Ok(paddr) = self.mmu.translate(pid, virt, crate::messaging::AccessType::Read) {
            let hw = self.hardware_memory.lock().map_err(|_| GenshinError::Service(ServiceError::Other { code: 80, msg: "lock".into() }))?;
            let _ = hw.read_slice(paddr as usize, &mut frame_data);
        }
        // Write to swap disk
        if let Err(e) = swap.swap_out(slot.number, &frame_data) {
            let _ = envelope.respond_error(MessagingServiceError::Other { code: 81, msg: e });
            return Err(GenshinError::Service(ServiceError::Other { code: 81, msg: "swap_out failed".into() }));
        }
        println!("MemoryService: Swapped out pid {} page {:#x} to slot {}", pid, virt, slot.number);
        let _ = envelope.respond_success(ResponseData::Integer(slot.number));
        Ok(())
    }

    fn handle_swap_in(&self, pid: Pid, virt: VirtAddr) -> GenshinResult<()> {
        let mut swap = Self::lock_mutex(&self.swap_manager)?;

        // Get process swap slots
        let slots = swap.get_process_slots(pid);

        // Find the slot for this virtual page
        let vpn = virt / 4096; // TODO: Get from page size

        let slot = slots.iter()
            .find(|s| s.vpn == Some(vpn))
            .ok_or_else(|| GenshinError::Service(ServiceError::NotFound {
                resource_type: "Swap slot".to_string(),
                id: format!("VPN {}", vpn),
            }))?;

        println!("MemoryService: Swapped in pid {} page {:#x} from swap slot {}", pid, virt, slot.number);

        // Free the swap slot
        swap.free_slot(slot.number);

        // TODO: Actually read from swap device
        Ok(())
    }

    fn handle_swap_in_with_response(&self, pid: Pid, virt: VirtAddr, envelope: &Envelope) -> GenshinResult<()> {
        let mut swap = Self::lock_mutex(&self.swap_manager)?;

        // Get process swap slots
        let slots = swap.get_process_slots(pid);

        // Find the slot for this virtual page
        let vpn = virt / 4096; // TODO: Get from page size

        let slot = slots.iter()
            .find(|s| s.vpn == Some(vpn))
            .ok_or_else(|| {
                envelope.respond_error(MessagingServiceError::NotFound {
                    resource: "Swap slot".to_string(),
                    id: format!("VPN {}", vpn),
                }).ok();
                GenshinError::Service(ServiceError::NotFound {
                    resource_type: "Swap slot".to_string(),
                    id: format!("VPN {}", vpn),
                })
            })?;

        println!("MemoryService: Swapped in pid {} page {:#x} from swap slot {}", pid, virt, slot.number);

        // Free the swap slot
        swap.free_slot(slot.number);

        // Read frame data from swap disk and write to physical memory
        match swap.swap_in(slot.number) {
            Ok(frame_data) => {
                if let Ok(paddr) = self.mmu.translate(pid, virt, crate::messaging::AccessType::Write) {
                    let hw = self.hardware_memory.lock().map_err(|_| GenshinError::Service(ServiceError::Other { code: 82, msg: "lock".into() }))?;
                    hw.write_slice(paddr as usize, &frame_data).map_err(|_| GenshinError::Service(ServiceError::Other { code: 83, msg: "write".into() }))?;
                }
                println!("MemoryService: Swapped in pid {} page {:#x} from slot {}", pid, virt, slot.number);
                let _ = envelope.respond_success(ResponseData::Void);
                Ok(())
            }
            Err(e) => {
                let _ = envelope.respond_error(MessagingServiceError::Other { code: 84, msg: e });
                Err(GenshinError::Service(ServiceError::Other { code: 84, msg: "swap_in failed".into() }))
            }
        }
    }

    // ========== Query Methods ==========

    /// Get memory usage statistics
    pub fn memory_usage(&self) -> MemoryUsage {
        let memory = self.memory_manager.lock().unwrap();
        memory.usage()
    }

    /// Get swap statistics
    pub fn swap_stats(&self) -> crate::services::memory::swap::SwapStats {
        let swap = self.swap_manager.lock().unwrap();
        swap.stats()
    }

    /// Get page table statistics for all processes
    pub fn page_table_stats(&self) -> Vec<(Pid, crate::services::memory::paging::PageTableStats)> {
        let page_tables = self.page_tables.lock().unwrap();
        page_tables.all_tables()
    }

    /// Get page table statistics for a specific process
    pub fn process_page_table_stats(&self, pid: Pid) -> Option<crate::services::memory::paging::PageTableStats> {
        let page_tables = self.page_tables.lock().unwrap();
        page_tables.get_table(pid).and_then(|table| {
            table.lock().ok().map(|tbl| tbl.stats())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::LockedBus;

    #[test]
    fn test_memory_service_creation() {
        let bus = Arc::new(LockedBus::new());
        let swap_config = SwapConfig::default();

        let service = MemoryService::new(
            bus,
            1024 * 1024, // 1MB memory
            4096,           // 4KB frames
            4096,           // 4KB pages
            256,            // 256 pages per process
            swap_config,
        );

        // Service should be created successfully
        assert!(service.memory_usage().total_frames > 0);
        assert!(service.swap_stats().total_slots > 0);
    }

    #[test]
    fn test_alloc_frame() {
        let bus = Arc::new(LockedBus::new());
        let service = MemoryService::new(
            bus,
            4096 * 100, // 100 frames
            4096,
            4096,
            256,
            SwapConfig::default(),
        );

        // Allocate frames via request
        let result = service.handle_alloc_frame(5);
        assert!(result.is_ok());

        // Memory usage should reflect allocation
        let usage = service.memory_usage();
        assert_eq!(usage.allocated_frames, 5);
    }

    #[test]
    fn test_map_page() {
        let bus = Arc::new(LockedBus::new());
        let service = MemoryService::new(
            bus,
            4096 * 100,
            4096,
            4096,
            256,
            SwapConfig::default(),
        );

        // Map a page
        let result = service.handle_map_page(100, 0x1000, 0x1000, MemProt::read_write());
        assert!(result.is_ok());

        // Get process page table stats
        let stats = service.process_page_table_stats(100);
        assert!(stats.is_some());
        assert_eq!(stats.unwrap().mapped_pages, 1);
    }

    #[test]
    fn test_unmap_page() {
        let bus = Arc::new(LockedBus::new());
        let service = MemoryService::new(
            bus,
            4096 * 100,
            4096,
            4096,
            256,
            SwapConfig::default(),
        );

        // First map a page
        service.handle_map_page(100, 0x1000, 0x1000, MemProt::read_write()).unwrap();

        // Then unmap it
        let result = service.handle_unmap_page(100, 0x1000);
        assert!(result.is_ok());

        // Verify page was unmapped
        let stats = service.process_page_table_stats(100);
        assert_eq!(stats.unwrap().mapped_pages, 0);
    }

    #[test]
    fn test_page_fault_handling() {
        let bus = Arc::new(LockedBus::new());
        let service = MemoryService::new(
            bus,
            4096 * 10,  // Small memory to trigger swapping
            4096,
            4096,
            256,
            SwapConfig::default(),
        );

        // Map a page first
        service.handle_map_page(100, 0x1000, 0x1000, MemProt::read_write()).unwrap();

        // Try to access with wrong permissions (should fail)
        let result = service.handle_page_fault(100, 0x1000, AccessType::Execute);
        assert!(result.is_err());

        // Access non-mapped page (should allocate and map)
        let result = service.handle_page_fault(100, 0x2000, AccessType::Read);
        assert!(result.is_ok());

        // Check that page was mapped
        let stats = service.process_page_table_stats(100);
        assert!(stats.unwrap().mapped_pages >= 2);
    }

    #[test]
    fn test_swap_operations() {
        let bus = Arc::new(LockedBus::new());
        let swap_config = SwapConfig {
            total_size: 4096 * 10,
            slot_size: 4096,
            max_slots: 10,
            device_path: "/test/swap".to_string(),
        };

        let service = MemoryService::new(
            bus,
            4096 * 100,
            4096,
            4096,
            256,
            swap_config,
        );

        // Test swap out
        let result = service.handle_swap_out(100, 0x1000);
        assert!(result.is_ok());

        // Check swap stats
        let stats = service.swap_stats();
        assert_eq!(stats.used_slots, 1);

        // Test swap in
        let result = service.handle_swap_in(100, 0x1000);
        assert!(result.is_ok());

        // Check swap stats
        let stats = service.swap_stats();
        assert_eq!(stats.used_slots, 0);
    }

    #[test]
    fn test_memory_usage_stats() {
        let bus = Arc::new(LockedBus::new());
        let service = MemoryService::new(
            bus,
            4096 * 1000,
            4096,
            4096,
            256,
            SwapConfig::default(),
        );

        let usage = service.memory_usage();
        assert_eq!(usage.total_frames, 1000); // (4096 * 1000) / 4096 = 1000
        assert_eq!(usage.free_frames, 1000);
        assert_eq!(usage.usage_percent(), 0.0);
    }

    #[test]
    fn test_all_process_stats() {
        let bus = Arc::new(LockedBus::new());
        let service = MemoryService::new(
            bus,
            4096 * 1000,
            4096,
            4096,
            256,
            SwapConfig::default(),
        );

        // Create page tables for multiple processes
        let _ = service.handle_map_page(100, 0x1000, 0x1000, MemProt::read_write());
        let _ = service.handle_map_page(100, 0x2000, 0x2000, MemProt::read_write());
        let _ = service.handle_map_page(200, 0x1000, 0x3000, MemProt::read_write());

        let all_stats = service.page_table_stats();
        assert!(all_stats.len() >= 2);

        // Check process 100 has 2 pages
        let pid_100_stats: Vec<_> = all_stats.iter().filter(|(pid, _)| *pid == 100).collect();
        assert_eq!(pid_100_stats.len(), 1);
        assert_eq!(pid_100_stats[0].1.mapped_pages, 2);
    }

    #[test]
    fn test_free_frame() {
        let bus = Arc::new(LockedBus::new());
        let service = MemoryService::new(
            bus,
            4096 * 100,
            4096,
            4096,
            256,
            SwapConfig::default(),
        );

        // Allocate then free a frame
        service.handle_alloc_frame(1).unwrap();

        // Get the frame that was allocated (it would be frame 0 at address 0x0)
        let result = service.handle_free_frame(0x0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_swap_disabled() {
        let bus = Arc::new(LockedBus::new());
        let service = MemoryService::new(
            bus,
            4096 * 100,
            4096,
            4096,
            256,
            SwapConfig::default(),
        );

        // Disable swap
        {
            let mut swap = service.swap_manager.lock().unwrap();
            swap.disable();
        }

        // Swap should fail when disabled
        let result = service.handle_swap_out(100, 0x1000);
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_process_page_tables() {
        let bus = Arc::new(LockedBus::new());
        let service = MemoryService::new(
            bus,
            4096 * 1000,
            4096,
            4096,
            256,
            SwapConfig::default(),
        );

        // Map pages for different processes
        service.handle_map_page(100, 0x1000, 0x1000, MemProt::read_write()).unwrap();
        service.handle_map_page(200, 0x1000, 0x2000, MemProt::read_write()).unwrap();
        service.handle_map_page(300, 0x1000, 0x3000, MemProt::read_write()).unwrap();

        // Each process should have its own page table
        let stats = service.page_table_stats();
        assert!(stats.len() >= 3);
    }
}
