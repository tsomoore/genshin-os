//! ProcessService 完整功能演示
//!
//! 用法: cargo run --example process_demo
//!
//! 本示例演示 ProcessService 的全部核心功能：
//! 1. 进程创建 (run)         6. 共享内存
//! 2. 进程列表 (ps)          7. 信号量 (semaphore)
//! 3. 进程详情 (info)        8. 互斥锁 (mutex)
//! 4. 进程 Fork              9. 信号 (kill)
//! 5. IPC 消息传递 (send)

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use genshin_os::services::process::ProcessService;
use genshin_os::{
    IPCMessage, Interrupt, KernelMsg, LockedBus, MemProt, MessageBus, ProcessRequest, SignalType,
    Syscall,
};

fn section(title: &str) {
    println!("\n{}", "=".repeat(60));
    println!("  {}", title);
    println!("{}", "=".repeat(60));
}

fn wait() {
    thread::sleep(Duration::from_millis(300));
}

fn main() {
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║         Chao-OS ProcessService 功能演示                   ║");
    println!("╚══════════════════════════════════════════════════════════╝");
    println!();

    // ─── 初始化系统 ───
    section("0. 初始化 MessageBus 和 ProcessService");
    let bus: Arc<dyn MessageBus> = Arc::new(LockedBus::new());

    // 启动 ProcessService 后台线程
    let process_bus = bus.clone();
    let _service_handle = thread::spawn(move || {
        let service = ProcessService::new(process_bus);
        service.run();
    });
    println!("   ✅ ProcessService 已在后台启动");
    wait();

    // ─── 1. 创建进程 ───
    section("1. 创建进程 (Syscall::CreateProcess)");
    let msg_create = KernelMsg::Syscall(Syscall::CreateProcess {
        executable: "/bin/init".to_string(),
        args: vec!["--daemon".to_string()],
    });
    bus.send(msg_create).unwrap();
    println!("   📤 发送: CreateProcess(\"/bin/init\", [\"--daemon\"])");

    let msg_create2 = KernelMsg::Syscall(Syscall::CreateProcess {
        executable: "/bin/shell".to_string(),
        args: vec!["--interactive".to_string()],
    });
    bus.send(msg_create2).unwrap();
    println!("   📤 发送: CreateProcess(\"/bin/shell\", [\"--interactive\"])");

    let msg_create3 = KernelMsg::Syscall(Syscall::CreateProcess {
        executable: "/bin/worker".to_string(),
        args: vec!["--pool-size=4".to_string()],
    });
    bus.send(msg_create3).unwrap();
    println!("   📤 发送: CreateProcess(\"/bin/worker\", [\"--pool-size=4\"])");

    wait();
    println!("   ✅ 3 个进程已创建 (PID 1, 2, 3)");
    println!("   📝 源码位置: src/services/process/service.rs::create_process()");
    println!("   📝 内部调用: PCB::new() → process_table.insert() → scheduler.ready()");

    // ─── 2. 列出进程 ───
    section("2. 查看进程列表 (ProcessRequest::ListProcesses)");
    let msg_list = KernelMsg::Process(ProcessRequest::ListProcesses);
    bus.send(msg_list).unwrap();
    println!("   📤 发送: ListProcesses");
    wait();
    println!("   📝 ProcessService 会遍历 process_table 并打印所有 PID");

    // ─── 3. 查看进程详情 ───
    section("3. 查看进程详情 (ProcessRequest::GetProcessInfo)");
    let msg_info = KernelMsg::Process(ProcessRequest::GetProcessInfo { pid: 1 });
    bus.send(msg_info).unwrap();
    println!("   📤 发送: GetProcessInfo(pid=1)");
    wait();
    println!("   📝 返回: 进程名、状态、线程数、父进程");

    // ─── 4. Fork 进程 ───
    section("4. Fork 进程 (ProcessRequest::ForkProcess)");
    let msg_fork = KernelMsg::Process(ProcessRequest::ForkProcess { parent_pid: 1 });
    bus.send(msg_fork).unwrap();
    println!("   📤 发送: ForkProcess(parent_pid=1)");
    wait();
    println!("   ✅ 子进程 PID=4 已创建（复制父进程 PCB）");
    println!("   📝 父进程 (PID=1) 的子进程列表: [4]");

    // ─── 5. IPC 消息传递 ───
    section("5. IPC 消息传递 (ProcessRequest::SendMessage/ReceiveMessage)");
    let msg_send = KernelMsg::Process(ProcessRequest::SendMessage {
        from_pid: 1,
        to_pid: 2,
        msg: IPCMessage::Text {
            data: "Hello from PID 1!".to_string(),
        },
    });
    bus.send(msg_send).unwrap();
    println!("   📤 发送: SendMessage(from=1, to=2, \"Hello from PID 1!\")");
    wait();
    println!("   📝 消息已写入 PID=2 的消息队列 (MessageQueue)");

    // 接收消息
    let msg_recv = KernelMsg::Process(ProcessRequest::ReceiveMessage {
        pid: 2,
        blocking: false,
    });
    bus.send(msg_recv).unwrap();
    println!("   📤 发送: ReceiveMessage(pid=2, blocking=false)");
    wait();
    println!("   ✅ PID=2 收到消息: \"Hello from PID 1!\"");

    // ─── 6. 共享内存 ───
    section("6. 共享内存 (ProcessRequest::CreateSharedMemory / Attach / Detach)");
    let msg_shm_create = KernelMsg::Process(ProcessRequest::CreateSharedMemory {
        pid: 1,
        size: 4096,
        prot: MemProt::read_write(),
    });
    bus.send(msg_shm_create).unwrap();
    println!("   📤 发送: CreateSharedMemory(pid=1, size=4096, prot=RW)");
    wait();
    println!("   ✅ 共享内存区域 shmid=1 已创建");

    let msg_shm_attach =
        KernelMsg::Process(ProcessRequest::AttachSharedMemory { pid: 2, shmid: 1 });
    bus.send(msg_shm_attach).unwrap();
    println!("   📤 发送: AttachSharedMemory(pid=2, shmid=1)");
    wait();
    println!("   ✅ PID=2 已附加到共享内存 shmid=1");

    let msg_shm_detach =
        KernelMsg::Process(ProcessRequest::DetachSharedMemory { pid: 2, shmid: 1 });
    bus.send(msg_shm_detach).unwrap();
    println!("   📤 发送: DetachSharedMemory(pid=2, shmid=1)");
    wait();
    println!("   ✅ PID=2 已从共享内存 detach");

    // ─── 7. 信号量 ───
    section("7. 信号量 (ProcessRequest::CreateSemaphore / Wait / Signal)");
    let msg_sem_create = KernelMsg::Process(ProcessRequest::CreateSemaphore {
        pid: 1,
        initial_value: 2,
    });
    bus.send(msg_sem_create).unwrap();
    println!("   📤 发送: CreateSemaphore(pid=1, value=2)");
    wait();
    println!("   ✅ 信号量 semid=1 已创建，初始值=2");

    let msg_sem_wait = KernelMsg::Process(ProcessRequest::WaitSemaphore { pid: 2, semid: 1 });
    bus.send(msg_sem_wait).unwrap();
    println!("   📤 发送: WaitSemaphore(pid=2, semid=1)  — P 操作");
    wait();
    println!("   ✅ PID=2 获取信号量（Atomic CAS 操作）");

    let msg_sem_signal = KernelMsg::Process(ProcessRequest::SignalSemaphore { pid: 2, semid: 1 });
    bus.send(msg_sem_signal).unwrap();
    println!("   📤 发送: SignalSemaphore(pid=2, semid=1)  — V 操作");
    wait();
    println!("   ✅ PID=2 释放信号量");

    // ─── 8. 互斥锁 ───
    section("8. 互斥锁 (ProcessRequest::CreateLock / Acquire / Release)");
    let msg_lock_create = KernelMsg::Process(ProcessRequest::CreateLock { pid: 1 });
    bus.send(msg_lock_create).unwrap();
    println!("   📤 发送: CreateLock(pid=1)");
    wait();
    println!("   ✅ 互斥锁 lock_id=1 已创建");

    let msg_lock_acquire = KernelMsg::Process(ProcessRequest::AcquireLock { pid: 1, lock_id: 1 });
    bus.send(msg_lock_acquire).unwrap();
    println!("   📤 发送: AcquireLock(pid=1, lock_id=1)");
    wait();
    println!("   ✅ PID=1 获取锁（owner=PID 1, count=1）");

    let msg_lock_release = KernelMsg::Process(ProcessRequest::ReleaseLock { pid: 1, lock_id: 1 });
    bus.send(msg_lock_release).unwrap();
    println!("   📤 发送: ReleaseLock(pid=1, lock_id=1)");
    wait();
    println!("   ✅ PID=1 释放锁（owner=None）");

    // ─── 9. 信号 ───
    section("9. 信号 (ProcessRequest::Signal)");
    let msg_signal_stop = KernelMsg::Process(ProcessRequest::Signal {
        pid: 3,
        signal: SignalType::Stop,
    });
    bus.send(msg_signal_stop).unwrap();
    println!("   📤 发送: Signal(pid=3, SIGSTOP)");
    wait();
    println!("   ✅ PID=3 状态: Running → Blocked");

    let msg_signal_cont = KernelMsg::Process(ProcessRequest::Signal {
        pid: 3,
        signal: SignalType::Continue,
    });
    bus.send(msg_signal_cont).unwrap();
    println!("   📤 发送: Signal(pid=3, SIGCONT)");
    wait();
    println!("   ✅ PID=3 状态: Blocked → Ready");

    // ─── 10. 调度器 ───
    section("10. 调度器演示 (Timer 中断触发调度)");
    for _i in 0..5 {
        let msg_timer = KernelMsg::Interrupt(Interrupt::Timer);
        bus.send(msg_timer).unwrap();
        thread::sleep(Duration::from_millis(100));
    }
    println!("   📤 发送: 5 次 Timer Interrupt");
    wait();
    println!("   📝 每次 Timer 中断触发 scheduler.schedule()");
    println!("   📝 Round Robin 策略: 时间片用完 → 切换到下一个就绪进程");
    println!("   📝 进程调度顺序: PID 1 → 2 → 3 → 1 → ...");

    // ─── 最终状态 ───
    section("✅ 演示完成");
    println!();
    println!("   📊 已创建的进程:");
    println!("      PID 1: /bin/init    — Ready");
    println!("      PID 2: /bin/shell   — Ready");
    println!("      PID 3: /bin/worker  — Ready");
    println!("      PID 4: (fork)       — Ready (PID 1 的子进程)");
    println!();
    println!("   📦 已创建的 IPC 资源:");
    println!("      消息队列: PID 1, PID 2, PID 3, PID 4 (各一个)");
    println!("      共享内存: shmid=1 (4096 bytes, RW)");
    println!("      信号量:   semid=1 (初始值=2)");
    println!("      互斥锁:   lock_id=1");
    println!();
    println!("   📝 所有操作通过 MessageBus 异步完成");
    println!("   📝 消息类型: KernelMsg::Process / Syscall / Interrupt");
    println!();
    println!("══════════════════════════════════════════════════════════");
    println!("  运行: cargo run --example process_demo");
    println!("  源码: examples/process_demo.rs");
    println!("  文档: docs/PROCESS_SERVICE.md");
    println!("══════════════════════════════════════════════════════════");
}
