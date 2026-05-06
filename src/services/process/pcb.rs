// Process Control Block (PCB) and Thread Control Block (TCB)
//
// 曾国藩曰：
// "治军之道，首在知兵；治进程之道，首在知其状态。"
// PCB 乃进程之魂，记录其一切信息，当详加管理。

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, Duration};
use crate::messaging::{
    Pid, Tid, VirtAddr, SignalType, IPCMessage, BlockReason,
};
use crate::hardware::{CPUState, MMUState};

/// Process state
#[derive(Debug, Clone, PartialEq)]
pub enum ProcessState {
    /// Process is being created
    Creating,

    /// Process is ready to run
    Ready,

    /// Process is currently running
    Running,

    /// Process is blocked (waiting for I/O, memory, etc.)
    Blocked(BlockReason),

    /// Process has terminated
    Terminated { exit_code: i32 },

    /// Process is zombie (terminated but parent hasn't waited)
    Zombie { exit_code: i32 },
}

impl ProcessState {
    pub fn is_alive(&self) -> bool {
        matches!(self, Self::Creating | Self::Ready | Self::Running | Self::Blocked(_))
    }

    pub fn is_runnable(&self) -> bool {
        matches!(self, Self::Ready)
    }

    pub fn is_blocked(&self) -> bool {
        matches!(self, Self::Blocked(_))
    }

    pub fn is_terminated(&self) -> bool {
        matches!(self, Self::Terminated { .. } | Self::Zombie { .. })
    }
}

/// Thread state (within a process)
#[derive(Debug, Clone, PartialEq)]
pub enum ThreadState {
    /// Thread is being created
    Creating,

    /// Thread is ready to run
    Ready,

    /// Thread is currently running
    Running,

    /// Thread is blocked
    Blocked(BlockReason),

    /// Thread has terminated
    Terminated,
}

/// Thread Control Block (TCB)
///
/// 曾国藩曰：
// "一兵一卒，皆当登记造册，方知军力几何。"
/// TCB 记录线程之所有状态信息。
#[derive(Debug, Clone, )]
pub struct TCB {
    /// Thread ID (unique within process)
    pub tid: Tid,

    /// Thread state
    pub state: ThreadState,

    /// CPU state when thread is not running
    pub cpu_state: Option<CPUState>,

    /// Thread priority (0-255, higher = more priority)
    pub priority: u8,

    /// Reason for being blocked (if applicable)
    pub block_reason: Option<BlockReason>,

    /// Thread statistics
    pub stats: ThreadStats,

    /// Stack pointer
    pub stack_pointer: VirtAddr,

    /// Entry point
    pub entry_point: VirtAddr,
}

impl TCB {
    /// Create a new TCB
    pub fn new(tid: Tid, entry_point: VirtAddr, stack_pointer: VirtAddr) -> Self {
        Self {
            tid,
            state: ThreadState::Creating,
            cpu_state: None,
            priority: 128, // Default priority
            block_reason: None,
            stats: ThreadStats::default(),
            stack_pointer,
            entry_point,
        }
    }

    /// Set thread state
    pub fn set_state(&mut self, state: ThreadState) {
        self.state = state;
    }

    /// Block the thread
    pub fn block(&mut self, reason: BlockReason) {
        self.state = ThreadState::Blocked(reason.clone());
        self.block_reason = Some(reason);
    }

    /// Unblock the thread
    pub fn unblock(&mut self) {
        if matches!(self.state, ThreadState::Blocked(_)) {
            self.state = ThreadState::Ready;
            self.block_reason = None;
        }
    }

    /// Save CPU state
    pub fn save_cpu_state(&mut self, state: CPUState) {
        self.cpu_state = Some(state);
    }

    /// Restore CPU state
    pub fn take_cpu_state(&mut self) -> Option<CPUState> {
        self.cpu_state.take()
    }

    /// Check if thread is runnable
    pub fn is_runnable(&self) -> bool {
        self.state == ThreadState::Ready
    }

    /// Check if thread is blocked
    pub fn is_blocked(&self) -> bool {
        matches!(self.state, ThreadState::Blocked(_))
    }
}

/// Thread statistics
#[derive(Debug, Clone, )]
pub struct ThreadStats {
    /// Total CPU time used (in nanoseconds)
    pub cpu_time: u64,

    /// Number of times thread was scheduled
    pub schedule_count: u64,

    /// Number of context switches involving this thread
    pub context_switches: u64,

    /// Creation time
    pub created_at: SystemTime,
}

impl Default for ThreadStats {
    fn default() -> Self {
        Self {
            cpu_time: 0,
            schedule_count: 0,
            context_switches: 0,
            created_at: SystemTime::now(),
        }
    }
}

/// Process Control Block (PCB)
///
/// 曾国藩曰：
/// "为将者，当知部下之姓名、籍贯、特长、性格，方能用人得当。"
/// PCB 乃进程之档案，记录其一切信息，管理进程当以此为据。
#[derive(Debug, Clone, )]
pub struct PCB {
    /// Process ID
    pub pid: Pid,

    /// Parent process ID
    pub parent_pid: Option<Pid>,

    /// Process state
    pub state: ProcessState,

    /// Process priority (0-255, higher = more priority)
    pub priority: u8,

    /// Threads in this process
    pub threads: HashMap<Tid, TCB>,

    /// Main thread ID
    pub main_tid: Option<Tid>,

    /// Current running thread
    pub current_tid: Option<Tid>,

    /// Message queue for IPC
    pub message_queue: VecDeque<IPCMessage>,

    /// Shared memory regions owned by this process
    pub shared_memory: HashMap<u64, VirtAddr>, // shmid -> virtual address

    /// Pending signals
    pub pending_signals: Vec<SignalType>,

    /// Signal mask (blocked signals)
    pub signal_mask: u64,

    /// Exit code (if terminated)
    pub exit_code: Option<i32>,

    /// Process statistics
    pub stats: ProcessStats,

    /// Program name
    pub name: String,

    /// Command line arguments
    pub args: Vec<String>,

    /// Working directory
    pub working_dir: String,

    /// Environment variables
    pub env: HashMap<String, String>,

    /// Open file descriptors
    pub file_descriptors: HashMap<u32, FileDescriptor>,

    /// Next file descriptor number
    pub next_fd: u32,

    /// Creation time
    pub created_at: SystemTime,

    /// MMU state (page tables)
    pub mmu_state: Option<MMUState>,
}

impl PCB {
    /// Create a new PCB
    pub fn new(pid: Pid, name: String, parent_pid: Option<Pid>) -> Self {
        Self {
            pid,
            parent_pid,
            state: ProcessState::Creating,
            priority: 128,
            threads: HashMap::new(),
            main_tid: None,
            current_tid: None,
            message_queue: VecDeque::new(),
            shared_memory: HashMap::new(),
            pending_signals: Vec::new(),
            signal_mask: 0,
            exit_code: None,
            stats: ProcessStats::default(),
            name,
            args: Vec::new(),
            working_dir: "/".to_string(),
            env: HashMap::new(),
            file_descriptors: HashMap::new(),
            next_fd: 3, // 0, 1, 2 reserved for stdin, stdout, stderr
            created_at: SystemTime::now(),
            mmu_state: None,
        }
    }

    /// Add a thread to this process
    pub fn add_thread(&mut self, tcb: TCB) -> Tid {
        let tid = tcb.tid;
        self.threads.insert(tid, tcb);

        // First thread becomes main thread
        if self.main_tid.is_none() {
            self.main_tid = Some(tid);
        }

        // If process is ready and no thread is current, make this current
        if self.current_tid.is_none() && self.state == ProcessState::Ready {
            self.current_tid = Some(tid);
        }

        tid
    }

    /// Get a thread by TID
    pub fn get_thread(&self, tid: Tid) -> Option<&TCB> {
        self.threads.get(&tid)
    }

    /// Get a mutable thread by TID
    pub fn get_thread_mut(&mut self, tid: Tid) -> Option<&mut TCB> {
        self.threads.get_mut(&tid)
    }

    /// Remove a thread
    pub fn remove_thread(&mut self, tid: Tid) -> Option<TCB> {
        let removed = self.threads.remove(&tid);

        // Update current_tid if needed
        if self.current_tid == Some(tid) {
            self.current_tid = self.threads.keys().next().copied();
        }

        // Update main_tid if needed
        if self.main_tid == Some(tid) {
            self.main_tid = self.threads.keys().next().copied();
        }

        removed
    }

    /// Get current thread
    pub fn current_thread(&self) -> Option<&TCB> {
        self.current_tid.and_then(|tid| self.get_thread(tid))
    }

    /// Get current thread mutably
    pub fn current_thread_mut(&mut self) -> Option<&mut TCB> {
        let tid = self.current_tid?;
        self.get_thread_mut(tid)
    }

    /// Set process state
    pub fn set_state(&mut self, state: ProcessState) {
        self.state = state;
    }

    /// Block the process
    pub fn block(&mut self, reason: BlockReason) {
        self.state = ProcessState::Blocked(reason.clone());
        // Block all threads
        for tcb in self.threads.values_mut() {
            tcb.block(reason.clone());
        }
    }

    /// Unblock the process
    pub fn unblock(&mut self) {
        if matches!(self.state, ProcessState::Blocked(_)) {
            self.state = ProcessState::Ready;
            // Unblock all threads
            for tcb in self.threads.values_mut() {
                tcb.unblock();
            }
        }
    }

    /// Terminate the process
    pub fn terminate(&mut self, exit_code: i32) {
        self.state = ProcessState::Terminated { exit_code };
        self.exit_code = Some(exit_code);
        // Terminate all threads
        for tcb in self.threads.values_mut() {
            tcb.state = ThreadState::Terminated;
        }
    }

    /// Check if process is alive
    pub fn is_alive(&self) -> bool {
        self.state.is_alive()
    }

    /// Check if process is runnable
    pub fn is_runnable(&self) -> bool {
        self.state == ProcessState::Ready && !self.threads.is_empty()
    }

    /// Check if process is blocked
    pub fn is_blocked(&self) -> bool {
        self.state.is_blocked()
    }

    /// Check if process is terminated
    pub fn is_terminated(&self) -> bool {
        self.state.is_terminated()
    }

    /// Add a message to the message queue
    pub fn enqueue_message(&mut self, msg: IPCMessage) {
        self.message_queue.push_back(msg);
    }

    /// Get the next message from the queue
    pub fn dequeue_message(&mut self) -> Option<IPCMessage> {
        self.message_queue.pop_front()
    }

    /// Peek at the next message without removing it
    pub fn peek_message(&self) -> Option<&IPCMessage> {
        self.message_queue.front()
    }

    /// Get message queue length
    pub fn message_count(&self) -> usize {
        self.message_queue.len()
    }

    /// Add a pending signal
    pub fn add_signal(&mut self, signal: SignalType) {
        self.pending_signals.push(signal);
    }

    /// Get next pending signal
    pub fn take_signal(&mut self) -> Option<SignalType> {
        self.pending_signals.pop()
    }

    /// Allocate a new file descriptor
    pub fn allocate_fd(&mut self) -> u32 {
        let fd = self.next_fd;
        self.next_fd += 1;
        fd
    }

    /// Add a file descriptor
    pub fn add_fd(&mut self, fd: u32, file_desc: FileDescriptor) {
        self.file_descriptors.insert(fd, file_desc);
    }

    /// Get a file descriptor
    pub fn get_fd(&self, fd: u32) -> Option<&FileDescriptor> {
        self.file_descriptors.get(&fd)
    }

    /// Remove a file descriptor
    pub fn remove_fd(&mut self, fd: u32) -> Option<FileDescriptor> {
        self.file_descriptors.remove(&fd)
    }

    /// Get process uptime
    pub fn uptime(&self) -> Duration {
        self.created_at
            .elapsed()
            .unwrap_or(Duration::ZERO)
    }

    /// Get thread count
    pub fn thread_count(&self) -> usize {
        self.threads.len()
    }

    /// Get active (non-terminated) thread count
    pub fn active_thread_count(&self) -> usize {
        self.threads.values()
            .filter(|t| !matches!(t.state, ThreadState::Terminated))
            .count()
    }
}

/// File descriptor information
#[derive(Debug, Clone, PartialEq)]
pub struct FileDescriptor {
    /// File path or device name
    pub path: String,

    /// File descriptor flags
    pub flags: u32,

    /// Current offset
    pub offset: u64,

    /// Whether it's open for reading
    pub read: bool,

    /// Whether it's open for writing
    pub write: bool,

    /// File type (file, directory, device, etc.)
    pub fd_type: FDType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, )]
pub enum FDType {
    RegularFile,
    Directory,
    CharDevice,
    BlockDevice,
    Pipe,
    Socket,
}

/// Process statistics
#[derive(Debug, Clone, )]
pub struct ProcessStats {
    /// Total CPU time used (in nanoseconds)
    pub cpu_time: u64,

    /// Number of times process was scheduled
    pub schedule_count: u64,

    /// Number of voluntary context switches
    pub voluntary_context_switches: u64,

    /// Number of involuntary context switches
    pub involuntary_context_switches: u64,

    /// Maximum resident set size (in bytes)
    pub max_rss: usize,

    /// Peak memory usage
    pub peak_memory: usize,

    /// Number of page faults
    pub page_faults: u64,

    /// Number of signals received
    pub signals_received: u64,
}

impl Default for ProcessStats {
    fn default() -> Self {
        Self {
            cpu_time: 0,
            schedule_count: 0,
            voluntary_context_switches: 0,
            involuntary_context_switches: 0,
            max_rss: 0,
            peak_memory: 0,
            page_faults: 0,
            signals_received: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::BlockReason;

    #[test]
    fn test_pcb_creation() {
        let pcb = PCB::new(1, "test".to_string(), None);
        assert_eq!(pcb.pid, 1);
        assert_eq!(pcb.name, "test");
        assert!(pcb.parent_pid.is_none());
        assert_eq!(pcb.state, ProcessState::Creating);
        assert_eq!(pcb.thread_count(), 0);
    }

    #[test]
    fn test_pcb_with_parent() {
        let pcb = PCB::new(2, "child".to_string(), Some(1));
        assert_eq!(pcb.pid, 2);
        assert_eq!(pcb.parent_pid, Some(1));
    }

    #[test]
    fn test_add_thread() {
        let mut pcb = PCB::new(1, "test".to_string(), None);

        // Set state to Ready so current_tid will be set
        pcb.state = ProcessState::Ready;

        let tcb = TCB::new(1, 0x1000, 0xFFFF_FFFF_FFFF_F000);

        let tid = pcb.add_thread(tcb);
        assert_eq!(tid, 1);
        assert_eq!(pcb.thread_count(), 1);
        assert_eq!(pcb.main_tid, Some(1));
        assert_eq!(pcb.current_tid, Some(1));
    }

    #[test]
    fn test_process_state_transitions() {
        let mut pcb = PCB::new(1, "test".to_string(), None);

        // Add a thread so is_runnable() will work
        pcb.state = ProcessState::Ready;
        let tcb = TCB::new(1, 0x1000, 0xFFFF_FFFF_FFFF_F000);
        pcb.add_thread(tcb);

        // Creating -> Ready
        pcb.set_state(ProcessState::Ready);
        assert!(pcb.is_runnable());

        // Ready -> Blocked
        pcb.block(BlockReason::Sleeping { duration_ms: 100 });
        assert!(pcb.is_blocked());

        // Blocked -> Ready
        pcb.unblock();
        assert!(pcb.is_runnable());

        // Ready -> Terminated
        pcb.terminate(0);
        assert!(pcb.is_terminated());
        assert_eq!(pcb.exit_code, Some(0));
    }

    #[test]
    fn test_message_queue() {
        let mut pcb = PCB::new(1, "test".to_string(), None);

        // Initially empty
        assert_eq!(pcb.message_count(), 0);
        assert!(pcb.peek_message().is_none());

        // Add messages
        let msg1 = IPCMessage::Text { data: "Hello".to_string() };
        let msg2 = IPCMessage::Text { data: "World".to_string() };

        pcb.enqueue_message(msg1.clone());
        pcb.enqueue_message(msg2.clone());

        assert_eq!(pcb.message_count(), 2);

        // Peek doesn't remove
        assert_eq!(pcb.peek_message(), Some(&msg1));
        assert_eq!(pcb.message_count(), 2);

        // Dequeue removes
        assert_eq!(pcb.dequeue_message(), Some(msg1));
        assert_eq!(pcb.dequeue_message(), Some(msg2));
        assert_eq!(pcb.message_count(), 0);
    }

    #[test]
    fn test_signals() {
        let mut pcb = PCB::new(1, "test".to_string(), None);

        pcb.add_signal(SignalType::Terminate);
        pcb.add_signal(SignalType::User1);

        assert_eq!(pcb.take_signal(), Some(SignalType::User1)); // LIFO
        assert_eq!(pcb.take_signal(), Some(SignalType::Terminate));
        assert!(pcb.take_signal().is_none());
    }

    #[test]
    fn test_file_descriptors() {
        let mut pcb = PCB::new(1, "test".to_string(), None);

        // Allocate FDs
        let fd1 = pcb.allocate_fd();
        let fd2 = pcb.allocate_fd();

        assert_eq!(fd1, 3); // Starts at 3
        assert_eq!(fd2, 4);

        // Add file descriptor
        let file_desc = FileDescriptor {
            path: "/test".to_string(),
            flags: 0,
            offset: 0,
            read: true,
            write: false,
            fd_type: FDType::RegularFile,
        };

        pcb.add_fd(fd1, file_desc.clone());
        assert_eq!(pcb.get_fd(fd1), Some(&file_desc));
        assert!(pcb.get_fd(fd2).is_none());

        // Remove file descriptor
        assert_eq!(pcb.remove_fd(fd1), Some(file_desc));
        assert!(pcb.get_fd(fd1).is_none());
    }

    #[test]
    fn test_tcb_state_transitions() {
        let mut tcb = TCB::new(1, 0x1000, 0xFFFF_FFFF_FFFF_F000);

        // Creating -> Ready
        tcb.set_state(ThreadState::Ready);
        assert!(tcb.is_runnable());

        // Ready -> Blocked
        tcb.block(BlockReason::Sleeping { duration_ms: 100 });
        assert!(tcb.is_blocked());
        assert_eq!(tcb.block_reason, Some(BlockReason::Sleeping { duration_ms: 100 }));

        // Blocked -> Ready
        tcb.unblock();
        assert!(tcb.is_runnable());
        assert!(tcb.block_reason.is_none());
    }

    #[test]
    fn test_cpu_state_save_restore() {
        let mut tcb = TCB::new(1, 0x1000, 0xFFFF_FFFF_FFFF_F000);

        // Initially no CPU state
        assert!(tcb.take_cpu_state().is_none());

        // Save CPU state
        let cpu_state = CPUState {
            registers: [1, 2, 3, 4],
            pc: 0x1000,
            sp: 0xFFFF_FFFF_FFFF_F000,
            flags: crate::hardware::CPUFlags::new(),
            halted: false,
            current_pid: 1,
            instruction_count: 100,
        };

        tcb.save_cpu_state(cpu_state.clone());
        assert!(tcb.cpu_state.is_some());

        // Restore CPU state
        let restored = tcb.take_cpu_state().unwrap();
        assert_eq!(restored.registers, cpu_state.registers);
        assert_eq!(restored.pc, cpu_state.pc);
        assert!(tcb.cpu_state.is_none());
    }

    #[test]
    fn test_multiple_threads() {
        let mut pcb = PCB::new(1, "test".to_string(), None);

        let tcb1 = TCB::new(1, 0x1000, 0xFFFF_FFFF_FFFF_F000);
        let tcb2 = TCB::new(2, 0x2000, 0xFFFF_FFFF_FFFF_E000);
        let tcb3 = TCB::new(3, 0x3000, 0xFFFF_FFFF_FFFF_D000);

        pcb.add_thread(tcb1);
        pcb.add_thread(tcb2);
        pcb.add_thread(tcb3);

        assert_eq!(pcb.thread_count(), 3);
        assert_eq!(pcb.main_tid, Some(1)); // First thread is main

        // Get threads
        assert!(pcb.get_thread(1).is_some());
        assert!(pcb.get_thread(2).is_some());
        assert!(pcb.get_thread(99).is_none());

        // Remove thread
        let removed = pcb.remove_thread(2);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().tid, 2);
        assert_eq!(pcb.thread_count(), 2);
    }

    #[test]
    fn test_process_uptime() {
        let pcb = PCB::new(1, "test".to_string(), None);

        // Uptime should be very small (just created)
        let uptime = pcb.uptime();
        assert!(uptime.as_millis() < 100);
    }

    #[test]
    fn test_environment_variables() {
        let mut pcb = PCB::new(1, "test".to_string(), None);

        pcb.env.insert("PATH".to_string(), "/bin:/usr/bin".to_string());
        pcb.env.insert("HOME".to_string(), "/root".to_string());

        assert_eq!(pcb.env.get("PATH"), Some(&"/bin:/usr/bin".to_string()));
        assert_eq!(pcb.env.len(), 2);
    }

    #[test]
    fn test_zombie_state() {
        let mut pcb = PCB::new(1, "test".to_string(), None);
        let parent_pid = Some(0);

        // Create child process
        let mut child = PCB::new(2, "child".to_string(), Some(1));

        // Terminate but don't wait yet (becomes zombie)
        child.terminate(42);
        child.state = ProcessState::Zombie { exit_code: 42 };

        assert!(child.is_terminated());
        assert_eq!(child.exit_code, Some(42));
    }
}
