// IPC 消息格式使用示例
//
// 演示 genshin-OS 中所有进程间通信(IPC)如何通过消息总线进行
//
// 核心原则：
// 1. 所有 IPC 消息都必须经过 KernelMsg → ProcessRequest → MessageBus
// 2. 这样可以监控所有进程间通信
// 3. 内核服务层负责实现真正的进程管理逻辑

use genshin_os::{IPCMessage, KernelMsg, LockedBus, MessageBus, Pid, ProcessRequest, SignalType};
use std::sync::Arc;

fn main() {
    println!("=== genshin-OS IPC 消息格式演示 ===\n");

    let bus = Arc::new(LockedBus::new());

    // ========================================
    // 1. 消息传递 (Message Passing)
    // ========================================
    println!("1. 进程间消息传递:");
    demo_message_passing(&bus);

    // ========================================
    // 2. 共享内存 (Shared Memory)
    // ========================================
    println!("\n2. 共享内存通信:");
    demo_shared_memory(&bus);

    // ========================================
    // 3. 同步原语 (Synchronization)
    // ========================================
    println!("\n3. 进程同步:");
    demo_synchronization(&bus);

    // ========================================
    // 4. 信号 (Signals)
    // ========================================
    println!("\n4. 进程信号:");
    demo_signals(&bus);

    // ========================================
    // 5. 进程生命周期 (Process Lifecycle)
    // ========================================
    println!("\n5. 进程生命周期管理:");
    demo_process_lifecycle(&bus);

    println!("\n=== 演示完成 ===");
}

/// 演示进程间消息传递
fn demo_message_passing(bus: &Arc<LockedBus>) {
    // 进程 A 发送文本消息给进程 B
    let msg = KernelMsg::Process(ProcessRequest::SendMessage {
        from_pid: 100,
        to_pid: 200,
        msg: IPCMessage::Text {
            data: "Hello from process A!".to_string(),
        },
    });
    let _ = bus.send(msg);
    println!("   ✓ 进程 100 → 进程 200: \"Hello from process A!\"");

    // 进程 A 传递文件描述符给进程 B
    let msg = KernelMsg::Process(ProcessRequest::SendMessage {
        from_pid: 100,
        to_pid: 200,
        msg: IPCMessage::PassFd { fd: 4 },
    });
    let _ = bus.send(msg);
    println!("   ✓ 进程 100 → 进程 200: 传递文件描述符 4");

    // 进程 B 接收消息（阻塞）
    let msg = KernelMsg::Process(ProcessRequest::ReceiveMessage {
        pid: 200,
        blocking: true,
    });
    let _ = bus.send(msg);
    println!("   ✓ 进程 200 阻塞等待消息");
}

/// 演示共享内存通信
fn demo_shared_memory(bus: &Arc<LockedBus>) {
    // 进程 A 创建共享内存区
    let msg = KernelMsg::Process(ProcessRequest::CreateSharedMemory {
        pid: 100,
        size: 4096,
        prot: genshin_os::MemProt::read_write(),
    });
    let _ = bus.send(msg);
    println!("   ✓ 进程 100 创建 4KB 共享内存区");

    // 进程 B 附加到共享内存区
    let msg = KernelMsg::Process(ProcessRequest::AttachSharedMemory {
        pid: 200,
        shmid: 1, // 假设返回的 shmid 是 1
    });
    let _ = bus.send(msg);
    println!("   ✓ 进程 200 附加到共享内存区 1");

    // 通过共享内存通知
    let msg = KernelMsg::Process(ProcessRequest::SendMessage {
        from_pid: 100,
        to_pid: 200,
        msg: IPCMessage::SharedMemory { shmid: 1 },
    });
    let _ = bus.send(msg);
    println!("   ✓ 进程 100 通知进程 200: 共享内存区 1 就绪");
}

/// 演示进程同步原语
fn demo_synchronization(bus: &Arc<LockedBus>) {
    // 创建信号量
    let msg = KernelMsg::Process(ProcessRequest::CreateSemaphore {
        pid: 100,
        initial_value: 1,
    });
    let _ = bus.send(msg);
    println!("   ✓ 创建信号量，初始值 = 1");

    // 等待信号量 (P 操作)
    let msg = KernelMsg::Process(ProcessRequest::WaitSemaphore { pid: 100, semid: 1 });
    let _ = bus.send(msg);
    println!("   ✓ 进程 100 等待信号量 1");

    // 创建互斥锁
    let msg = KernelMsg::Process(ProcessRequest::CreateLock { pid: 200 });
    let _ = bus.send(msg);
    println!("   ✓ 进程 200 创建互斥锁");

    // 获取锁
    let msg = KernelMsg::Process(ProcessRequest::AcquireLock {
        pid: 200,
        lock_id: 1,
    });
    let _ = bus.send(msg);
    println!("   ✓ 进程 200 获取锁 1");

    // 释放锁
    let msg = KernelMsg::Process(ProcessRequest::ReleaseLock {
        pid: 200,
        lock_id: 1,
    });
    let _ = bus.send(msg);
    println!("   ✓ 进程 200 释放锁 1");

    // 发送信号量 (V 操作)
    let msg = KernelMsg::Process(ProcessRequest::SignalSemaphore { pid: 100, semid: 1 });
    let _ = bus.send(msg);
    println!("   ✓ 进程 100 发送信号量 1");
}

/// 演示进程信号
fn demo_signals(bus: &Arc<LockedBus>) {
    // 发送 SIGTERM 信号
    let msg = KernelMsg::Process(ProcessRequest::Signal {
        pid: 200,
        signal: SignalType::Terminate,
    });
    let _ = bus.send(msg);
    println!("   ✓ 向进程 200 发送 SIGTERM");

    // 发送 SIGUSR1 用户自定义信号
    let msg = KernelMsg::Process(ProcessRequest::Signal {
        pid: 200,
        signal: SignalType::User1,
    });
    let _ = bus.send(msg);
    println!("   ✓ 向进程 200 发送 SIGUSR1");
}

/// 演示进程生命周期管理
fn demo_process_lifecycle(bus: &Arc<LockedBus>) {
    // Fork 进程
    let msg = KernelMsg::Process(ProcessRequest::ForkProcess { parent_pid: 100 });
    let _ = bus.send(msg);
    println!("   ✓ 进程 100 fork 子进程");

    // 执行新程序
    let msg = KernelMsg::Process(ProcessRequest::ExecProcess {
        pid: 101,
        executable: "/bin/test".to_string(),
        args: vec!["arg1".to_string(), "arg2".to_string()],
    });
    let _ = bus.send(msg);
    println!("   ✓ 进程 101 执行 /bin/test");

    // 等待子进程
    let msg = KernelMsg::Process(ProcessRequest::WaitChild {
        pid: 100,
        child_pid: Some(101),
    });
    let _ = bus.send(msg);
    println!("   ✓ 进程 100 等待进程 101 结束");

    // 查询进程信息
    let msg = KernelMsg::Process(ProcessRequest::GetProcessInfo { pid: 100 });
    let _ = bus.send(msg);
    println!("   ✓ 查询进程 100 的信息");

    // 列出所有进程
    let msg = KernelMsg::Process(ProcessRequest::ListProcesses);
    let _ = bus.send(msg);
    println!("   ✓ 请求列出所有进程");
}

// ========================================
// 内核服务层实现示例（仅供参考）
// ========================================
//
// 负责内核服务的同学需要实现一个 ProcessService，
// 订阅 ProcessRequest 类型的消息并处理：
//
// ```rust
// use genshin_os::{KernelMsg, ProcessRequest, MessageBus};
//
// struct ProcessService {
//     bus: Arc<dyn MessageBus>,
//     // 进程表、PCB 等数据结构
// }
//
// impl ProcessService {
//     fn run(&self) {
//         let rx = self.bus.subscribe();
//
//         loop {
//             match rx.recv() {
//                 Ok(KernelMsg::Process(req)) => {
//                     self.handle_request(req);
//                 }
//                 _ => {}
//             }
//         }
//     }
//
//     fn handle_request(&self, req: ProcessRequest) {
//         match req {
//             ProcessRequest::SendMessage { from_pid, to_pid, msg } => {
//                 // 1. 查找目标进程的 PCB
//                 // 2. 将消息放入目标进程的消息队列
//                 // 3. 如果目标进程在等待消息，唤醒它
//             }
//
//             ProcessRequest::ReceiveMessage { pid, blocking } => {
//                 // 1. 检查进程的消息队列
//                 // 2. 如果有消息，返回
//                 // 3. 如果没有消息且 blocking=true，阻塞进程
//             }
//
//             ProcessRequest::CreateSharedMemory { pid, size, prot } => {
//                 // 1. 分配物理内存
//                 // 2. 创建共享内存描述符
//                 // 3. 返回 shmid
//             }
//
//             // ... 其他请求类型的实现
//         }
//     }
// }
// ```
//
// 关键点：
// 1. 所有请求都通过 bus 到达，方便监控和调试
// 2. 真正的进程管理逻辑在 ProcessService 中实现
// 3. 使用统一的错误处理 (GenshinError)
// 4. 使用响应机制 (RequestWithResponse) 获取操作结果
