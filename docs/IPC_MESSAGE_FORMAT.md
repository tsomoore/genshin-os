# IPC 消息格式文档

## 概述

genshin-OS 的所有进程间通信(IPC)都通过标准的消息总线进行。这种设计的优点：

1. **可监控性**: 所有IPC消息都经过总线，便于监控和调试
2. **统一接口**: 不需要特殊的IPC通道，使用标准的 `KernelMsg` 枚举
3. **可扩展性**: 新增IPC机制只需添加新的 `ProcessRequest` 变体

## IPC 消息类型

所有IPC消息都封装在 `KernelMsg::Process(ProcessRequest)` 中。

### 1. 消息传递 (Message Passing)

```rust
// 发送消息
ProcessRequest::SendMessage {
    from_pid: Pid,
    to_pid: Pid,
    msg: IPCMessage,
}

// 接收消息
ProcessRequest::ReceiveMessage {
    pid: Pid,
    blocking: bool,  // true=阻塞, false=非阻塞
}

// 查看消息队列
ProcessRequest::PeekMessage {
    pid: Pid,
}
```

**消息类型 (IPCMessage)**:

- `Text { data: String }` - 文本消息
- `Binary { addr: VirtAddr, size: usize }` - 二进制数据
- `PassFd { fd: u32 }` - 传递文件描述符
- `SharedMemory { shmid: u64 }` - 共享内存通知
- `Signal { signal: SignalType }` - 信号通知
- `Control { cmd: String, args: Vec<String> }` - 控制消息

### 2. 共享内存 (Shared Memory)

```rust
// 创建共享内存区
ProcessRequest::CreateSharedMemory {
    pid: Pid,
    size: usize,
    prot: MemProt,
}

// 附加到共享内存区
ProcessRequest::AttachSharedMemory {
    pid: Pid,
    shmid: u64,
}

// 分离共享内存区
ProcessRequest::DetachSharedMemory {
    pid: Pid,
    shmid: u64,
}
```

### 3. 同步原语 (Synchronization)

#### 信号量 (Semaphore)

```rust
// 创建信号量
ProcessRequest::CreateSemaphore {
    pid: Pid,
    initial_value: u32,
}

// 等待信号量 (P 操作)
ProcessRequest::WaitSemaphore {
    pid: Pid,
    semid: u64,
}

// 发送信号量 (V 操作)
ProcessRequest::SignalSemaphore {
    pid: Pid,
    semid: u64,
}
```

#### 互斥锁 (Mutex)

```rust
// 创建互斥锁
ProcessRequest::CreateLock {
    pid: Pid,
}

// 获取锁
ProcessRequest::AcquireLock {
    pid: Pid,
    lock_id: u64,
}

// 释放锁
ProcessRequest::ReleaseLock {
    pid: Pid,
    lock_id: u64,
}
```

### 4. 信号 (Signals)

```rust
ProcessRequest::Signal {
    pid: Pid,
    signal: SignalType,
}
```

**信号类型 (SignalType)**:

- `Terminate` (SIGTERM, 15) - 终止进程
- `Kill` (SIGKILL, 9) - 强制杀死
- `Stop` (SIGSTOP, 19) - 停止进程
- `Continue` (SIGCONT, 18) - 继续执行
- `User1` (SIGUSR1, 10) - 用户自定义信号1
- `User2` (SIGUSR2, 12) - 用户自定义信号2
- `Alarm` (SIGALRM, 14) - 定时器
- `Child` (SIGCHLD, 17) - 子进程状态变化
- `SegmentationFault` (SIGSEGV, 11) - 段错误
- `IllegalInstruction` (SIGILL, 4) - 非法指令
- `FloatingPointException` (SIGFPE, 8) - 浮点异常

### 5. 进程生命周期 (Process Lifecycle)

```rust
// Fork 进程
ProcessRequest::ForkProcess {
    parent_pid: Pid,
}

// 执行新程序
ProcessRequest::ExecProcess {
    pid: Pid,
    executable: String,
    args: Vec<String>,
}

// 等待子进程
ProcessRequest::WaitChild {
    pid: Pid,
    child_pid: Option<Pid>,  // None = 等待任意子进程
}

// 查询进程信息
ProcessRequest::GetProcessInfo {
    pid: Pid,
}

// 列出所有进程
ProcessRequest::ListProcesses,
```

## 使用示例

完整的示例程序参见: `examples/ipc_messages_demo.rs`

### 发送消息示例

```rust
use genshin_os::{KernelMsg, ProcessRequest, IPCMessage, MessageBus, LockedBus};

let bus = Arc::new(LockedBus::new());

// 进程 A 发送文本消息给进程 B
let msg = KernelMsg::Process(ProcessRequest::SendMessage {
    from_pid: 100,
    to_pid: 200,
    msg: IPCMessage::Text {
        data: "Hello!".to_string(),
    },
});
bus.send(msg)?;
```

### 接收消息示例

```rust
// 进程 B 接收消息（阻塞）
let msg = KernelMsg::Process(ProcessRequest::ReceiveMessage {
    pid: 200,
    blocking: true,
});
bus.send(msg)?;
```

## 内核服务层实现要点

负责内核服务层的同学需要实现 `ProcessService` 来处理这些请求：

1. **订阅消息**: `let rx = bus.subscribe();`
2. **处理请求**: 匹配 `ProcessRequest` 的各个变体
3. **管理数据结构**: 进程表、PCB、消息队列、共享内存表等
4. **返回响应**: 使用 `RequestWithResponse` 机制返回操作结果
5. **错误处理**: 使用统一的 `GenshinError` 错误类型

### 关键数据结构建议

```rust
struct ProcessControlBlock {
    pid: Pid,
    state: ProcessState,
    message_queue: Vec<IPCMessage>,
    // ... 其他字段
}

struct SharedMemoryDescriptor {
    shmid: u64,
    size: usize,
    physical_addr: PhysAddr,
    ref_count: usize,
    // ... 其他字段
}
```

## 设计原则

1. **所有通信都经过总线**: 便于监控和调试
2. **异步fire-and-forget**: 发送消息不等待，需要响应使用 `RequestWithResponse`
3. **类型安全**: 使用强类型的枚举，避免字符串错误
4. **可扩展**: 新增IPC机制只需添加新的枚举变体

## 导出的类型

所有IPC相关的类型都已从 `genshin_os` crate 导出：

```rust
use genshin_os::{
    KernelMsg, ProcessRequest, IPCMessage, SignalType,
    MessageBus, LockedBus, Pid,
};
```

参见 `src/messaging/msg.rs` 中的完整定义。
