// Generic Device Interface and Device Manager Support
//
// 曾国藩曰：
// "治大国若烹小鲜，需分门别类，各司其职。"
// 设备管理亦需分类明确，接口统一。

use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::fmt;
use crate::{GenshinError, GenshinResult, HardwareError};
use crate::messaging::{VirtAddr, Pid};

/// Device type classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceType {
    /// Block device (disk, SSD)
    BlockDevice,
    /// Character device (keyboard, mouse)
    CharDevice,
    /// Network device
    NetworkDevice,
    /// Timer device
    TimerDevice,
    /// Graphics device
    GraphicsDevice,
    /// Unknown device
    Unknown,
}

/// Device status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceStatus {
    /// Device is working normally
    Online,
    /// Device is offline/disconnected
    Offline,
    /// Device has an error
    Error,
    /// Device is busy
    Busy,
    /// Device is initializing
    Initializing,
}

/// Generic device trait
///
/// All devices must implement this trait for device manager integration.
pub trait Device: Send + Sync {
    /// Get device ID
    fn device_id(&self) -> u32;

    /// Get device type
    fn device_type(&self) -> DeviceType;

    /// Get device name
    fn device_name(&self) -> &str;

    /// Get current device status
    fn status(&self) -> DeviceStatus;

    /// Initialize device
    fn init(&mut self) -> GenshinResult<()>;

    /// Shutdown device
    fn shutdown(&mut self) -> GenshinResult<()>;

    /// Reset device
    fn reset(&mut self) -> GenshinResult<()>;

    /// Get device-specific state snapshot
    fn snapshot(&self) -> DeviceSnapshot;
}

/// Device state snapshot for monitoring/debugging
#[derive(Debug, Clone)]
pub struct DeviceSnapshot {
    pub device_id: u32,
    pub device_type: DeviceType,
    pub name: String,
    pub status: DeviceStatus,
    pub state_data: String,  // Device-specific state info
}

/// Keyboard device simulation
pub struct KeyboardDevice {
    device_id: u32,
    status: DeviceStatus,
    buffer: Arc<Mutex<Vec<char>>>,
}

impl KeyboardDevice {
    pub fn new(device_id: u32) -> Self {
        Self {
            device_id,
            status: DeviceStatus::Online,
            buffer: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Simulate key press
    pub fn key_press(&self, c: char) {
        let mut buffer = self.buffer.lock().unwrap();
        buffer.push(c);
    }

    /// Read character from keyboard buffer
    pub fn read_char(&self) -> Option<char> {
        let mut buffer = self.buffer.lock().unwrap();
        if buffer.is_empty() {
            None
        } else {
            Some(buffer.remove(0))
        }
    }

    /// Check if key is available
    pub fn has_key(&self) -> bool {
        let buffer = self.buffer.lock().unwrap();
        !buffer.is_empty()
    }
}

impl Device for KeyboardDevice {
    fn device_id(&self) -> u32 {
        self.device_id
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::CharDevice
    }

    fn device_name(&self) -> &str {
        "keyboard"
    }

    fn status(&self) -> DeviceStatus {
        self.status
    }

    fn init(&mut self) -> GenshinResult<()> {
        self.status = DeviceStatus::Online;
        Ok(())
    }

    fn shutdown(&mut self) -> GenshinResult<()> {
        self.status = DeviceStatus::Offline;
        Ok(())
    }

    fn reset(&mut self) -> GenshinResult<()> {
        self.buffer.lock().unwrap().clear();
        self.status = DeviceStatus::Online;
        Ok(())
    }

    fn snapshot(&self) -> DeviceSnapshot {
        DeviceSnapshot {
            device_id: self.device_id,
            device_type: DeviceType::CharDevice,
            name: "keyboard".to_string(),
            status: self.status,
            state_data: format!("buffer_size: {}", self.buffer.lock().unwrap().len()),
        }
    }
}

/// Serial console device (UART simulation)
pub struct SerialDevice {
    device_id: u32,
    status: DeviceStatus,
    tx_buffer: Arc<Mutex<Vec<u8>>>,
    rx_buffer: Arc<Mutex<Vec<u8>>>,
    baud_rate: u32,
}

impl SerialDevice {
    pub fn new(device_id: u32, baud_rate: u32) -> Self {
        Self {
            device_id,
            status: DeviceStatus::Online,
            tx_buffer: Arc::new(Mutex::new(Vec::new())),
            rx_buffer: Arc::new(Mutex::new(Vec::new())),
            baud_rate,
        }
    }

    /// Write byte to serial port
    pub fn write_byte(&self, byte: u8) -> GenshinResult<()> {
        let mut tx = self.tx_buffer.lock().unwrap();
        tx.push(byte);
        Ok(())
    }

    /// Read byte from serial port
    pub fn read_byte(&self) -> Option<u8> {
        let mut rx = self.rx_buffer.lock().unwrap();
        if rx.is_empty() {
            None
        } else {
            Some(rx.remove(0))
        }
    }

    /// Simulate receiving data (for testing)
    pub fn receive_data(&self, data: &[u8]) {
        let mut rx = self.rx_buffer.lock().unwrap();
        rx.extend_from_slice(data);
    }

    /// Get TX buffer content
    pub fn get_tx_data(&self) -> Vec<u8> {
        let tx = self.tx_buffer.lock().unwrap();
        tx.clone()
    }
}

impl Device for SerialDevice {
    fn device_id(&self) -> u32 {
        self.device_id
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::CharDevice
    }

    fn device_name(&self) -> &str {
        "serial"
    }

    fn status(&self) -> DeviceStatus {
        self.status
    }

    fn init(&mut self) -> GenshinResult<()> {
        self.status = DeviceStatus::Online;
        Ok(())
    }

    fn shutdown(&mut self) -> GenshinResult<()> {
        self.status = DeviceStatus::Offline;
        Ok(())
    }

    fn reset(&mut self) -> GenshinResult<()> {
        self.tx_buffer.lock().unwrap().clear();
        self.rx_buffer.lock().unwrap().clear();
        self.status = DeviceStatus::Online;
        Ok(())
    }

    fn snapshot(&self) -> DeviceSnapshot {
        DeviceSnapshot {
            device_id: self.device_id,
            device_type: DeviceType::CharDevice,
            name: format!("serial@{}", self.baud_rate),
            status: self.status,
            state_data: format!(
                "tx: {} bytes, rx: {} bytes",
                self.tx_buffer.lock().unwrap().len(),
                self.rx_buffer.lock().unwrap().len()
            ),
        }
    }
}

/// Network interface card (NIC) simulation
pub struct NetworkDevice {
    device_id: u32,
    status: DeviceStatus,
    mac_address: [u8; 6],
    rx_queue: Arc<Mutex<Vec<Vec<u8>>>>,
    tx_queue: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl NetworkDevice {
    pub fn new(device_id: u32, mac_address: [u8; 6]) -> Self {
        Self {
            device_id,
            status: DeviceStatus::Online,
            mac_address,
            rx_queue: Arc::new(Mutex::new(Vec::new())),
            tx_queue: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Send packet
    pub fn send_packet(&self, packet: &[u8]) -> GenshinResult<()> {
        if self.status != DeviceStatus::Online {
            return Err(GenshinError::Hardware(HardwareError::Device {
                device_id: self.device_id,
                error: "Device not online".to_string(),
            }));
        }

        let mut tx = self.tx_queue.lock().unwrap();
        tx.push(packet.to_vec());
        Ok(())
    }

    /// Receive packet
    pub fn receive_packet(&self) -> Option<Vec<u8>> {
        let mut rx = self.rx_queue.lock().unwrap();
        if rx.is_empty() {
            None
        } else {
            Some(rx.remove(0))
        }
    }

    /// Simulate incoming packet (for testing)
    pub fn inject_packet(&self, packet: &[u8]) {
        let mut rx = self.rx_queue.lock().unwrap();
        rx.push(packet.to_vec());
    }

    /// Get MAC address
    pub fn mac_address(&self) -> [u8; 6] {
        self.mac_address
    }

    /// Check if packet is available
    pub fn has_packet(&self) -> bool {
        let rx = self.rx_queue.lock().unwrap();
        !rx.is_empty()
    }
}

impl Device for NetworkDevice {
    fn device_id(&self) -> u32 {
        self.device_id
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::NetworkDevice
    }

    fn device_name(&self) -> &str {
        "eth0"
    }

    fn status(&self) -> DeviceStatus {
        self.status
    }

    fn init(&mut self) -> GenshinResult<()> {
        self.status = DeviceStatus::Online;
        Ok(())
    }

    fn shutdown(&mut self) -> GenshinResult<()> {
        self.status = DeviceStatus::Offline;
        Ok(())
    }

    fn reset(&mut self) -> GenshinResult<()> {
        self.rx_queue.lock().unwrap().clear();
        self.tx_queue.lock().unwrap().clear();
        self.status = DeviceStatus::Online;
        Ok(())
    }

    fn snapshot(&self) -> DeviceSnapshot {
        DeviceSnapshot {
            device_id: self.device_id,
            device_type: DeviceType::NetworkDevice,
            name: format!("eth0({:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X})",
                self.mac_address[0], self.mac_address[1], self.mac_address[2],
                self.mac_address[3], self.mac_address[4], self.mac_address[5]),
            status: self.status,
            state_data: format!(
                "rx_queue: {} packets, tx_queue: {} packets",
                self.rx_queue.lock().unwrap().len(),
                self.tx_queue.lock().unwrap().len()
            ),
        }
    }
}

/// Device registry for device manager
///
/// Maintains a registry of all devices in the system.
pub struct DeviceRegistry {
    devices: Arc<Mutex<HashMap<u32, Arc<Mutex<dyn Device>>>>>,
    next_device_id: Arc<Mutex<u32>>,
}

impl DeviceRegistry {
    pub fn new() -> Self {
        Self {
            devices: Arc::new(Mutex::new(HashMap::new())),
            next_device_id: Arc::new(Mutex::new(1)),
        }
    }

    /// Register a new device
    pub fn register_device(&self, device: Arc<Mutex<dyn Device>>) -> u32 {
        let mut next_id = self.next_device_id.lock().unwrap();
        let device_id = *next_id;
        *next_id += 1;

        let mut devices = self.devices.lock().unwrap();
        devices.insert(device_id, device);

        device_id
    }

    /// Unregister a device
    pub fn unregister_device(&self, device_id: u32) -> GenshinResult<()> {
        let mut devices = self.devices.lock().unwrap();
        devices.remove(&device_id)
            .ok_or_else(|| GenshinError::Hardware(HardwareError::Device {
                device_id,
                error: "Device not found".to_string(),
            }))?;
        Ok(())
    }

    /// Get device by ID
    pub fn get_device(&self, device_id: u32) -> Option<Arc<Mutex<dyn Device>>> {
        let devices = self.devices.lock().unwrap();
        devices.get(&device_id).cloned()
    }

    /// List all devices
    pub fn list_devices(&self) -> Vec<DeviceSnapshot> {
        let devices = self.devices.lock().unwrap();
        devices.values()
            .map(|d| {
                let device = d.lock().unwrap();
                device.snapshot()
            })
            .collect()
    }

    /// Get devices by type
    pub fn get_devices_by_type(&self, device_type: DeviceType) -> Vec<DeviceSnapshot> {
        let devices = self.devices.lock().unwrap();
        devices.values()
            .filter(|d| {
                let device = d.lock().unwrap();
                device.device_type() == device_type
            })
            .map(|d| {
                let device = d.lock().unwrap();
                device.snapshot()
            })
            .collect()
    }
}

impl Default for DeviceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keyboard_device() {
        let keyboard = KeyboardDevice::new(1);
        assert_eq!(keyboard.device_id(), 1);
        assert_eq!(keyboard.device_type(), DeviceType::CharDevice);
        assert!(!keyboard.has_key());

        keyboard.key_press('A');
        assert!(keyboard.has_key());
        assert_eq!(keyboard.read_char(), Some('A'));
        assert!(!keyboard.has_key());
    }

    #[test]
    fn test_serial_device() {
        let serial = SerialDevice::new(2, 115200);
        assert_eq!(serial.device_id(), 2);
        assert_eq!(serial.device_type(), DeviceType::CharDevice);

        serial.write_byte(0x48).unwrap();  // 'H'
        serial.write_byte(0x65).unwrap();  // 'e'

        let tx_data = serial.get_tx_data();
        assert_eq!(tx_data, vec![0x48, 0x65]);
    }

    #[test]
    fn test_network_device() {
        let mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
        let nic = NetworkDevice::new(3, mac);

        assert_eq!(nic.device_id(), 3);
        assert_eq!(nic.device_type(), DeviceType::NetworkDevice);
        assert_eq!(nic.mac_address(), mac);

        // Test sending packet (goes to TX queue)
        let packet = vec![0x00, 0x01, 0x02, 0x03];
        nic.send_packet(&packet).unwrap();
        assert!(!nic.has_packet());  // RX queue is still empty

        // Test receiving packet (from RX queue)
        nic.inject_packet(&packet);
        assert!(nic.has_packet());  // Now RX queue has data
        let received = nic.receive_packet().unwrap();
        assert_eq!(received, packet);
        assert!(!nic.has_packet());  // RX queue is empty again
    }

    #[test]
    fn test_device_registry() {
        let registry = DeviceRegistry::new();
        let keyboard = Arc::new(Mutex::new(KeyboardDevice::new(10)));

        let device_id = registry.register_device(keyboard);
        assert_eq!(device_id, 1);  // First device gets ID 1

        let devices = registry.list_devices();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].device_type, DeviceType::CharDevice);

        let retrieved = registry.get_device(device_id);
        assert!(retrieved.is_some());
    }
}
