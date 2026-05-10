// Memory Management Unit (MMU) Simulation
//
// 曾国藩曰：
// "凡读古书，遇有名理，必反复沉思，以求确解。"
// MMU 乃虚实转换之枢纽，一字一码皆当反复推敲，不可有误。

use std::sync::{Arc, Mutex};
use std::collections::HashMap;

use crate::hardware::memory::PhysicalMemory;
use crate::error::{MMUError, AccessType as ErrorAccessType, PageFlags as ErrorPageFlags};
use crate::messaging::{VirtAddr, PhysAddr, Pid, MemProt, AccessType};

/// Page table entry flags
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageFlags {
    pub present: bool,
    pub writable: bool,
    pub user_accessible: bool,
}

impl PageFlags {
    pub const fn new() -> Self {
        Self {
            present: false,
            writable: false,
            user_accessible: false,
        }
    }

    pub const fn present_writable() -> Self {
        Self {
            present: true,
            writable: true,
            user_accessible: true,
        }
    }

    pub const fn present_readonly() -> Self {
        Self {
            present: true,
            writable: false,
            user_accessible: true,
        }
    }
}

impl Default for PageFlags {
    fn default() -> Self {
        Self::new()
    }
}

impl From<MemProt> for PageFlags {
    fn from(prot: MemProt) -> Self {
        Self {
            present: true,
            writable: prot.writable,
            user_accessible: true,
        }
    }
}

/// Page table entry
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageTableEntry {
    /// Physical frame number (page frame address)
    pub frame: PhysAddr,
    /// Page flags
    pub flags: PageFlags,
}

impl PageTableEntry {
    pub const fn new(frame: PhysAddr, flags: PageFlags) -> Self {
        Self { frame, flags }
    }

    /// Create an invalid (non-present) entry
    pub const fn invalid() -> Self {
        Self {
            frame: 0,
            flags: PageFlags::new(),
        }
    }

    pub fn is_present(&self) -> bool {
        self.flags.present
    }

    pub fn is_writable(&self) -> bool {
        self.flags.writable
    }
}

/// Memory Management Unit
///
/// Responsible for virtual-to-physical address translation.
/// When translation fails or permission is denied, the MMU reports
/// the error but does NOT handle it - that's the kernel's job.
///
/// 曾国藩曰：
/// "治事之法，当各司其职，不可越俎代庖。"
/// MMU 只负责地址转换，缺页处理交给内核，此乃各司其职。
pub struct MMU {
    /// Page tables: one per process
    /// Maps (pid, virt_page) -> (phys_frame, flags)
    page_tables: Arc<Mutex<HashMap<Pid, HashMap<VirtAddr, PageTableEntry>>>>,

    /// Reference to physical memory for actual data access
    memory: PhysicalMemory,

    /// Page size (must be power of 2)
    page_size: usize,
}

impl MMU {
    /// Create a new MMU
    pub fn new(memory: PhysicalMemory, page_size: usize) -> Self {
        // Page size must be power of 2
        assert!(page_size.is_power_of_two(), "Page size must be power of 2");

        Self {
            page_tables: Arc::new(Mutex::new(HashMap::new())),
            memory,
            page_size,
        }
    }

    /// Get page size
    pub fn page_size(&self) -> usize {
        self.page_size
    }

    /// Create a new page table for a process
    pub fn create_page_table(&self, pid: Pid) {
        let mut tables = self.page_tables.lock().unwrap();
        tables.entry(pid).or_insert_with(HashMap::new);
    }

    /// Remove a page table (process cleanup)
    pub fn remove_page_table(&self, pid: Pid) {
        let mut tables = self.page_tables.lock().unwrap();
        tables.remove(&pid);
    }

    /// Get all page table entries for a process (used for fork)
    pub fn get_page_entries(&self, pid: Pid) -> Vec<(VirtAddr, PhysAddr, PageFlags)> {
        let tables = self.page_tables.lock().unwrap();
        if let Some(table) = tables.get(&pid) {
            table.iter().map(|(&vaddr, entry)| (vaddr, entry.frame, entry.flags)).collect()
        } else {
            Vec::new()
        }
    }

    /// Map a virtual page to a physical frame
    ///
    /// 曾国藩曰：
    /// "绘图之法，先定其位；映射之法，先定其址。"
    /// 页表映射乃是虚地址到实地址的桥梁，必须精准无误。
    pub fn map_page(
        &self,
        pid: Pid,
        vaddr: VirtAddr,
        paddr: PhysAddr,
        flags: PageFlags,
    ) -> Result<(), MMUError> {
        let page_vaddr = self.align_down(vaddr);
        let page_paddr = self.align_down(paddr);

        let mut tables = self.page_tables.lock().unwrap();
        let table = tables.entry(pid).or_insert_with(HashMap::new);

        table.insert(page_vaddr, PageTableEntry::new(page_paddr, flags));
        Ok(())
    }

    /// Unmap a virtual page
    pub fn unmap_page(&self, pid: Pid, vaddr: VirtAddr) -> Result<(), MMUError> {
        let page_vaddr = self.align_down(vaddr);

        let mut tables = self.page_tables.lock().unwrap();
        let table = tables.get_mut(&pid)
            .ok_or(MMUError::PageTableNotFound { pid })?;

        table.remove(&page_vaddr);
        Ok(())
    }

    /// Translate virtual address to physical address
    ///
    /// This is the core MMU function. It performs the translation
    /// and checks permissions. If translation fails, an error is
    /// returned - the caller (kernel) must handle the page fault.
    ///
    /// 曾国藩曰：
    /// "译书之法，信达雅为上；译址之法，准快稳为先。"
    /// 地址转换必须准确无误，错误将导致系统崩溃。
    pub fn translate(
        &self,
        pid: Pid,
        vaddr: VirtAddr,
        access: AccessType,
    ) -> Result<PhysAddr, MMUError> {
        let page_vaddr = self.align_down(vaddr);
        let offset = vaddr - page_vaddr;

        // Look up page table
        let tables = self.page_tables.lock().unwrap();
        let table = tables.get(&pid)
            .ok_or(MMUError::PageTableNotFound { pid })?;

        // Look up page entry
        let entry = table.get(&page_vaddr)
            .ok_or(MMUError::PageNotPresent { pid, vaddr })?;

        // Check if page is present
        if !entry.is_present() {
            return Err(MMUError::PageNotPresent { pid, vaddr });
        }

        // Check permissions
        match access {
            AccessType::Read => {
                // Read is always allowed if present
            }
            AccessType::Write => {
                if !entry.is_writable() {
                    let required = ErrorPageFlags {
                        present: true,
                        writable: true,
                        user_accessible: entry.flags.user_accessible,
                    };
                    return Err(MMUError::PermissionDenied {
                        pid,
                        vaddr,
                        access_type: ErrorAccessType::Write,
                        required,
                    });
                }
            }
            AccessType::Execute => {
                // For simplicity, execute requires write permission (executable = writable in this simple model)
                if !entry.is_writable() {
                    let required = ErrorPageFlags {
                        present: true,
                        writable: true,
                        user_accessible: entry.flags.user_accessible,
                    };
                    return Err(MMUError::PermissionDenied {
                        pid,
                        vaddr,
                        access_type: ErrorAccessType::Execute,
                        required,
                    });
                }
            }
        }

        // Calculate physical address
        let paddr = entry.frame + offset;

        Ok(paddr)
    }

    /// Read from virtual address
    ///
    /// 曾国藩曰：
    /// "取物于库，必先登记；取数于内存，必先译址。"
    /// 每次虚存读取，必须经过地址转换。
    pub fn read_u8(&self, pid: Pid, vaddr: VirtAddr) -> Result<u8, MMUError> {
        let paddr = self.translate(pid, vaddr, AccessType::Read)?;
        self.memory.read_u8(paddr as usize)
            .map_err(|_| MMUError::InvalidPhysicalAddress { paddr })
    }

    pub fn read_u32(&self, pid: Pid, vaddr: VirtAddr) -> Result<u32, MMUError> {
        let paddr = self.translate(pid, vaddr, AccessType::Read)?;
        self.memory.read_u32(paddr as usize)
            .map_err(|_| MMUError::InvalidPhysicalAddress { paddr })
    }

    /// Write to virtual address
    pub fn write_u8(&self, pid: Pid, vaddr: VirtAddr, value: u8) -> Result<(), MMUError> {
        let paddr = self.translate(pid, vaddr, AccessType::Write)?;
        self.memory.write_u8(paddr as usize, value)
            .map_err(|_| MMUError::InvalidPhysicalAddress { paddr })
    }

    pub fn write_u32(&self, pid: Pid, vaddr: VirtAddr, value: u32) -> Result<(), MMUError> {
        let paddr = self.translate(pid, vaddr, AccessType::Write)?;
        self.memory.write_u32(paddr as usize, value)
            .map_err(|_| MMUError::InvalidPhysicalAddress { paddr })
    }

    /// Dump MMU state for debugging
    ///
    /// 曾国藩曰：
    /// "每日检点账目，方能知其盈虚。"
    /// MMU 状态亦当定期检查，方能知虚实映射之全貌。
    pub fn dump_state(&self, pid: Pid) -> MMUState {
        let tables = self.page_tables.lock().unwrap();

        if let Some(table) = tables.get(&pid) {
            let mappings: Vec<_> = table.iter()
                .map(|(&vaddr, entry)| (vaddr, entry.frame, entry.flags))
                .collect();

            MMUState {
                page_count: mappings.len(),
                mappings,
            }
        } else {
            MMUState {
                page_count: 0,
                mappings: Vec::new(),
            }
        }
    }

    /// Align address down to page boundary
    fn align_down(&self, addr: u64) -> u64 {
        addr & !(self.page_size as u64 - 1)
    }
}

/// MMU state snapshot for TUI display
#[derive(Debug, Clone)]
pub struct MMUState {
    pub page_count: usize,
    pub mappings: Vec<(VirtAddr, PhysAddr, PageFlags)>,
}

impl MMUState {
    pub fn format_mappings(&self) -> String {
        let mut output = String::new();
        output.push_str(&format!("Total pages: {}\n", self.page_count));
        output.push_str("Virtual Addr    -> Physical Addr    | Flags\n");
        output.push_str("---------------|-------------------|----------\n");

        for (vaddr, paddr, flags) in &self.mappings {
            output.push_str(&format!(
                "{:#016x} -> {:#016x} | P{} W{} U{}\n",
                vaddr,
                paddr,
                if flags.present { 1 } else { 0 },
                if flags.writable { 1 } else { 0 },
                if flags.user_accessible { 1 } else { 0 },
            ));
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::AccessType;

    #[test]
    fn test_mmu_creation() {
        let mem = PhysicalMemory::new(1024 * 1024);
        let mmu = MMU::new(mem.clone(), 4096);
        assert_eq!(mmu.page_size(), 4096);
    }

    #[test]
    fn test_page_table_creation() {
        let mem = PhysicalMemory::new(1024 * 1024);
        let mmu = MMU::new(mem, 4096);
        mmu.create_page_table(1);
        mmu.create_page_table(2);
        // Should not panic
    }

    #[test]
    fn test_map_unmap_page() {
        let mem = PhysicalMemory::new(1024 * 1024);
        let mmu = MMU::new(mem, 4096);

        mmu.create_page_table(1);
        mmu.map_page(1, 0x1000, 0x5000, PageFlags::present_readonly()).unwrap();

        // Verify mapping exists
        let state = mmu.dump_state(1);
        assert_eq!(state.page_count, 1);

        // Unmap
        mmu.unmap_page(1, 0x1000).unwrap();

        // Verify removed
        let state = mmu.dump_state(1);
        assert_eq!(state.page_count, 0);
    }

    #[test]
    fn test_translate_present_page() {
        let mem = PhysicalMemory::new(1024 * 1024);
        let mmu = MMU::new(mem.clone(), 4096);

        mmu.create_page_table(1);
        mmu.map_page(1, 0x1000, 0x5000, PageFlags::present_readonly()).unwrap();

        // Translate
        let paddr = mmu.translate(1, 0x1000, AccessType::Read).unwrap();
        assert_eq!(paddr, 0x5000);

        // With offset
        let paddr = mmu.translate(1, 0x10FF, AccessType::Read).unwrap();
        assert_eq!(paddr, 0x50FF);
    }

    #[test]
    fn test_translate_not_present() {
        let mem = PhysicalMemory::new(1024 * 1024);
        let mmu = MMU::new(mem, 4096);

        mmu.create_page_table(1);

        // Try to translate non-existent page
        let result = mmu.translate(1, 0x1000, AccessType::Read);
        assert!(matches!(result, Err(MMUError::PageNotPresent { .. })));
    }

    #[test]
    fn test_permission_denied() {
        let mem = PhysicalMemory::new(1024 * 1024);
        let mmu = MMU::new(mem, 4096);

        mmu.create_page_table(1);
        mmu.map_page(1, 0x1000, 0x5000, PageFlags::present_readonly()).unwrap();

        // Try to write to read-only page
        let result = mmu.translate(1, 0x1000, AccessType::Write);
        assert!(matches!(result, Err(MMUError::PermissionDenied { .. })));
    }

    #[test]
    fn test_read_write_virtual() {
        let mem = PhysicalMemory::new(1024 * 1024);
        let mmu = MMU::new(mem.clone(), 4096);

        mmu.create_page_table(1);
        mmu.map_page(1, 0x1000, 0x5000, PageFlags::present_writable()).unwrap();

        // Write via virtual address
        mmu.write_u32(1, 0x1000, 0xDEADBEEF).unwrap();

        // Read back via virtual address
        let value = mmu.read_u32(1, 0x1000).unwrap();
        assert_eq!(value, 0xDEADBEEF);

        // Verify physical memory
        let phys_value = mem.read_u32(0x5000).unwrap();
        assert_eq!(phys_value, 0xDEADBEEF);
    }
}
