// Swap Space Management Module
//
// 曾国藩曰：
// "库有库管，仓有仓管，各有其职。"
// 交换空间管理负责将不常用的内存页换出到磁盘。

use std::collections::{VecDeque, HashMap};
use std::sync::{Arc, Mutex};
use crate::messaging::{Pid, VirtAddr, PhysAddr};

/// Swap slot on disk
///
/// 曾国藩曰：
/// "寸土寸金，当善加利用。"
/// 每个交换槽位都是宝贵的存储空间。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwapSlot {
    /// Slot number
    pub number: u64,

    /// Size in bytes
    pub size: usize,

    /// Whether this slot is occupied
    pub occupied: bool,

    /// Process that owns this slot
    pub owner: Option<Pid>,

    /// Virtual page number that was swapped out
    pub vpn: Option<u64>,
}

impl SwapSlot {
    /// Create a new swap slot
    pub fn new(number: u64, size: usize) -> Self {
        Self {
            number,
            size,
            occupied: false,
            owner: None,
            vpn: None,
        }
    }

    /// Occupy this slot
    pub fn occupy(&mut self, pid: Pid, vpn: u64) {
        self.occupied = true;
        self.owner = Some(pid);
        self.vpn = Some(vpn);
    }

    /// Free this slot
    pub fn free(&mut self) {
        self.occupied = false;
        self.owner = None;
        self.vpn = None;
    }

    /// Check if slot is free
    pub fn is_free(&self) -> bool {
        !self.occupied
    }
}

/// Swap space configuration
#[derive(Debug, Clone)]
pub struct SwapConfig {
    /// Total swap space size in bytes
    pub total_size: usize,

    /// Slot size (page size)
    pub slot_size: usize,

    /// Maximum number of slots
    pub max_slots: u64,

    /// Swap file/device path
    pub device_path: String,
}

impl Default for SwapConfig {
    fn default() -> Self {
        Self {
            total_size: 1024 * 1024 * 1024, // 1GB default
            slot_size: 4096,
            max_slots: 262144, // 1GB / 4KB
            device_path: "/swap/swapfile".to_string(),
        }
    }
}

/// Swap space manager
///
/// 曾国藩曰：
/// "调盈济虚，当有预案。"
/// 交换管理器在内存不足时，将不常用页面换出到磁盘。
#[derive(Debug)]
pub struct SwapManager {
    /// Configuration
    config: SwapConfig,

    /// All swap slots
    slots: Vec<SwapSlot>,

    /// Free slot queue
    free_queue: VecDeque<u64>,

    /// Process swap usage (pid -> slot numbers)
    process_slots: HashMap<Pid, Vec<u64>>,

    /// Total number of slots
    total_slots: u64,

    /// Number of used slots
    used_slots: u64,

    /// Whether swap is enabled
    enabled: bool,
}

impl SwapManager {
    /// Create a new swap manager
    pub fn new(config: SwapConfig) -> Self {
        let total_slots = (config.total_size / config.slot_size) as u64;

        let mut slots = Vec::new();
        let mut free_queue = VecDeque::new();

        for i in 0..total_slots {
            slots.push(SwapSlot::new(i, config.slot_size));
            free_queue.push_back(i);
        }

        Self {
            config,
            slots,
            free_queue,
            process_slots: HashMap::new(),
            total_slots,
            used_slots: 0,
            enabled: true,
        }
    }

    /// Enable swap
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Disable swap
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Check if swap is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Allocate a swap slot
    pub fn allocate_slot(&mut self, pid: Pid, vpn: u64) -> Option<SwapSlot> {
        if !self.enabled {
            return None;
        }

        if let Some(slot_num) = self.free_queue.pop_front() {
            let slot = &mut self.slots[slot_num as usize];
            slot.occupy(pid, vpn);

            self.process_slots
                .entry(pid)
                .or_insert_with(Vec::new)
                .push(slot_num);

            self.used_slots += 1;

            Some(*slot)
        } else {
            None // No free swap space
        }
    }

    /// Free a swap slot
    pub fn free_slot(&mut self, slot_num: u64) -> bool {
        if slot_num >= self.total_slots {
            return false;
        }

        let slot = &mut self.slots[slot_num as usize];
        if !slot.occupied {
            return false; // Already free
        }

        let pid = slot.owner.unwrap();

        slot.free();

        // Remove from process slots
        if let Some(slots) = self.process_slots.get_mut(&pid) {
            if let Some(pos) = slots.iter().position(|&x| x == slot_num) {
                slots.remove(pos);
            }
        }

        self.free_queue.push_back(slot_num);
        self.used_slots -= 1;

        true
    }

    /// Get swap slot by number
    pub fn get_slot(&self, slot_num: u64) -> Option<SwapSlot> {
        if slot_num < self.total_slots {
            Some(self.slots[slot_num as usize])
        } else {
            None
        }
    }

    /// Get swap slots for a process
    pub fn get_process_slots(&self, pid: Pid) -> Vec<SwapSlot> {
        if let Some(slot_nums) = self.process_slots.get(&pid) {
            slot_nums.iter()
                .filter_map(|&num| self.get_slot(num))
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get swap statistics
    pub fn stats(&self) -> SwapStats {
        SwapStats {
            enabled: self.enabled,
            total_slots: self.total_slots,
            used_slots: self.used_slots,
            free_slots: self.total_slots - self.used_slots,
            total_size: self.config.total_size,
            used_size: self.used_slots * self.config.slot_size as u64,
            usage_percent: (self.used_slots as f64 / self.total_slots as f64) * 100.0,
        }
    }

    /// Free all swap slots for a process
    pub fn free_process_slots(&mut self, pid: Pid) -> u64 {
        if let Some(slot_nums) = self.process_slots.remove(&pid) {
            let mut freed = 0;
            for slot_num in slot_nums {
                if self.free_slot(slot_num) {
                    freed += 1;
                }
            }
            freed
        } else {
            0
        }
    }

    /// Check if swap space is available
    pub fn has_space(&self) -> bool {
        !self.free_queue.is_empty()
    }

    /// Get configuration
    pub fn config(&self) -> &SwapConfig {
        &self.config
    }
}

/// Swap statistics
#[derive(Debug, Clone)]
pub struct SwapStats {
    pub enabled: bool,
    pub total_slots: u64,
    pub used_slots: u64,
    pub free_slots: u64,
    pub total_size: usize,
    pub used_size: u64,
    pub usage_percent: f64,
}

/// Swapping policy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapPolicy {
    /// Clock algorithm
    Clock,

    /// Not Recently Used (NRU)
    NRU,

    /// First-In-First-Out
    FIFO,

    /// Random
    Random,
}

impl Default for SwapPolicy {
    fn default() -> Self {
        Self::Clock
    }
}

/// Swap operation result
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwapResult {
    Success,
    NoSwapSpace,
    SwapDisabled,
    PageLocked,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_swap_slot() {
        let mut slot = SwapSlot::new(0, 4096);

        assert!(slot.is_free());
        assert!(!slot.occupied);

        slot.occupy(100, 5);
        assert!(slot.occupied);
        assert_eq!(slot.owner, Some(100));
        assert_eq!(slot.vpn, Some(5));

        slot.free();
        assert!(slot.is_free());
        assert_eq!(slot.owner, None);
        assert_eq!(slot.vpn, None);
    }

    #[test]
    fn test_swap_manager() {
        let config = SwapConfig {
            total_size: 4096 * 10, // 10 slots
            slot_size: 4096,
            max_slots: 10,
            device_path: "/test".to_string(),
        };

        let mut manager = SwapManager::new(config);

        assert_eq!(manager.total_slots, 10);
        assert_eq!(manager.used_slots, 0);

        // Allocate a slot
        let slot = manager.allocate_slot(100, 1);
        assert!(slot.is_some());
        assert_eq!(manager.used_slots, 1);

        // Free the slot
        assert!(manager.free_slot(slot.unwrap().number));
        assert_eq!(manager.used_slots, 0);
    }

    #[test]
    fn test_swap_exhaustion() {
        let config = SwapConfig {
            total_size: 4096 * 2,
            slot_size: 4096,
            max_slots: 2,
            device_path: "/test".to_string(),
        };

        let mut manager = SwapManager::new(config);

        // Allocate all slots
        let s1 = manager.allocate_slot(100, 1);
        let s2 = manager.allocate_slot(200, 2);
        assert!(s1.is_some());
        assert!(s2.is_some());

        // No more space
        let s3 = manager.allocate_slot(300, 3);
        assert!(s3.is_none());

        // Free one slot
        assert!(manager.free_slot(s1.unwrap().number));

        // Now can allocate
        let s4 = manager.allocate_slot(300, 3);
        assert!(s4.is_some());
    }

    #[test]
    fn test_swap_stats() {
        let config = SwapConfig {
            total_size: 4096 * 100,
            slot_size: 4096,
            max_slots: 100,
            device_path: "/test".to_string(),
        };

        let mut manager = SwapManager::new(config);

        manager.allocate_slot(100, 1);
        manager.allocate_slot(100, 2);
        manager.allocate_slot(100, 3);

        let stats = manager.stats();
        assert_eq!(stats.used_slots, 3);
        assert_eq!(stats.free_slots, 97);
        assert_eq!(stats.usage_percent, 3.0);
    }

    #[test]
    fn test_process_slots() {
        let config = SwapConfig::default();
        let mut manager = SwapManager::new(config);

        manager.allocate_slot(100, 1);
        manager.allocate_slot(100, 2);
        manager.allocate_slot(200, 1);

        let slots_100 = manager.get_process_slots(100);
        assert_eq!(slots_100.len(), 2);

        let slots_200 = manager.get_process_slots(200);
        assert_eq!(slots_200.len(), 1);
    }

    #[test]
    fn test_free_process_slots() {
        let config = SwapConfig::default();
        let mut manager = SwapManager::new(config);

        manager.allocate_slot(100, 1);
        manager.allocate_slot(100, 2);
        manager.allocate_slot(200, 1);

        // Free all slots for process 100
        let freed = manager.free_process_slots(100);
        assert_eq!(freed, 2);

        // Verify
        let slots_100 = manager.get_process_slots(100);
        assert_eq!(slots_100.len(), 0);
    }

    #[test]
    fn test_enable_disable() {
        let config = SwapConfig::default();
        let mut manager = SwapManager::new(config);

        assert!(manager.is_enabled());

        manager.disable();
        assert!(!manager.is_enabled());

        manager.enable();
        assert!(manager.is_enabled());

        // When disabled, allocation should fail
        manager.disable();
        let slot = manager.allocate_slot(100, 1);
        assert!(slot.is_none());
    }

    #[test]
    fn test_has_space() {
        let config = SwapConfig {
            total_size: 4096 * 5,
            slot_size: 4096,
            max_slots: 5,
            device_path: "/test".to_string(),
        };

        let mut manager = SwapManager::new(config);

        assert!(manager.has_space());

        // Fill up swap space
        for _ in 0..5 {
            manager.allocate_slot(100, 1);
        }

        assert!(!manager.has_space());

        // Free one slot
        let slot = manager.get_slot(0).unwrap();
        manager.free_slot(slot.number);

        assert!(manager.has_space());
    }

    #[test]
    fn test_swap_config_default() {
        let config = SwapConfig::default();
        assert_eq!(config.total_size, 1024 * 1024 * 1024);
        assert_eq!(config.slot_size, 4096);
        assert_eq!(config.max_slots, 262144);
    }

    #[test]
    fn test_get_slot() {
        let config = SwapConfig::default();
        let mut manager = SwapManager::new(config);

        // Get non-existent slot
        assert!(manager.get_slot(999999).is_none());

        // Allocate and get
        let allocated = manager.allocate_slot(100, 1).unwrap();
        let retrieved = manager.get_slot(allocated.number);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().number, allocated.number);
    }
}
