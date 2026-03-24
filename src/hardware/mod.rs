// Hardware Simulation Layer
//
// This layer simulates hardware components following these principles:
// 1. Hardware reports only, does not decide: All anomalies are reported via KernelMsg
// 2. Single communication path: All components use Arc<dyn MessageBus>
// 3. Authentic simulation: Complete fetch-decode-execute-interrupt cycle

mod memory;
mod disk;
mod mmu;
mod timer;
mod cpu;
mod ivt;
mod block;
mod device;

pub use memory::{PhysicalMemory, MemoryState};
pub use disk::{VirtualDisk, DiskState};
pub use mmu::{MMU, PageTableEntry, PageFlags, MMUState};
pub use timer::{Timer, TimerConfig, TimerSnapshot};
pub use cpu::{VirtualCPU, Register, Instruction, CPUFlags, CPUState};
pub use ivt::{InterruptVector, InterruptType, IVT};

// Block device support for file systems
pub use block::{
    BlockDevice, PhysicalBlockDevice, PartitionDevice,
    Partition, PartitionType, PartitionLayout,
    SECTOR_SIZE, BLOCK_SIZE,
};

// Generic device support for device manager
pub use device::{
    Device, DeviceType, DeviceStatus, DeviceSnapshot,
    DeviceRegistry,
    KeyboardDevice, SerialDevice, NetworkDevice,
};
