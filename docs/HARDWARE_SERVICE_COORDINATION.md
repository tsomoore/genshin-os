# 硬件层与内核服务层协作指南

本文档说明 genshin-OS 中**硬件模拟层**与**内核服务层**之间的协作机制、协议接口和消息传递流程。

---

## 📐 架构概览

```
┌─────────────────────────────────────────────────────────────┐
│                     用户交互层 (UI)                          │
│  ┌────────────────┐    ┌─────────────────────────────────┐ │
│  │  CLI Shell     │    │  TUI 系统监控器                  │ │
│  └────────┬───────┘    └──────────────┬──────────────────┘ │
└───────────┼──────────────────────────┼────────────────────┘
            │                          │
            ▼                          ▼
┌─────────────────────────────────────────────────────────────┐
│                    交换层 (Exchange)                        │
│              ╔═════════════════════════╗                    │
│              ║    Message Bus (总线)   ║                    │
│              ║   ┌───────────────┐    ║                    │
│              ║   │ crossbeam     │    ║                    │
│              ║   │ channel       │    ║                    │
│              ║   │ (async/MPMC)  │    ║                    │
│              ║   └───────────────┘    ║                    │
│              ╚═════════┬═══════════════╝                    │
└──────────────────────────┼──────────────────────────────────┘
                           │
        ┌──────────────────┼──────────────────┐
        │                  │                  │
        ▼                  ▼                  ▼
┌──────────────┐  ┌──────────────┐  ┌──────────────┐
│ 硬件模拟层    │  │ 内核服务层    │  │ 内核服务层    │
│              │  │ (Process)     │  │ (Memory)     │
│ ┌──────────┐ │  │              │  │              │
│ │ VirtualCPU│ │  │ ProcessService│ │ MemoryService│
│ │   MMU    │ │  │              │  │              │
│ │  Timer   │ │  │ 订阅 Process  │  │ 订阅 Memory  │
│ │   Disk   │ │  │ 消息并处理    │  │ 消息并处理   │
│ └──────────┘ │  └──────────────┘  └──────────────┘
└──────────────┘
```

**核心设计原则**：
- ✅ **所有通信必须经过 MessageBus** - 无跨层直接调用
- ✅ **单向异步消息传递** - Fire-and-forget 模式
- ✅ **统一消息格式** - 使用 `KernelMsg` 枚举
- ✅ **清晰的职责分离** - 硬件报告异常，服务做决策

---

## 🔄 消息传递机制

### 1. MessageBus 工作原理

```rust
pub trait MessageBus: Send + Sync {
    // 发送消息到总线（异步，不等待响应）
    fn send(&self, msg: KernelMsg) -> Result<(), BusError>;

    // 订阅总线消息（返回接收通道）
    fn subscribe(&self) -> Receiver<KernelMsg>;
}
```

**实现**：`LockedBus` 使用 crossbeam channel
- **多生产者、多消费者 (MPMC)**
- **无锁并发**，高性能
- **异步传递**，发送者不阻塞

### 2. 消息流详解

#### 场景 1：硬件中断报告

```
1. VirtualCPU 检测到除零异常
   └─> cpu.rs: trigger_divide_by_zero()

2. CPU 构造中断消息
   └─> KernelMsg::Interrupt(Interrupt::HardwareFailure { ... })

3. CPU 发送到总线
   └─> self.bus.send(msg)

4. 总线广播消息
   └─> 所有订阅者都能收到

5. ProcessService 收到消息
   └─> rx.recv() -> KernelMsg::Interrupt

6. ProcessService 处理
   └─> 决定如何处理异常（终止进程、记录日志等）
```

**代码示例**：
```rust
// 硬件层 (cpu.rs)
impl VirtualCPU {
    fn report_divide_by_zero(&self) {
        let msg = KernelMsg::Interrupt(Interrupt::HardwareFailure {
            component: "CPU".to_string(),
        });
        let _ = self.bus.send(msg);  // 发送后立即返回
    }
}

// 内核服务层 (由内核服务同学实现)
struct ProcessService {
    rx: Receiver<KernelMsg>,
}

impl ProcessService {
    fn run(&self) {
        loop {
            match self.rx.recv() {
                Ok(KernelMsg::Interrupt(Interrupt::HardwareFailure { component })) => {
                    eprintln!("Hardware failure in: {}", component);
                    // 处理硬件故障
                }
                // ... 处理其他消息类型
            }
        }
    }
}
```

#### 场景 2：进程间通信 (IPC)

```
1. 用户进程 A 发送 IPC 请求（通过系统调用）
   └─> Syscall::SendMessage { to_pid: 200, ... }

2. UI层转换为 KernelMsg
   └─> KernelMsg::Syscall(Syscall::SendMessage { ... })

3. 总线传递消息
   └─> ProcessService 收到

4. ProcessService 处理 IPC 请求
   └─> 查找目标进程，将消息放入其消息队列

5. 如果目标进程在等待，ProcessService 唤醒它
   └─> 发送 KernelMsg::Process(ProcessRequest::Unblock { ... })
```

#### 场景 3：内存缺页异常

```
1. VirtualCPU 执行指令，访问虚拟地址 0x1000
   └─> cpu.rs: fetch_byte()

2. MMU 翻译地址失败
   └─> mmu.rs: translate() 返回 Err(MMUError::PageNotPresent)

3. CPU 捕获错误，发送中断
   └─> KernelMsg::Interrupt(Interrupt::PageFault { addr: 0x1000, ... })

4. ProcessService 收到缺页中断
   └─> 决定如何处理

5. ProcessService 请求内存服务加载页面
   └─> 发送 KernelMsg::Memory(MemoryRequest::PageFaultHandler { ... })

6. MemoryService 处理缺页
   └─> 分配物理页、从磁盘读取、更新页表

7. MemoryService 通知 ProcessService 完成
   └─> 发送 KernelMsg::Process(ProcessRequest::Unblock { ... })
```

---

## 📡 KernelMsg 协议

### 消息类型分类

```rust
pub enum KernelMsg {
    // 1. 用户空间系统调用
    Syscall(Syscall),

    // 2. 硬件中断
    Interrupt(Interrupt),

    // 3. 进程服务请求
    Process(ProcessRequest),

    // 4. 内存/存储服务请求
    Memory(MemoryRequest),

    // 5. 文件系统服务请求
    File(FileRequest),

    // 6. 设备I/O服务请求
    Device(DeviceRequest),
}
```

### 各服务订阅的消息类型

| 服务层 | 订阅的消息类型 | 职责 |
|--------|---------------|------|
| **ProcessService** | `Process`, `Syscall`, `Interrupt` | 进程管理、调度、IPC |
| **MemoryService** | `Memory`, `Interrupt` | 内存分配、分页、交换 |
| **FileService** | `File` | 文件系统操作 |
| **DeviceService** | `Device`, `Interrupt::IoComplete` | 设备驱动、I/O处理 |

---

## 🔌 硬件层提供的接口

### 1. VirtualCPU (src/hardware/cpu.rs)

**职责**：模拟 CPU 的取指-译码-执行周期

**公开接口**：
```rust
impl VirtualCPU {
    pub fn new(mmu: Arc<MMU>, bus: Arc<dyn MessageBus>, pid: Pid) -> Self;
    pub fn step(&mut self) -> Result<(), CPUError>;
    pub fn run(&mut self, instructions: u64) -> Result<(), CPUError>;
    pub fn halt(&mut self);
    pub fn is_halted(&self) -> bool;
    pub fn dump_state(&self) -> CPUState;
}
```

**报告的消息**：
- `Interrupt::HardwareFailure` - 硬件故障
- `Interrupt::PageFault` - 通过 MMU 转发的缺页异常
- `Interrupt::SyscallTrap` - 系统调用陷入

### 2. MMU (src/hardware/mmu.rs)

**职责**：虚拟地址到物理地址的转换

**公开接口**：
```rust
impl MMU {
    pub fn new(memory: PhysicalMemory, page_size: usize) -> Self;
    pub fn create_page_table(&self, pid: Pid);
    pub fn remove_page_table(&self, pid: Pid);
    pub fn map_page(&self, pid: Pid, vaddr: VirtAddr, paddr: PhysAddr, flags: PageFlags);
    pub fn unmap_page(&self, pid: Pid, vaddr: VirtAddr);
    pub fn read_u8(&self, pid: Pid, vaddr: VirtAddr) -> Result<u8, MMUError>;
    pub fn write_u8(&self, pid: Pid, vaddr: VirtAddr, value: u8) -> Result<(), MMUError>;
    pub fn dump_state(&self, pid: Pid) -> MMUState;
}
```

**返回的错误**（由 CPU 捕获并转换为中断）：
- `MMUError::PageNotPresent` - 页不在内存中
- `MMUError::PermissionDenied` - 权限不足
- `MMUError::InvalidPhysicalAddress` - 无效物理地址

### 3. Timer (src/hardware/timer.rs)

**职责**：周期性发送时钟中断

**公开接口**：
```rust
impl Timer {
    pub fn new(config: TimerConfig, bus: Arc<dyn MessageBus>) -> Self;
    pub fn start(&mut self);
    pub fn stop(&mut self);
    pub fn pause(&mut self);
    pub fn resume(&mut self);
    pub fn snapshot(&self) -> TimerSnapshot;
}
```

**报告的消息**：
- `Interrupt::Timer` - 周期性时钟中断（用于进程调度）

### 4. VirtualDisk (src/hardware/disk.rs)

**职责**：模拟块存储设备

**公开接口**：
```rust
impl VirtualDisk {
    pub fn new(total_sectors: u32) -> Self;
    pub fn read_sector(&self, sector: u32) -> Result<Vec<u8>, DiskError>;
    pub fn write_sector(&self, sector: u32, buf: &[u8]) -> Result<(), DiskError>;
    pub fn zero_sector(&self, sector: u32) -> Result<(), DiskError>;
    pub fn dump_state(&self) -> DiskState;
}
```

### 5. PhysicalMemory (src/hardware/memory.rs)

**职责**：物理内存模拟

**公开接口**：
```rust
impl PhysicalMemory {
    pub fn new(size: usize) -> Self;
    pub fn read_u8(&self, addr: usize) -> Result<u8, MemoryError>;
    pub fn read_u32(&self, addr: usize) -> Result<u32, MemoryError>;
    pub fn write_u8(&self, addr: usize, value: u8) -> Result<(), MemoryError>;
    pub fn write_u32(&self, addr: usize, value: u32) -> Result<(), MemoryError>;
    pub fn dump_state(&self) -> MemoryState;
}
```

---

## 🔧 内核服务层实现指南

### 服务层基本框架

```rust
use genshin_os::{
    KernelMsg, ProcessRequest, MessageBus,
    GenshinError, GenshinResult,
};
use std::sync::Arc;
use crossbeam_channel::Receiver;

pub struct ProcessService {
    bus: Arc<dyn MessageBus>,
    rx: Receiver<KernelMsg>,
    // 服务内部状态
    // process_table: HashMap<Pid, PCB>,
    // message_queue: HashMap<Pid, Vec<IPCMessage>>,
    // ...
}

impl ProcessService {
    pub fn new(bus: Arc<dyn MessageBus>) -> Self {
        let rx = bus.subscribe();
        Self {
            bus,
            rx,
            // 初始化内部状态...
        }
    }

    pub fn run(&self) {
        loop {
            match self.rx.recv() {
                Ok(msg) => {
                    if let Err(e) = self.handle_message(msg) {
                        eprintln!("ProcessService error: {}", e);
                    }
                }
                Err(_) => {
                    eprintln!("Message bus disconnected");
                    break;
                }
            }
        }
    }

    fn handle_message(&self, msg: KernelMsg) -> GenshinResult<()> {
        match msg {
            KernelMsg::Process(req) => self.handle_process_request(req)?,
            KernelMsg::Syscall(req) => self.handle_syscall(req)?,
            KernelMsg::Interrupt(int) => self.handle_interrupt(int)?,
            _ => {} // 忽略不相关的消息
        }
        Ok(())
    }

    fn handle_process_request(&self, req: ProcessRequest) -> GenshinResult<()> {
        match req {
            ProcessRequest::SendMessage { from_pid, to_pid, msg } => {
                // 1. 验证 from_pid 和 to_pid 是否存在
                // 2. 将消息放入 to_pid 的消息队列
                // 3. 如果目标进程在等待，发送 Unblock 消息
            }

            ProcessRequest::ReceiveMessage { pid, blocking } => {
                // 1. 检查进程的消息队列
                // 2. 如果有消息，返回
                // 3. 如果没有消息且 blocking=true，发送 Block 消息
            }

            // ... 处理其他 ProcessRequest 变体
        }
        Ok(())
    }
}
```

### 响应机制使用

对于需要返回结果的操作，使用 `RequestWithResponse`：

```rust
use genshin_os::{RequestWithResponse, Response, ResponseData};

// 发送请求
let (req, rx) = RequestWithResponse::new(
    KernelMsg::Memory(MemoryRequest::AllocFrame { count: 1 })
);

let _ = self.bus.send(req.message);

// 等待响应（可以带超时）
match rx.recv_timeout(std::time::Duration::from_secs(1)) {
    Ok(resp) => {
        if resp.is_success() {
            match resp.unwrap_data() {
                ResponseData::PhysicalAddr(paddr) => {
                    // 使用分配的物理地址
                }
                _ => {}
            }
        }
    }
    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
        // 处理超时
    }
}
```

### 错误处理

使用统一的错误类型：

```rust
use genshin_error::{GenshinError, ServiceError};

fn some_operation(&self) -> GenshinResult<()> {
    if some_condition {
        return Err(GenshinError::Service(ServiceError::NotFound {
            resource_type: "Process".to_string(),
            id: "123".to_string(),
        }));
    }
    Ok(())
}
```

---

## 📊 消息传递流程图

### 完整的进程创建流程

```
用户          UI层         总线         ProcessService      MemoryService
 │             │            │                │                   │
 │ exec("/bin/test")
 │             │            │                │                   │
 ├────────────>│            │                │                   │
 │             │            │                │                   │
 │             │ KernelMsg::Syscall(CreateProcess)             │
 │             ├───────────>│               │                   │
 │             │            │               │                   │
 │             │            ├──────────────>│                   │
 │             │            │  订阅 Process 消息                  │
 │             │            │                │                   │
 │             │            │                │ 1. 创建PCB         │
 │             │            │                │ 2. 请求内存分配     │
 │             │            │                ├─────────────────>│
 │             │            │                │  KernelMsg::Memory(AllocFrame)
 │             │            │                │                   │
 │             │            │                │                   │ 3. 分配物理页
 │             │            │                │<─────────────────┤
 │             │            │                │  ResponseData::PhysicalAddr
 │             │            │                │                   │
 │             │            │                │ 4. 创建页表        │
 │             │            │                ├─────────────────>│
 │             │            │                │  MapPage          │
 │             │            │<──────────────┤                   │
 │             │            │  返回成功      │                   │
<─────────────┤            │                │                   │
 │             │            │                │                   │
```

### 缺页异常处理流程

```
VirtualCPU        总线         ProcessService      MemoryService      MMU
    │              │                │                   │              │
    │ 执行指令     │                │                   │              │
    │ 访问 0x1000  │                │                   │              │
    │              │                │                   │              │
    ├─> MMU翻译   │                │                   │              │
    │   失败       │                │                   │              │
    │<─┴           │                │                   │              │
    │              │                │                   │              │
    │ KernelMsg::Interrupt(PageFault)               │              │
    ├─────────────>│               │                   │              │
    │              │               │                   │              │
    │              ├──────────────>│                   │              │
    │              │               │ 1. 决定处理策略     │              │
    │              │               │ 2. 请求加载页面     │              │
    │              │               ├─────────────────>│              │
    │              │               │ PageFaultHandler  │              │
    │              │               │                   │              │
    │              │               │                   │ 1. 分配物理帧 │
    │              │               │                   ├────────────>│
    │              │               │                   │  AllocFrame │
    │              │               │                   │<──────────┤
    │              │               │                   │              │
    │              │               │                   │ 2. 从磁盘读  │
    │              │               │                   ├────────────>│
    │              │               │                   │  ReadSector │
    │              │               │                   │<──────────┤
    │              │               │                   │              │
    │              │               │                   │ 3. 更新页表  │
    │              │               │                   ├────────────>│
    │              │               │                   │  MapPage    │
    │              │               │                   │              │
    │              │<──────────────┤  完成响应          │              │
    │              │               │                   │              │
    │              │ Unblock 进程  │                   │              │
    ├─────────────>│               │                   │              │
    │              │               │                   │              │
    │ 继续执行     │                │                   │              │
```

---

## 🎯 关键协作点

### 1. 内存管理协作

**硬件层提供**：
- `MMU` - 地址转换、权限检查
- `PhysicalMemory` - 物理内存读写

**服务层负责**：
- 页表管理（调用 `MMU::map_page`）
- 缺页处理（响应 `Interrupt::PageFault`）
- 交换管理（使用 `MemoryRequest::SwapOut/In`）

### 2. 进程调度协作

**硬件层提供**：
- `Timer` - 周期性时钟中断
- `VirtualCPU` - 执行上下文切换接口

**服务层负责**：
- 响应 `Interrupt::Timer` 实现调度算法
- 管理进程状态（就绪、运行、阻塞）
- 发送 `ProcessRequest::ContextSwitch` 触发切换

### 3. IPC 消息传递

**硬件层不涉及** - IPC 纯属服务层逻辑

**服务层负责**：
- 处理 `ProcessRequest::SendMessage/ReceiveMessage`
- 维护消息队列
- 实现共享内存和同步原语

### 4. 设备I/O协作

**硬件层提供**：
- `VirtualDisk` - 块设备接口
- `Interrupt::IoComplete` - I/O完成通知

**服务层负责**：
- 响应 `FileRequest::Read/Write`
- 向 `DeviceService` 发送 I/O 请求
- 处理异步 I/O 完成

---

## 📝 开发检查清单

### 硬件层开发检查
- [ ] 所有异常都通过 `KernelMsg::Interrupt` 报告
- [ ] 不做决策，只报告异常
- [ ] 使用 `Arc<dyn MessageBus>` 发送消息
- [ ] 提供 `dump_state()` 用于调试
- [ ] 返回统一的错误类型（`GenshinError`）

### 服务层开发检查
- [ ] 订阅 `MessageBus` 接收消息
- [ ] 只处理相关的消息类型
- [ ] 使用 `RequestWithResponse` 获取操作结果
- [ ] 返回统一的错误类型
- [ ] 避免阻塞主循环（使用独立线程处理耗时操作）

---

## 🚀 快速开始

### 运行示例

```bash
# 1. 编译项目
cargo build

# 2. 运行 IPC 消息演示
cargo run --example ipc_messages_demo

# 3. 运行测试
cargo test --lib

# 4. 启动 TUI 监控器（如果实现）
cargo run --example tui_monitor
```

### 导入需要的类型

```rust
use genshin_os::{
    // 核心类型
    KernelMsg, MessageBus, LockedBus,

    // 硬件层
    VirtualCPU, MMU, PhysicalMemory, Timer, VirtualDisk,

    // 消息类型
    ProcessRequest, MemoryRequest, FileRequest,
    IPCMessage, SignalType, Interrupt,

    // 错误处理
    GenshinError, GenshinResult,

    // 响应机制
    RequestWithResponse, Response, ResponseData,
};
```

---

## 📚 相关文档

- `docs/API_QUICK_REFERENCE.md` - API 快速参考
- `docs/INTERFACE_REVIEW.md` - 完整接口设计
- `docs/IPC_MESSAGE_FORMAT.md` - IPC 消息格式详解
- `examples/ipc_messages_demo.rs` - IPC 使用示例
- `src/messaging/msg.rs` - 消息类型定义

---

**作者**: genshin-OS 硬件层开发组
**最后更新**: 2026-03-24
