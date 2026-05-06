// Device Service
//
// This service handles all device-related operations:
// - Device management (register, unregister, lookup)
// - Device driver management
// - Device I/O operations
// - Interrupt handling
// - Character and block device support

pub mod device;
pub mod driver;
pub mod manager;
pub mod service;

// Re-export key types
pub use device::{Device, DeviceType, DeviceStatus, DeviceId};
pub use driver::{Driver, DriverManager, DriverType};
pub use manager::{DeviceManager, DeviceSnapshot};
pub use service::DeviceService;
