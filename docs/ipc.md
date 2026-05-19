# 进程间通信（IPC）机制详解

> 本文档全面解析 genshin-OS 微内核的进程间通信架构。

---

## 一、架构总览

genshin-OS 中，**一切跨模块通信都经过消息总线**。没有任何两个模块可以直接调用对方的函数。

```
┌──────────┐   KernelMsg    ┌──────────┐   unbounded    ┌──────────────┐
│  Shell   │ ──────────────→ │  Locked  │ ─────────────→ │   Kernel     │
│  (用户态) │   send_request  │   Bus    │   subscribe    │  (路由器)     │
└──────────┘                 └──────────┘                └──┬──┬──┬──┬──┘
                                                           │  │  │  │
                                              ┌────────────┘  │  │  └──→ FileService
                                              ↓               │  └─────→ MemoryService
                                         ProcessService       └────────→ DeviceService
```

**两套消息路径**：

| 路径 | 场景 | 走总线？ | 代码入口 |
|------|------|:---:|------|
| **请求-响应**（send_and_wait） | Shell 命令（ls、mkdir、ps） | ✅ | `shell/mod.rs:255` |
| **直接系统调用**（INT 0x80） | 进程内 syscall（sem_wait、print） | ❌ 不走 | `service.rs:1428 handle_file_syscall()` |

---

## 二、消息总线（MessageBus）

### 2.1 核心抽象

```rust
// 文件: src/messaging/bus.rs:96
pub trait MessageBus: Send + Sync {
    fn send(&self, msg: KernelMsg) -> Result<(), BusError>;        // 发后即忘
    fn send_request(&self, msg: KernelMsg) -> Result<Receiver<Response>, BusError>;  // 请求-响应
    fn subscribe(&self) -> Receiver<Envelope>;                     // 订阅
}
```

### 2.2 LockedBus 实现

```rust
// 文件: src/messaging/bus.rs:159
pub struct LockedBus {
    state: Arc<Mutex<LockedBusState>>,  // 订阅者列表
}
```

- **内部状态**：一个 `Vec<Sender<Envelope>>`，存储所有订阅者的发送端
- **send()**：遍历所有订阅者，用 `try_send` 广播（unbounded channel，不阻塞）
- **subscribe()**：创建新的 unbounded channel，返回接收端

### 2.3 Envelope（信封）

```rust
// 文件: src/messaging/bus.rs:31
pub struct Envelope {
    pub message: KernelMsg,                       // 消息体
    pub request_id: Option<RequestId>,             // 请求 ID（请求-响应模式）
    pub response_channel: Option<Sender<Response>>, // 响应通道（请求-响应模式）
}
```

两个构造模式：
- **fire_and_forget(msg)** → 无请求 ID，无响应通道
- **with_response(msg)** → 自动生成请求 ID，创建响应通道

### 2.4 KernelMsg（所有消息的根枚举）

```rust
// 文件: src/messaging/msg.rs:27
pub enum KernelMsg {
    Syscall(Syscall),         // 系统调用
    Interrupt(Interrupt),     // 硬件中断（Timer、PageFault 等）
    Process(ProcessRequest),  // 进程服务（IPC、生命周期、同步）
    Memory(MemoryRequest),    // 内存服务（分配、映射、换页）
    File(FileRequest),        // 文件服务（CRUD、目录操作）
    Device(DeviceRequest),    // 设备服务（剪贴板等）
}
```

---

## 三、Kernel 路由器

```rust
// 文件: src/services/kernel.rs
pub struct Kernel {
    receiver: Receiver<Envelope>,    // 从总线订阅
    process_tx: Sender<Envelope>,    // → ProcessService
    intr_tx: Sender<Envelope>,       // → 中断通道
    memory_tx: Sender<Envelope>,     // → MemoryService
    file_tx: Sender<Envelope>,       // → FileService
}
```

**路由规则**（`kernel.rs:41-49`）：

| KernelMsg 变体 | 路由到 |
|:---|:---|
| `KernelMsg::Interrupt(_)` | `intr_tx` → ProcessService 中断通道 |
| `KernelMsg::Process(_)` / `Syscall(_)` | `process_tx` → ProcessService 主通道 |
| `KernelMsg::Memory(_)` | `memory_tx` → MemoryService |
| `KernelMsg::File(_)` | `file_tx` → FileService |
| `KernelMsg::Device(_)` | 不路由（DeviceService 直接订阅总线） |

Kernel 是**唯一订阅总线的组件**。其他服务通过专用的 `unbounded` channel 接收消息。

---

## 四、IPC 消息传递（Message Passing）

### 4.1 消息类型

```rust
// 文件: src/messaging/msg.rs:639
pub enum IPCMessage {
    Text { data: String },                    // 文本消息
    Binary { addr: VirtAddr, size: usize },   // 二进制消息（传地址）
    PassFd { fd: u32 },                       // 文件描述符传递
    SharedMemory { shmid: u64 },              // 共享内存通知
    Signal { signal: SignalType },            // 同步信号
    Control { cmd: String, args: Vec<String>, path: Option<String> },  // 控制消息
}
```

### 4.2 消息队列

```rust
// 文件: src/services/process/ipc.rs:19
pub struct MessageQueue {
    pub owner_pid: Pid,                   // 队列所有者
    pub capacity: usize,                  // 容量（0 = 无限）
    messages: VecDeque<QueuedMessage>,     // 消息队列
    pub total_sent: u64,                  // 累计发送数
    pub total_received: u64,              // 累计接收数
}
```

每个进程有一个消息队列。`get_message_queue()` 按需创建（惰性初始化）。

### 4.3 发送消息（send）

```rust
// ProcessRequest::SendMessage { from_pid, to_pid, msg }
// 处理函数: service.rs handle_send_message()

fn handle_send_message(&self, from_pid: Pid, to_pid: Pid, msg: IPCMessage) -> GenshinResult<()> {
    // 1. 验证发送方和接收方进程存在
    // 2. 获取接收方的消息队列
    // 3. queue.send(from_pid, 1, msg) → 推入消息
}
```

### 4.4 接收消息（receive）

```rust
// ProcessRequest::ReceiveMessage { pid, blocking }
// 处理函数: service.rs handle_receive_message()

fn handle_receive_message(&self, pid: Pid, blocking: bool) -> GenshinResult<()> {
    // 1. 获取进程的消息队列
    // 2. queue.receive() → 取出队首消息
    // 3. 如果 blocking=true 且队列为空 → 阻塞进程
}
```

### 4.5 窥探消息（peek）

```rust
// ProcessRequest::PeekMessage { pid }
// 只看不取，用于检查是否有新消息到达
```

---

## 五、共享内存（Shared Memory）

### 5.1 数据结构

```rust
// 文件: src/services/process/ipc.rs:143
pub struct SharedMemoryRegion {
    pub shmid: u64,                          // 唯一 ID
    pub creator_pid: Pid,                    // 创建者
    pub size: usize,                         // 大小（字节）
    pub physical_addr: PhysAddr,             // 物理地址
    pub prot: MemProt,                       // 保护标志（读/写/执行）
    pub ref_count: usize,                    // 引用计数
    pub attached: HashMap<Pid, VirtAddr>,    // 已附加进程的虚拟地址映射
    pub marked_for_deletion: bool,           // 是否标记删除
}
```

### 5.2 生命周期

```
CreateSharedMemory { pid, size, prot }
  → 分配物理帧 → 创建 SharedMemoryRegion → 返回 shmid

AttachSharedMemory { pid, shmid }
  → 查找 shmid → 分配虚拟地址 → 映射到物理帧 → 加入 attached 表

DetachSharedMemory { pid, shmid }
  → 从 attached 表移除 → 取消映射 → ref_count--

当 ref_count == 0 且 marked_for_deletion → 释放物理帧
```

---

## 六、同步原语（Synchronization）

### 6.1 信号量（Semaphore）

```rust
// 文件: src/services/process/sync.rs:25
pub struct Semaphore {
    pub id: SemaphoreId,
    value: AtomicU32,        // 当前值（CAS 原子操作）
    initial_value: u32,      // 初始值
    max_value: u32,          // 最大值
    pub owner_pid: Pid,      // 创建者
    wait_count: AtomicU32,   // 等待者数量
    valid: AtomicBool,       // 是否有效（未被销毁）
}
```

**系统调用**：

| R0 | 操作 | 语义 |
|:--:|------|------|
| 201 | `sem_wait(sem_id)` | P 操作：count=0 则阻塞，count>0 则 count-- |
| 202 | `sem_signal(sem_id)` | V 操作：有等待者则转移所有权（TOCTOU），无等待者则 count++ |

**TOCTOU 所有权转移**（`service.rs:1575-1607`）：

```
1. 查找 Blocked(WaitingForLock { lock_addr: sem_id }) 的等待者
2. 如果有等待者：
   - PCB → Ready
   - scheduler.ready(wpid)
   - cpu.halted = false
   - 信号量 count 保持 0（不增加）
   - 等待者被唤醒后直接进入临界区（不需要重新 sem_wait）
3. 如果没有等待者：
   - sem.signal() → count++
```

### 6.2 互斥锁（Mutex）

```rust
// 文件: src/services/process/sync.rs:218
pub struct MutexLock {
    pub id: LockId,
    owner: AtomicU64,        // 持有者 PID（u64::MAX = 未锁定）
    count: AtomicU32,        // 递归锁计数
    pub creator_pid: Pid,
    recursive: bool,         // 是否支持递归
    wait_count: AtomicU32,
    valid: AtomicBool,
}
```

**系统调用**：

| R0 | 操作 | 语义 |
|:--:|------|------|
| 203 | `lock_create` | 创建互斥锁，返回 lock_id 到 R1 |
| 204 | `lock_acquire(lock_id)` | 获取锁：已锁定则阻塞 |
| 205 | `lock_release(lock_id)` | 释放锁：唤醒等待者 |

---

## 七、完整通信流程示例

### 7.1 Shell 执行 `ps` 命令

```
1. Shell:
   let msg = KernelMsg::Process(ProcessRequest::ListProcesses);
   self.send_and_wait(msg)                         // shell/mod.rs:255
     → context.send_request(msg)
       → bus.send_request(msg)
         → LockedBus: 创建 Envelope + response_channel
         → 广播给所有订阅者（Kernel）

2. Kernel:
   receiver.recv() → 收到 Envelope
   route() → KernelMsg::Process(_) → process_tx.send(envelope)

3. ProcessService:
   receiver.try_recv() → 收到 Envelope
   handle_envelope() → handle_process_request_with_response()
     → ProcessRequest::ListProcesses
     → handle_list_processes_with_response()
       → 遍历 process_table 构建进程树
       → envelope.respond_success(tree_string)

4. Shell:
   rx.recv_timeout(3s) → 收到 Response
   → 打印进程树
```

### 7.2 进程执行 `sem_wait(0)`

```
1. CPU 执行 INT 0x80（R0=201, R1=0）
   → cpu.syscall_pending = true
   → cpu.syscall_regs = [201, 0, ...]

2. handle_timer_interrupt() 的 step loop:
   if cpu.syscall_pending {
       handle_file_syscall(cpu, 201, 0, 0)
   }

3. handle_file_syscall() → 201 分支:
   let blocked = {
       sync_manager.lock() → 获取 SyncManager
       sem = sync.get_semaphore(0)
       sem.wait() → CAS: value 1→0 → Acquired
       false  // 不阻塞
   }

4. 如果 count=0 → WouldBlock → blocked=true:
   PCB.state = Blocked(WaitingForLock { lock_addr: 0 })
   scheduler.block(pid, 1)  → 移出就绪队列
   cpu.halt()                → CPU 停机
```

### 7.3 进程执行 `sem_signal(0)`

```
1. CPU 执行 INT 0x80（R0=202, R1=0）

2. handle_file_syscall() → 202 分支:
   扫描 process_table 查找 Blocked(WaitingForLock { lock_addr: 0 })

3. 找到等待者 wpid:
   PCB → Ready
   scheduler.ready(wpid)
   cpu.halted = false
   // 不调用 sem.signal()，count 保持 0

4. 没找到等待者:
   sem.signal() → CAS: count 0→1
   sem.clear_holder()
```

---

## 八、关键代码位置速查表

| 组件 | 文件 | 行号 |
|------|------|:---:|
| **KernelMsg 枚举** | `src/messaging/msg.rs` | 27-47 |
| **ProcessRequest 枚举** | `src/messaging/msg.rs` | 154-288 |
| **IPCMessage 枚举** | `src/messaging/msg.rs` | 639-664 |
| **MessageBus trait** | `src/messaging/bus.rs` | 96-127 |
| **LockedBus** | `src/messaging/bus.rs` | 159-239 |
| **Envelope** | `src/messaging/bus.rs` | 31-90 |
| **Kernel 路由器** | `src/services/kernel.rs` | 全文 52 行 |
| **IPCManager** | `src/services/process/ipc.rs` | 240-674 |
| **MessageQueue** | `src/services/process/ipc.rs` | 19-120 |
| **SharedMemoryRegion** | `src/services/process/ipc.rs` | 143-232 |
| **信号量 Semaphore** | `src/services/process/sync.rs` | 25-170 |
| **互斥锁 MutexLock** | `src/services/process/sync.rs` | 218-320 |
| **SyncManager** | `src/services/process/sync.rs` | 389-500 |
| **sem_wait 处理** | `src/services/process/service.rs` | 1555-1573 |
| **sem_signal 处理** | `src/services/process/service.rs` | 1575-1607 |
| **lock_acquire 处理** | `src/services/process/service.rs` | 1620-1638 |
| **lock_release 处理** | `src/services/process/service.rs` | 1640-1663 |
| **handle_file_syscall** | `src/services/process/service.rs` | 1428-1690 |
| **Shell send_and_wait** | `src/ui/shell/mod.rs` | 255-260 |

---

## 九、答辩要点

**Q: 为什么直接系统调用不走总线？**

A: 性能。每个 INT 0x80 都走总线 → Kernel → ProcessService 的完整链路会导致大量 SyscallTrap 消息泛滥。我们通过 `cpu.syscall_pending` 标志，在 `handle_timer_interrupt` 的 step loop 中直接调用 `handle_file_syscall()`，省去消息序列化/路由开销。

**Q: 信号量和互斥锁的区别？**

A: 信号量是**计数器**——不记录持有者，可以有多个进程同时持有（二元信号量=互斥锁的特例）。互斥锁记录**持有者 PID**——只有持有者能释放，支持递归。

**Q: TOCTOU 是什么意思？**

A: Time-of-Check to Time-of-Use。在多核 SMP 中，`sem_signal` 恢复 count 后，调用者可能立即重新 `sem_wait` 抢回信号量，导致等待者永远得不到运行。我们采用**所有权转移**——直接把信号量交给等待者，不经过 "count++ 再 count--" 的中间状态。
