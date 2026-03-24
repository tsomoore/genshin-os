// Message bus and kernel message types
//
// This module defines the core communication contract for genshin-os.
// All inter-module communication MUST use KernelMsg via MessageBus.

mod msg;
mod bus;
mod response;

pub use msg::{KernelMsg, Syscall, Interrupt};
pub use msg::{VirtAddr, PhysAddr, Pid, Tid, MemProt, AccessType};
pub use msg::{ProcessRequest, BlockReason, IPCMessage, SignalType};
pub use msg::{MemoryRequest};
pub use msg::{FileRequest, OpenFlags, FileSystemType, SeekWhence};
pub use msg::{DeviceRequest, DeviceClass, DeviceConfig};
pub use bus::{MessageBus, LockedBus, BusError, DirectBus, DirectBusSender, DirectBusReceiver};

// Response mechanism
pub use response::{
    RequestId, Response, ResponseData, ServiceError,
    RequestWithResponse, KernelMessage,
    generate_request_id,
};
