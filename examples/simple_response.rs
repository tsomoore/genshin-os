// 响应机制使用示例
//
// 简单演示如何使用 genshin-OS 的请求-响应机制

use crossbeam_channel::RecvTimeoutError;
use genshin_os::{
    KernelMsg, LockedBus, MemoryRequest, MessageBus, RequestWithResponse, Response, ResponseData,
    ServiceError,
};
use std::sync::Arc;
use std::thread;

fn main() {
    println!("=== genshin-OS 响应机制示例 ===\n");

    // 创建消息总线
    let bus = Arc::new(LockedBus::new());

    // 示例 1: Fire-and-forget 模式（原有方式）
    println!("1. Fire-and-forget 模式:");
    let msg = KernelMsg::Memory(MemoryRequest::AllocFrame { count: 1 });
    match bus.send(msg) {
        Ok(_) => println!("   ✓ 消息已发送（不等待响应）\n"),
        Err(e) => println!("   ✗ 发送失败: {}\n", e),
    }

    // 示例 2: 请求-响应模式
    println!("2. 请求-响应模式:");

    // 创建请求
    let msg = KernelMsg::Memory(MemoryRequest::AllocFrame { count: 1 });
    let (req, rx) = RequestWithResponse::new(msg);

    // 发送请求消息
    let _ = bus.send(req.message.clone());

    // 在另一个线程模拟服务端处理
    thread::spawn(move || {
        thread::sleep(std::time::Duration::from_millis(100));
        let _ = req.respond_success(ResponseData::PhysicalAddr(0x1000));
    });

    // 等待响应
    match rx.recv() {
        Ok(resp) => {
            if resp.is_success() {
                println!("   ✓ 请求成功: {}", resp.unwrap_data());
            } else {
                println!("   ✗ 请求失败: {}", resp.service_error().unwrap());
            }
        }
        Err(e) => println!("   ✗ 接收响应失败: {:?}\n", e),
    }

    // 示例 3: 带超时的请求
    println!("3. 带超时的请求:");

    let msg = KernelMsg::Memory(MemoryRequest::AllocFrame { count: 1 });
    let (req, rx) = RequestWithResponse::with_timeout(msg, 500);

    let _ = bus.send(req.message.clone());

    // 模拟延迟响应（超过超时时间）
    thread::spawn(move || {
        thread::sleep(std::time::Duration::from_millis(1000));
        let _ = req.respond_success(ResponseData::PhysicalAddr(0x2000));
    });

    match rx.recv_timeout(std::time::Duration::from_millis(200)) {
        Ok(resp) => println!("   ✓ 收到响应: {}\n", resp),
        Err(RecvTimeoutError::Timeout) => println!("   ✓ 请求超时（预期行为）\n"),
        Err(e) => println!("   ✗ 接收失败: {:?}\n", e),
    }

    println!("=== 示例完成 ===");
}
