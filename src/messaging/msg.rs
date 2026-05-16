// KernelMsg: Central enumeration for all inter-module communication
//
// All messages in genshin-os flow through this enum, following the
// fire-and-forget pattern. The message bus routes each variant to
// the appropriate service layer.

use std::fmt;

/// Process identifier type
pub type Pid = u64;

/// Thread identifier type
pub type Tid = u64;

/// Virtual address type
pub type VirtAddr = u64;

/// Physical address type
pub type PhysAddr = u64;

/// Core kernel message enumeration
///
/// This enum represents ALL possible messages that can flow between
/// layers in the system. Each variant is routed by the message bus
/// to the appropriate service.
#[derive(Debug, Clone, PartialEq)]
pub enum KernelMsg {
    /// System call requests from user space
    Syscall(Syscall),

    /// Hardware interrupts from the hardware layer
    Interrupt(Interrupt),

    /// Process service requests (PCB/TCB management)
    Process(ProcessRequest),

    /// Memory/Storage service requests (paging, swap, allocation)
    Memory(MemoryRequest),

    /// File system service requests
    File(FileRequest),

    /// Device I/O service requests
    Device(DeviceRequest),
}

/// System call requests from user processes
#[derive(Debug, Clone, PartialEq)]
pub enum Syscall {
    /// Create a new process
    /// Arguments: (executable_path, args)
    CreateProcess { executable: String, args: Vec<String> },

    /// Terminate current process
    ExitProcess { exit_code: i32 },

    /// Create a new thread
    CreateThread { entry_point: VirtAddr },

    /// Read from file descriptor
    /// Arguments: (fd, buffer_address, size)
    Read { fd: u32, buf: VirtAddr, size: usize },

    /// Write to file descriptor
    /// Arguments: (fd, buffer_address, size)
    Write { fd: u32, buf: VirtAddr, size: usize },

    /// Allocate memory
    /// Arguments: (size, permissions)
    Mmap { size: usize, prot: MemProt },

    /// Free memory
    /// Arguments: (virtual_address, size)
    Munmap { addr: VirtAddr, size: usize },
}

/// Memory protection flags
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemProt {
    pub readable: bool,
    pub writable: bool,
    pub executable: bool,
}

impl MemProt {
    pub const fn new() -> Self {
        Self {
            readable: false,
            writable: false,
            executable: false,
        }
    }

    pub const fn read_only() -> Self {
        Self {
            readable: true,
            writable: false,
            executable: false,
        }
    }

    pub const fn read_write() -> Self {
        Self {
            readable: true,
            writable: true,
            executable: false,
        }
    }

    pub const fn execute() -> Self {
        Self {
            readable: false,
            writable: false,
            executable: true,
        }
    }
}

impl Default for MemProt {
    fn default() -> Self {
        Self::new()
    }
}

/// Hardware interrupts from the hardware simulation layer
#[derive(Debug, Clone, PartialEq)]
pub enum Interrupt {
    /// Timer interrupt (scheduler tick)
    Timer,

    /// Page fault (no mapping or permission denied)
    PageFault { addr: VirtAddr, access_type: AccessType },

    /// I/O interrupt (device completed operation)
    IoComplete { device_id: u32 },

    /// System call interrupt (trap into kernel)
    SyscallTrap,

    /// Hardware failure
    HardwareFailure { component: String },
}

/// Memory access type for page faults
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessType {
    Read,
    Write,
    Execute,
}

/// Process service requests
#[derive(Debug, Clone, PartialEq)]
pub enum ProcessRequest {
    /// Schedule a process/thread
    Schedule { pid: Pid, tid: Tid },

    /// Block a process/thread (waiting for event)
    Block { pid: Pid, tid: Tid, reason: BlockReason },

    /// Unblock a process/thread
    Unblock { pid: Pid, tid: Tid },

    /// Query process state
    QueryState { pid: Pid },

    /// Context switch request
    ContextSwitch { from_pid: Pid, to_pid: Pid },

    // ========== IPC: Message Passing ==========
    /// Send message to another process
    SendMessage {
        from_pid: Pid,
        to_pid: Pid,
        msg: IPCMessage,
    },

    /// Receive message (blocking or non-blocking)
    ReceiveMessage {
        pid: Pid,
        blocking: bool,
    },

    /// Get next message from process mailbox
    /// Response includes message or indicates no message available
    PeekMessage { pid: Pid },

    // ========== IPC: Shared Memory ==========
    /// Create shared memory region
    CreateSharedMemory {
        pid: Pid,
        size: usize,
        prot: MemProt,
    },

    /// Attach to existing shared memory region
    AttachSharedMemory {
        pid: Pid,
        shmid: u64,  // shared memory ID
    },

    /// Detach from shared memory region
    DetachSharedMemory {
        pid: Pid,
        shmid: u64,
    },

    // ========== IPC: Synchronization ==========
    /// Create semaphore
    CreateSemaphore {
        pid: Pid,
        initial_value: u32,
    },

    /// Wait on semaphore (P operation)
    WaitSemaphore {
        pid: Pid,
        semid: u64,
    },

    /// Signal semaphore (V operation)
    SignalSemaphore {
        pid: Pid,
        semid: u64,
    },

    /// Create mutex lock
    CreateLock {
        pid: Pid,
    },

    /// Acquire lock
    AcquireLock {
        pid: Pid,
        lock_id: u64,
    },

    /// Release lock
    ReleaseLock {
        pid: Pid,
        lock_id: u64,
    },

    // ========== Process Lifecycle ==========
    /// Fork a new process (duplicate current process)
    ForkProcess {
        parent_pid: Pid,
    },

    /// Execute a new program in current process
    ExecProcess {
        pid: Pid,
        executable: String,
        args: Vec<String>,
        path: Option<String>,
    },

    /// Wait for child process to exit
    WaitChild {
        pid: Pid,
        child_pid: Option<Pid>,  // None = wait for any child
    },

    /// Send signal to process
    Signal {
        pid: Pid,
        signal: SignalType,
    },

    /// Query process info (for debugging/monitoring)
    GetProcessInfo { pid: Pid },

    /// Spawn a program on the CPU
    Spawn { program: String, params: Vec<u8> },

    /// List all processes
    ListProcesses,

    /// Get system stats for TUI monitor
    GetStats,
}

/// Reason for a thread being blocked
#[derive(Debug, Clone, PartialEq)]
pub enum BlockReason {
    WaitingForIo { device_id: u32 },
    WaitingForMemory,
    WaitingForLock { lock_addr: VirtAddr },
    Sleeping { duration_ms: u64 },
    WaitingForChild { pid: Pid },
}

/// Memory and memory service requests
#[derive(Debug, Clone, PartialEq)]
pub enum MemoryRequest {
    /// Allocate physical memory frame
    AllocFrame { count: usize },

    /// Free physical memory frame
    FreeFrame { paddr: PhysAddr },

    /// Create page table mapping
    MapPage {
        pid: Pid,
        virt: VirtAddr,
        phys: PhysAddr,
        prot: MemProt,
    },

    /// Remove page table mapping
    UnmapPage { pid: Pid, virt: VirtAddr },

    /// Handle page fault (bring page from disk)
    PageFaultHandler {
        pid: Pid,
        faulting_addr: VirtAddr,
        access_type: AccessType,
    },

    /// Swap out page to disk
    SwapOut { pid: Pid, virt: VirtAddr },

    /// Swap in page from disk
    SwapIn { pid: Pid, virt: VirtAddr },

    /// Get per-frame ownership map for memory visualization
    GetFrameMap,

    /// Get memory stats for TUI monitor
    GetStats,
}

/// File system service requests
#[derive(Debug, Clone, PartialEq)]
pub enum FileRequest {
    // ========== File Operations ==========
    /// Open file
    Open { path: String, flags: OpenFlags },

    /// Close file descriptor
    Close { fd: u32 },

    /// Read from file
    Read {
        fd: u32,
        offset: u64,
        buf: VirtAddr,
        size: usize,
    },

    /// Write to file
    Write {
        fd: u32,
        offset: u64,
        buf: VirtAddr,
        size: usize,
    },

    /// Write data directly (simulation-friendly)
    WriteData { fd: u32, data: Vec<u8> },
    /// Delete file
    Unlink { path: String },

    /// Query file metadata
    Stat { path: String },

    // ========== Directory Operations ==========
    /// Create directory
    CreateDirectory { path: String },

    /// Remove directory
    RemoveDirectory { path: String },

    /// Open directory for listing
    OpenDirectory { path: String },

    /// Read directory entry
    ReadDirectory { dir_fd: u32 },

    /// Close directory
    CloseDirectory { dir_fd: u32 },

    /// List directory entries
    ListDir { path: String },

    /// Query disk info
    DiskInfo,

    // ========== File System Management ==========
    /// Mount file system
    Mount {
        device_id: u32,
        mount_point: String,
        fs_type: FileSystemType,
    },

    /// Unmount file system
    Unmount { mount_point: String },

    /// Sync file system buffers
    Sync,

    // ========== File Metadata ==========
    /// Change file permissions
    Chmod { path: String, mode: u32 },

    /// Change file owner
    Chown { path: String, uid: u32, gid: u32 },

    /// Create hard link
    Link { oldpath: String, newpath: String },

    /// Create symbolic link
    Symlink { oldpath: String, newpath: String },

    /// Read symbolic link
    Readlink { path: String },

    // ========== File Position ==========
    /// Seek to position in file
    Seek {
        fd: u32,
        offset: i64,
        whence: SeekWhence,
    },

    /// Get current file position
    Tell { fd: u32 },

    // ========== Process Integration ==========
    /// Clone all file descriptors from one process to another (for fork)
    CloneFds { from_pid: Pid, to_pid: Pid },
}

/// File system type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileSystemType {
    /// FAT32 file system
    FAT32,
    /// ext4 file system
    EXT4,
    /// Simple file system (for educational purposes)
    SimpleFS,
    /// proc file system
    ProcFS,
    /// Unknown
    Unknown,
}

/// Seek origin
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeekWhence {
    /// Seek from beginning
    Set = 0,
    /// Seek from current position
    Cur = 1,
    /// Seek from end
    End = 2,
}

/// File open flags
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenFlags {
    pub read: bool,
    pub write: bool,
    pub create: bool,
    pub truncate: bool,
    pub append: bool,
}

impl OpenFlags {
    pub const fn read_only() -> Self {
        Self {
            read: true,
            write: false,
            create: false,
            truncate: false,
            append: false,
        }
    }

    pub const fn write_only() -> Self {
        Self {
            read: false,
            write: true,
            create: false,
            truncate: false,
            append: false,
        }
    }

    pub const fn read_write() -> Self {
        Self {
            read: true,
            write: true,
            create: false,
            truncate: false,
            append: false,
        }
    }

    pub const fn create() -> Self {
        Self {
            read: false,
            write: true,
            create: true,
            truncate: false,
            append: false,
        }
    }
}

/// Device service requests
#[derive(Debug, Clone, PartialEq)]
pub enum DeviceRequest {
    // ========== Basic I/O ==========
    /// Read from device
    Read {
        device_id: u32,
        buf: VirtAddr,
        size: usize,
    },

    /// Write to device
    Write {
        device_id: u32,
        buf: VirtAddr,
        size: usize,
    },

    // ========== Device Lifecycle ==========
    /// Initialize device
    Init { device_id: u32 },

    /// Shutdown device
    Shutdown { device_id: u32 },

    /// Reset device
    Reset { device_id: u32 },

    /// Query device status
    Status { device_id: u32 },

    // ========== Device Management ==========
    /// Register new device
    RegisterDevice {
        device_type: DeviceClass,
        name: String,
    },

    /// Unregister device
    UnregisterDevice { device_id: u32 },

    /// List all devices
    ListDevices,

    /// Get devices by type
    GetDevicesByType { device_type: DeviceClass },

    // ========== Device I/O Control ==========
    /// I/O control command (device-specific)
    Ioctl {
        device_id: u32,
        request: u32,
        arg: VirtAddr,
    },

    // ========== Device Interrupts ==========
    /// Enable device interrupt
    EnableInterrupt { device_id: u32, irq: u32 },

    /// Disable device interrupt
    DisableInterrupt { device_id: u32, irq: u32 },

    // ========== Device Configuration ==========
    /// Set device configuration
    SetConfig {
        device_id: u32,
        config: DeviceConfig,
    },

    /// Get device configuration
    GetConfig { device_id: u32 },

    // ========== Clipboard ==========
    ClipboardSet { data: Vec<u8> },
    ClipboardGet { max_size: usize },
}

/// Device class/category
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceClass {
    Block,
    Char,
    Network,
    Timer,
    Graphics,
    Clipboard,
    Unknown,
}

/// Device configuration
#[derive(Debug, Clone, PartialEq)]
pub enum DeviceConfig {
    /// Serial port configuration
    Serial {
        baud_rate: u32,
        data_bits: u8,
        stop_bits: u8,
        parity: char,
    },

    /// Network interface configuration
    Network {
        ip_address: String,
        netmask: String,
        gateway: String,
        mac_address: [u8; 6],
    },

    /// Block device configuration
    Block {
        block_size: u32,
        read_only: bool,
    },

    /// Generic key-value configuration
    Generic { key: String, value: String },
}

/// IPC message type for process communication
///
/// Carries actual message payload between processes.
/// All IPC messages flow through the message bus for monitoring.
#[derive(Debug, Clone, PartialEq)]
pub enum IPCMessage {
    /// Simple text message
    Text { data: String },

    /// Binary data message
    Binary {
        addr: VirtAddr,  // Address in sender's memory space
        size: usize,
    },

    /// File descriptor passing
    PassFd { fd: u32 },

    /// Shared memory notification
    SharedMemory { shmid: u64 },

    /// Synchronization notification
    Signal { signal: SignalType },

    /// Control message
    Control {
        cmd: String,
        args: Vec<String>,
        path: Option<String>,
    },
}

/// Signal types for process notification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalType {
    /// Terminate process (SIGTERM)
    Terminate = 15,

    /// Kill process immediately (SIGKILL)
    Kill = 9,

    /// Stop process (SIGSTOP)
    Stop = 19,

    /// Continue stopped process (SIGCONT)
    Continue = 18,

    /// User-defined signal 1 (SIGUSR1)
    User1 = 10,

    /// User-defined signal 2 (SIGUSR2)
    User2 = 12,

    /// Alarm signal (SIGALRM)
    Alarm = 14,

    /// Child process stopped/exited (SIGCHLD)
    Child = 17,

    /// Segment violation (SIGSEGV)
    SegmentationFault = 11,

    /// Illegal instruction (SIGILL)
    IllegalInstruction = 4,

    /// Floating point exception (SIGFPE)
    FloatingPointException = 8,
}

impl fmt::Display for KernelMsg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KernelMsg::Syscall(s) => write!(f, "Syscall: {:?}", s),
            KernelMsg::Interrupt(i) => write!(f, "Interrupt: {:?}", i),
            KernelMsg::Process(p) => write!(f, "Process: {:?}", p),
            KernelMsg::Memory(m) => write!(f, "Memory: {:?}", m),
            KernelMsg::File(fi) => write!(f, "File: {:?}", fi),
            KernelMsg::Device(d) => write!(f, "Device: {:?}", d),
        }
    }
}

impl fmt::Display for IPCMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IPCMessage::Text { data } => write!(f, "Text({})", data),
            IPCMessage::Binary { addr, size } => {
                write!(f, "Binary(addr={:#x}, size={})", addr, size)
            }
            IPCMessage::PassFd { fd } => write!(f, "PassFd({})", fd),
            IPCMessage::SharedMemory { shmid } => write!(f, "SharedMemory({})", shmid),
            IPCMessage::Signal { signal } => write!(f, "Signal({:?})", signal),
            IPCMessage::Control { cmd, .. } => write!(f, "Control({})", cmd),
        }
    }
}

impl fmt::Display for SignalType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SignalType::Terminate => write!(f, "SIGTERM"),
            SignalType::Kill => write!(f, "SIGKILL"),
            SignalType::Stop => write!(f, "SIGSTOP"),
            SignalType::Continue => write!(f, "SIGCONT"),
            SignalType::User1 => write!(f, "SIGUSR1"),
            SignalType::User2 => write!(f, "SIGUSR2"),
            SignalType::Alarm => write!(f, "SIGALRM"),
            SignalType::Child => write!(f, "SIGCHLD"),
            SignalType::SegmentationFault => write!(f, "SIGSEGV"),
            SignalType::IllegalInstruction => write!(f, "SIGILL"),
            SignalType::FloatingPointException => write!(f, "SIGFPE"),
        }
    }
}
