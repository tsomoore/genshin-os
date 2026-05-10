// Device Abstraction Module
//
// 曾国藩曰：
// "器利者，事必善。"
// 设备抽象定义统一接口，管理各类硬件设备。

use std::sync::{Arc, Mutex};
use crate::messaging::Pid;
use crate::{GenshinResult, GenshinError, ServiceError};

/// Device identifier
pub type DeviceId = u32;

/// Device type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceType {
    /// Character device (keyboard, serial, etc.)
    Character,

    /// Block device (disk, etc.)
    Block,

    /// Network device
    Network,

    /// Terminal device
    Terminal,

    /// Graphics device
    Graphics,

    /// Audio device
    Audio,

    /// Input device
    Input,

    /// Clipboard device
    Clipboard,

    /// Unknown device type
    Unknown,
}

/// Device status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceStatus {
    /// Device is being initialized
    Initializing,

    /// Device is ready for use
    Ready,

    /// Device is busy
    Busy,

    /// Device has an error
    Error,

    /// Device is disabled
    Disabled,

    /// Device was removed
    Removed,
}

/// Device operations trait
pub trait DeviceOperations: Send + Sync {
    /// Read from device
    fn read(&self, data: &mut [u8]) -> GenshinResult<usize>;

    /// Write to device
    fn write(&self, data: &[u8]) -> GenshinResult<usize>;

    /// Get device status
    fn status(&self) -> DeviceStatus;

    /// Reset device
    fn reset(&self) -> GenshinResult<()>;

    /// Get device info
    fn info(&self) -> DeviceInfo;

    /// Check if device is ready
    fn is_ready(&self) -> bool {
        self.status() == DeviceStatus::Ready
    }
}

/// Device information
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// Device name
    pub name: String,

    /// Device description
    pub description: String,

    /// Device type
    pub device_type: DeviceType,

    /// Device major number
    pub major: u32,

    /// Device minor number
    pub minor: u32,

    /// Device ID
    pub id: DeviceId,

    /// Parent device ID (if any)
    pub parent: Option<DeviceId>,

    /// Device status
    pub status: DeviceStatus,

    /// IO ports used by device
    pub io_ports: Vec<u16>,

    /// IRQ lines used by device
    pub irq_lines: Vec<u8>,

    /// Memory regions used by device
    pub memory_regions: Vec<(u64, usize)>, // (base address, size)
}

impl DeviceInfo {
    /// Create new device info
    pub fn new(
        name: String,
        description: String,
        device_type: DeviceType,
        major: u32,
        minor: u32,
        id: DeviceId,
    ) -> Self {
        Self {
            name,
            description,
            device_type,
            major,
            minor,
            id,
            parent: None,
            status: DeviceStatus::Initializing,
            io_ports: Vec::new(),
            irq_lines: Vec::new(),
            memory_regions: Vec::new(),
        }
    }

    /// Get full device name
    pub fn full_name(&self) -> String {
        format!("{}:{}", self.major, self.minor)
    }
}

/// Device - represents a hardware device
pub struct Device {
    /// Device information
    pub info: DeviceInfo,

    /// Device operations
    pub operations: Option<Box<dyn DeviceOperations>>,

    /// Open count (number of processes using this device)
    pub open_count: u32,

    /// Exclusive use flag
    pub exclusive: bool,

    /// Process that has exclusive access (if any)
    pub exclusive_owner: Option<Pid>,
}

impl std::fmt::Debug for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Device")
            .field("info", &self.info)
            .field("open_count", &self.open_count)
            .field("exclusive", &self.exclusive)
            .field("exclusive_owner", &self.exclusive_owner)
            .field("has_operations", &self.operations.is_some())
            .finish()
    }
}

impl Device {
    /// Create a new device
    pub fn new(info: DeviceInfo) -> Self {
        Self {
            info,
            operations: None,
            open_count: 0,
            exclusive: false,
            exclusive_owner: None,
        }
    }

    /// Set device operations
    pub fn set_operations(&mut self, ops: Box<dyn DeviceOperations>) {
        self.operations = Some(ops);
    }

    /// Read from device
    pub fn read(&self, data: &mut [u8]) -> GenshinResult<usize> {
        if let Some(ref ops) = self.operations {
            ops.read(data)
        } else {
            Err(GenshinError::Service(ServiceError::NotImplemented {
                feature: "Device read".to_string(),
            }))
        }
    }

    /// Write to device
    pub fn write(&self, data: &[u8]) -> GenshinResult<usize> {
        if let Some(ref ops) = self.operations {
            ops.write(data)
        } else {
            Err(GenshinError::Service(ServiceError::NotImplemented {
                feature: "Device write".to_string(),
            }))
        }
    }

    /// Get device status
    pub fn status(&self) -> DeviceStatus {
        if let Some(ref ops) = self.operations {
            ops.status()
        } else {
            self.info.status
        }
    }

    /// Reset device
    pub fn reset(&self) -> GenshinResult<()> {
        if let Some(ref ops) = self.operations {
            ops.reset()
        } else {
            Err(GenshinError::Service(ServiceError::NotImplemented {
                feature: "Device reset".to_string(),
            }))
        }
    }

    /// Get device info
    pub fn info(&self) -> &DeviceInfo {
        &self.info
    }

    /// Check if device is ready
    pub fn is_ready(&self) -> bool {
        self.status() == DeviceStatus::Ready
    }

    /// Open device
    pub fn open(&mut self, pid: Pid, exclusive: bool) -> GenshinResult<()> {
        // Check if device is ready
        if !self.is_ready() {
            return Err(GenshinError::Service(ServiceError::InvalidArguments {
                param: "device".to_string(),
                reason: "Device not ready".to_string(),
            }));
        }

        // Check exclusive access
        if self.exclusive {
            if self.exclusive_owner != Some(pid) {
                return Err(GenshinError::Service(ServiceError::PermissionDenied {
                    operation: "open".to_string(),
                    reason: "Device opened exclusively by another process".to_string(),
                }));
            }
        } else if exclusive {
            if self.open_count > 0 {
                return Err(GenshinError::Service(ServiceError::PermissionDenied {
                    operation: "open".to_string(),
                    reason: "Cannot open exclusively, already in use".to_string(),
                }));
            }
            self.exclusive = true;
            self.exclusive_owner = Some(pid);
        }

        self.open_count += 1;
        Ok(())
    }

    /// Close device
    pub fn close(&mut self, pid: Pid) -> GenshinResult<()> {
        if self.open_count == 0 {
            return Err(GenshinError::Service(ServiceError::InvalidArguments {
                param: "device".to_string(),
                reason: "Device not open".to_string(),
            }));
        }

        self.open_count -= 1;

        // Release exclusive access if this was the exclusive owner
        if self.exclusive && self.exclusive_owner == Some(pid) && self.open_count == 0 {
            self.exclusive = false;
            self.exclusive_owner = None;
        }

        Ok(())
    }

    /// Check if device is in use
    pub fn in_use(&self) -> bool {
        self.open_count > 0
    }

    /// Get open count
    pub fn open_count(&self) -> u32 {
        self.open_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_info_creation() {
        let info = DeviceInfo::new(
            "test".to_string(),
            "Test device".to_string(),
            DeviceType::Character,
            1,
            0,
            100,
        );

        assert_eq!(info.name, "test");
        assert_eq!(info.device_type, DeviceType::Character);
        assert_eq!(info.major, 1);
        assert_eq!(info.minor, 0);
        assert_eq!(info.id, 100);
    }

    #[test]
    fn test_device_full_name() {
        let info = DeviceInfo::new(
            "test".to_string(),
            "Test device".to_string(),
            DeviceType::Character,
            4,
            0,
            100,
        );

        assert_eq!(info.full_name(), "4:0");
    }

    #[test]
    fn test_device_creation() {
        let info = DeviceInfo::new(
            "test".to_string(),
            "Test device".to_string(),
            DeviceType::Character,
            1,
            0,
            100,
        );

        let device = Device::new(info);

        assert_eq!(device.open_count(), 0);
        assert!(!device.in_use());
        assert!(!device.exclusive);
    }

    #[test]
    fn test_device_open_close() {
        let info = DeviceInfo::new(
            "test".to_string(),
            "Test device".to_string(),
            DeviceType::Character,
            1,
            0,
            100,
        );

        let mut info = DeviceInfo::new(
            "test".to_string(),
            "Test device".to_string(),
            DeviceType::Character,
            1,
            0,
            100,
        );
        info.status = DeviceStatus::Ready;

        let mut device = Device::new(info);

        // Open device
        let result = device.open(100, false);
        assert!(result.is_ok());
        assert_eq!(device.open_count(), 1);
        assert!(device.in_use());

        // Close device
        device.close(100).unwrap();
        assert_eq!(device.open_count(), 0);
        assert!(!device.in_use());
    }

    #[test]
    fn test_exclusive_open() {
        let mut info = DeviceInfo::new(
            "test".to_string(),
            "Test device".to_string(),
            DeviceType::Character,
            1,
            0,
            100,
        );
        info.status = DeviceStatus::Ready;

        let mut device = Device::new(info);

        // Open exclusively
        device.open(100, true).unwrap();
        assert!(device.exclusive);
        assert_eq!(device.exclusive_owner, Some(100));

        // Try to open from another process
        let result = device.open(200, false);
        assert!(result.is_err());

        // Close exclusive access
        device.close(100).unwrap();
        assert!(!device.exclusive);
        assert_eq!(device.exclusive_owner, None);
    }

    #[test]
    fn test_open_not_ready() {
        let mut info = DeviceInfo::new(
            "test".to_string(),
            "Test device".to_string(),
            DeviceType::Character,
            1,
            0,
            100,
        );
        info.status = DeviceStatus::Initializing;

        let mut device = Device::new(info);

        // Try to open when not ready
        let result = device.open(100, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_close_when_not_open() {
        let info = DeviceInfo::new(
            "test".to_string(),
            "Test device".to_string(),
            DeviceType::Character,
            1,
            0,
            100,
        );

        let mut device = Device::new(info);

        // Try to close when not open
        let result = device.close(100);
        assert!(result.is_err());
    }

    #[test]
    fn test_device_type_equality() {
        assert_eq!(DeviceType::Character, DeviceType::Character);
        assert_ne!(DeviceType::Character, DeviceType::Block);
    }

    #[test]
    fn test_status_equality() {
        assert_eq!(DeviceStatus::Ready, DeviceStatus::Ready);
        assert_ne!(DeviceStatus::Ready, DeviceStatus::Busy);
    }
}
