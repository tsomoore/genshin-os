// 响应机制使用示例
//
// 演示如何使用 genshin-OS 的请求-响应机制

use genshin_os::{
    KernelMsg, ProcessRequest, MemoryRequest,
    MessageBus, LockedBus,
    RequestWithResponse, Response, ResponseData, ServiceError,
};
use std::sync::Arc;
use std::thread;

fn main() {
    println!("=== genshin-OS 响应机制示例 ===\n");

    // 创建消息总线
    let bus = Arc::new(LockedBus::new());

    // 示例 1: Fire-and-forget 模式（原有方式）
    println!("1. Fire-and-forget 模式:");
    fire_and_forget_example(&bus);

    // 示例 2: 请求-响应模式（新功能）
    println!("\n2. 请求-响应模式:");
    request_response_example(&bus);

    // 示例 3: 带超时的请求
    println!("\n3. 带超时的请求:");
    timeout_example(&bus);

    // 示例 4: 服务端处理
    println!("\n4. 服务端处理示例:");
    service_example(&bus);
}

/// 示例 1: Fire-and-forget 模式
fn fire_and_forget_example(bus: &Arc<LockedBus>) {
    // 发送消息，不等待响应
    let msg = KernelMsg::Process(ProcessRequest::Schedule {
        pid: 1,
        tid: 1,
    });

    match bus.send(msg) {
        Ok(_) => println!("   ✓ 消息已发送（fire-and-forget）"),
        Err(e) => println!("   ✗ 发送失败: {}", e),
    }
}

/// 示例 2: 请求-响应模式
fn request_response_example(bus: &Arc<LockedBus>) {
    // 发送请求并等待响应
    let msg = KernelMsg::Memory(MemoryRequest::AllocFrame { count: 1 });

    // 创建请求
    let (req, rx) = RequestWithResponse::new(msg);

    // 发送请求
    let _ = bus.send(req.message.clone());

    // 在实际场景中，这里会由服务端处理请求并发送响应
    // 为了演示，我们在另一个线程模拟服务端
    let bus_clone = bus.clone();
    thread::spawn(move || {
        // 模拟服务端处理
        thread::sleep(std::time::Duration::from_millis(100));

        // 发送成功响应
        let _ = req.respond_success(ResponseData::PhysicalAddr(0x1000));
    });

    // 等待响应
    match rx.recv() {
        Ok(resp) => {
            if resp.is_success() {
                println!("   ✓ 请求成功: {}", resp.unwrap_data());
            } else {
                println!("   ✗ 请求失败: {}", resp.unwrap_error());
            }
        }
        Err(e) => println!("   ✗ 接收响应失败: {:?}", e),
    }
}

/// 示例 3: 带超时的请求
fn timeout_example(bus: &Arc<LockedBus>) {
    let msg = KernelMsg::Memory(MemoryRequest::AllocFrame { count: 1 });

    // 创建带超时的请求（1 秒超时）
    let (req, rx) = RequestWithResponse::with_timeout(msg, 1000);

    println!("   发送带超时的请求（1000ms）...");

    // 模拟服务端延迟响应
    let bus_clone = bus.clone();
    thread::spawn(move || {
        // 延迟 2 秒（超过超时时间）
        thread::sleep(std::time::Duration::from_millis(2000));
        let _ = req.respond_success(ResponseData::PhysicalAddr(0x1000));
    });

    // 等待响应（会超时）
    match rx.recv_timeout(std::time::Duration::from_millis(1500)) {
        Ok(resp) => {
            println!("   ✓ 收到响应: {}", resp);
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            println!("   ✗ 请求超时（预期行为）");
        }
        Err(e) => {
            println!("   ✗ 接收失败: {:?}", e);
        }
    }
}

/// 示例 4: 完整的服务端处理
fn service_example(bus: &Arc<LockedBus>) {
    println!("   启动服务端...");

    // 订阅消息
    let receiver = bus.subscribe();

    // 在后台线程处理请求
    thread::spawn(move || {
        // 处理几个请求后退出
        for _ in 0..3 {
            match receiver.recv() {
                Ok(msg) => {
                    println!("   服务端收到消息: {:?}", msg);

                    // 这里应该处理消息并发送响应
                    // 但由于我们用的是 fire-and-forget 的订阅，
                    // 实际的服务端需要使用不同的方式来获取 RequestWithResponse
                }
                Err(e) => {
                    println!("   服务端接收失败: {:?}", e);
                    break;
                }
            }
        }
    });

    // 等待一下让服务端处理
    thread::sleep(std::time::Duration::from_millis(100));
    println!("   服务端示例完成");
}

/// 实际的服务端处理模式（推荐）
///
/// 在实际应用中，服务端可以这样实现：
fn actual_service_pattern() {
    let bus = Arc::new(LockedBus::new());

    // 订阅消息
    let receiver = bus.subscribe();

    thread::spawn(move || {
        loop {
            match receiver.recv() {
                Ok(msg) => {
                    // 处理消息
                    handle_service_message(msg);
                }
                Err(_) => break,
            }
        }
    });
}

/// 处理服务消息
fn handle_service_message(msg: KernelMsg) {
    match msg {
        KernelMsg::Memory(MemoryRequest::AllocFrame { count }) => {
            // 处理内存分配请求
            // 这里通常会：
            // 1. 分配物理内存
            // 2. 通过响应通道返回结果

            println!("处理内存分配请求: {} 帧", count);
            // 实际实现会通过 RequestWithResponse 的响应通道返回结果
        }
        _ => {
            println!("未处理的消息类型");
        }
    }
}
