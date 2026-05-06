// Device Driver Module
//
// 曾国藩曰：
// "工欲善其事，必先利其器。"
// 设备驱动管理器负责加载、卸载和管理设备驱动程序。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use crate::messaging::Pid;
use crate::{GenshinResult, GenshinError, ServiceError};
use super::device::{Device, DeviceType, DeviceInfo, DeviceId};

/// Driver type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverType {
    /// Character device driver
    Character,

    /// Block device driver
    Block,

    /// Network device driver
    Network,

    /// Generic driver
    Generic,
}

/// Driver information
#[derive(Debug, Clone)]
pub struct DriverInfo {
    /// Driver name
    pub name: String,

    /// Driver version
    pub version: String,

    /// Driver type
    pub driver_type: DriverType,

    /// Supported device types
    pub supported_types: Vec<DeviceType>,

    /// Driver author
    pub author: String,

    /// Driver description
    pub description: String,
}

impl DriverInfo {
    /// Create new driver info
    pub fn new(
        name: String,
        version: String,
        driver_type: DriverType,
        supported_types: Vec<DeviceType>,
        author: String,
        description: String,
    ) -> Self {
        Self {
            name,
            version,
            driver_type,
            supported_types,
            author,
            description,
        }
    }
}

/// Driver - represents a device driver
#[derive(Debug)]
pub struct Driver {
    /// Driver information
    pub info: DriverInfo,

    /// Loaded flag
    pub loaded: bool,

    /// Reference count (number of devices using this driver)
    pub ref_count: u32,

    /// Device IDs managed by this driver
    pub devices: Vec<DeviceId>,
}

impl Driver {
    /// Create a new driver
    pub fn new(info: DriverInfo) -> Self {
        Self {
            info,
            loaded: false,
            ref_count: 0,
            devices: Vec::new(),
        }
    }

    /// Check if driver supports a device type
    pub fn supports(&self, device_type: DeviceType) -> bool {
        self.info.supported_types.contains(&device_type)
    }

    /// Load driver
    pub fn load(&mut self) -> GenshinResult<()> {
        if self.loaded {
            return Err(GenshinError::Service(ServiceError::InvalidArguments {
                param: "driver".to_string(),
                reason: "Driver already loaded".to_string(),
            }));
        }

        // TODO: Actual driver loading logic
        self.loaded = true;

        Ok(())
    }

    /// Unload driver
    pub fn unload(&mut self) -> GenshinResult<()> {
        if !self.loaded {
            return Err(GenshinError::Service(ServiceError::InvalidArguments {
                param: "driver".to_string(),
                reason: "Driver not loaded".to_string(),
            }));
        }

        if self.ref_count > 0 || !self.devices.is_empty() {
            return Err(GenshinError::Service(ServiceError::InvalidArguments {
                param: "driver".to_string(),
                reason: "Driver in use".to_string(),
            }));
        }

        // TODO: Actual driver unloading logic
        self.loaded = false;

        Ok(())
    }

    /// Add device to driver
    pub fn add_device(&mut self, device_id: DeviceId) -> GenshinResult<()> {
        if self.devices.contains(&device_id) {
            return Err(GenshinError::Service(ServiceError::InvalidArguments {
                param: "device_id".to_string(),
                reason: "Device already managed by this driver".to_string(),
            }));
        }

        self.devices.push(device_id);
        self.ref_count += 1;

        Ok(())
    }

    /// Remove device from driver
    pub fn remove_device(&mut self, device_id: DeviceId) -> GenshinResult<()> {
        if let Some(pos) = self.devices.iter().position(|&id| id == device_id) {
            self.devices.remove(pos);
            if self.ref_count > 0 {
                self.ref_count -= 1;
            }
            Ok(())
        } else {
            Err(GenshinError::Service(ServiceError::NotFound {
                resource_type: "Device".to_string(),
                id: device_id.to_string(),
            }))
        }
    }

    /// Check if driver is loaded
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// Get device count
    pub fn device_count(&self) -> usize {
        self.devices.len()
    }
}

/// Driver Manager - manages all device drivers
#[derive(Debug)]
pub struct DriverManager {
    /// All drivers (driver name -> Driver)
    drivers: HashMap<String, Arc<Mutex<Driver>>>,

    /// Next driver ID
    next_driver_id: u32,
}

impl DriverManager {
    /// Create a new driver manager
    pub fn new() -> Self {
        Self {
            drivers: HashMap::new(),
            next_driver_id: 1,
        }
    }

    /// Register a driver
    pub fn register(&mut self, driver: Driver) -> GenshinResult<String> {
        let driver_name = driver.info.name.clone();

        if self.drivers.contains_key(&driver_name) {
            return Err(GenshinError::Service(ServiceError::InvalidArguments {
                param: "driver".to_string(),
                reason: "Driver already registered".to_string(),
            }));
        }

        let driver = Arc::new(Mutex::new(driver));
        self.drivers.insert(driver_name.clone(), driver);

        Ok(driver_name)
    }

    /// Unregister a driver
    pub fn unregister(&mut self, driver_name: &str) -> GenshinResult<()> {
        let driver = self.drivers.get(driver_name)
            .ok_or_else(|| GenshinError::Service(ServiceError::NotFound {
                resource_type: "Driver".to_string(),
                id: driver_name.to_string(),
            }))?;

        let driver = driver.lock().map_err(|e| {
            GenshinError::Service(ServiceError::Other {
                code: 1,
                msg: format!("Mutex poisoned: {}", e),
            })
        })?;

        // Ensure driver is unloaded
        if driver.is_loaded() {
            return Err(GenshinError::Service(ServiceError::InvalidArguments {
                param: "driver".to_string(),
                reason: "Driver is still loaded".to_string(),
            }));
        }

        // Remove from map after all checks pass
        drop(driver);
        self.drivers.remove(driver_name);

        Ok(())
    }

    /// Get driver by name
    pub fn get(&self, driver_name: &str) -> Option<Arc<Mutex<Driver>>> {
        self.drivers.get(driver_name).cloned()
    }

    /// Load driver
    pub fn load(&mut self, driver_name: &str) -> GenshinResult<()> {
        let driver = self.drivers.get_mut(driver_name)
            .ok_or_else(|| GenshinError::Service(ServiceError::NotFound {
                resource_type: "Driver".to_string(),
                id: driver_name.to_string(),
            }))?;

        let mut driver = driver.lock().map_err(|e| {
            GenshinError::Service(ServiceError::Other {
                code: 2,
                msg: format!("Mutex poisoned: {}", e),
            })
        })?;

        driver.load()
    }

    /// Unload driver
    pub fn unload(&mut self, driver_name: &str) -> GenshinResult<()> {
        let driver = self.drivers.get_mut(driver_name)
            .ok_or_else(|| GenshinError::Service(ServiceError::NotFound {
                resource_type: "Driver".to_string(),
                id: driver_name.to_string(),
            }))?;

        let mut driver = driver.lock().map_err(|e| {
            GenshinError::Service(ServiceError::Other {
                code: 3,
                msg: format!("Mutex poisoned: {}", e),
            })
        })?;

        driver.unload()
    }

    /// Find driver for device type
    pub fn find_driver(&self, device_type: DeviceType) -> Option<String> {
        for (name, driver) in &self.drivers {
            let driver = driver.lock().unwrap();
            if driver.is_loaded() && driver.supports(device_type) {
                return Some(name.clone());
            }
        }
        None
    }

    /// Get all driver names
    pub fn list_drivers(&self) -> Vec<String> {
        self.drivers.keys().cloned().collect()
    }

    /// Get driver count
    pub fn count(&self) -> usize {
        self.drivers.len()
    }

    /// Get all loaded drivers
    pub fn loaded_drivers(&self) -> Vec<String> {
        self.drivers.iter()
            .filter(|(_, driver)| {
                driver.lock().map(|d| d.is_loaded()).unwrap_or(false)
            })
            .map(|(name, _)| name.clone())
            .collect()
    }
}

impl Default for DriverManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_driver_info_creation() {
        let info = DriverInfo::new(
            "test_driver".to_string(),
            "1.0.0".to_string(),
            DriverType::Character,
            vec![DeviceType::Character, DeviceType::Terminal],
            "Test Author".to_string(),
            "Test driver".to_string(),
        );

        assert_eq!(info.name, "test_driver");
        assert_eq!(info.version, "1.0.0");
        assert_eq!(info.driver_type, DriverType::Character);
        assert_eq!(info.supported_types.len(), 2);
    }

    #[test]
    fn test_driver_creation() {
        let info = DriverInfo::new(
            "test_driver".to_string(),
            "1.0.0".to_string(),
            DriverType::Character,
            vec![DeviceType::Character],
            "Test Author".to_string(),
            "Test driver".to_string(),
        );

        let driver = Driver::new(info);

        assert!(!driver.is_loaded());
        assert_eq!(driver.ref_count, 0);
        assert_eq!(driver.device_count(), 0);
    }

    #[test]
    fn test_driver_load_unload() {
        let info = DriverInfo::new(
            "test_driver".to_string(),
            "1.0.0".to_string(),
            DriverType::Character,
            vec![DeviceType::Character],
            "Test Author".to_string(),
            "Test driver".to_string(),
        );

        let mut driver = Driver::new(info);

        // Load driver
        let result = driver.load();
        assert!(result.is_ok());
        assert!(driver.is_loaded());

        // Try to load again
        let result = driver.load();
        assert!(result.is_err());

        // Unload driver
        let result = driver.unload();
        assert!(result.is_ok());
        assert!(!driver.is_loaded());
    }

    #[test]
    fn test_driver_device_management() {
        let info = DriverInfo::new(
            "test_driver".to_string(),
            "1.0.0".to_string(),
            DriverType::Character,
            vec![DeviceType::Character],
            "Test Author".to_string(),
            "Test driver".to_string(),
        );

        let mut driver = Driver::new(info);

        // Add device
        let result = driver.add_device(100);
        assert!(result.is_ok());
        assert_eq!(driver.device_count(), 1);
        assert_eq!(driver.ref_count, 1);

        // Try to add same device again
        let result = driver.add_device(100);
        assert!(result.is_err());

        // Remove device
        let result = driver.remove_device(100);
        assert!(result.is_ok());
        assert_eq!(driver.device_count(), 0);
        assert_eq!(driver.ref_count, 0);
    }

    #[test]
    fn test_driver_manager() {
        let mut manager = DriverManager::new();

        let info = DriverInfo::new(
            "test_driver".to_string(),
            "1.0.0".to_string(),
            DriverType::Character,
            vec![DeviceType::Character],
            "Test Author".to_string(),
            "Test driver".to_string(),
        );

        let driver = Driver::new(info);

        // Register driver
        let result = manager.register(driver);
        assert!(result.is_ok());
        assert_eq!(manager.count(), 1);

        // Get driver
        let retrieved = manager.get("test_driver");
        assert!(retrieved.is_some());
    }

    #[test]
    fn test_driver_manager_unload() {
        let mut manager = DriverManager::new();

        let info = DriverInfo::new(
            "test_driver".to_string(),
            "1.0.0".to_string(),
            DriverType::Character,
            vec![DeviceType::Character],
            "Test Author".to_string(),
            "Test driver".to_string(),
        );

        let driver = Driver::new(info);

        // Register and load driver
        manager.register(driver).unwrap();
        manager.load("test_driver").unwrap();

        // Try to unregister while loaded
        let result = manager.unregister("test_driver");
        assert!(result.is_err());

        // Unload first
        manager.unload("test_driver").unwrap();

        // Now can unregister
        let result = manager.unregister("test_driver");
        assert!(result.is_ok());
    }

    #[test]
    fn test_find_driver() {
        let mut manager = DriverManager::new();

        let info = DriverInfo::new(
            "char_driver".to_string(),
            "1.0.0".to_string(),
            DriverType::Character,
            vec![DeviceType::Character],
            "Test Author".to_string(),
            "Test driver".to_string(),
        );

        let driver = Driver::new(info);

        manager.register(driver).unwrap();
        manager.load("char_driver").unwrap();

        // Find driver for character device
        let found = manager.find_driver(DeviceType::Character);
        assert!(found.is_some());
        assert_eq!(found.unwrap(), "char_driver");

        // Try to find driver for unsupported type
        let found = manager.find_driver(DeviceType::Block);
        assert!(found.is_none());
    }

    #[test]
    fn test_list_drivers() {
        let mut manager = DriverManager::new();

        // Register multiple drivers
        for i in 0..3 {
            let info = DriverInfo::new(
                format!("driver_{}", i),
                "1.0.0".to_string(),
                DriverType::Generic,
                vec![DeviceType::Unknown],
                "Test".to_string(),
                "Test".to_string(),
            );

            let driver = Driver::new(info);
            manager.register(driver).unwrap();
        }

        assert_eq!(manager.list_drivers().len(), 3);

        // Load one driver
        manager.load("driver_1").unwrap();

        // Check loaded drivers
        let loaded = manager.loaded_drivers();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0], "driver_1");
    }
}
