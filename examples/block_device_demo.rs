// Block Device and Device Manager Demo
//
// 演示 genshin-OS 中块设备和设备管理器的使用

use genshin_os::{
    // 块设备相关
    VirtualDisk, PhysicalBlockDevice, PartitionDevice,
    BlockDevice, Partition, PartitionType, PartitionLayout,
    DISK_SECTOR_SIZE, DISK_BLOCK_SIZE,

    // 设备管理相关
    Device, DeviceType, DeviceStatus, DeviceRegistry,
    KeyboardDevice, SerialDevice, NetworkDevice,

    // 消息总线
    KernelMsg, MessageBus, LockedBus,
    FileRequest, FileSystemType, SeekWhence,
    DeviceRequest, DeviceClass,

    // 其他
    GenshinResult, GenshinError,
};

use std::sync::Arc;

fn main() -> GenshinResult<()> {
    println!("=== genshin-OS 块设备和设备管理器演示 ===\n");

    // ========================================
    // 1. 块设备演示
    // ========================================
    println!("1. 块设备演示:");
    demo_block_devices()?;

    // ========================================
    // 2. 磁盘分区演示
    // ========================================
    println!("\n2. 磁盘分区演示:");
    demo_partitions()?;

    // ========================================
    // 3. 设备管理演示
    // ========================================
    println!("\n3. 设备管理演示:");
    demo_device_manager()?;

    // ========================================
    // 4. 文件系统消息演示
    // ========================================
    println!("\n4. 文件系统消息演示:");
    demo_filesystem_messages()?;

    // ========================================
    // 5. 设备管理器消息演示
    // ========================================
    println!("\n5. 设备管理器消息演示:");
    demo_device_manager_messages()?;

    println!("\n=== 演示完成 ===");
    Ok(())
}

/// 演示块设备的基本使用
fn demo_block_devices() -> GenshinResult<()> {
    // 创建虚拟磁盘
    let disk = Arc::new(VirtualDisk::new(1024));  // 1024 扇区
    println!("   ✓ 创建虚拟磁盘: 1024 扇区 ({} KB)", 1024 * DISK_SECTOR_SIZE / 1024);

    // 创建块设备
    let block_device = PhysicalBlockDevice::new(disk, "sda".to_string());
    println!("   ✓ 创建块设备: {}", block_device.device_name());
    println!("   ✓ 块大小: {} 字节", block_device.block_size());
    println!("   ✓ 总块数: {}", block_device.total_blocks());

    // 读取块
    let block_data = block_device.read_block(0)?;
    println!("   ✓ 读取块 0: {} 字节", block_data.len());

    // 写入块
    let write_data = vec![0xABu8; DISK_BLOCK_SIZE];
    block_device.write_block(0, &write_data)?;
    println!("   ✓ 写入块 0: {} 字节 (0xAB...)", write_data.len());

    // 刷新
    block_device.flush()?;
    println!("   ✓ 刷新设备缓存");

    Ok(())
}

/// 演示磁盘分区
fn demo_partitions() -> GenshinResult<()> {
    // 创建磁盘和块设备
    let disk = Arc::new(VirtualDisk::new(2048));
    let block_device = Arc::new(PhysicalBlockDevice::new(disk, "sdb".to_string()));

    // 创建分区布局
    let mut layout = PartitionLayout::new();

    // 分区 1: EXT4, 256 块
    let part1 = Partition {
        number: 1,
        start_block: 0,
        total_blocks: 256,
        partition_type: PartitionType::EXT4,
        bootable: true,
    };
    layout.add_partition(part1);
    println!("   ✓ 创建分区 1: EXT4, 256 块, 可启动");

    // 分区 2: Linux Swap, 128 块
    let part2 = Partition {
        number: 2,
        start_block: 256,
        total_blocks: 128,
        partition_type: PartitionType::LinuxSwap,
        bootable: false,
    };
    layout.add_partition(part2);
    println!("   ✓ 创建分区 2: Linux Swap, 128 块");

    // 分区 3: FAT32, 512 块
    let part3 = Partition {
        number: 3,
        start_block: 384,
        total_blocks: 512,
        partition_type: PartitionType::FAT32,
        bootable: false,
    };
    layout.add_partition(part3);
    println!("   ✓ 创建分区 3: FAT32, 512 块");

    // 使用分区设备
    if let Some(part1_info) = layout.get_partition(1) {
        let partition_device = PartitionDevice::new(block_device.clone(), *part1_info);
        println!("   ✓ 创建分区设备: 分区 1");

        // 对分区进行读写
        let data = partition_device.read_block(0)?;
        println!("   ✓ 从分区 1 读取块 0: {} 字节", data.len());
    }

    Ok(())
}

/// 演示设备管理器
fn demo_device_manager() -> GenshinResult<()> {
    let registry = DeviceRegistry::new();

    // 注册键盘
    let keyboard = Arc::new(std::sync::Mutex::new(KeyboardDevice::new(1)));
    let keyboard_id = registry.register_device(keyboard);
    println!("   ✓ 注册键盘设备 (ID: {})", keyboard_id);

    // 模拟按键
    let kbd = registry.get_device(keyboard_id).unwrap();
    let kbd = kbd.lock().unwrap();
    if let Some(kb) = kbd.as_any().downcast_ref::<KeyboardDevice>() {
        kb.key_press('H');
        kb.key_press('e');
        kb.key_press('l');
        kb.key_press('l');
        kb.key_press('o');
        println!("   ✓ 模拟按键: 'Hello'");
    }

    // 注册串口
    let serial = Arc::new(std::sync::Mutex::new(SerialDevice::new(2, 115200)));
    let serial_id = registry.register_device(serial);
    println!("   ✓ 注册串口设备 (ID: {}, 波特率: 115200)", serial_id);

    // 注册网卡
    let nic = Arc::new(std::sync::Mutex::new(NetworkDevice::new(
        3,
        [0x52, 0x54, 0x00, 0x12, 0x34, 0x56]
    )));
    let nic_id = registry.register_device(nic);
    println!("   ✓ 注册网卡设备 (ID: {}, MAC: 52:54:00:12:34:56)", nic_id);

    // 列出所有设备
    let devices = registry.list_devices();
    println!("   ✓ 总设备数: {}", devices.len());
    for device in devices {
        println!("     - {}: {:?} ({:?})", device.name, device.device_type, device.status);
    }

    // 按类型查询
    let char_devices = registry.get_devices_by_type(DeviceType::CharDevice);
    println!("   ✓ 字符设备数: {}", char_devices.len());

    Ok(())
}

/// 演示文件系统消息
fn demo_filesystem_messages() -> GenshinResult<()> {
    let bus = Arc::new(LockedBus::new());

    // 打开文件
    let msg = KernelMsg::File(FileRequest::Open {
        path: "/etc/hosts".to_string(),
        flags: genshin_os::OpenFlags::read_only(),
    });
    let _ = bus.send(msg);
    println!("   ✓ 请求打开文件: /etc/hosts (只读)");

    // 读取文件
    let msg = KernelMsg::File(FileRequest::Read {
        fd: 3,
        offset: 0,
        buf: 0x1000,
        size: 1024,
    });
    let _ = bus.send(msg);
    println!("   ✓ 请求读取文件: fd=3, offset=0, size=1024");

    // 写入文件
    let msg = KernelMsg::File(FileRequest::Write {
        fd: 4,
        offset: 0,
        buf: 0x2000,
        size: 512,
    });
    let _ = bus.send(msg);
    println!("   ✓ 请求写入文件: fd=4, offset=0, size=512");

    // 创建目录
    let msg = KernelMsg::File(FileRequest::CreateDirectory {
        path: "/tmp/test".to_string(),
    });
    let _ = bus.send(msg);
    println!("   ✓ 请求创建目录: /tmp/test");

    // 挂载文件系统
    let msg = KernelMsg::File(FileRequest::Mount {
        device_id: 1,
        mount_point: "/mnt/data".to_string(),
        fs_type: FileSystemType::EXT4,
    });
    let _ = bus.send(msg);
    println!("   ✓ 请求挂载文件系统: 设备1 -> /mnt/data (EXT4)");

    // 文件定位
    let msg = KernelMsg::File(FileRequest::Seek {
        fd: 3,
        offset: 1024,
        whence: SeekWhence::Set,
    });
    let _ = bus.send(msg);
    println!("   ✓ 请求文件定位: fd=3, offset=1024 (从文件开始)");

    Ok(())
}

/// 演示设备管理器消息
fn demo_device_manager_messages() -> GenshinResult<()> {
    let bus = Arc::new(LockedBus::new());

    // 初始化设备
    let msg = KernelMsg::Device(DeviceRequest::Init { device_id: 1 });
    let _ = bus.send(msg);
    println!("   ✓ 请求初始化设备: ID=1");

    // 查询设备状态
    let msg = KernelMsg::Device(DeviceRequest::Status { device_id: 1 });
    let _ = bus.send(msg);
    println!("   ✓ 请求查询设备状态: ID=1");

    // 列出所有设备
    let msg = KernelMsg::Device(DeviceRequest::ListDevices);
    let _ = bus.send(msg);
    println!("   ✓ 请求列出所有设备");

    // 按类型查询设备
    let msg = KernelMsg::Device(DeviceRequest::GetDevicesByType {
        device_type: DeviceClass::Char,
    });
    let _ = bus.send(msg);
    println!("   ✓ 请求查询字符设备");

    // 从设备读取
    let msg = KernelMsg::Device(DeviceRequest::Read {
        device_id: 2,
        buf: 0x1000,
        size: 256,
    });
    let _ = bus.send(msg);
    println!("   ✓ 请求从设备读取: ID=2, size=256");

    // 向设备写入
    let msg = KernelMsg::Device(DeviceRequest::Write {
        device_id: 2,
        buf: 0x2000,
        size: 128,
    });
    let _ = bus.send(msg);
    println!("   ✓ 请求向设备写入: ID=2, size=128");

    // 关闭设备
    let msg = KernelMsg::Device(DeviceRequest::Shutdown { device_id: 1 });
    let _ = bus.send(msg);
    println!("   ✓ 请求关闭设备: ID=1");

    Ok(())
}

// Note: as_any() method would need to be added to the Device trait
// for downcasting to specific device types. This is omitted for brevity.
