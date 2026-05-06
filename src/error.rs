// Unified error handling for genshin-os
//
// This module provides a centralized error type hierarchy that can be
// used across all layers of the system.

use std::fmt;

/// Unified error type for genshin-os
///
/// This enum encompasses all possible errors that can occur in the system,
/// from hardware failures to service errors.
#[derive(Debug, Clone, PartialEq)]
pub enum GenshinError {
    /// Hardware layer errors
    Hardware(HardwareError),

    /// Service layer errors
    Service(ServiceError),

    /// Message bus errors
    Bus(BusError),

    /// Generic errors
    Other {
        code: u32,
        message: String,
    },
}

impl fmt::Display for GenshinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hardware(err) => write!(f, "[Hardware] {}", err),
            Self::Service(err) => write!(f, "[Service] {}", err),
            Self::Bus(err) => write!(f, "[Bus] {}", err),
            Self::Other { code, message } => {
                write!(f, "[Error {}] {}", code, message)
            }
        }
    }
}

impl std::error::Error for GenshinError {}

/// Hardware layer errors
///
/// These errors originate from the hardware simulation layer.
#[derive(Debug, Clone, PartialEq)]
pub enum HardwareError {
    /// Memory errors
    Memory(MemoryError),

    /// Disk errors
    Disk(DiskError),

    /// MMU errors
    MMU(MMUError),

    /// CPU errors
    CPU(CPUError),

    /// Timer errors
    Timer(TimerError),

    /// Device errors
    Device {
        device_id: u32,
        error: String,
    },
}

impl fmt::Display for HardwareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Memory(err) => write!(f, "Memory: {}", err),
            Self::Disk(err) => write!(f, "Disk: {}", err),
            Self::MMU(err) => write!(f, "MMU: {}", err),
            Self::CPU(err) => write!(f, "CPU: {}", err),
            Self::Timer(err) => write!(f, "Timer: {}", err),
            Self::Device { device_id, error } => {
                write!(f, "Device {}: {}", device_id, error)
            }
        }
    }
}

impl std::error::Error for HardwareError {}

impl From<MemoryError> for HardwareError {
    fn from(err: MemoryError) -> Self {
        Self::Memory(err)
    }
}

impl From<DiskError> for HardwareError {
    fn from(err: DiskError) -> Self {
        Self::Disk(err)
    }
}

impl From<MMUError> for HardwareError {
    fn from(err: MMUError) -> Self {
        Self::MMU(err)
    }
}

impl From<CPUError> for HardwareError {
    fn from(err: CPUError) -> Self {
        Self::CPU(err)
    }
}

impl From<TimerError> for HardwareError {
    fn from(err: TimerError) -> Self {
        Self::Timer(err)
    }
}

impl From<HardwareError> for GenshinError {
    fn from(err: HardwareError) -> Self {
        Self::Hardware(err)
    }
}

impl From<ServiceError> for GenshinError {
    fn from(err: ServiceError) -> Self {
        Self::Service(err)
    }
}

impl From<BusError> for GenshinError {
    fn from(err: BusError) -> Self {
        Self::Bus(err)
    }
}

/// Memory errors
#[derive(Debug, Clone, PartialEq)]
pub enum MemoryError {
    /// Address out of bounds
    OutOfBounds {
        addr: usize,
        size: usize,
        max_size: usize,
    },

    /// Misaligned access
    Misaligned {
        addr: usize,
        required_alignment: usize,
    },

    /// Memory locked
    Locked,

    /// Allocation failed
    AllocationFailed {
        size: usize,
    },
}

impl fmt::Display for MemoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfBounds { addr, size, max_size } => {
                write!(f, "Out of bounds: addr={:#x}, size={}, max_size={:#x}",
                       addr, size, max_size)
            }
            Self::Misaligned { addr, required_alignment } => {
                write!(f, "Misaligned: addr={:#x}, must be aligned to {} bytes",
                       addr, required_alignment)
            }
            Self::Locked => write!(f, "Memory region is locked"),
            Self::AllocationFailed { size } => {
                write!(f, "Failed to allocate {} bytes", size)
            }
        }
    }
}

impl std::error::Error for MemoryError {}

/// Disk errors
#[derive(Debug, Clone, PartialEq)]
pub enum DiskError {
    /// Sector number out of range
    InvalidSector {
        sector: u32,
        max_sector: u32,
    },

    /// Read/write operation failed
    IoFailed {
        operation: String,
        sector: u32,
    },

    /// Disk is busy
    Busy,

    /// Disk is not ready
    NotReady,
}

impl fmt::Display for DiskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSector { sector, max_sector } => {
                write!(f, "Invalid sector {} (max {})", sector, max_sector)
            }
            Self::IoFailed { operation, sector } => {
                write!(f, "{} failed on sector {}", operation, sector)
            }
            Self::Busy => write!(f, "Disk is busy"),
            Self::NotReady => write!(f, "Disk is not ready"),
        }
    }
}

impl std::error::Error for DiskError {}

/// MMU errors
#[derive(Debug, Clone, PartialEq)]
pub enum MMUError {
    /// Page not present in page table
    PageNotPresent {
        pid: u64,
        vaddr: u64,
    },

    /// Permission denied for access
    PermissionDenied {
        pid: u64,
        vaddr: u64,
        access_type: AccessType,
        required: PageFlags,
    },

    /// Invalid physical address
    InvalidPhysicalAddress {
        paddr: u64,
    },

    /// Page table not found for process
    PageTableNotFound {
        pid: u64,
    },

    /// Page table entry invalid
    InvalidPageTableEntry {
        pid: u64,
        vaddr: u64,
    },
}

impl fmt::Display for MMUError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PageNotPresent { pid, vaddr } => {
                write!(f, "Page not present: pid={}, vaddr={:#x}", pid, vaddr)
            }
            Self::PermissionDenied { pid, vaddr, access_type, .. } => {
                write!(f, "Permission denied: pid={}, vaddr={:#x}, access={:?}",
                       pid, vaddr, access_type)
            }
            Self::InvalidPhysicalAddress { paddr } => {
                write!(f, "Invalid physical address: {:#x}", paddr)
            }
            Self::PageTableNotFound { pid } => {
                write!(f, "Page table not found for pid={}", pid)
            }
            Self::InvalidPageTableEntry { pid, vaddr } => {
                write!(f, "Invalid page table entry: pid={}, vaddr={:#x}", pid, vaddr)
            }
        }
    }
}

impl std::error::Error for MMUError {}

/// CPU errors
#[derive(Debug, Clone, PartialEq)]
pub enum CPUError {
    /// Invalid instruction at PC
    InvalidInstruction {
        pc: u64,
        opcode: u8,
    },

    /// Divide by zero exception
    DivideByZero {
        pc: u64,
    },

    /// Page fault during execution
    PageFault {
        vaddr: u64,
        access_type: AccessType,
    },

    /// Invalid register index
    InvalidRegister {
        index: usize,
    },

    /// CPU is halted
    Halted,

    /// Invalid operation mode
    InvalidMode {
        current: String,
        requested: String,
    },

    /// Stack overflow
    StackOverflow {
        sp: u64,
    },

    /// Stack underflow
    StackUnderflow {
        sp: u64,
    },
}

impl fmt::Display for CPUError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInstruction { pc, opcode } => {
                write!(f, "Invalid instruction at PC={:#x}, opcode={:#x}", pc, opcode)
            }
            Self::DivideByZero { pc } => {
                write!(f, "Divide by zero at PC={:#x}", pc)
            }
            Self::PageFault { vaddr, access_type } => {
                write!(f, "Page fault at vaddr={:#x}, access={:?}", vaddr, access_type)
            }
            Self::InvalidRegister { index } => {
                write!(f, "Invalid register index: {}", index)
            }
            Self::Halted => write!(f, "CPU is halted"),
            Self::InvalidMode { current, requested } => {
                write!(f, "Invalid mode: current={}, requested={}", current, requested)
            }
            Self::StackOverflow { sp } => {
                write!(f, "Stack overflow at SP={:#x}", sp)
            }
            Self::StackUnderflow { sp } => {
                write!(f, "Stack underflow at SP={:#x}", sp)
            }
        }
    }
}

impl std::error::Error for CPUError {}

/// Timer errors
#[derive(Debug, Clone, PartialEq)]
pub enum TimerError {
    /// Timer already running
    AlreadyRunning,

    /// Timer not running
    NotRunning,

    /// Invalid tick interval
    InvalidInterval {
        interval_ms: u64,
        min_ms: u64,
        max_ms: u64,
    },

    /// Timer initialization failed
    InitFailed,
}

impl fmt::Display for TimerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyRunning => write!(f, "Timer is already running"),
            Self::NotRunning => write!(f, "Timer is not running"),
            Self::InvalidInterval { interval_ms, min_ms, max_ms } => {
                write!(f, "Invalid interval {}ms (must be {}-{}ms)",
                       interval_ms, min_ms, max_ms)
            }
            Self::InitFailed => write!(f, "Timer initialization failed"),
        }
    }
}

impl std::error::Error for TimerError {}

/// Memory access type (for MMU errors)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessType {
    Read,
    Write,
    Execute,
}

impl fmt::Display for AccessType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read => write!(f, "Read"),
            Self::Write => write!(f, "Write"),
            Self::Execute => write!(f, "Execute"),
        }
    }
}

/// Page flags (for MMU errors)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageFlags {
    pub present: bool,
    pub writable: bool,
    pub user_accessible: bool,
}

impl fmt::Display for PageFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "P{}W{}U{}",
            if self.present { 1 } else { 0 },
            if self.writable { 1 } else { 0 },
            if self.user_accessible { 1 } else { 0 })
    }
}

/// Service layer errors
#[derive(Debug, Clone, PartialEq)]
pub enum ServiceError {
    /// Invalid arguments
    InvalidArguments {
        param: String,
        reason: String,
    },

    /// Resource not found
    NotFound {
        resource_type: String,
        id: String,
    },

    /// Permission denied
    PermissionDenied {
        operation: String,
        reason: String,
    },

    /// Resource exhausted
    ResourceExhausted {
        resource: String,
        available: usize,
        requested: usize,
    },

    /// I/O error
    Io {
        operation: String,
        details: String,
    },

    /// Operation timeout
    Timeout {
        operation: String,
        duration_ms: u64,
        timeout_ms: u64,
    },

    /// Not implemented
    NotImplemented {
        feature: String,
    },

    /// Generic service error
    Other {
        code: u32,
        msg: String,
    },
}

impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArguments { param, reason } => {
                write!(f, "Invalid argument '{}': {}", param, reason)
            }
            Self::NotFound { resource_type, id } => {
                write!(f, "{} not found: {}", resource_type, id)
            }
            Self::PermissionDenied { operation, reason } => {
                write!(f, "Permission denied for '{}': {}", operation, reason)
            }
            Self::ResourceExhausted { resource, available, requested } => {
                write!(f, "Resource exhausted: {} (available: {}, requested: {})",
                       resource, available, requested)
            }
            Self::Io { operation, details } => {
                write!(f, "I/O error during '{}': {}", operation, details)
            }
            Self::Timeout { operation, duration_ms, timeout_ms } => {
                write!(f, "Operation '{}' timed out after {}ms (timeout: {}ms)",
                       operation, duration_ms, timeout_ms)
            }
            Self::NotImplemented { feature } => {
                write!(f, "Not implemented: {}", feature)
            }
            Self::Other { code, msg } => {
                write!(f, "Error (code {}): {}", code, msg)
            }
        }
    }
}

impl std::error::Error for ServiceError {}

/// Message bus errors
#[derive(Debug, Clone, PartialEq)]
pub enum BusError {
    /// Bus disconnected
    Disconnected,

    /// Bus full
    Full,

    /// No receiver
    NoReceiver {
        message_type: String,
    },

    /// Send failed
    SendFailed {
        reason: String,
    },
}

impl fmt::Display for BusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disconnected => write!(f, "Bus disconnected"),
            Self::Full => write!(f, "Bus full"),
            Self::NoReceiver { message_type } => {
                write!(f, "No receiver for message type: {}", message_type)
            }
            Self::SendFailed { reason } => {
                write!(f, "Send failed: {}", reason)
            }
        }
    }
}

impl std::error::Error for BusError {}

// Import for crossbeam channel support
use crate::messaging::Response;

impl From<crossbeam_channel::SendError<Response>> for GenshinError {
    fn from(err: crossbeam_channel::SendError<Response>) -> Self {
        Self::Bus(BusError::SendFailed {
            reason: format!("Channel send error: {}", err),
        })
    }
}

/// Convenience type alias for Result
pub type Result<T> = std::result::Result<T, GenshinError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = MemoryError::OutOfBounds {
            addr: 0x1000,
            size: 4,
            max_size: 0x1000,
        };
        assert!(format!("{}", err).contains("Out of bounds"));
    }

    #[test]
    fn test_unified_error() {
        let hw_err = HardwareError::Memory(MemoryError::OutOfBounds {
            addr: 0x1000,
            size: 4,
            max_size: 0x1000,
        });
        let unified: GenshinError = hw_err.into();
        assert!(format!("{}", unified).contains("[Hardware]"));
    }

    #[test]
    fn test_service_error() {
        let err = ServiceError::NotFound {
            resource_type: "Process".to_string(),
            id: "123".to_string(),
        };
        assert!(format!("{}", err).contains("Process not found"));
    }

    #[test]
    fn test_error_chain() {
        let cpu_err = CPUError::DivideByZero { pc: 0x1000 };
        let hw_err = HardwareError::CPU(cpu_err);
        let unified: GenshinError = hw_err.into();

        assert!(format!("{}", unified).contains("[Hardware]"));
        assert!(format!("{}", unified).contains("CPU"));
        assert!(format!("{}", unified).contains("Divide by zero"));
    }

    #[test]
    fn test_bus_error() {
        let bus_err = BusError::Disconnected;
        let unified: GenshinError = bus_err.into();
        assert!(format!("{}", unified).contains("[Bus]"));
    }
}
