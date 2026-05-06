// Device Service - Main device management service
//
// 曾国藩曰：
// "百工之事，当有统筹。"
// 设备服务统一管理所有硬件设备的驱动、IO和中断处理。

use std::sync::{Arc, Mutex};
use crossbeam_channel::{Receiver, Sender};
use crate::messaging::{
    KernelMsg, DeviceRequest, DeviceClass, DeviceConfig,
    Pid, MessageBus, Response, ResponseData, ServiceError as MessagingServiceError,
};
use crate::messaging::bus::Envelope;
use crate::{GenshinResult, GenshinError, ServiceError};

// Import device service components
use super::device::{Device, DeviceType, DeviceStatus, DeviceInfo, DeviceOperations, DeviceId};
use super::driver::{Driver, DriverInfo, DriverType, DriverManager};
use super::manager::{DeviceManager, DeviceSnapshot};

/// Device Service - Main device management service
///
/// 曾国藩曰：
/// "统管百工，当知其详。"
/// 设备服务统筹设备注册、驱动管理和IO操作。
pub struct DeviceService {
    /// Message bus
    bus: Arc<dyn MessageBus>,

    /// Receiver for message bus
    receiver: Receiver<Envelope>,

    /// Device manager
    device_manager: Arc<Mutex<DeviceManager>>,
}

impl DeviceService {
    /// Create a new device service
    pub fn new(bus: Arc<dyn MessageBus>) -> Self {
        let receiver = bus.subscribe();
        let device_manager = Arc::new(Mutex::new(DeviceManager::new()));

        Self {
            bus,
            receiver,
            device_manager,
        }
    }

    /// Run the device service (main loop)
    pub fn run(&self) {
        println!("DeviceService starting...");

        loop {
            match self.receiver.recv() {
                Ok(envelope) => {
                    if let Err(e) = self.handle_envelope(envelope) {
                        eprintln!("DeviceService error: {}", e);
                    }
                }
                Err(_) => {
                    eprintln!("Message bus disconnected");
                    break;
                }
            }
        }
    }

    /// Handle incoming envelope
    fn handle_envelope(&self, envelope: Envelope) -> GenshinResult<()> {
        // Handle the message based on envelope type
        let result = match &envelope.message {
            KernelMsg::Device(req) => {
                if envelope.expects_response() {
                    self.handle_device_request_with_response(req.clone(), &envelope)
                } else {
                    self.handle_device_request(req.clone())
                }
            }
            KernelMsg::Interrupt(int) => self.handle_interrupt(int.clone()),
            _ => {
                // Ignore other messages
                Ok(())
            }
        };

        // Log errors but don't fail the service
        if let Err(e) = result {
            eprintln!("DeviceService error handling message: {}", e);

            // If this was a request, send error response
            if envelope.expects_response() {
                let _ = envelope.respond_error(MessagingServiceError::Other {
                    code: 1,
                    msg: e.to_string(),
                });
            }
        }

        Ok(())
    }

    /// Handle hardware interrupt
    fn handle_interrupt(&self, interrupt: crate::messaging::Interrupt) -> GenshinResult<()> {
        match interrupt {
            crate::messaging::Interrupt::HardwareFailure { component } => {
                eprintln!("DeviceService: Hardware failure in {}", component);
            }
            _ => {
                println!("DeviceService: Received interrupt {:?}", interrupt);
            }
        }
        Ok(())
    }

    /// Handle device service request with response
    fn handle_device_request_with_response(&self, req: DeviceRequest, envelope: &Envelope) -> GenshinResult<()> {
        match req {
            // ========== Device Management ==========
            DeviceRequest::RegisterDevice { device_type, name } => {
                self.handle_register_device_with_response(device_type, name, envelope)?;
            }

            DeviceRequest::UnregisterDevice { device_id } => {
                self.handle_unregister_device_with_response(device_id, envelope)?;
            }

            DeviceRequest::ListDevices => {
                self.handle_list_devices_with_response(envelope)?;
            }

            DeviceRequest::Status { device_id } => {
                self.handle_get_device_status_with_response(device_id, envelope)?;
            }

            // ========== Device I/O ==========
            DeviceRequest::Read { device_id, buf: _, size } => {
                self.handle_read_device_with_response(device_id, size, envelope)?;
            }

            DeviceRequest::Write { device_id, buf: _, size } => {
                self.handle_write_device_with_response(device_id, size, envelope)?;
            }

            // For other requests, use the regular handler and return void response
            _ => {
                self.handle_device_request(req)?;
                let _ = envelope.respond_success(ResponseData::Void);
            }
        }

        Ok(())
    }

    /// Handle device service request
    fn handle_device_request(&self, req: DeviceRequest) -> GenshinResult<()> {
        match req {
            // ========== Device Management ==========
            DeviceRequest::RegisterDevice { device_type, name } => {
                self.handle_register_device(device_type, name)?;
            }

            DeviceRequest::UnregisterDevice { device_id } => {
                self.handle_unregister_device(device_id)?;
            }

            DeviceRequest::ListDevices => {
                self.handle_list_devices()?;
            }

            DeviceRequest::GetDevicesByType { device_type } => {
                self.handle_get_devices_by_type(device_type)?;
            }

            // ========== Device I/O ==========
            DeviceRequest::Read { device_id, buf: _, size } => {
                self.handle_read_device(device_id, size)?;
            }

            DeviceRequest::Write { device_id, buf: _, size } => {
                self.handle_write_device(device_id, size)?;
            }

            DeviceRequest::Ioctl { device_id, request, arg: _ } => {
                self.handle_ioctl(device_id, request as u64)?;
            }

            // ========== Device Lifecycle ==========
            DeviceRequest::Init { device_id } => {
                self.handle_init_device(device_id)?;
            }

            DeviceRequest::Shutdown { device_id } => {
                self.handle_shutdown_device(device_id)?;
            }

            DeviceRequest::Reset { device_id } => {
                self.handle_reset_device(device_id)?;
            }

            DeviceRequest::Status { device_id } => {
                self.handle_get_device_status(device_id)?;
            }

            // ========== Device Interrupts ==========
            DeviceRequest::EnableInterrupt { device_id, irq } => {
                self.handle_enable_interrupt(device_id, irq)?;
            }

            DeviceRequest::DisableInterrupt { device_id, irq } => {
                self.handle_disable_interrupt(device_id, irq)?;
            }

            _ => {
                println!("DeviceService: Unhandled device request");
            }
        }

        Ok(())
    }

    // ========== Device Management Handlers ==========

    fn handle_register_device(&self, device_type: DeviceClass, name: String) -> GenshinResult<()> {
        let mut device_manager = Self::lock_mutex(&self.device_manager)?;

        // Create device info from config
        let device_type = Self::device_class_to_type(device_type)?;

        let info = DeviceInfo::new(
            name.clone(),
            format!("{} device", format!("{:?}", device_type)),
            device_type,
            0, // TODO: Get major number
            0, // TODO: Get minor number
            0, // Will be set by register
        );

        // Create device
        let device = Device::new(info);

        // Register device
        let registered_id = device_manager.register(device)?;

        println!("DeviceService: Registered device {} as device_id {}", name, registered_id);

        // TODO: Send response with registered_id
        Ok(())
    }

    fn handle_register_device_with_response(&self, device_type: DeviceClass, name: String, envelope: &Envelope) -> GenshinResult<()> {
        let mut device_manager = Self::lock_mutex(&self.device_manager)?;

        // Create device info from config
        let device_type = Self::device_class_to_type(device_type)?;

        let info = DeviceInfo::new(
            name.clone(),
            format!("{} device", format!("{:?}", device_type)),
            device_type,
            0, // TODO: Get major number
            0, // TODO: Get minor number
            0, // Will be set by register
        );

        // Create device
        let device = Device::new(info);

        // Register device
        let registered_id = device_manager.register(device)?;

        println!("DeviceService: Registered device {} as device_id {}", name, registered_id);

        // Send response with device_id
        let _ = envelope.respond_success(ResponseData::Integer(registered_id as u64));
        Ok(())
    }

    fn handle_unregister_device(&self, device_id: DeviceId) -> GenshinResult<()> {
        let mut device_manager = Self::lock_mutex(&self.device_manager)?;

        device_manager.unregister(device_id)?;

        println!("DeviceService: Unregistered device {}", device_id);

        // TODO: Send response
        Ok(())
    }

    fn handle_unregister_device_with_response(&self, device_id: DeviceId, envelope: &Envelope) -> GenshinResult<()> {
        let mut device_manager = Self::lock_mutex(&self.device_manager)?;

        device_manager.unregister(device_id)?;

        println!("DeviceService: Unregistered device {}", device_id);

        // Send response
        let _ = envelope.respond_success(ResponseData::Void);
        Ok(())
    }

    fn handle_list_devices(&self) -> GenshinResult<()> {
        let device_manager = Self::lock_mutex(&self.device_manager)?;

        let devices = device_manager.list_devices();

        println!("DeviceService: Listed {} devices", devices.len());

        Ok(())
    }

    // ========== Device I/O Handlers ==========

    fn handle_read_device(&self, device_id: DeviceId, size: usize) -> GenshinResult<()> {
        let device_manager = Self::lock_mutex(&self.device_manager)?;

        // Create buffer and read
        let mut buffer = vec![0u8; size];
        let bytes_read = device_manager.read(device_id, &mut buffer)?;

        println!("DeviceService: Read {} bytes from device {}", bytes_read, device_id);

        // TODO: Send response with data
        Ok(())
    }

    fn handle_read_device_with_response(&self, device_id: DeviceId, size: usize, envelope: &Envelope) -> GenshinResult<()> {
        let device_manager = Self::lock_mutex(&self.device_manager)?;

        // Create buffer and read
        let mut buffer = vec![0u8; size];
        let bytes_read = device_manager.read(device_id, &mut buffer)?;

        println!("DeviceService: Read {} bytes from device {}", bytes_read, device_id);

        // Send response with bytes read
        let _ = envelope.respond_success(ResponseData::BytesProcessed(bytes_read));
        Ok(())
    }

    fn handle_write_device(&self, device_id: DeviceId, size: usize) -> GenshinResult<()> {
        let device_manager = Self::lock_mutex(&self.device_manager)?;

        // Create dummy data and write
        let data = vec![0u8; size];
        let bytes_written = device_manager.write(device_id, &data)?;

        println!("DeviceService: Wrote {} bytes to device {}", bytes_written, device_id);

        // TODO: Send response with bytes written
        Ok(())
    }

    fn handle_write_device_with_response(&self, device_id: DeviceId, size: usize, envelope: &Envelope) -> GenshinResult<()> {
        let device_manager = Self::lock_mutex(&self.device_manager)?;

        // Create dummy data and write
        let data = vec![0u8; size];
        let bytes_written = device_manager.write(device_id, &data)?;

        println!("DeviceService: Wrote {} bytes to device {}", bytes_written, device_id);

        // Send response with bytes written
        let _ = envelope.respond_success(ResponseData::BytesProcessed(bytes_written));
        Ok(())
    }

    fn handle_ioctl(&self, device_id: DeviceId, request: u64) -> GenshinResult<()> {
        println!("DeviceService: Ioctl on device {}, request={}", device_id, request);

        // TODO: Implement actual ioctl logic
        Ok(())
    }

    // ========== Device Lifecycle Handlers ==========

    fn handle_init_device(&self, device_id: DeviceId) -> GenshinResult<()> {
        println!("DeviceService: Init device {}", device_id);

        // TODO: Implement actual init logic
        Ok(())
    }

    fn handle_shutdown_device(&self, device_id: DeviceId) -> GenshinResult<()> {
        println!("DeviceService: Shutdown device {}", device_id);

        // TODO: Implement actual shutdown logic
        Ok(())
    }

    fn handle_reset_device(&self, device_id: DeviceId) -> GenshinResult<()> {
        println!("DeviceService: Reset device {}", device_id);

        // TODO: Implement actual reset logic
        Ok(())
    }

    // ========== Device Query Handlers ==========

    fn handle_list_devices_with_response(&self, envelope: &Envelope) -> GenshinResult<()> {
        let device_manager = Self::lock_mutex(&self.device_manager)?;

        let devices = device_manager.list_devices();

        // Format device list as string
        let device_list = format!("Device count: {}", devices.len());

        println!("DeviceService: Listed {} devices", devices.len());

        // Send response with device count
        let _ = envelope.respond_success(ResponseData::String(device_list));
        Ok(())
    }

    fn handle_get_devices_by_type(&self, device_type: DeviceClass) -> GenshinResult<()> {
        let device_manager = Self::lock_mutex(&self.device_manager)?;

        let converted_type = Self::device_class_to_type(device_type)?;
        let devices = device_manager.get_by_type(converted_type);

        println!("DeviceService: Found {} devices of type {:?}", devices.len(), device_type);

        // TODO: Send response with device list
        Ok(())
    }

    // ========== Device Interrupt Handlers ==========

    fn handle_enable_interrupt(&self, device_id: DeviceId, irq: u32) -> GenshinResult<()> {
        println!("DeviceService: Enable interrupt {} for device {}", irq, device_id);

        // TODO: Implement actual interrupt enabling logic
        Ok(())
    }

    fn handle_disable_interrupt(&self, device_id: DeviceId, irq: u32) -> GenshinResult<()> {
        println!("DeviceService: Disable interrupt {} for device {}", irq, device_id);

        // TODO: Implement actual interrupt disabling logic
        Ok(())
    }

    // ========== Device Query Handlers ==========

    fn handle_get_device_status(&self, device_id: DeviceId) -> GenshinResult<()> {
        let device_manager = Self::lock_mutex(&self.device_manager)?;

        let status = device_manager.status(device_id)?;

        println!("DeviceService: Device {} status: {:?}", device_id, status);

        // TODO: Send response with status
        Ok(())
    }

    fn handle_get_device_status_with_response(&self, device_id: DeviceId, envelope: &Envelope) -> GenshinResult<()> {
        let device_manager = Self::lock_mutex(&self.device_manager)?;

        let status = device_manager.status(device_id)?;

        println!("DeviceService: Device {} status: {:?}", device_id, status);

        // Send response with status as string
        let status_str = format!("{:?}", status);
        let _ = envelope.respond_success(ResponseData::String(status_str));
        Ok(())
    }

    // ========== Helper Methods ==========

    /// Helper function to lock mutex and convert poison errors
    fn lock_mutex<T>(mutex: &Mutex<T>) -> GenshinResult<std::sync::MutexGuard<T>> {
        mutex.lock().map_err(|e| {
            GenshinError::Service(ServiceError::InvalidArguments {
                param: "mutex".to_string(),
                reason: format!("Mutex poisoned: {}", e)
            })
        })
    }

    /// Convert DeviceClass to DeviceType
    fn device_class_to_type(class: DeviceClass) -> GenshinResult<DeviceType> {
        match class {
            DeviceClass::Block => Ok(DeviceType::Block),
            DeviceClass::Char => Ok(DeviceType::Character),
            DeviceClass::Network => Ok(DeviceType::Network),
            DeviceClass::Graphics => Ok(DeviceType::Graphics),
            DeviceClass::Timer => Ok(DeviceType::Unknown),
            DeviceClass::Unknown => Ok(DeviceType::Unknown),
        }
    }

    // ========== Query Methods ==========

    /// Get device count
    pub fn device_count(&self) -> usize {
        let device_manager = Self::lock_mutex(&self.device_manager).unwrap();
        device_manager.count()
    }

    /// Get all devices snapshot
    pub fn list_devices(&self) -> Vec<DeviceSnapshot> {
        let device_manager = Self::lock_mutex(&self.device_manager).unwrap();
        device_manager.snapshot()
    }

    /// Get devices by type
    pub fn get_devices_by_type(&self, device_type: DeviceType) -> Vec<DeviceSnapshot> {
        let device_manager = Self::lock_mutex(&self.device_manager).unwrap();
        device_manager
            .get_by_type(device_type)
            .iter()
            .filter_map(|device| {
                device.lock().ok().map(|d| {
                    DeviceSnapshot {
                        device_id: d.info.id,
                        name: d.info.name.clone(),
                        device_type: d.info.device_type,
                        status: d.status(),
                        open_count: d.open_count(),
                        exclusive: d.exclusive,
                    }
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::LockedBus;

    #[test]
    fn test_device_service_creation() {
        let bus = Arc::new(LockedBus::new());
        let service = DeviceService::new(bus);

        // Service should be created successfully
        assert_eq!(service.device_count(), 0);
    }

    #[test]
    fn test_device_class_to_type() {
        assert_eq!(
            DeviceService::device_class_to_type(DeviceClass::Block).unwrap(),
            DeviceType::Block
        );

        assert_eq!(
            DeviceService::device_class_to_type(DeviceClass::Char).unwrap(),
            DeviceType::Character
        );

        assert_eq!(
            DeviceService::device_class_to_type(DeviceClass::Network).unwrap(),
            DeviceType::Network
        );
    }

    #[test]
    fn test_list_devices() {
        let bus = Arc::new(LockedBus::new());
        let service = DeviceService::new(bus);

        // Initially empty
        let devices = service.list_devices();
        assert_eq!(devices.len(), 0);
    }

    #[test]
    fn test_get_devices_by_type() {
        let bus = Arc::new(LockedBus::new());
        let service = DeviceService::new(bus);

        // Initially empty
        let devices = service.get_devices_by_type(DeviceType::Character);
        assert_eq!(devices.len(), 0);

        let devices = service.get_devices_by_type(DeviceType::Block);
        assert_eq!(devices.len(), 0);
    }
}
