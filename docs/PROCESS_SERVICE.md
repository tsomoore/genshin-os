# ProcessService 设计与实现文档

## 📐 架构概览

ProcessService 是 genshin-OS 微内核的进程管理服务，运行在 Service Layer，通过 MessageBus
与硬件层、其他服务层通信。它负责进程生命周期管理、线程调度、IPC 通信和同步原语。

```mermaid
flowchart TB
    accTitle: ProcessService 在四层架构中的位置
    accDescr: 展示 ProcessService 位于 Service Layer，通过 MessageBus 与上下层通信。

    subgraph UI["用户交互层"]
        Shell["CLI Shell"]
    end

    subgraph BUS["交换层 — MessageBus"]
        KBus["KernelMsg 枚举<br/>统一消息路由"]
    end

    subgraph SVC["服务层"]
        PS["🎯 ProcessService<br/>进程·调度·IPC·同步"]
        FS["FileService"]
        SS["StorageService"]
        DS["DeviceService"]
    end

    subgraph HW["硬件层"]
        CPU["VirtualCPU"]
        MMU["MMU"]
        Timer["Timer"]
        Disk["VirtualDisk"]
    end

    Shell -->|"KernelMsg::Process(..)"| KBus
    KBus --> PS
    PS -->|"KernelMsg::Interrupt(..)"| KBus
    PS -.->|请求调度| KBus
    CPU --- KBus
    MMU --- KBus
    Timer --- KBus
    Disk --- KBus

    classDef ui fill:#fef3c7,stroke:#f59e0b,color:#92400e
    classDef bus fill:#dbeafe,stroke:#3b82f6,color:#1e40af
    classDef svc fill:#dcfce7,stroke:#22c55e,color:#14532d
    classDef hw fill:#f3e8ff,stroke:#a855f7,color:#6b21a8
    classDef focus fill:#22c55e,stroke:#16a34a,stroke-width:3px,color:#052e16

    class Shell ui
    class KBus bus
    class PS focus
    class FS,SS,DS svc
    class CPU,MMU,Timer,Disk hw
```

## 🧩 组件结构

ProcessService 由五个子模块组成，各司其职：

```mermaid
classDiagram
    accTitle: ProcessService 组件类图
    accDescr: 展示 ProcessService 及其五个子模块的组成关系与依赖。

    class ProcessService {
        +bus: Arc~dyn MessageBus~
        +process_table: HashMap~Pid, PCB~
        +ipc_manager: IPCManager
        +sync_manager: SyncManager
        +scheduler: Scheduler
        +parent_children: HashMap~Pid, Vec~Pid~~
        --
        +new(bus) ProcessService
        +run()
        +handle_envelope(envelope) Result
        +create_process(name, args) Pid
    }

    class PCB {
        +pid: Pid
        +parent_pid: Option~Pid~
        +state: ProcessState
        +threads: HashMap~Tid, TCB~
        +message_queue: VecDeque~IPCMessage~
        +shared_memory: HashMap~u64, VirtAddr~
        +file_descriptors: HashMap~u32, FileDescriptor~
        +pending_signals: Vec~SignalType~
        +mmu_state: Option~MMUState~
        --
        +add_thread(tcb) Tid
        +block(reason)
        +unblock()
        +terminate(exit_code)
        +enqueue_message(msg)
        +dequeue_message() Option~IPCMessage~
    }

    class TCB {
        +tid: Tid
        +state: ThreadState
        +cpu_state: Option~CPUState~
        +priority: u8
        +stack_pointer: VirtAddr
        +entry_point: VirtAddr
        --
        +save_cpu_state(state)
        +take_cpu_state() Option~CPUState~
        +block(reason)
        +unblock()
    }

    class Scheduler {
        +policy: SchedulingPolicy
        +ready_queue: VecDeque~ReadyQueueEntry~
        --
        +ready(pid, tid, priority)
        +schedule() SchedulingDecision
        +block(pid, tid)
        +ready_count() usize
    }

    class IPCManager {
        +message_queues: HashMap~Pid, MessageQueue~
        +shared_memory: HashMap~u64, SharedMemoryRegion~
        --
        +ensure_message_queue(pid) MessageQueue
        +create_shared_memory(pid, size, addr, prot) u64
        +attach_shared_memory(shmid, pid, vaddr) Result
        +detach_shared_memory(shmid, pid) Result
    }

    class SyncManager {
        +semaphores: HashMap~SemaphoreId, Semaphore~
        +mutexes: HashMap~LockId, MutexLock~
        --
        +create_semaphore(pid, val) SemaphoreId
        +create_mutex(pid, recursive) LockId
        +get_semaphore(id) Option
        +get_mutex(id) Option
    }

    ProcessService *-- PCB : manages
    ProcessService *-- Scheduler : owns
    ProcessService *-- IPCManager : owns
    ProcessService *-- SyncManager : owns
    PCB *-- TCB : contains
    IPCManager o-- MessageQueue : manages
    IPCManager o-- SharedMemoryRegion : manages
    SyncManager o-- Semaphore : manages
    SyncManager o-- MutexLock : manages
```

### 📊 模块职责

| 模块 | 文件 | 行数 | 职责 |
|------|------|------|------|
| `PCB` / `TCB` | `pcb.rs` | 788 | 进程/线程控制块：状态管理、文件描述符、信号队列 |
| `Scheduler` | `scheduler.rs` | 589 | 调度器：支持 FIFO、Round Robin、Priority 三种策略 |
| `IPCManager` | `ipc.rs` | 677 | 进程间通信：消息队列、共享内存区域管理 |
| `SyncManager` | `sync.rs` | 730 | 同步原语：信号量（原子操作）、互斥锁（支持递归） |
| `ProcessService` | `service.rs` | 1257 | 主服务：消息分发、进程生命周期、IPC/Sync 协调 |

## 🔄 进程生命周期

```mermaid
stateDiagram-v2
    accTitle: 进程状态转换图
    accDescr: 展示进程从创建到终止的六种状态及其转换条件。

    [*] --> Creating : create_process()
    Creating --> Ready : PCB 初始化完成
    Ready --> Running : scheduler.schedule()
    Running --> Ready : 时间片耗尽 / 抢占
    Running --> Blocked : block(reason)
    Blocked --> Ready : unblock()
    Running --> Terminated : exit / kill signal
    Running --> Zombie : terminate() 但父进程未 wait
    Zombie --> [*] : 父进程 wait 回收
    Terminated --> [*] : 资源回收
    Blocked --> Terminated : SIGKILL
```

**状态说明**：

| 状态 | 含义 | 触发条件 |
|------|------|---------|
| `Creating` | 进程正在创建 | `create_process()` 被调用 |
| `Ready` | 就绪，等待调度 | 创建完成或从阻塞恢复 |
| `Running` | 当前正在 CPU 执行 | 调度器选中 |
| `Blocked(reason)` | 阻塞等待 | I/O、锁、信号量、sleep、waitpid |
| `Terminated` | 正常终止 | `exit()` 或 SIGTERM |
| `Zombie` | 已终止但未被回收 | 父进程尚未调用 `wait()` |

## 📨 消息处理流程

ProcessService 接收三类 `KernelMsg` 变体：`Process`、`Syscall`、`Interrupt`。

```mermaid
sequenceDiagram
    accTitle: 典型 IPC 消息发送-接收流程
    accDescr: 展示进程A通过MessageBus向进程B发送消息的完整流程，包括PCB查找、消息队列写入和阻塞处理。

    participant A as 进程 A
    participant Bus as MessageBus
    participant PS as ProcessService
    participant Table as ProcessTable
    participant Queue as MessageQueue
    participant Sched as Scheduler

    A->>Bus: KernelMsg::Process(SendMessage {from, to, msg})
    Bus->>PS: Envelope {message, ...}
    activate PS
    PS->>Table: lock().get(from_pid).is_some()?
    Table-->>PS: ✓ 发送方存在
    PS->>Table: lock().get(to_pid).is_some()?
    Table-->>PS: ✓ 接收方存在
    PS->>Queue: ensure_message_queue(to_pid)
    PS->>Queue: queue.send(from, tid, msg)
    Queue-->>PS: Ok(())
    PS->>Table: 检查接收方是否阻塞在 recv
    Table-->>PS: 接收方处于 Blocked 状态
    PS->>Sched: scheduler.ready() / unblock(to_pid)
    PS-->>Bus: 完成
    deactivate PS
```

### 完整的消息路由表

ProcessService 的 `handle_envelope()` 根据消息类型进行分发：

```mermaid
flowchart LR
    accTitle: ProcessService 消息路由
    accDescr: 展示 Envelope 进入 ProcessService 后按 KernelMsg 变体类型分发到不同的处理函数。

    E["📨 Envelope"] --> M{"msg 类型?"}

    M -->|"ProcessRequest"| PR["handle_process_request()"]
    M -->|"Syscall"| SY["handle_syscall()"]
    M -->|"Interrupt"| IN["handle_interrupt()"]
    M -->|"其他"| IG["忽略 (Ok(()))"]

    PR -->|"expects_response?"| PRR["handle_process_request_with_response()"]
    IN -->|"Timer"| HT["handle_timer_interrupt()"]
    IN -->|"PageFault"| PF["转发至 MemoryService"]
    IN -->|"HardwareFailure"| HF["日志记录"]

    classDef handler fill:#dbeafe,stroke:#3b82f6,color:#1e40af
    classDef route fill:#dcfce7,stroke:#22c55e,color:#14532d
    classDef ignore fill:#fef3c7,stroke:#f59e0b,color:#92400e

    class PR,SY,IN handler
    class PRR,HT,PF,HF route
    class IG ignore
```

## 📋 ProcessRequest 消息全集

ProcessService 支持以下全部消息类型。所有消息通过 `KernelMsg::Process(ProcessRequest::...)` 发送。

### 🔀 调度相关

| 消息变体 | 参数 | 说明 |
|---------|------|------|
| `Schedule` | `pid: Pid, tid: Tid` | 将进程/线程加入就绪队列 |
| `Block` | `pid, tid, reason: BlockReason` | 阻塞进程/线程 |
| `Unblock` | `pid, tid` | 解除阻塞 |
| `QueryState` | `pid` | 查询进程状态 |
| `ContextSwitch` | `from_pid, to_pid` | 上下文切换请求 |

### 📮 IPC — 消息传递

| 消息变体 | 参数 | 说明 |
|---------|------|------|
| `SendMessage` | `from_pid, to_pid, msg: IPCMessage` | 发送消息至目标进程邮箱 |
| `ReceiveMessage` | `pid, blocking: bool` | 接收消息（可选阻塞） |
| `PeekMessage` | `pid` | 查看但不取出队首消息 |

IPCMessage 支持六种负载类型：

| 负载类型 | 说明 |
|---------|------|
| `Text { data: String }` | 文本消息 |
| `Binary { addr, size }` | 二进制数据（发送方地址空间） |
| `PassFd { fd: u32 }` | 文件描述符传递 |
| `SharedMemory { shmid: u64 }` | 共享内存通知 |
| `Signal { signal: SignalType }` | 信号通知 |
| `Control { cmd, args }` | 控制命令 |

### 🧠 IPC — 共享内存

| 消息变体 | 参数 | 说明 |
|---------|------|------|
| `CreateSharedMemory` | `pid, size, prot: MemProt` | 创建共享内存区域 |
| `AttachSharedMemory` | `pid, shmid` | 映射共享内存到进程地址空间 |
| `DetachSharedMemory` | `pid, shmid` | 解除映射 |

共享内存生命周期：

```mermaid
flowchart LR
    accTitle: 共享内存生命周期
    accDescr: 展示共享内存从创建、附着、使用到标记删除和最终清理的流程。

    A["CreateShm"] --> B["Attach (pid1)"]
    B --> C["Attach (pid2)"]
    C --> D["使用中<br/>ref_count > 0"]
    D --> E["Detach (pid1)"]
    E --> F["ref_count = 1"]
    F --> G["MarkForDeletion"]
    G --> H["Detach (pid2)"]
    H --> I["ref_count = 0<br/>→ 清理"]

    classDef create fill:#dcfce7,stroke:#22c55e,color:#14532d
    classDef active fill:#dbeafe,stroke:#3b82f6,color:#1e40af
    classDef destroy fill:#fecaca,stroke:#ef4444,color:#7f1d1d

    class A,B,C create
    class D,E,F active
    class G,H,I destroy
```

### 🔒 IPC — 同步原语

| 消息变体 | 参数 | 说明 |
|---------|------|------|
| `CreateSemaphore` | `pid, initial_value: u32` | 创建信号量 |
| `WaitSemaphore` | `pid, semid` | P 操作（可能阻塞） |
| `SignalSemaphore` | `pid, semid` | V 操作 |
| `CreateLock` | `pid` | 创建互斥锁 |
| `AcquireLock` | `pid, lock_id` | 获取锁（可能阻塞） |
| `ReleaseLock` | `pid, lock_id` | 释放锁 |

Semaphore 使用 `AtomicU32` 实现无锁 CAS 操作：

```mermaid
flowchart TD
    accTitle: 信号量 Wait 操作流程
    accDescr: 使用 CAS 原子操作实现信号量 P 操作，展示从读取当前值到成功获取或返回 WouldBlock 的完整逻辑。

    S["wait() 调用"] --> R["读取 value (Atomic)"]
    R --> C{"value == 0 ?"}
    C -->|"是"| W["wait_count++ <br/>返回 WouldBlock"]
    C -->|"否"| CAS["CAS(value, value-1)"]
    CAS -->|"成功"| A["返回 Acquired"]
    CAS -->|"失败"| R

    classDef op fill:#dbeafe,stroke:#3b82f6,color:#1e40af
    classDef result fill:#dcfce7,stroke:#22c55e,color:#14532d
    classDef block fill:#fecaca,stroke:#ef4444,color:#7f1d1d

    class S,R,CAS op
    class A result
    class W block
```

### 🧬 进程生命周期

| 消息变体 | 参数 | 说明 |
|---------|------|------|
| `ForkProcess` | `parent_pid` | 复制父进程创建子进程 |
| `ExecProcess` | `pid, executable, args` | 替换进程映像 |
| `WaitChild` | `pid, child_pid: Option<Pid>` | 等待子进程退出 |
| `Signal` | `pid, signal: SignalType` | 向进程发送信号 |
| `GetProcessInfo` | `pid` | 查询进程详情 |
| `ListProcesses` | — | 列出所有进程 |

支持的信号类型：

| 信号 | 值 | 行为 |
|------|----|------|
| `SIGTERM` | 15 | 终止进程（可捕获） |
| `SIGKILL` | 9 | 立即终止（不可捕获） |
| `SIGSTOP` | 19 | 暂停进程 |
| `SIGCONT` | 18 | 继续运行 |
| `SIGUSR1` | 10 | 用户自定义 |
| `SIGUSR2` | 12 | 用户自定义 |
| `SIGALRM` | 14 | 定时器超时 |
| `SIGCHLD` | 17 | 子进程状态变更 |
| `SIGSEGV` | 11 | 段错误 |
| `SIGILL` | 4 | 非法指令 |
| `SIGFPE` | 8 | 浮点异常 |

### Syscall 处理

| 变体 | 参数 | 说明 |
|------|------|------|
| `CreateProcess` | `executable, args` | 从可执行文件创建新进程 |
| `ExitProcess` | `exit_code` | 退出当前进程 |
| `CreateThread` | `entry_point` | 创建新线程 |
| `Read` | `fd, buf, size` | 读文件描述符 |
| `Write` | `fd, buf, size` | 写文件描述符 |
| `Mmap` | `size, prot` | 内存映射 |
| `Munmap` | `addr, size` | 解除内存映射 |

## ⏱️ 调度器设计

```mermaid
flowchart TD
    accTitle: 调度器决策流程
    accDescr: 展示 Round Robin 调度器的决策流程，包括时间片检查、就绪队列弹出和 Idle 状态。

    schedule["schedule()"] --> policy{"策略?"}
    policy -->|"RoundRobin"| rr{"当前进程?"}
    policy -->|"FIFO"| fifo{"当前进程?"}
    policy -->|"Priority"| prio{"有更高优先级?"}

    rr -->|"有"| tick{"time_used >= time_slice ?"}
    tick -->|"是"| reAdd["回入就绪队列<br/>time_used = 0"]
    tick -->|"否"| keep["继续运行当前"]
    reAdd --> next["弹出队首"]
    rr -->|"无"| next
    next -->|"有"| run["Run{pid, tid}"]
    next -->|"无"| idle["Idle"]

    fifo -->|"有且队列非空"| next
    fifo -->|"其他"| keep

    prio -->|"是"| preempt["抢占 → 回入队列"]
    prio -->|"否"| keep
    preempt --> next

    classDef decision fill:#fef3c7,stroke:#f59e0b,color:#92400e
    classDef action fill:#dbeafe,stroke:#3b82f6,color:#1e40af
    classDef output fill:#dcfce7,stroke:#22c55e,color:#14532d
    classDef halt fill:#fecaca,stroke:#ef4444,color:#7f1d1d

    class policy,rr,fifo,prio,tick decision
    class reAdd,preempt,next action
    class run,keep output
    class idle halt
```

### 调度策略对比

| 策略 | Preemptive | 时间片 | 饥饿风险 | 适用场景 |
|------|:---:|:---:|:---:|------|
| FIFO | ❌ | 无 | 高（长进程阻塞短进程） | 批处理 |
| Round Robin | ✅ | 可配置（默认10） | 无 | 交互式系统 |
| Priority | ✅ | 无 | 低优先级可能饥饿 | 实时系统 |
| SJF | ❌ | 无 | 长作业可能饥饿 | 批处理（已知执行时间） |
| MLFQ | ✅ | 多级 | 低 | 通用系统（未来实现） |

## 🔧 线程安全设计

所有共享状态都通过 `Arc<Mutex<T>>` 保护，使用统一的 `lock_mutex` 辅助函数：

```rust
fn lock_mutex<T>(mutex: &Mutex<T>) -> GenshinResult<MutexGuard<T>> {
    mutex.lock().map_err(|e| GenshinError::Service(ServiceError::InvalidArguments {
        param: "mutex".to_string(),
        reason: format!("Mutex poisoned: {}", e),
    }))
}
```

**锁层级**（按获取顺序，避免死锁）：

```mermaid
flowchart TD
    accTitle: ProcessService 锁层级
    accDescr: 展示 ProcessService 中 Mutex 的获取顺序层级，较高层级不能在其持有时获取较低层级的锁。

    L1["process_table"] --> L2["PCB (单个进程)"]
    L2 --> L3["parent_children"]
    L3 --> L4["ipc_manager"]
    L4 --> L5["MessageQueue (单个队列)"]
    L1 --> L6["scheduler"]
    L6 --> L7["sync_manager"]
    L7 --> L8["Semaphore / MutexLock"]

    classDef lock fill:#dbeafe,stroke:#3b82f6,color:#1e40af
    class L1,L2,L3,L4,L5,L6,L7,L8 lock
```

> **注意**：`scheduler` 和 `process_table` 是同一层级，不可嵌套互锁。之前 `handle_schedule` 中存在的双重锁 `scheduler` 已在修复中消除。

## 🧪 测试覆盖

```
ProcessService 集成测试:  17 tests  ✅
PCB/TCB 单元测试:         13 tests  ✅
IPC 单元测试:             10 tests  ✅
Sync 单元测试:            12 tests  ✅
Scheduler 单元测试:       12 tests  ✅
─────────────────────────────────
合计:                     64 tests
```

测试覆盖的核心场景：

| 测试类别 | 覆盖场景 |
|---------|---------|
| 进程创建/终止 | `create_process`, `fork`, `exec`, `exit` |
| 调度 | schedule、block/unblock、context switch、timer interrupt |
| IPC 消息 | send、receive（阻塞/非阻塞）、peek、队列容量 |
| 共享内存 | create、attach、detach、ref_count、延迟删除 |
| 同步 | semaphore wait/signal/overflow/reset、mutex acquire/release/recursive/deadlock |
| 信号 | SIGTERM、SIGSTOP、SIGCONT 状态转换 |

## 🔌 接入方式

在 `main.rs` 中作为后台线程启动：

```rust
use genshin_os::services::process::ProcessService;

let bus = Arc::new(LockedBus::new());
let process_bus = bus.clone();

thread::spawn(move || {
    let service = ProcessService::new(process_bus);
    service.run(); // 无限循环：接收 Envelope → 处理消息
});
```

服务启动后自动订阅 MessageBus，持续处理 `KernelMsg::Process`、`KernelMsg::Syscall` 和 `KernelMsg::Interrupt` 消息。
