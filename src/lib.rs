// genshin-os: A microkernel simulation in Rust
//
// Architecture: 4-layer design with async message bus communication
// - UI Layer: CLI Shell and TUI monitor
// - Exchange Layer: Kernel message bus (LockedBus/LockFreeBus)
// - Service Layer: Process/Storage/File/Device services
// - Hardware Layer: Virtual CPU/MMU/Disk/Timer

pub mod messaging;
pub mod hardware;
pub mod error;

// Re-export core types for convenience
pub use messaging::{
    KernelMsg, MessageBus, LockedBus,
    Pid, Tid, VirtAddr, PhysAddr,
    Syscall, Interrupt, ProcessRequest, MemoryRequest,
    FileRequest, DeviceRequest,
    MemProt, OpenFlags, AccessType, BlockReason,
    IPCMessage, SignalType,
    FileSystemType, SeekWhence, DeviceClass, DeviceConfig,
    RequestWithResponse, Response, ResponseData,
};
pub use hardware::{
    PhysicalMemory, VirtualDisk, MMU, Timer, VirtualCPU,
    PageTableEntry, PageFlags,
    InterruptVector, InterruptType, IVT,
    MemoryState, DiskState, MMUState, TimerSnapshot, CPUState,
    // Block device support for file systems
    BlockDevice, PhysicalBlockDevice, PartitionDevice,
    Partition, PartitionType, PartitionLayout,
    SECTOR_SIZE as DISK_SECTOR_SIZE, BLOCK_SIZE as DISK_BLOCK_SIZE,
    // Generic device support for device manager
    Device, DeviceType, DeviceStatus, DeviceSnapshot, DeviceRegistry,
    KeyboardDevice, SerialDevice, NetworkDevice,
};
pub use error::{
    GenshinError, HardwareError, ServiceError, BusError,
    MemoryError, DiskError, MMUError, CPUError, TimerError,
    AccessType as ErrorAccessType, PageFlags as ErrorPageFlags,
    Result as GenshinResult,
};
