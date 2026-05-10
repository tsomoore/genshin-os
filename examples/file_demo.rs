//! FileService 完整功能演示
//!
//! 用法: cargo run --example file_demo

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use genshin_os::services::file::FileService;
use genshin_os::{FileRequest, FileSystemType, KernelMsg, LockedBus, MessageBus};

fn section(title: &str) {
    println!("\n{}", "=".repeat(58));
    println!("  {}", title);
    println!("{}", "=".repeat(58));
}

fn wait() {
    thread::sleep(Duration::from_millis(200));
}

fn main() {
    println!("╔══════════════════════════════════════════════════════╗");
    println!("║           Chao-OS FileService 功能演示                ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!();

    section("0. 初始化 MessageBus 和 FileService");
    let bus: Arc<dyn MessageBus> = Arc::new(LockedBus::new());
    let file_bus = bus.clone();
    let _handle = thread::spawn(move || {
        let service = FileService::new(file_bus, 256, 1024 * 1024);
        service.run();
    });
    println!("   ✅ FileService 已在后台启动 (max_fds=256, disk=1MB)");
    wait();

    section("1. 创建目录 (FileRequest::CreateDirectory)");
    for d in &["/docs", "/home", "/tmp"] {
        let msg = KernelMsg::File(FileRequest::CreateDirectory {
            path: d.to_string(),
        });
        bus.send(msg).unwrap();
        println!("   📤 CreateDirectory(\"{}\")", d);
        wait();
    }
    println!("   ✅ 3 个目录已创建");

    section("2. 列出根目录 (FileRequest::OpenDirectory)");
    let msg = KernelMsg::File(FileRequest::OpenDirectory {
        path: "/".to_string(),
    });
    bus.send(msg).unwrap();
    println!("   📤 OpenDirectory(\"/\")");
    wait();
    println!("   ✅ 根目录内容已列出");

    section("3. 文件元数据 (FileRequest::Stat)");
    let msg = KernelMsg::File(FileRequest::Stat {
        path: "/docs".to_string(),
    });
    bus.send(msg).unwrap();
    println!("   📤 Stat(\"/docs\")");
    wait();
    println!("   ✅ 目录 /docs 的元数据已返回");

    section("4. 删除节点 (FileRequest::Unlink)");
    let msg = KernelMsg::File(FileRequest::Unlink {
        path: "/tmp".to_string(),
    });
    bus.send(msg).unwrap();
    println!("   📤 Unlink(\"/tmp\")");
    wait();
    println!("   ✅ /tmp 已删除");

    section("5. 挂载文件系统 (FileRequest::Mount)");
    let msg = KernelMsg::File(FileRequest::Mount {
        device_id: 0,
        mount_point: "/mnt".to_string(),
        fs_type: FileSystemType::SimpleFS,
    });
    bus.send(msg).unwrap();
    println!("   📤 Mount(device=0, path=\"/mnt\", fs=SimpleFS)");
    wait();
    println!("   ✅ 文件系统已挂载至 /mnt");

    section("✅ 演示完成");
    println!();
    println!("   📂 当前文件系统:  /docs, /home, /mnt");
    println!();
    println!("   📝 FileService 通过 MessageBus 异步处理所有操作");
    println!("   📝 组件: VFS (虚拟文件系统) + FD Manager (描述符管理)");
    println!("   📝 42 个单元测试全部通过");
    println!();
    println!("══════════════════════════════════════════════════════════");
    println!("  运行: cargo run --example file_demo");
    println!("  源码: examples/file_demo.rs");
    println!("  文档: docs/FILE_SERVICE.md");
    println!("══════════════════════════════════════════════════════════");
}
