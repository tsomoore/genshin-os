// Memory Management Module
//


use std::collections::{VecDeque, HashMap};
use std::sync::{Arc, Mutex};
use crate::messaging::{Pid, VirtAddr, PhysAddr};
use crate::hardware::PhysicalMemory;

/// Physical memory frame
///

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Frame {
    /// Physical frame number
    pub number: u64,

    /// Physical address
    pub address: PhysAddr,

    /// Size in bytes
    pub size: usize,

    /// Whether this frame is allocated
    pub allocated: bool,

    /// Process that owns this frame (None = free)
    pub owner: Option<Pid>,
}

impl Frame {
    /// Create a new frame
    pub fn new(number: u64, address: PhysAddr, size: usize) -> Self {
        Self {
            number,
            address,
            size,
            allocated: false,
            owner: None,
        }
    }

    /// Mark frame as allocated
    pub fn allocate(&mut self, pid: Pid) {
        self.allocated = true;
        self.owner = Some(pid);
    }

    /// Mark frame as free
    pub fn free(&mut self) {
        self.allocated = false;
        self.owner = None;
    }

    /// Check if frame is free
    pub fn is_free(&self) -> bool {
        !self.allocated
    }

    /// Get the end address of this frame
    pub fn end_address(&self) -> PhysAddr {
        self.address + self.size as u64
    }
}

/// Frame Allocator - Manages physical memory frames
///

#[derive(Debug)]
pub struct FrameAllocator {
    /// All frames
    frames: Vec<Frame>,

    /// Free frame queue (for quick allocation)
    free_queue: VecDeque<u64>,

    /// Total number of frames
    total_frames: u64,

    /// Frame size in bytes
    frame_size: usize,

    /// Number of free frames
    free_count: u64,

    /// Number of allocated frames
    allocated_count: u64,
}

impl FrameAllocator {
    /// Create a new frame allocator
    pub fn new(memory_size: usize, frame_size: usize) -> Self {
        let total_frames = (memory_size / frame_size) as u64;

        let mut frames = Vec::new();
        let mut free_queue = VecDeque::new();

        for i in 0..total_frames {
            let address = (i as usize * frame_size) as PhysAddr;
            let frame = Frame::new(i, address, frame_size);
            frames.push(frame);
            free_queue.push_back(i);
        }

        Self {
            frames,
            free_queue,
            total_frames,
            frame_size,
            free_count: total_frames,
            allocated_count: 0,
        }
    }

    /// Allocate a frame
    pub fn allocate(&mut self, pid: Pid) -> Option<Frame> {
        if let Some(frame_num) = self.free_queue.pop_front() {
            let frame = &mut self.frames[frame_num as usize];
            frame.allocate(pid);

            self.free_count -= 1;
            self.allocated_count += 1;

            Some(*frame)
        } else {
            None // No free frames
        }
    }

    /// Free a frame
    pub fn free(&mut self, frame_num: u64) -> bool {
        if frame_num >= self.total_frames {
            return false;
        }

        let frame = &mut self.frames[frame_num as usize];
        if !frame.allocated {
            return false; // Already free
        }

        frame.free();
        self.free_queue.push_back(frame_num);
        self.free_count += 1;
        self.allocated_count -= 1;

        true
    }

    /// Get a frame by number
    pub fn get_frame(&self, frame_num: u64) -> Option<Frame> {
        if frame_num < self.total_frames {
            Some(self.frames[frame_num as usize])
        } else {
            None
        }
    }

    /// Get the number of free frames
    pub fn free_count(&self) -> u64 {
        self.free_count
    }

    /// Get the number of allocated frames
    pub fn allocated_count(&self) -> u64 {
        self.allocated_count
    }

    /// Get total frames
    pub fn total_frames(&self) -> u64 {
        self.total_frames
    }

    /// Get memory usage statistics
    pub fn usage(&self) -> MemoryUsage {
        MemoryUsage {
            total_frames: self.total_frames,
            free_frames: self.free_count,
            allocated_frames: self.allocated_count,
            total_memory: self.total_frames * self.frame_size as u64,
            free_memory: self.free_count * self.frame_size as u64,
            used_memory: self.allocated_count * self.frame_size as u64,
        }
    }

    /// Find frames owned by a process
    pub fn frames_by_owner(&self, pid: Pid) -> Vec<Frame> {
        self.frames.iter()
            .filter(|f| f.owner == Some(pid))
            .copied()
            .collect()
    }

    /// Free all frames owned by a process
    pub fn free_by_owner(&mut self, pid: Pid) -> u64 {
        let mut freed = 0;
        for frame in &mut self.frames {
            if frame.owner == Some(pid) {
                frame.free();
                self.free_queue.push_back(frame.number);
                self.free_count += 1;
                self.allocated_count -= 1;
                freed += 1;
            }
        }
        freed
    }

    /// Get ownership map: (frame_number, owner_pid_or_0_if_free) for all frames
    pub fn frame_owners(&self) -> Vec<(u64, u64)> {
        self.frames.iter()
            .map(|f| (f.number, f.owner.unwrap_or(0)))
            .collect()
    }
}

/// Memory usage statistics
#[derive(Debug, Clone)]
pub struct MemoryUsage {
    pub total_frames: u64,
    pub free_frames: u64,
    pub allocated_frames: u64,
    pub total_memory: u64,
    pub free_memory: u64,
    pub used_memory: u64,
}

impl MemoryUsage {
    /// Get memory usage percentage
    pub fn usage_percent(&self) -> f64 {
        if self.total_memory == 0 {
            return 0.0;
        }
        (self.used_memory as f64 / self.total_memory as f64) * 100.0
    }
}

/// Physical Memory Manager
///

#[derive(Debug)]
pub struct PhysicalMemoryManager {
    /// Frame allocator
    allocator: FrameAllocator,

    /// Process memory maps (pid -> list of frame numbers)
    process_frames: HashMap<Pid, Vec<u64>>,
}

impl PhysicalMemoryManager {
    /// Create a new physical memory manager
    pub fn new(memory_size: usize, frame_size: usize) -> Self {
        Self {
            allocator: FrameAllocator::new(memory_size, frame_size),
            process_frames: HashMap::new(),
        }
    }

    /// Allocate frames for a process
    pub fn allocate_frames(&mut self, pid: Pid, count: usize) -> Vec<Frame> {
        let mut frames = Vec::new();

        for _ in 0..count {
            if let Some(frame) = self.allocator.allocate(pid) {
                // Track frame for this process
                self.process_frames
                    .entry(pid)
                    .or_insert_with(Vec::new)
                    .push(frame.number);

                frames.push(frame);
            } else {
                // Allocation failed, free already allocated frames
                for frame in &frames {
                    self.free_frame(pid, frame.number);
                }
                frames.clear();
                break;
            }
        }

        frames
    }

    /// Free a frame
    pub fn free_frame(&mut self, pid: Pid, frame_num: u64) -> bool {
        // Remove from process frame list
        if let Some(frames) = self.process_frames.get_mut(&pid) {
            if let Some(pos) = frames.iter().position(|&x| x == frame_num) {
                frames.remove(pos);
            }
        }

        // Free the frame
        self.allocator.free(frame_num)
    }

    /// Free all frames for a process
    pub fn free_process_frames(&mut self, pid: Pid) -> u64 {
        let frame_count = self.allocator.free_by_owner(pid);
        self.process_frames.remove(&pid);
        frame_count
    }

    /// Get frames owned by a process
    pub fn get_process_frames(&self, pid: Pid) -> Vec<Frame> {
        if let Some(frame_nums) = self.process_frames.get(&pid) {
            frame_nums.iter()
                .filter_map(|&num| self.allocator.get_frame(num))
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get memory usage
    pub fn usage(&self) -> MemoryUsage {
        self.allocator.usage()
    }

    /// Get allocator reference
    pub fn allocator(&self) -> &FrameAllocator {
        &self.allocator
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_creation() {
        let frame = Frame::new(0, 0x1000, 4096);

        assert_eq!(frame.number, 0);
        assert_eq!(frame.address, 0x1000);
        assert_eq!(frame.size, 4096);
        assert!(frame.is_free());
    }

    #[test]
    fn test_frame_allocate_free() {
        let mut frame = Frame::new(0, 0x1000, 4096);

        assert!(frame.is_free());
        frame.allocate(100);
        assert!(!frame.is_free());
        assert_eq!(frame.owner, Some(100));

        frame.free();
        assert!(frame.is_free());
        assert_eq!(frame.owner, None);
    }

    #[test]
    fn test_frame_allocator_creation() {
        let allocator = FrameAllocator::new(1024 * 1024, 4096); // 1MB, 4KB frames

        assert_eq!(allocator.total_frames(), 256); // 1MB / 4KB = 256
        assert_eq!(allocator.free_count(), 256);
        assert_eq!(allocator.allocated_count(), 0);
    }

    #[test]
    fn test_frame_allocate() {
        let mut allocator = FrameAllocator::new(4096 * 10, 4096);

        // Allocate a frame
        let frame = allocator.allocate(100);
        assert!(frame.is_some());
        let frame = frame.unwrap();

        assert_eq!(frame.number, 0);
        assert!(frame.allocated);
        assert_eq!(frame.owner, Some(100));

        assert_eq!(allocator.free_count(), 9);
        assert_eq!(allocator.allocated_count(), 1);
    }

    #[test]
    fn test_frame_free() {
        let mut allocator = FrameAllocator::new(4096 * 10, 4096);

        // Allocate and free
        let frame = allocator.allocate(100).unwrap();
        assert_eq!(allocator.free_count(), 9);

        assert!(allocator.free(frame.number));
        assert_eq!(allocator.free_count(), 10);
        assert_eq!(allocator.allocated_count(), 0);
    }

    #[test]
    fn test_exhaustion() {
        let mut allocator = FrameAllocator::new(4096 * 2, 4096);

        // Allocate all frames
        let f1 = allocator.allocate(100);
        let f2 = allocator.allocate(200);
        assert!(f1.is_some());
        assert!(f2.is_some());

        // No more frames
        let f3 = allocator.allocate(300);
        assert!(f3.is_none());

        // Free one frame
        assert!(allocator.free(f1.unwrap().number));

        // Now can allocate again
        let f4 = allocator.allocate(300);
        assert!(f4.is_some());
    }

    #[test]
    fn test_memory_usage() {
        let mut allocator = FrameAllocator::new(4096 * 10, 4096);

        allocator.allocate(100);
        allocator.allocate(100);
        allocator.allocate(100);

        let usage = allocator.usage();

        assert_eq!(usage.total_frames, 10);
        assert_eq!(usage.free_frames, 7);
        assert_eq!(usage.allocated_frames, 3);
        assert_eq!(usage.usage_percent(), 30.0);
    }

    #[test]
    fn test_frames_by_owner() {
        let mut allocator = FrameAllocator::new(4096 * 10, 4096);

        allocator.allocate(100);
        allocator.allocate(100);
        allocator.allocate(100);
        allocator.allocate(200);

        let frames_100 = allocator.frames_by_owner(100);
        assert_eq!(frames_100.len(), 3);

        let frames_200 = allocator.frames_by_owner(200);
        assert_eq!(frames_200.len(), 1);
    }

    #[test]
    fn test_free_by_owner() {
        let mut allocator = FrameAllocator::new(4096 * 10, 4096);

        allocator.allocate(100);
        allocator.allocate(100);
        allocator.allocate(200);

        // Free all frames owned by process 100
        let freed = allocator.free_by_owner(100);
        assert_eq!(freed, 2);
        assert_eq!(allocator.free_count(), 9); // 10 total - 1 allocated (process 200) = 9 free
    }

    #[test]
    fn test_physical_memory_manager() {
        let mut manager = PhysicalMemoryManager::new(4096 * 100, 4096);

        // Allocate frames for process 100
        let frames = manager.allocate_frames(100, 5);
        assert_eq!(frames.len(), 5);

        // Check process frames
        let process_frames = manager.get_process_frames(100);
        assert_eq!(process_frames.len(), 5);

        // Free process frames
        let freed = manager.free_process_frames(100);
        assert_eq!(freed, 5);

        // Verify freed
        let process_frames = manager.get_process_frames(100);
        assert_eq!(process_frames.len(), 0);
    }

    #[test]
    fn test_frame_end_address() {
        let frame = Frame::new(0, 0x1000, 4096);
        assert_eq!(frame.end_address(), 0x2000);

        let frame = Frame::new(1, 0x2000, 8192);
        assert_eq!(frame.end_address(), 0x4000);
    }
}
