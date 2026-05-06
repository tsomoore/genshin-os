// IPC (Inter-Process Communication) Module
//
// 曾国藩曰：
// "沟通者，成事之基也。"
// 进程间通信乃系统协作之基础，当慎之又慎。

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use crate::messaging::{
    Pid, Tid, VirtAddr, PhysAddr, MemProt, IPCMessage, SignalType,
};
use crate::messaging::BlockReason;

/// Message queue for IPC
///
/// 曾国藩曰：
/// "书信往来，当有案可稽。"
/// 消息队列记录进程间之一切通信。
#[derive(Debug, Clone, )]
pub struct MessageQueue {
    /// Owner process ID
    pub owner_pid: Pid,

    /// Queue capacity (0 = unlimited)
    pub capacity: usize,

    /// Messages in the queue
    messages: VecDeque<QueuedMessage>,

    /// Creation time
    pub created_at: SystemTime,

    /// Total messages ever sent
    pub total_sent: u64,

    /// Total messages ever received
    pub total_received: u64,
}

/// A message in the queue with metadata
#[derive(Debug, Clone, )]
pub struct QueuedMessage {
    /// The actual message
    pub message: IPCMessage,

    /// Sender process ID
    pub from_pid: Pid,

    /// Sender thread ID
    pub from_tid: Tid,

    /// Timestamp when message was sent
    pub timestamp: SystemTime,
}

impl MessageQueue {
    /// Create a new message queue
    pub fn new(owner_pid: Pid, capacity: usize) -> Self {
        Self {
            owner_pid,
            capacity,
            messages: VecDeque::new(),
            created_at: SystemTime::now(),
            total_sent: 0,
            total_received: 0,
        }
    }

    /// Send a message to this queue
    pub fn send(&mut self, from_pid: Pid, from_tid: Tid, message: IPCMessage) -> Result<(), QueueError> {
        // Check capacity
        if self.capacity > 0 && self.messages.len() >= self.capacity {
            return Err(QueueError::Full);
        }

        let queued = QueuedMessage {
            message,
            from_pid,
            from_tid,
            timestamp: SystemTime::now(),
        };

        self.messages.push_back(queued);
        self.total_sent += 1;
        Ok(())
    }

    /// Receive a message from this queue
    pub fn receive(&mut self) -> Option<QueuedMessage> {
        let msg = self.messages.pop_front();
        if msg.is_some() {
            self.total_received += 1;
        }
        msg
    }

    /// Peek at the first message without removing it
    pub fn peek(&self) -> Option<&QueuedMessage> {
        self.messages.front()
    }

    /// Get the number of messages in the queue
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Check if the queue is empty
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Check if the queue is full
    pub fn is_full(&self) -> bool {
        self.capacity > 0 && self.messages.len() >= self.capacity
    }

    /// Clear all messages
    pub fn clear(&mut self) {
        self.messages.clear();
    }
}

/// IPC queue errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueError {
    /// Queue is full
    Full,

    /// Queue doesn't exist
    NotFound,

    /// Permission denied
    PermissionDenied,

    /// Queue was closed
    Closed,
}

/// Shared memory region
///
/// 曾国藩曰：
/// "共用地土，当立界碑，以免争端。"
/// 共享内存乃进程间共享之地，当明确界限与权限。
#[derive(Debug, Clone, )]
pub struct SharedMemoryRegion {
    /// Unique shared memory ID
    pub shmid: u64,

    /// Creator process ID
    pub creator_pid: Pid,

    /// Size in bytes
    pub size: usize,

    /// Physical address of the shared memory
    pub physical_addr: PhysAddr,

    /// Memory protection flags
    pub prot: MemProt,

    /// Reference count (number of attached processes)
    pub ref_count: usize,

    /// Creation time
    pub created_at: SystemTime,

    /// Last attach time
    pub last_attach: Option<SystemTime>,

    /// Attached processes (pid -> virtual address mapping)
    pub attached: HashMap<Pid, VirtAddr>,

    /// Whether this region is marked for deletion
    pub marked_for_deletion: bool,
}

impl SharedMemoryRegion {
    /// Create a new shared memory region
    pub fn new(shmid: u64, creator_pid: Pid, size: usize, physical_addr: PhysAddr, prot: MemProt) -> Self {
        Self {
            shmid,
            creator_pid,
            size,
            physical_addr,
            prot,
            ref_count: 0,
            created_at: SystemTime::now(),
            last_attach: None,
            attached: HashMap::new(),
            marked_for_deletion: false,
        }
    }

    /// Attach a process to this shared memory region
    pub fn attach(&mut self, pid: Pid, vaddr: VirtAddr) {
        self.attached.insert(pid, vaddr);
        self.ref_count += 1;
        self.last_attach = Some(SystemTime::now());
    }

    /// Detach a process from this shared memory region
    pub fn detach(&mut self, pid: Pid) -> bool {
        let was_attached = self.attached.remove(&pid).is_some();
        if was_attached {
            self.ref_count -= 1;
        }
        was_attached
    }

    /// Get the virtual address for a process
    pub fn get_vaddr(&self, pid: Pid) -> Option<VirtAddr> {
        self.attached.get(&pid).copied()
    }

    /// Check if a process is attached
    pub fn is_attached(&self, pid: Pid) -> bool {
        self.attached.contains_key(&pid)
    }

    /// Get the list of attached process IDs
    pub fn attached_pids(&self) -> Vec<Pid> {
        self.attached.keys().copied().collect()
    }

    /// Check if the region can be deleted (no attachments and marked)
    pub fn can_delete(&self) -> bool {
        self.marked_for_deletion && self.ref_count == 0
    }

    /// Mark for deletion (will be deleted when ref_count reaches 0)
    pub fn mark_for_deletion(&mut self) {
        self.marked_for_deletion = true;
    }
}

/// IPC Manager - Manages all IPC resources
///
/// 曾国藩曰：
/// "百官之事，皆需登记造册，方知其详。"
/// IPC 管理器统筹所有进程间通信资源。
#[derive(Debug)]
pub struct IPCManager {
    /// Message queues (shmid -> queue)
    /// Note: We use Pid as key for per-process message queues
    message_queues: HashMap<Pid, Arc<Mutex<MessageQueue>>>,

    /// Shared memory regions (shmid -> region)
    shared_memory: HashMap<u64, Arc<Mutex<SharedMemoryRegion>>>,

    /// Next shared memory ID
    next_shmid: u64,
}

impl IPCManager {
    /// Create a new IPC manager
    pub fn new() -> Self {
        Self {
            message_queues: HashMap::new(),
            shared_memory: HashMap::new(),
            next_shmid: 1,
        }
    }

    // ========== Message Queue Management ==========

    /// Get or create a message queue for a process
    pub fn get_message_queue(&self, pid: Pid) -> Arc<Mutex<MessageQueue>> {
        if let Some(queue) = self.message_queues.get(&pid) {
            return queue.clone();
        }

        // Create new queue with unlimited capacity
        let queue = Arc::new(Mutex::new(MessageQueue::new(pid, 0)));
        queue
    }

    /// Ensure a process has a message queue
    pub fn ensure_message_queue(&mut self, pid: Pid) -> Arc<Mutex<MessageQueue>> {
        if let Some(queue) = self.message_queues.get(&pid) {
            return queue.clone();
        }

        let queue = Arc::new(Mutex::new(MessageQueue::new(pid, 0)));
        self.message_queues.insert(pid, queue.clone());
        queue
    }

    /// Remove a message queue
    pub fn remove_message_queue(&mut self, pid: Pid) -> Option<Arc<Mutex<MessageQueue>>> {
        self.message_queues.remove(&pid)
    }

    // ========== Shared Memory Management ==========

    /// Create a new shared memory region
    pub fn create_shared_memory(
        &mut self,
        creator_pid: Pid,
        size: usize,
        physical_addr: PhysAddr,
        prot: MemProt,
    ) -> u64 {
        let shmid = self.next_shmid;
        self.next_shmid += 1;

        let region = SharedMemoryRegion::new(shmid, creator_pid, size, physical_addr, prot);
        self.shared_memory.insert(shmid, Arc::new(Mutex::new(region)));

        shmid
    }

    /// Get a shared memory region
    pub fn get_shared_memory(&self, shmid: u64) -> Option<Arc<Mutex<SharedMemoryRegion>>> {
        self.shared_memory.get(&shmid).cloned()
    }

    /// Attach a process to shared memory
    pub fn attach_shared_memory(
        &self,
        shmid: u64,
        pid: Pid,
        vaddr: VirtAddr,
    ) -> Result<(), IPCError> {
        let region = self.shared_memory.get(&shmid)
            .ok_or(IPCError::NotFound(shmid))?;

        let mut region = region.lock()
            .map_err(|_| IPCError::Locked)?;

        region.attach(pid, vaddr);
        Ok(())
    }

    /// Detach a process from shared memory
    pub fn detach_shared_memory(
        &self,
        shmid: u64,
        pid: Pid,
    ) -> Result<bool, IPCError> {
        let region = self.shared_memory.get(&shmid)
            .ok_or(IPCError::NotFound(shmid))?;

        let mut region = region.lock()
            .map_err(|_| IPCError::Locked)?;

        Ok(region.detach(pid))
    }

    /// Mark shared memory for deletion
    pub fn mark_shared_memory_for_deletion(
        &self,
        shmid: u64,
    ) -> Result<(), IPCError> {
        let region = self.shared_memory.get(&shmid)
            .ok_or(IPCError::NotFound(shmid))?;

        let mut region = region.lock()
            .map_err(|_| IPCError::Locked)?;

        region.mark_for_deletion();
        Ok(())
    }

    /// Clean up shared memory regions that can be deleted
    pub fn cleanup_shared_memory(&mut self) -> Vec<u64> {
        let mut to_remove = Vec::new();

        for (&shmid, region) in &self.shared_memory {
            if let Ok(region) = region.lock() {
                if region.can_delete() {
                    to_remove.push(shmid);
                }
            }
        }

        for shmid in &to_remove {
            self.shared_memory.remove(shmid);
        }

        to_remove
    }

    /// Get all shared memory regions
    pub fn list_shared_memory(&self) -> Vec<(u64, SharedMemoryInfo)> {
        self.shared_memory.iter()
            .filter_map(|(&shmid, region)| {
                region.lock().ok().map(|r| {
                    (shmid, SharedMemoryInfo {
                        shmid,
                        creator_pid: r.creator_pid,
                        size: r.size,
                        ref_count: r.ref_count,
                        attached_pids: r.attached_pids(),
                    })
                })
            })
            .collect()
    }
}

impl Default for IPCManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Information about a shared memory region
#[derive(Debug, Clone)]
pub struct SharedMemoryInfo {
    pub shmid: u64,
    pub creator_pid: Pid,
    pub size: usize,
    pub ref_count: usize,
    pub attached_pids: Vec<Pid>,
}

/// IPC errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IPCError {
    /// Shared memory region not found
    NotFound(u64),

    /// Region is locked
    Locked,

    /// Permission denied
    PermissionDenied,

    /// Invalid size
    InvalidSize,

    /// Out of memory
    OutOfMemory,
}

impl std::fmt::Display for IPCError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "Shared memory region {} not found", id),
            Self::Locked => write!(f, "Region is locked"),
            Self::PermissionDenied => write!(f, "Permission denied"),
            Self::InvalidSize => write!(f, "Invalid size"),
            Self::OutOfMemory => write!(f, "Out of memory"),
        }
    }
}

impl std::error::Error for IPCError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_queue_send_receive() {
        let mut queue = MessageQueue::new(1, 10);

        let msg = IPCMessage::Text { data: "Hello".to_string() };

        // Send message
        assert!(queue.send(2, 1, msg.clone()).is_ok());
        assert_eq!(queue.len(), 1);

        // Receive message
        let received = queue.receive().unwrap();
        assert_eq!(received.from_pid, 2);
        assert_eq!(received.from_tid, 1);
        assert!(matches!(received.message, IPCMessage::Text { .. }));

        // Queue is now empty
        assert!(queue.is_empty());
    }

    #[test]
    fn test_message_queue_capacity() {
        let mut queue = MessageQueue::new(1, 2);

        let msg = IPCMessage::Text { data: "test".to_string() };

        // Fill to capacity
        assert!(queue.send(2, 1, msg.clone()).is_ok());
        assert!(queue.send(2, 1, msg.clone()).is_ok());
        assert_eq!(queue.len(), 2);

        // Exceed capacity
        assert_eq!(queue.send(2, 1, msg), Err(QueueError::Full));
    }

    #[test]
    fn test_message_queue_peek() {
        let mut queue = MessageQueue::new(1, 10);

        let msg = IPCMessage::Text { data: "Hello".to_string() };
        queue.send(2, 1, msg.clone()).unwrap();

        // Peek doesn't remove
        let peeked = queue.peek().unwrap();
        assert!(matches!(peeked.message, IPCMessage::Text { .. }));
        assert_eq!(queue.len(), 1);

        // Receive removes
        let received = queue.receive().unwrap();
        assert_eq!(queue.len(), 0);
    }

    #[test]
    fn test_message_queue_stats() {
        let mut queue = MessageQueue::new(1, 10);

        queue.send(2, 1, IPCMessage::Text { data: "msg1".to_string() }).unwrap();
        queue.send(3, 1, IPCMessage::Text { data: "msg2".to_string() }).unwrap();

        assert_eq!(queue.total_sent, 2);
        assert_eq!(queue.total_received, 0);

        queue.receive();
        assert_eq!(queue.total_received, 1);
    }

    #[test]
    fn test_shared_memory_region() {
        let mut region = SharedMemoryRegion::new(
            1,
            100,
            4096,
            0x1000,
            MemProt::read_write(),
        );

        assert_eq!(region.shmid, 1);
        assert_eq!(region.creator_pid, 100);
        assert_eq!(region.size, 4096);
        assert_eq!(region.ref_count, 0);

        // Attach processes
        region.attach(100, 0x5000);
        assert_eq!(region.ref_count, 1);
        assert!(region.is_attached(100));
        assert_eq!(region.get_vaddr(100), Some(0x5000));

        region.attach(200, 0x6000);
        assert_eq!(region.ref_count, 2);

        // Detach process
        assert!(region.detach(100));
        assert_eq!(region.ref_count, 1);
        assert!(!region.is_attached(100));

        // Detach non-existent
        assert!(!region.detach(999));
    }

    #[test]
    fn test_shared_memory_deletion() {
        let mut region = SharedMemoryRegion::new(
            1,
            100,
            4096,
            0x1000,
            MemProt::read_write(),
        );

        // Can't delete yet (not marked for deletion)
        assert!(!region.can_delete());

        // Mark for deletion
        region.mark_for_deletion();
        assert!(region.marked_for_deletion);

        // Can delete now (ref_count == 0 && marked_for_deletion == true)
        assert!(region.can_delete());

        // Attach a process
        region.attach(100, 0x5000);

        // Now can't delete (ref_count > 0)
        assert!(!region.can_delete());

        // Detach process
        region.detach(100);

        // Now can delete again
        assert!(region.can_delete());
    }

    #[test]
    fn test_ipc_manager() {
        let mut manager = IPCManager::new();

        // Ensure message queue
        let queue = manager.ensure_message_queue(100);
        {
            let mut q = queue.lock().unwrap();
            q.send(200, 1, IPCMessage::Text { data: "test".to_string() }).unwrap();
        }

        // Get existing queue
        let queue2 = manager.get_message_queue(100);
        assert_eq!(queue.lock().unwrap().len(), queue2.lock().unwrap().len());

        // Create shared memory
        let shmid = manager.create_shared_memory(100, 4096, 0x1000, MemProt::read_only());
        assert_eq!(shmid, 1);

        // Get shared memory
        let region = manager.get_shared_memory(shmid);
        assert!(region.is_some());

        // Attach to shared memory
        assert!(manager.attach_shared_memory(shmid, 100, 0x5000).is_ok());

        // Detach from shared memory
        assert!(manager.detach_shared_memory(shmid, 100).is_ok());

        // Mark for deletion
        assert!(manager.mark_shared_memory_for_deletion(shmid).is_ok());

        // Cleanup
        let removed = manager.cleanup_shared_memory();
        assert_eq!(removed, vec![shmid]);
    }

    #[test]
    fn test_multiple_shared_memory_regions() {
        let mut manager = IPCManager::new();

        let shmid1 = manager.create_shared_memory(100, 4096, 0x1000, MemProt::read_only());
        let shmid2 = manager.create_shared_memory(100, 8192, 0x2000, MemProt::read_write());
        let shmid3 = manager.create_shared_memory(200, 2048, 0x3000, MemProt::read_only());

        assert_eq!(shmid1, 1);
        assert_eq!(shmid2, 2);
        assert_eq!(shmid3, 3);

        // List all
        let list = manager.list_shared_memory();
        assert_eq!(list.len(), 3);

        // Find specific region
        let found = list.iter().find(|(id, _)| *id == shmid2);
        assert!(found.is_some());
        let info = &found.unwrap().1;
        assert_eq!(info.size, 8192);
        assert_eq!(info.creator_pid, 100);
    }

    #[test]
    fn test_attached_pids() {
        let mut region = SharedMemoryRegion::new(
            1,
            100,
            4096,
            0x1000,
            MemProt::read_write(),
        );

        region.attach(100, 0x5000);
        region.attach(200, 0x6000);
        region.attach(300, 0x7000);

        let pids = region.attached_pids();
        assert_eq!(pids.len(), 3);
        assert!(pids.contains(&100));
        assert!(pids.contains(&200));
        assert!(pids.contains(&300));
    }

    #[test]
    fn test_ipc_error_display() {
        let err = IPCError::NotFound(42);
        assert_eq!(format!("{}", err), "Shared memory region 42 not found");

        let err = IPCError::PermissionDenied;
        assert_eq!(format!("{}", err), "Permission denied");
    }
}
