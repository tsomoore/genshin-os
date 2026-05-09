// Paging Module
//
// 曾国藩曰：
// "书分卷册，事当分门别类。"
// 分页将虚拟地址空间划分为页，便于管理。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use crate::messaging::{Pid, VirtAddr, PhysAddr, MemProt, AccessType};

/// Page flags
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageFlags {
    /// Present in physical memory
    pub present: bool,

    /// Writable
    pub writable: bool,

    /// User accessible (vs supervisor only)
    pub user: bool,

    /// Accessed (for replacement algorithms)
    pub accessed: bool,

    /// Dirty (modified since last sync)
    pub dirty: bool,

    /// Execute permission
    pub executable: bool,

    /// Global (shared across address spaces)
    pub global: bool,
}

impl PageFlags {
    /// Create default page flags (not present)
    pub fn new() -> Self {
        Self {
            present: false,
            writable: false,
            user: false,
            accessed: false,
            dirty: false,
            executable: false,
            global: false,
        }
    }

    /// Create page flags from MemProt
    pub fn from_prot(prot: MemProt) -> Self {
        Self {
            present: true,
            writable: prot.writable,
            user: true,
            accessed: false,
            dirty: false,
            executable: prot.executable,
            global: false,
        }
    }

    /// Convert to MemProt
    pub fn to_prot(&self) -> Option<MemProt> {
        if !self.present {
            return None;
        }

        Some(MemProt {
            readable: true,
            writable: self.writable,
            executable: self.executable,
        })
    }

    /// Check if access is allowed
    pub fn check_access(&self, access: AccessType) -> bool {
        if !self.present {
            return false;
        }

        match access {
            AccessType::Read => true, // Always readable if present
            AccessType::Write => self.writable,
            AccessType::Execute => self.executable,
        }
    }

    /// Mark page as accessed
    pub fn mark_accessed(&mut self) {
        self.accessed = true;
    }

    /// Mark page as dirty
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Clear dirty bit
    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }
}

impl Default for PageFlags {
    fn default() -> Self {
        Self::new()
    }
}

/// Page table entry (maps virtual page to physical frame)
///
/// 曾国藩曰：
/// "目录索引，方知所在。"
/// 页表项记录虚拟页到物理帧的映射。
#[derive(Debug, Clone)]
pub struct PageTableEntry {
    /// Virtual page number
    pub vpn: u64,

    /// Physical frame number
    pub pfn: u64,

    /// Page flags
    pub flags: PageFlags,

    /// Virtual address of this page
    pub virt_addr: VirtAddr,

    /// Physical address of this page
    pub phys_addr: PhysAddr,

    /// Page size
    pub size: usize,
}

impl PageTableEntry {
    /// Create a new page table entry
    pub fn new(vpn: u64, pfn: u64, virt_addr: VirtAddr, phys_addr: PhysAddr, size: usize, flags: PageFlags) -> Self {
        Self {
            vpn,
            pfn,
            flags,
            virt_addr,
            phys_addr,
            size,
        }
    }

    /// Create an unmapped entry
    pub fn unmapped(vpn: u64, virt_addr: VirtAddr, size: usize) -> Self {
        Self {
            vpn,
            pfn: 0,
            flags: PageFlags::new(),
            virt_addr,
            phys_addr: 0,
            size,
        }
    }

    /// Check if page is present in memory
    pub fn is_present(&self) -> bool {
        self.flags.present
    }

    /// Check if access is allowed
    pub fn check_access(&self, access: AccessType) -> bool {
        self.flags.check_access(access)
    }

    /// Get physical address for an offset within this page
    pub fn phys_address_for_offset(&self, offset: usize) -> Option<PhysAddr> {
        if !self.is_present() || offset >= self.size {
            None
        } else {
            Some(self.phys_addr + offset as u64)
        }
    }

    /// Mark as accessed
    pub fn mark_accessed(&mut self) {
        self.flags.mark_accessed();
    }

    /// Mark as dirty
    pub fn mark_dirty(&mut self) {
        self.flags.mark_dirty();
    }

    /// Set flags
    pub fn set_flags(&mut self, flags: PageFlags) {
        self.flags = flags;
    }
}

/// Page table - maps virtual pages to physical frames
///
/// 曾国藩曰：
/// "目录书录，纲举目张。"
/// 页表维护虚拟地址到物理地址的映射关系。
#[derive(Debug)]
pub struct PageTable {
    /// Process ID
    pub pid: Pid,

    /// Page size in bytes
    page_size: usize,

    /// Page table entries (vpn -> entry)
    entries: HashMap<u64, PageTableEntry>,

    /// Number of mapped pages
    mapped_count: usize,

    /// Total pages
    total_pages: usize,
}

impl PageTable {
    /// Create a new page table
    pub fn new(pid: Pid, page_size: usize, total_pages: usize) -> Self {
        Self {
            pid,
            page_size,
            entries: HashMap::new(),
            mapped_count: 0,
            total_pages,
        }
    }

    /// Map a virtual page to a physical frame
    pub fn map(&mut self, virt_addr: VirtAddr, phys_addr: PhysAddr, prot: MemProt) -> Result<(), PageError> {
        let vpn = virt_addr / self.page_size as u64;
        let pfn = phys_addr / self.page_size as u64;

        let entry = PageTableEntry::new(
            vpn,
            pfn,
            virt_addr,
            phys_addr,
            self.page_size,
            PageFlags::from_prot(prot),
        );

        if self.entries.contains_key(&vpn) {
            return Err(PageError::AlreadyMapped { vpn });
        }

        self.entries.insert(vpn, entry);
        self.mapped_count += 1;

        Ok(())
    }

    /// Unmap a virtual page
    pub fn unmap(&mut self, virt_addr: VirtAddr) -> Result<PageTableEntry, PageError> {
        let vpn = virt_addr / self.page_size as u64;

        if let Some(entry) = self.entries.remove(&vpn) {
            self.mapped_count -= 1;
            Ok(entry)
        } else {
            Err(PageError::NotMapped { vpn })
        }
    }

    /// Look up a page table entry
    pub fn lookup(&self, virt_addr: VirtAddr) -> Option<&PageTableEntry> {
        let vpn = virt_addr / self.page_size as u64;
        self.entries.get(&vpn)
    }

    /// Translate virtual address to physical address
    pub fn translate(&self, virt_addr: VirtAddr, access: AccessType) -> Result<PhysAddr, PageError> {
        let entry = self.lookup(virt_addr).ok_or(PageError::NotMapped {
            vpn: virt_addr / self.page_size as u64,
        })?;

        if !entry.check_access(access) {
            return Err(PageError::PermissionDenied {
                virt_addr,
                access,
            });
        }

        let offset = (virt_addr % self.page_size as u64) as usize;
        entry.phys_address_for_offset(offset)
            .ok_or(PageError::InvalidOffset { virt_addr, offset })
    }

    /// Get all mapped pages
    pub fn mapped_pages(&self) -> Vec<PageTableEntry> {
        self.entries.values().cloned().collect()
    }

    /// Clear all mappings
    pub fn clear(&mut self) {
        self.entries.clear();
        self.mapped_count = 0;
    }

    /// Get page table statistics
    pub fn stats(&self) -> PageTableStats {
        PageTableStats {
            pid: self.pid,
            page_size: self.page_size,
            total_pages: self.total_pages,
            mapped_pages: self.mapped_count,
            usage_percent: (self.mapped_count as f64 / self.total_pages as f64) * 100.0,
        }
    }

    /// Update page flags
    pub fn update_flags(&mut self, virt_addr: VirtAddr, flags: PageFlags) -> Result<(), PageError> {
        let vpn = virt_addr / self.page_size as u64;

        if let Some(entry) = self.entries.get_mut(&vpn) {
            entry.set_flags(flags);
            Ok(())
        } else {
            Err(PageError::NotMapped { vpn })
        }
    }
}

/// Page table statistics
#[derive(Debug, Clone)]
pub struct PageTableStats {
    pub pid: Pid,
    pub page_size: usize,
    pub total_pages: usize,
    pub mapped_pages: usize,
    pub usage_percent: f64,
}

/// Page table manager for all processes
///
/// 曾国藩曰：
/// "总管诸册，当知其详。"
/// 页表管理器统筹所有进程的页表。
#[derive(Debug)]
pub struct PageTableManager {
    /// Page tables (pid -> page table)
    tables: HashMap<Pid, Arc<Mutex<PageTable>>>,

    /// Page size
    page_size: usize,

    /// Total pages per process
    pages_per_process: usize,
}

impl PageTableManager {
    /// Create a new page table manager
    pub fn new(page_size: usize, pages_per_process: usize) -> Self {
        Self {
            tables: HashMap::new(),
            page_size,
            pages_per_process,
        }
    }

    /// Create a page table for a process
    pub fn create_table(&mut self, pid: Pid) -> Arc<Mutex<PageTable>> {
        let table = Arc::new(Mutex::new(PageTable::new(
            pid,
            self.page_size,
            self.pages_per_process,
        )));

        self.tables.insert(pid, table.clone());
        table
    }

    /// Get a process's page table
    pub fn get_table(&self, pid: Pid) -> Option<Arc<Mutex<PageTable>>> {
        self.tables.get(&pid).cloned()
    }

    /// Remove a process's page table
    pub fn remove_table(&mut self, pid: Pid) -> Option<Arc<Mutex<PageTable>>> {
        self.tables.remove(&pid)
    }

    /// Get all page tables
    pub fn all_tables(&self) -> Vec<(Pid, PageTableStats)> {
        self.tables.iter()
            .filter_map(|(pid, table)| {
                table.lock().ok().map(|tbl| (*pid, tbl.stats()))
            })
            .collect()
    }

    /// Clean up empty page tables
    pub fn cleanup(&mut self) -> usize {
        let mut removed = 0;
        self.tables.retain(|_, table| {
            if let Ok(tbl) = table.lock() {
                if tbl.mapped_count == 0 {
                    removed += 1;
                    return false;
                }
            }
            true
        });
        removed
    }
}

impl Default for PageTableManager {
    fn default() -> Self {
        Self::new(4096, 1024) // 4KB pages, 1024 pages per process
    }
}

/// Page errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageError {
    /// Page already mapped
    AlreadyMapped { vpn: u64 },

    /// Page not mapped
    NotMapped { vpn: u64 },

    /// Permission denied
    PermissionDenied { virt_addr: VirtAddr, access: AccessType },

    /// Invalid offset within page
    InvalidOffset { virt_addr: VirtAddr, offset: usize },

    /// Page fault
    PageFault { virt_addr: VirtAddr, access: AccessType },
}

impl std::fmt::Display for PageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyMapped { vpn } => write!(f, "Page already mapped: vpn={}", vpn),
            Self::NotMapped { vpn } => write!(f, "Page not mapped: vpn={}", vpn),
            Self::PermissionDenied { virt_addr, access } => {
                write!(f, "Permission denied: addr={:#x}, access={:?}", virt_addr, access)
            }
            Self::InvalidOffset { virt_addr, offset } => {
                write!(f, "Invalid offset: addr={:#x}, offset={}", virt_addr, offset)
            }
            Self::PageFault { virt_addr, access } => {
                write!(f, "Page fault: addr={:#x}, access={:?}", virt_addr, access)
            }
        }
    }
}

impl std::error::Error for PageError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_flags() {
        let flags = PageFlags::new();
        assert!(!flags.present);

        let flags = PageFlags::from_prot(MemProt::read_write());
        assert!(flags.present);
        assert!(flags.writable);
        assert!(flags.user);

        assert!(flags.check_access(AccessType::Read));
        assert!(flags.check_access(AccessType::Write));
        assert!(!flags.check_access(AccessType::Execute));
    }

    #[test]
    fn test_page_flags_mark_accessed() {
        let mut flags = PageFlags::from_prot(MemProt::read_write());
        assert!(!flags.accessed);

        flags.mark_accessed();
        assert!(flags.accessed);
    }

    #[test]
    fn test_page_table_entry() {
        let entry = PageTableEntry::new(
            0,
            100,
            0x1000,
            0x1000,
            4096,
            PageFlags::from_prot(MemProt::read_only()),
        );

        assert!(entry.is_present());
        assert_eq!(entry.vpn, 0);
        assert_eq!(entry.pfn, 100);
        assert!(entry.check_access(AccessType::Read));
        assert!(!entry.check_access(AccessType::Write));
    }

    #[test]
    fn test_page_table() {
        let mut pt = PageTable::new(100, 4096, 256);

        // Map a page
        let result = pt.map(0x1000, 0x1000, MemProt::read_write());
        assert!(result.is_ok());

        // Lookup
        let entry = pt.lookup(0x1000);
        assert!(entry.is_some());
        assert!(entry.unwrap().is_present());

        // Translate
        let phys = pt.translate(0x1000, AccessType::Read);
        assert!(phys.is_ok());
        assert_eq!(phys.unwrap(), 0x1000);

        // Unmap
        let result = pt.unmap(0x1000);
        assert!(result.is_ok());

        // Now should fail to translate
        let phys = pt.translate(0x1000, AccessType::Read);
        assert!(phys.is_err());
    }

    #[test]
    fn test_page_table_already_mapped() {
        let mut pt = PageTable::new(100, 4096, 256);

        pt.map(0x1000, 0x1000, MemProt::read_write()).unwrap();

        // Try to map again
        let result = pt.map(0x1000, 0x2000, MemProt::read_write());
        assert!(result.is_err());
    }

    #[test]
    fn test_page_table_manager() {
        let mut manager = PageTableManager::new(4096, 256);

        // Create tables
        let table1 = manager.create_table(100);
        let table2 = manager.create_table(200);

        assert_eq!(manager.tables.len(), 2);

        // Get table
        let retrieved = manager.get_table(100);
        assert!(retrieved.is_some());

        // Remove table
        let removed = manager.remove_table(100);
        assert!(removed.is_some());

        // Should be gone
        assert!(manager.get_table(100).is_none());
    }

    #[test]
    fn test_page_table_stats() {
        let mut pt = PageTable::new(100, 4096, 256);

        pt.map(0x1000, 0x1000, MemProt::read_write()).unwrap();
        pt.map(0x2000, 0x2000, MemProt::read_write()).unwrap();
        pt.map(0x3000, 0x3000, MemProt::read_write()).unwrap();

        let stats = pt.stats();
        assert_eq!(stats.pid, 100);
        assert_eq!(stats.mapped_pages, 3);
        assert_eq!(stats.total_pages, 256);
    }

    #[test]
    fn test_offset_translation() {
        let mut pt = PageTable::new(100, 4096, 256);

        pt.map(0x1000, 0x1000, MemProt::read_write()).unwrap();

        // Translate addresses with different offsets
        let phys1 = pt.translate(0x1000, AccessType::Read);
        assert_eq!(phys1.unwrap(), 0x1000);

        let phys2 = pt.translate(0x1000 + 100, AccessType::Read);
        assert_eq!(phys2.unwrap(), 0x1000 + 100);

        // Offset beyond page size should fail
        let phys3 = pt.translate(0x1000 + 5000, AccessType::Read);
        assert!(phys3.is_err());
    }

    #[test]
    fn test_update_flags() {
        let mut pt = PageTable::new(100, 4096, 256);

        pt.map(0x1000, 0x1000, MemProt::read_write()).unwrap();

        // Update flags to read-only
        let new_flags = PageFlags::from_prot(MemProt::read_only());
        assert!(pt.update_flags(0x1000, new_flags).is_ok());

        // Should fail to write now
        let result = pt.translate(0x1000, AccessType::Write);
        assert!(result.is_err());
    }

    #[test]
    fn test_clear_page_table() {
        let mut pt = PageTable::new(100, 4096, 256);

        pt.map(0x1000, 0x1000, MemProt::read_write()).unwrap();
        pt.map(0x2000, 0x2000, MemProt::read_write()).unwrap();
        assert_eq!(pt.mapped_count, 2);

        pt.clear();
        assert_eq!(pt.mapped_count, 0);
        assert!(pt.lookup(0x1000).is_none());
    }
}
