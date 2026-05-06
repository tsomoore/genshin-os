// Device Manager Module
//
// 曾国藩曰：
// "百工之事，当有统管。"
// 设备管理器统筹管理所有硬件设备的注册、查找和IO操作。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use crate::messaging::Pid;
use crate::{GenshinResult, GenshinError, ServiceError};
use super::device::{Device, DeviceType, DeviceStatus, DeviceInfo, DeviceOperations, DeviceId};
use super::driver::{DriverManager, DriverType};

/// Device snapshot - for debugging and monitoring
#[derive(Debug, Clone)]
pub struct DeviceSnapshot {
    pub device_id: DeviceId,
    pub name: String,
    pub device_type: DeviceType,
    pub status: DeviceStatus,
    pub open_count: u32,
    pub exclusive: bool,
}

/// Device Manager - manages all devices
#[derive(Debug)]
pub struct DeviceManager {
    /// All devices (device ID -> Device)
    devices: HashMap<DeviceId, Arc<Mutex<Device>>>,

    /// Next available device ID
    next_device_id: DeviceId,

    /// Driver manager
    driver_manager: DriverManager,

    /// Device name to ID mapping
    name_map: HashMap<String, DeviceId>,

    /// Devices by type (DeviceType -> Vec<DeviceId>)
    type_map: HashMap<DeviceType, Vec<DeviceId>>,
}

impl DeviceManager {
    /// Create a new device manager
    pub fn new() -> Self {
        Self {
            devices: HashMap::new(),
            next_device_id: 1,
            driver_manager: DriverManager::new(),
            name_map: HashMap::new(),
            type_map: HashMap::new(),
        }
    }

    /// Register a device
    pub fn register(&mut self, mut device: Device) -> GenshinResult<DeviceId> {
        let device_id = self.next_device_id;
        self.next_device_id += 1;

        device.info.id = device_id;

        let device_name = device.info.name.clone();
        let device_type = device.info.device_type;

        // Add to devices map
        let device = Arc::new(Mutex::new(device));
        self.devices.insert(device_id, device.clone());

        // Add to name map
        self.name_map.insert(device_name, device_id);

        // Add to type map
        self.type_map
            .entry(device_type)
            .or_insert_with(Vec::new)
            .push(device_id);

        Ok(device_id)
    }

    /// Unregister a device
    pub fn unregister(&mut self, device_id: DeviceId) -> GenshinResult<()> {
        let device = self.devices.get(&device_id)
            .ok_or_else(|| GenshinError::Service(ServiceError::NotFound {
                resource_type: "Device".to_string(),
                id: device_id.to_string(),
            }))?;

        let device = device.lock().map_err(|e| {
            GenshinError::Service(ServiceError::Other {
                code: 1,
                msg: format!("Mutex poisoned: {}", e),
            })
        })?;

        // Check if device is in use
        if device.in_use() {
            return Err(GenshinError::Service(ServiceError::InvalidArguments {
                param: "device".to_string(),
                reason: "Device is in use".to_string(),
            }));
        }

        // Collect info for cleanup
        let device_name = device.info.name.clone();
        let device_type = device.info.device_type;

        // Drop lock before removing
        drop(device);

        // Remove from devices map
        self.devices.remove(&device_id);

        // Remove from name map
        self.name_map.remove(&device_name);

        // Remove from type map
        if let Some(devices) = self.type_map.get_mut(&device_type) {
            devices.retain(|&id| id != device_id);
        }

        Ok(())
    }

    /// Get device by ID
    pub fn get(&self, device_id: DeviceId) -> Option<Arc<Mutex<Device>>> {
        self.devices.get(&device_id).cloned()
    }

    /// Get device by name
    pub fn get_by_name(&self, name: &str) -> Option<Arc<Mutex<Device>>> {
        if let Some(device_id) = self.name_map.get(name) {
            self.get(*device_id)
        } else {
            None
        }
    }

    /// Open device
    pub fn open(&mut self, device_id: DeviceId, pid: Pid, exclusive: bool) -> GenshinResult<()> {
        let device = self.get(device_id)
            .ok_or_else(|| GenshinError::Service(ServiceError::NotFound {
                resource_type: "Device".to_string(),
                id: device_id.to_string(),
            }))?;

        let mut device = device.lock().map_err(|e| {
            GenshinError::Service(ServiceError::Other {
                code: 2,
                msg: format!("Mutex poisoned: {}", e),
            })
        })?;

        device.open(pid, exclusive)
    }

    /// Close device
    pub fn close(&mut self, device_id: DeviceId, pid: Pid) -> GenshinResult<()> {
        let device = self.get(device_id)
            .ok_or_else(|| GenshinError::Service(ServiceError::NotFound {
                resource_type: "Device".to_string(),
                id: device_id.to_string(),
            }))?;

        let mut device = device.lock().map_err(|e| {
            GenshinError::Service(ServiceError::Other {
                code: 3,
                msg: format!("Mutex poisoned: {}", e),
            })
        })?;

        device.close(pid)
    }

    /// Read from device
    pub fn read(&self, device_id: DeviceId, data: &mut [u8]) -> GenshinResult<usize> {
        let device = self.get(device_id)
            .ok_or_else(|| GenshinError::Service(ServiceError::NotFound {
                resource_type: "Device".to_string(),
                id: device_id.to_string(),
            }))?;

        let device = device.lock().map_err(|e| {
            GenshinError::Service(ServiceError::Other {
                code: 4,
                msg: format!("Mutex poisoned: {}", e),
            })
        })?;

        device.read(data)
    }

    /// Write to device
    pub fn write(&self, device_id: DeviceId, data: &[u8]) -> GenshinResult<usize> {
        let device = self.get(device_id)
            .ok_or_else(|| GenshinError::Service(ServiceError::NotFound {
                resource_type: "Device".to_string(),
                id: device_id.to_string(),
            }))?;

        let device = device.lock().map_err(|e| {
            GenshinError::Service(ServiceError::Other {
                code: 5,
                msg: format!("Mutex poisoned: {}", e),
            })
        })?;

        device.write(data)
    }

    /// Get device status
    pub fn status(&self, device_id: DeviceId) -> GenshinResult<DeviceStatus> {
        let device = self.get(device_id)
            .ok_or_else(|| GenshinError::Service(ServiceError::NotFound {
                resource_type: "Device".to_string(),
                id: device_id.to_string(),
            }))?;

        let device = device.lock().map_err(|e| {
            GenshinError::Service(ServiceError::Other {
                code: 6,
                msg: format!("Mutex poisoned: {}", e),
            })
        })?;

        Ok(device.status())
    }

    /// Get all devices by type
    pub fn get_by_type(&self, device_type: DeviceType) -> Vec<Arc<Mutex<Device>>> {
        self.type_map
            .get(&device_type)
            .map(|ids| {
                ids.iter()
                    .filter_map(|&id| self.get(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// List all devices
    pub fn list_devices(&self) -> Vec<DeviceSnapshot> {
        self.devices.values()
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

    /// Get device count
    pub fn count(&self) -> usize {
        self.devices.len()
    }

    /// Get driver manager reference
    pub fn driver_manager(&self) -> &DriverManager {
        &self.driver_manager
    }

    /// Get mutable driver manager reference
    pub fn driver_manager_mut(&mut self) -> &mut DriverManager {
        &mut self.driver_manager
    }

    /// Create snapshot
    pub fn snapshot(&self) -> Vec<DeviceSnapshot> {
        self.list_devices()
    }
}

impl Default for DeviceManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_device(id: DeviceId) -> Device {
        let mut info = DeviceInfo::new(
            format!("test_device_{}", id),
            "Test device".to_string(),
            DeviceType::Character,
            1,
            0,
            id,
        );

        info.status = DeviceStatus::Ready;

        Device::new(info)
    }

    #[test]
    fn test_device_manager_creation() {
        let manager = DeviceManager::new();
        assert_eq!(manager.count(), 0);
    }

    #[test]
    fn test_register_device() {
        let mut manager = DeviceManager::new();
        let device = create_test_device(100);

        let device_id = manager.register(device).unwrap();
        assert_eq!(manager.count(), 1);
        assert_eq!(device_id, 1); // First device gets ID 1
    }

    #[test]
    fn test_unregister_device() {
        let mut manager = DeviceManager::new();
        let device = create_test_device(100);

        let device_id = manager.register(device).unwrap();

        // Can't unregister while in use
        manager.open(device_id, 100, false).unwrap();
        let result = manager.unregister(device_id);
        assert!(result.is_err());

        // Close and unregister
        manager.close(device_id, 100).unwrap();
        let result = manager.unregister(device_id);
        assert!(result.is_ok());
        assert_eq!(manager.count(), 0);
    }

    #[test]
    fn test_get_device() {
        let mut manager = DeviceManager::new();
        let device = create_test_device(100);

        let device_id = manager.register(device).unwrap();

        // Get by ID
        let retrieved = manager.get(device_id);
        assert!(retrieved.is_some());

        // Get by name
        let retrieved = manager.get_by_name("test_device_100");
        assert!(retrieved.is_some());

        // Get non-existent
        let retrieved = manager.get(999);
        assert!(retrieved.is_none());
    }

    #[test]
    fn test_open_close_device() {
        let mut manager = DeviceManager::new();
        let device = create_test_device(100);

        let device_id = manager.register(device).unwrap();

        // Open device
        let result = manager.open(device_id, 100, false);
        assert!(result.is_ok());

        // Close device
        let result = manager.close(device_id, 100);
        assert!(result.is_ok());
    }

    #[test]
    fn test_exclusive_device_access() {
        let mut manager = DeviceManager::new();
        let device = create_test_device(100);

        let device_id = manager.register(device).unwrap();

        // Open exclusively
        manager.open(device_id, 100, true).unwrap();

        // Try to open from another process
        let result = manager.open(device_id, 200, false);
        assert!(result.is_err());

        // Close exclusive access
        manager.close(device_id, 100).unwrap();

        // Now can open from another process
        let result = manager.open(device_id, 200, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_by_type() {
        let mut manager = DeviceManager::new();

        // Register multiple devices
        for i in 0..3 {
            let device = create_test_device(i);
            manager.register(device).unwrap();
        }

        // Get all character devices
        let char_devices = manager.get_by_type(DeviceType::Character);
        assert_eq!(char_devices.len(), 3);

        // Get block devices (should be none)
        let block_devices = manager.get_by_type(DeviceType::Block);
        assert_eq!(block_devices.len(), 0);
    }

    #[test]
    fn test_list_devices() {
        let mut manager = DeviceManager::new();

        // Register multiple devices
        for i in 0..3 {
            let device = create_test_device(i);
            manager.register(device).unwrap();
        }

        let devices = manager.list_devices();
        assert_eq!(devices.len(), 3);
    }

    #[test]
    fn test_device_snapshot() {
        let mut manager = DeviceManager::new();
        let device = create_test_device(100);

        let device_id = manager.register(device).unwrap();
        manager.open(device_id, 100, false).unwrap();

        let snapshot = manager.snapshot();
        assert_eq!(snapshot.len(), 1);

        let snap = &snapshot[0];
        assert_eq!(snap.device_id, device_id);
        assert_eq!(snap.open_count, 1);
        assert_eq!(snap.device_type, DeviceType::Character);
    }
}
