# File System 和 Device Manager 硬件支持指南

本文档说明 genshin-OS 硬件层为**文件系统**和**设备管理器**提供的接口和支持。

---

## 📚 目录

- [文件系统硬件支持](#文件系统硬件支持)
  - [块设备接口](#块设备接口)
  - [磁盘分区](#磁盘分区)
  - [文件系统消息协议](#文件系统消息协议)
- [设备管理器硬件支持](#设备管理器硬件支持)
  - [通用设备接口](#通用设备接口)
  - [设备注册表](#设备注册表)
  - [设备管理器消息协议](#设备管理器消息协议)
- [实现示例](#实现示例)
  - [文件系统服务](#文件系统服务)
  - [设备管理器服务](#设备管理器服务)

---

## 🗂️ 文件系统硬件支持

### 块设备接口

硬件层提供统一的 `BlockDevice` trait，所有块设备（磁盘、SSD等）都实现此接口。

```rust
pub trait BlockDevice: Send + Sync {
    /// 读取一个块
    fn read_block(&self, block_id: u64) -> GenshinResult<Vec<u8>>;

    /// 写入一个块
    fn write_block(&self, block_id: u64, data: &[u8]) -> GenshinResult<()>;

    /// 获取总块数
    fn total_blocks(&self) -> u64;

    /// 获取块大小（字节）
    fn block_size(&self) -> usize;

    /// 刷新缓存到设备
    fn flush(&self) -> GenshinResult<()>;

    /// 获取设备名称
    fn device_name(&self) -> &str;
}
```

**常量定义**：
- `SECTOR_SIZE = 512` - 扇区大小
- `BLOCK_SIZE = 4096` - 块大小（8个扇区）

### 可用的块设备实现

#### 1. PhysicalBlockDevice

将 `VirtualDisk` 封装为 `BlockDevice`：

```rust
use genshin_os::{VirtualDisk, PhysicalBlockDevice, BlockDevice};

// 创建虚拟磁盘
let disk = Arc::new(VirtualDisk::new(1024));  // 1024 扇区

// 创建块设备
let block_device = PhysicalBlockDevice::new(disk, "sda".to_string());

// 使用块设备接口
let block_data = block_device.read_block(0)?;
block_device.write_block(0, &[0u8; 4096])?;
```

#### 2. PartitionDevice

表示磁盘上的单个分区：

```rust
use genshin_os::{Partition, PartitionType, PartitionDevice};

// 定义分区
let partition = Partition {
    number: 1,
    start_block: 0,
    total_blocks: 1024,
    partition_type: PartitionType::EXT4,
    bootable: true,
};

// 创建分区设备
let partition_device = PartitionDevice::new(
    Arc::new(block_device),
    partition
);

// 分区设备支持完整的 BlockDevice 接口
let data = partition_device.read_block(0)?;
```

### 磁盘分区支持

硬件层提供分区数据结构和布局管理：

```rust
pub struct Partition {
    pub number: u8,           // 分区号（从1开始）
    pub start_block: u64,     // 起始块号
    pub total_blocks: u64,    // 总块数
    pub partition_type: PartitionType,
    pub bootable: bool,
}

pub enum PartitionType {
    Empty,
    FAT32,
    EXT4,
    LinuxSwap,
    Unknown(u8),
}

pub struct PartitionLayout {
    pub partitions: Vec<Partition>,
}
```

### 文件系统消息协议

File System 服务通过 `KernelMsg::File(FileRequest)` 接收请求：

#### 文件操作

```rust
pub enum FileRequest {
    // 打开文件
    Open { path: String, flags: OpenFlags },

    // 关闭文件
    Close { fd: u32 },

    // 读取文件
    Read {
        fd: u32,
        offset: u64,
        buf: VirtAddr,  // 进程虚拟地址
        size: usize,
    },

    // 写入文件
    Write {
        fd: u32,
        offset: u64,
        buf: VirtAddr,
        size: usize,
    },

    // 删除文件
    Unlink { path: String },

    // 获取文件元数据
    Stat { path: String },
}
```

#### 目录操作

```rust
pub enum FileRequest {
    // 创建目录
    CreateDirectory { path: String },

    // 删除目录
    RemoveDirectory { path: String },

    // 打开目录
    OpenDirectory { path: String },

    // 读取目录项
    ReadDirectory { dir_fd: u32 },

    // 关闭目录
    CloseDirectory { dir_fd: u32 },
}
```

#### 文件系统管理

```rust
pub enum FileRequest {
    // 挂载文件系统
    Mount {
        device_id: u32,
        mount_point: String,
        fs_type: FileSystemType,
    },

    // 卸载文件系统
    Unmount { mount_point: String },

    // 同步文件系统缓冲
    Sync,
}

pub enum FileSystemType {
    FAT32,
    EXT4,
    SimpleFS,   // 简单文件系统（教学用）
    ProcFS,     // 虚拟文件系统
    Unknown,
}
```

#### 文件元数据

```rust
pub enum FileRequest {
    // 修改文件权限
    Chmod { path: String, mode: u32 },

    // 修改文件所有者
    Chown { path: String, uid: u32, gid: u32 },

    // 创建硬链接
    Link { oldpath: String, newpath: String },

    // 创建符号链接
    Symlink { oldpath: String, newpath: String },

    // 读取符号链接
    Readlink { path: String },
}
```

#### 文件定位

```rust
pub enum FileRequest {
    // 定位文件位置
    Seek {
        fd: u32,
        offset: i64,
        whence: SeekWhence,  // Set/Cur/End
    },

    // 获取当前位置
    Tell { fd: u32 },
}

pub enum SeekWhence {
    Set = 0,  // 从文件开始
    Cur = 1,  // 从当前位置
    End = 2,  // 从文件末尾
}
```

---

## 🔧 设备管理器硬件支持

### 通用设备接口

硬件层提供 `Device` trait，所有设备都必须实现此接口：

```rust
pub trait Device: Send + Sync {
    /// 获取设备ID
    fn device_id(&self) -> u32;

    /// 获取设备类型
    fn device_type(&self) -> DeviceType;

    /// 获取设备名称
    fn device_name(&self) -> &str;

    /// 获取设备状态
    fn status(&self) -> DeviceStatus;

    /// 初始化设备
    fn init(&mut self) -> GenshinResult<()>;

    /// 关闭设备
    fn shutdown(&mut self) -> GenshinResult<()>;

    /// 重置设备
    fn reset(&mut self) -> GenshinResult<()>;

    /// 获取设备快照
    fn snapshot(&self) -> DeviceSnapshot;
}

pub enum DeviceType {
    BlockDevice,      // 块设备
    CharDevice,       // 字符设备
    NetworkDevice,    // 网络设备
    TimerDevice,      // 定时器设备
    GraphicsDevice,   // 图形设备
    Unknown,
}

pub enum DeviceStatus {
    Online,       // 在线
    Offline,      // 离线
    Error,        // 错误
    Busy,         // 忙碌
    Initializing, // 初始化中
}
```

### 可用的设备实现

#### 1. KeyboardDevice (键盘设备)

```rust
use genshin_os::{KeyboardDevice, Device};

let keyboard = KeyboardDevice::new(1);

// 模拟按键
keyboard.key_press('A');
keyboard.key_press('B');

// 检查是否有键
if keyboard.has_key() {
    let c = keyboard.read_char();  // 读取 'A'
}
```

#### 2. SerialDevice (串口设备)

```rust
use genshin_os::{SerialDevice, Device};

let serial = SerialDevice::new(2, 115200);  // 设备ID 2, 波特率 115200

// 写入数据
serial.write_byte(0x48)?;  // 'H'
serial.write_byte(0x65)?;  // 'e'

// 模拟接收数据
serial.receive_data(&[0x48, 0x69]);

// 读取数据
if let Some(byte) = serial.read_byte() {
    println!("Received: 0x{:02X}", byte);
}
```

#### 3. NetworkDevice (网卡设备)

```rust
use genshin_os::{NetworkDevice, Device};

let mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
let nic = NetworkDevice::new(3, mac);

// 发送数据包
let packet = vec![0x00, 0x01, 0x02, 0x03];
nic.send_packet(&packet)?;

// 接收数据包
if nic.has_packet() {
    let received = nic.receive_packet().unwrap();
}

// 模拟收到数据包
nic.inject_packet(&[0xFF, 0xFF, 0xFF, 0xFF]);
```

### 设备注册表

硬件层提供 `DeviceRegistry` 来管理所有设备：

```rust
use genshin_os::{DeviceRegistry, Device, KeyboardDevice};

let registry = DeviceRegistry::new();

// 注册设备
let keyboard = Arc::new(Mutex::new(KeyboardDevice::new(10)));
let device_id = registry.register_device(keyboard);

// 获取设备
if let Some(device) = registry.get_device(device_id) {
    let device = device.lock().unwrap();
    println!("Device: {}", device.device_name());
}

// 列出所有设备
let devices = registry.list_devices();
for snapshot in devices {
    println!("{}: {:?}", snapshot.name, snapshot.status);
}

// 按类型获取设备
let char_devices = registry.get_devices_by_type(DeviceType::CharDevice);
```

### 设备管理器消息协议

Device Manager 服务通过 `KernelMsg::Device(DeviceRequest)` 接收请求：

#### 基本 I/O

```rust
pub enum DeviceRequest {
    // 从设备读取
    Read {
        device_id: u32,
        buf: VirtAddr,
        size: usize,
    },

    // 向设备写入
    Write {
        device_id: u32,
        buf: VirtAddr,
        size: usize,
    },
}
```

#### 设备生命周期

```rust
pub enum DeviceRequest {
    // 初始化设备
    Init { device_id: u32 },

    // 关闭设备
    Shutdown { device_id: u32 },

    // 重置设备
    Reset { device_id: u32 },

    // 查询设备状态
    Status { device_id: u32 },
}
```

#### 设备管理

```rust
pub enum DeviceRequest {
    // 注册新设备
    RegisterDevice {
        device_type: DeviceClass,
        name: String,
    },

    // 注销设备
    UnregisterDevice { device_id: u32 },

    // 列出所有设备
    ListDevices,

    // 按类型获取设备
    GetDevicesByType { device_type: DeviceClass },
}

pub enum DeviceClass {
    Block,
    Char,
    Network,
    Timer,
    Graphics,
    Unknown,
}
```

#### I/O 控制

```rust
pub enum DeviceRequest {
    // I/O 控制命令（设备特定）
    Ioctl {
        device_id: u32,
        request: u32,
        arg: VirtAddr,
    },
}
```

#### 设备配置

```rust
pub enum DeviceRequest {
    // 设置设备配置
    SetConfig {
        device_id: u32,
        config: DeviceConfig,
    },

    // 获取设备配置
    GetConfig { device_id: u32 },
}

pub enum DeviceConfig {
    // 串口配置
    Serial {
        baud_rate: u32,
        data_bits: u8,
        stop_bits: u8,
        parity: char,
    },

    // 网卡配置
    Network {
        ip_address: String,
        netmask: String,
        gateway: String,
        mac_address: [u8; 6],
    },

    // 块设备配置
    Block {
        block_size: u32,
        read_only: bool,
    },

    // 通用键值配置
    Generic { key: String, value: String },
}
```

---

## 💡 实现示例

### 文件系统服务框架

```rust
use genshin_os::{
    KernelMsg, FileRequest, MessageBus,
    BlockDevice, Partition, PartitionType,
    GenshinResult, GenshinError,
};

pub struct FileSystemService {
    bus: Arc<dyn MessageBus>,
    rx: Receiver<KernelMsg>,
    // 文件系统内部状态
    mounted_filesystems: HashMap<String, Arc<dyn BlockDevice>>,
    open_files: HashMap<u32, OpenFile>,
    next_fd: u32,
}

struct OpenFile {
    fd: u32,
    path: String,
    offset: u64,
    flags: OpenFlags,
}

impl FileSystemService {
    pub fn new(bus: Arc<dyn MessageBus>) -> Self {
        let rx = bus.subscribe();
        Self {
            bus,
            rx,
            mounted_filesystems: HashMap::new(),
            open_files: HashMap::new(),
            next_fd: 3,  // 0, 1, 2 保留给 stdin, stdout, stderr
        }
    }

    pub fn run(&self) {
        loop {
            match self.rx.recv() {
                Ok(KernelMsg::File(req)) => {
                    if let Err(e) = self.handle_request(req) {
                        eprintln!("FileService error: {}", e);
                    }
                }
                _ => {}
            }
        }
    }

    fn handle_request(&self, req: FileRequest) -> GenshinResult<()> {
        match req {
            FileRequest::Mount { device_id, mount_point, fs_type } => {
                self.mount(device_id, mount_point, fs_type)?;
            }

            FileRequest::Open { path, flags } => {
                let fd = self.open(path, flags)?;
                // 通过响应机制返回 fd
            }

            FileRequest::Read { fd, offset, buf, size } => {
                self.read(fd, offset, buf, size)?;
            }

            FileRequest::Write { fd, offset, buf, size } => {
                self.write(fd, offset, buf, size)?;
            }

            // ... 处理其他请求
            _ => {}
        }
        Ok(())
    }

    fn mount(&self, device_id: u32, mount_point: String, fs_type: FileSystemType)
        -> GenshinResult<()>
    {
        // 1. 从 DeviceManager 获取块设备
        // 2. 根据 fs_type 初始化文件系统
        // 3. 注册到 mounted_filesystems
        Ok(())
    }

    fn open(&self, path: String, flags: OpenFlags) -> GenshinResult<u32> {
        // 1. 解析路径，找到对应的挂载点
        // 2. 在文件系统中查找文件
        // 3. 分配文件描述符
        Ok(0)
    }

    fn read(&self, fd: u32, offset: u64, buf: VirtAddr, size: usize)
        -> GenshinResult<()>
    {
        // 1. 根据 fd 找到打开的文件
        // 2. 从底层块设备读取数据
        // 3. 写入到进程虚拟地址
        Ok(())
    }

    fn write(&self, fd: u32, offset: u64, buf: VirtAddr, size: usize)
        -> GenshinResult<()>
    {
        // 1. 根据 fd 找到打开的文件
        // 2. 从进程虚拟地址读取数据
        // 3. 写入到底层块设备
        Ok(())
    }
}
```

### 设备管理器服务框架

```rust
use genshin_os::{
    KernelMsg, DeviceRequest, MessageBus, Device,
    DeviceRegistry, DeviceType, DeviceClass,
    GenshinResult,
};

pub struct DeviceManagerService {
    bus: Arc<dyn MessageBus>,
    rx: Receiver<KernelMsg>,
    registry: DeviceRegistry,
}

impl DeviceManagerService {
    pub fn new(bus: Arc<dyn MessageBus>) -> Self {
        let rx = bus.subscribe();
        let registry = DeviceRegistry::new();

        let mut service = Self {
            bus,
            rx,
            registry,
        };

        // 初始化基础设备
        service.initialize_builtin_devices();

        service
    }

    fn initialize_builtin_devices(&mut self) {
        // 注册键盘
        let keyboard = Arc::new(Mutex::new(KeyboardDevice::new(1)));
        self.registry.register_device(keyboard);

        // 注册串口
        let serial = Arc::new(Mutex::new(SerialDevice::new(2, 115200)));
        self.registry.register_device(serial);

        // 注册网卡
        let nic = Arc::new(Mutex::new(NetworkDevice::new(
            3,
            [0x52, 0x54, 0x00, 0x12, 0x34, 0x56]
        )));
        self.registry.register_device(nic);
    }

    pub fn run(&self) {
        loop {
            match self.rx.recv() {
                Ok(KernelMsg::Device(req)) => {
                    if let Err(e) = self.handle_request(req) {
                        eprintln!("DeviceManager error: {}", e);
                    }
                }
                _ => {}
            }
        }
    }

    fn handle_request(&self, req: DeviceRequest) -> GenshinResult<()> {
        match req {
            DeviceRequest::Init { device_id } => {
                self.init_device(device_id)?;
            }

            DeviceRequest::Shutdown { device_id } => {
                self.shutdown_device(device_id)?;
            }

            DeviceRequest::Status { device_id } => {
                self.get_status(device_id)?;
            }

            DeviceRequest::ListDevices => {
                let devices = self.registry.list_devices();
                // 返回设备列表
            }

            DeviceRequest::GetDevicesByType { device_type } => {
                let device_type = self.map_device_class(device_type);
                let devices = self.registry.get_devices_by_type(device_type);
                // 返回设备列表
            }

            DeviceRequest::Read { device_id, buf, size } => {
                self.read_device(device_id, buf, size)?;
            }

            DeviceRequest::Write { device_id, buf, size } => {
                self.write_device(device_id, buf, size)?;
            }

            // ... 处理其他请求
            _ => {}
        }
        Ok(())
    }

    fn init_device(&self, device_id: u32) -> GenshinResult<()> {
        if let Some(device) = self.registry.get_device(device_id) {
            let mut device = device.lock().unwrap();
            device.init()?;
        }
        Ok(())
    }

    fn shutdown_device(&self, device_id: u32) -> GenshinResult<()> {
        if let Some(device) = self.registry.get_device(device_id) {
            let mut device = device.lock().unwrap();
            device.shutdown()?;
        }
        Ok(())
    }

    fn get_status(&self, device_id: u32) -> GenshinResult<()> {
        if let Some(device) = self.registry.get_device(device_id) {
            let device = device.lock().unwrap();
            let snapshot = device.snapshot();
            // 返回设备状态
        }
        Ok(())
    }

    fn read_device(&self, device_id: u32, buf: VirtAddr, size: usize)
        -> GenshinResult<()>
    {
        // 1. 获取设备
        // 2. 根据设备类型调用特定的读取接口
        // 3. 将数据写入到进程虚拟地址
        Ok(())
    }

    fn write_device(&self, device_id: u32, buf: VirtAddr, size: usize)
        -> GenshinResult<()>
    {
        // 1. 获取设备
        // 2. 从进程虚拟地址读取数据
        // 3. 根据设备类型调用特定的写入接口
        Ok(())
    }

    fn map_device_class(&self, class: DeviceClass) -> DeviceType {
        match class {
            DeviceClass::Block => DeviceType::BlockDevice,
            DeviceClass::Char => DeviceType::CharDevice,
            DeviceClass::Network => DeviceType::NetworkDevice,
            DeviceClass::Timer => DeviceType::TimerDevice,
            DeviceClass::Graphics => DeviceType::GraphicsDevice,
            DeviceClass::Unknown => DeviceType::Unknown,
        }
    }
}
```

---

## 🔗 相关文档

- [硬件层与内核服务层协作指南](HARDWARE_SERVICE_COORDINATION.md)
- [API 快速参考](API_QUICK_REFERENCE.md)
- [完整接口文档](INTERFACE_REVIEW.md)

---

**最后更新**: 2026-03-24
