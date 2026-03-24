# genshin-OS 接口设计审查与文档

> **面向内核服务层开发者的完整接口指南**
>
> 本文档定义了硬件模拟层与内核服务层之间的所有接口契约

## 📋 目录

1. [架构概述](#架构概述)
2. [KernelMsg 消息契约](#kernelmsg-消息契约)
3. [硬件层接口](#硬件层接口)
4. [设计审查与改进建议](#设计审查与改进建议)
5. [使用示例](#使用示例)
6. [扩展指南](#扩展指南)

---

## 架构概述

### 层次结构

```
┌─────────────────────────────────────────────────────────┐
│              用户交互层 (UI Layer)                       │
│         CLI Shell / TUI Monitor                          │
└────────────────────┬────────────────────────────────────┘
                     │ KernelMsg
┌────────────────────┴────────────────────────────────────┐
│           核心交换层 (Exchange Layer)                    │
│              MessageBus (LockedBus)                     │
└────────────────────┬────────────────────────────────────┘
                     │ KernelMsg
┌────────────────────┴────────────────────────────────────┐
│            内核服务层 (Service Layer)  ← 你在这里        │
│  ProcessService | MemoryService | FileService | Device  │
└────────────────────┬────────────────────────────────────┘
                     │ KernelMsg
┌────────────────────┴────────────────────────────────────┐
│            硬件模拟层 (Hardware Layer)                   │
│  CPU | MMU | Timer | Memory | Disk | IVT                │
└─────────────────────────────────────────────────────────┘
```

### 通信原则

1. **Fire-and-Forget**：发送后不等待响应
2. **单向数据流**：UI → Exchange → Service → Hardware
3. **异步上报**：硬件异常通过 `KernelMsg::Interrupt` 上报
4. **无直接调用**：禁止跨层直接函数调用

---

## KernelMsg 消息契约

### 核心消息类型

```rust
pub enum KernelMsg {
    Syscall(Syscall),           // 用户系统调用
    Interrupt(Interrupt),       // 硬件中断
    Process(ProcessRequest),    // 进程服务请求
    Memory(MemoryRequest),      // 内存服务请求
    File(FileRequest),          // 文件服务请求
    Device(DeviceRequest),      // 设备服务请求
}
```

### 1. Syscall - 用户系统调用

**来源**：用户进程通过 `INT 0x80` 触发

**处理者**：内核服务层

```rust
pub enum Syscall {
    // 进程管理
    CreateProcess { executable: String, args: Vec<String> },
    ExitProcess { exit_code: i32 },
    CreateThread { entry_point: VirtAddr },

    // 文件 I/O
    Read { fd: u32, buf: VirtAddr, size: usize },
    Write { fd: u32, buf: VirtAddr, size: usize },

    // 内存管理
    Mmap { size: usize, prot: MemProt },
    Munmap { addr: VirtAddr, size: usize },
}
```

**注意事项**：
- `buf` 是用户空间虚拟地址，需要通过 MMU 转换
- `fd` 是文件描述符，由文件服务层维护
- 返回值需要写回用户寄存器（R0）

### 2. Interrupt - 硬件中断

**来源**：硬件模拟层

**处理者**：内核服务层

```rust
pub enum Interrupt {
    Timer,                          // 时钟中断（触发调度）
    PageFault {                    // 缺页异常
        addr: VirtAddr,
        access_type: AccessType,
    },
    IoComplete { device_id: u32 }, // I/O 完成中断
    SyscallTrap,                   // 系统调用陷阱
    HardwareFailure {              // 硬件故障
        component: String,
    },
}
```

**处理流程**：
1. 硬件检测到异常
2. 发送 `KernelMsg::Interrupt` 到消息总线
3. 内核服务层订阅并处理
4. 处理完成后恢复执行

### 3. ProcessRequest - 进程服务

**请求类型**：

```rust
pub enum ProcessRequest {
    Schedule { pid: Pid, tid: Tid },           // 调度进程/线程
    Block { pid: Pid, tid: Tid, reason: BlockReason },  // 阻塞进程
    Unblock { pid: Pid, tid: Tid },            // 唤醒进程
    QueryState { pid: Pid },                   // 查询进程状态
    ContextSwitch { from_pid: Pid, to_pid: Pid },  // 上下文切换
}
```

**阻塞原因**：
```rust
pub enum BlockReason {
    WaitingForIo { device_id: u32 },
    WaitingForMemory,
    WaitingForLock { lock_addr: VirtAddr },
    Sleeping { duration_ms: u64 },
    WaitingForChild { pid: Pid },
}
```

### 4. MemoryRequest - 内存服务

**请求类型**：

```rust
pub enum MemoryRequest {
    AllocFrame { count: usize },           // 分配物理帧
    FreeFrame { paddr: PhysAddr },         // 释放物理帧
    MapPage {                              // 建立页表映射
        pid: Pid,
        virt: VirtAddr,
        phys: PhysAddr,
        prot: MemProt,
    },
    UnmapPage { pid: Pid, virt: VirtAddr }, // 删除页表映射
    PageFaultHandler {                     // 缺页处理
        pid: Pid,
        faulting_addr: VirtAddr,
        access_type: AccessType,
    },
    SwapOut { pid: Pid, virt: VirtAddr },  // 换出到磁盘
    SwapIn { pid: Pid, virt: VirtAddr },   // 从磁盘换入
}
```

### 5. FileRequest - 文件服务

**请求类型**：

```rust
pub enum FileRequest {
    Open { path: String, flags: OpenFlags },
    Close { fd: u32 },
    Read {
        fd: u32,
        offset: u64,
        buf: VirtAddr,  // 注意：这是虚拟地址
        size: usize,
    },
    Write {
        fd: u32,
        offset: u64,
        buf: VirtAddr,  // 注意：这是虚拟地址
        size: usize,
    },
    Unlink { path: String },
    Stat { path: String },
}
```

### 6. DeviceRequest - 设备服务

**请求类型**：

```rust
pub enum DeviceRequest {
    Read {
        device_id: u32,
        buf: VirtAddr,
        size: usize,
    },
    Write {
        device_id: u32,
        buf: VirtAddr,
        size: usize,
    },
    Init { device_id: u32 },
    Shutdown { device_id: u32 },
    Status { device_id: u32 },
}
```

---

## 硬件层接口

### 概览

硬件层提供的所有公共接口：

| 组件 | 接口类型 | 主要功能 |
|------|---------|---------|
| `PhysicalMemory` | 内存访问 | 物理内存读写、状态快照 |
| `VirtualDisk` | 块设备 | 扇区读写、格式化 |
| `MMU` | 地址转换 | 虚实地址转换、页表管理 |
| `Timer` | 时钟中断 | 定时器控制、中断生成 |
| `VirtualCPU` | 指令执行 | 取指-译码-执行、异常上报 |
| `IVT` | 中断向量 | 中断号定义、类型查询 |

### 1. PhysicalMemory - 物理内存

```rust
pub struct PhysicalMemory;

impl PhysicalMemory {
    // 创建内存
    pub fn new(size: usize) -> Self;
    pub fn size(&self) -> usize;

    // 字节访问
    pub fn read_u8(&self, addr: usize) -> Result<u8, MemError>;
    pub fn write_u8(&self, addr: usize, value: u8) -> Result<(), MemError>;

    // 字访问（16-bit）
    pub fn read_u16(&self, addr: usize) -> Result<u16, MemError>;
    pub fn write_u16(&self, addr: usize, value: u16) -> Result<(), MemError>;

    // 双字访问（32-bit）
    pub fn read_u32(&self, addr: usize) -> Result<u32, MemError>;
    pub fn write_u32(&self, addr: usize, value: u32) -> Result<(), MemError>;

    // 四字访问（64-bit）
    pub fn read_u64(&self, addr: usize) -> Result<u64, MemError>;
    pub fn write_u64(&self, addr: usize, value: u64) -> Result<(), MemError>;

    // 块访问
    pub fn read_slice(&self, addr: usize, buf: &mut [u8]) -> Result<(), MemError>;
    pub fn write_slice(&self, addr: usize, buf: &[u8]) -> Result<(), MemError>;

    // 维护操作
    pub fn clear(&self) -> Result<(), MemError>;
    pub fn dump_state(&self) -> MemoryState;
}
```

**错误类型**：
```rust
pub enum MemError {
    OutOfBounds { addr: usize, size: usize },
    Misaligned { addr: usize },
    Locked,
}
```

### 2. VirtualDisk - 虚拟磁盘

```rust
pub struct VirtualDisk;

impl VirtualDisk {
    // 创建磁盘
    pub fn new(total_sectors: u32) -> Self;
    pub fn size_bytes(&self) -> u64;
    pub fn total_sectors(&self) -> u32;

    // 扇区访问
    pub fn read_sector(&self, sector: u32) -> Result<Vec<u8>, DiskError>;
    pub fn write_sector(&self, sector: u32, buf: &[u8]) -> Result<(), DiskError>;

    // 多扇区访问
    pub fn read_sectors(&self, start_sector: u32, count: u32)
        -> Result<Vec<u8>, DiskError>;
    pub fn write_sectors(&self, start_sector: u32, buf: &[u8])
        -> Result<(), DiskError>;

    // 维护操作
    pub fn zero_sector(&self, sector: u32) -> Result<(), DiskError>;
    pub fn zero_sectors(&self, start_sector: u32, count: u32)
        -> Result<(), DiskError>;
    pub fn dump_state(&self) -> DiskState;
}
```

**常量**：
```rust
pub const SECTOR_SIZE: usize = 512;
```

### 3. MMU - 内存管理单元

```rust
pub struct MMU;

impl MMU {
    // 创建 MMU
    pub fn new(memory: PhysicalMemory, page_size: usize) -> Self;
    pub fn page_size(&self) -> usize;

    // 页表管理
    pub fn create_page_table(&self, pid: Pid);
    pub fn remove_page_table(&self, pid: Pid);

    // 页表映射
    pub fn map_page(
        &self,
        pid: Pid,
        vaddr: VirtAddr,
        paddr: PhysAddr,
        flags: PageFlags,
    ) -> Result<(), MMUError>;

    pub fn unmap_page(&self, pid: Pid, vaddr: VirtAddr)
        -> Result<(), MMUError>;

    // 地址转换
    pub fn translate(
        &self,
        pid: Pid,
        vaddr: VirtAddr,
        access: AccessType,
    ) -> Result<PhysAddr, MMUError>;

    // 虚拟内存访问
    pub fn read_u8(&self, pid: Pid, vaddr: VirtAddr) -> Result<u8, MMUError>;
    pub fn read_u32(&self, pid: Pid, vaddr: VirtAddr) -> Result<u32, MMUError>;
    pub fn write_u8(&self, pid: Pid, vaddr: VirtAddr, value: u8)
        -> Result<(), MMUError>;
    pub fn write_u32(&self, pid: Pid, vaddr: VirtAddr, value: u32)
        -> Result<(), MMUError>;

    // 状态查询
    pub fn dump_state(&self, pid: Pid) -> MMUState;
}
```

**页表项标志**：
```rust
pub struct PageFlags {
    pub present: bool,
    pub writable: bool,
    pub user_accessible: bool,
}

impl PageFlags {
    pub const fn present_readonly() -> Self;
    pub const fn present_writable() -> Self;
}
```

**错误类型**：
```rust
pub enum MMUError {
    PageNotPresent { vaddr: VirtAddr },
    PermissionDenied { vaddr: VirtAddr, access: AccessType },
    InvalidPhysicalAddress { paddr: PhysAddr },
    PageTableNotFound { pid: Pid },
}
```

### 4. Timer - 时钟

```rust
pub struct Timer;

impl Timer {
    // 创建时钟
    pub fn new(bus: Arc<dyn MessageBus>, config: TimerConfig) -> Self;

    // 控制接口
    pub fn start(&self);
    pub fn stop(&self);
    pub fn pause(&self);
    pub fn resume(&self);
    pub fn is_running(&self) -> bool;

    // 状态查询
    pub fn tick_count(&self) -> u64;
    pub fn reset_counter(&self);
    pub fn dump_state(&self) -> TimerSnapshot;
}

pub struct TimerConfig {
    pub tick_interval_ms: u64,
    pub auto_start: bool,
}
```

**行为**：
- 定时器独立线程运行
- 每 `tick_interval_ms` 毫秒发送一次 `KernelMsg::Interrupt(Timer)`
- 这是进程调度的"第一推动力"

### 5. VirtualCPU - 虚拟 CPU

```rust
pub struct VirtualCPU;

impl VirtualCPU {
    // 创建 CPU
    pub fn new(mmu: Arc<MMU>, bus: Arc<dyn MessageBus>, pid: Pid) -> Self;

    // 进程管理
    pub fn pid(&self) -> Pid;
    pub fn set_pid(&mut self, pid: Pid);

    // 寄存器访问
    pub fn read_register(&self, reg: Register) -> u64;
    pub fn write_register(&mut self, reg: Register, value: u64);

    // 特殊寄存器
    pub fn pc(&self) -> u64;
    pub fn set_pc(&mut self, pc: u64);
    pub fn sp(&self) -> u64;
    pub fn set_sp(&mut self, sp: u64);
    pub fn flags(&self) -> CPUFlags;

    // 控制流
    pub fn is_halted(&self) -> bool;
    pub fn halt(&mut self);
    pub fn reset(&mut self);

    // 执行
    pub fn step(&mut self) -> Result<(), CPUError>;

    // 上下文切换
    pub fn save_state(&self) -> CPUState;
    pub fn restore_state(&mut self, state: CPUState);

    // 状态查询
    pub fn dump_state(&self) -> CPUState;
}
```

**寄存器定义**：
```rust
pub enum Register {
    R0 = 0,  // 通用寄存器（也用于返回值）
    R1 = 1,  // 通用寄存器
    R2 = 2,  // 通用寄存器
    R3 = 3,  // 通用寄存器
}
```

**CPU 标志**：
```rust
pub struct CPUFlags {
    pub zero: bool,       // 零标志
    pub sign: bool,       // 符号标志
    pub overflow: bool,   // 溢出标志
    pub carry: bool,      // 进位标志
}
```

**错误类型**：
```rust
pub enum CPUError {
    InvalidInstruction { pc: VirtAddr },
    DivideByZero { pc: VirtAddr },
    PageFault { vaddr: VirtAddr },
    InvalidRegister { index: usize },
    Halted,
}
```

### 6. IVT - 中断向量表

```rust
pub enum InterruptVector {
    DivideByZero = 0x00,
    PageFault = 0x0E,
    Timer = 0x20,
    Syscall = 0x80,
}

pub enum InterruptType {
    Exception,   // 同步异常
    Interrupt,   // 异步中断
    Trap,        // 陷阱（如系统调用）
}

impl IVT {
    pub fn get_vector(vector: u8) -> Option<(InterruptVector, InterruptType)>;
    pub fn all_vectors() -> &'static [(InterruptVector, InterruptType)];
    pub fn format_vector(vector: u8) -> String;
}
```

---

## 设计审查与改进建议

### ✅ 优点

1. **清晰的分层架构**：各层职责明确
2. **统一的通信机制**：所有交互通过 `KernelMsg`
3. **类型安全**：使用 Rust 类型系统保证安全
4. **异步解耦**：fire-and-forget 模式避免阻塞

### ⚠️ 潜在问题与改进建议

#### 1. 缺少响应机制

**问题**：当前是 fire-and-forget，某些操作需要同步响应

**建议**：增加回调机制或请求 ID

```rust
// 建议的改进
pub enum KernelMsg {
    // 现有变体...

    // 新增：带回调的请求
    Request {
        id: u64,              // 请求 ID
        msg: Box<KernelMsg>,  // 实际请求
        callback: Option<Sender<Response>>,
    },
}

pub enum Response {
    Success { id: u64, result: ResponseData },
    Error { id: u64, error: ServiceError },
}
```

#### 2. 缺少进程间通信 (IPC)

**建议**：增加 IPC 消息类型

```rust
pub enum KernelMsg {
    // 现有变体...

    // 新增：进程间通信
    IPC(IPCMessage),
}

pub enum IPCMessage {
    Send {
        from_pid: Pid,
        to_pid: Pid,
        data: Vec<u8>,
    },
    Receive {
        pid: Pid,
        buf: VirtAddr,
        max_size: usize,
    },
}
```

#### 3. 缺少信号机制

**建议**：增加 Unix-style 信号

```rust
pub enum KernelMsg {
    // 现有变体...

    // 新增：信号
    Signal(SignalMessage),
}

pub enum SignalMessage {
    Send {
        pid: Pid,
        signal: Signal,
    },
    Handle {
        pid: Pid,
        signal: Signal,
    },
}

pub enum Signal {
    SIGTERM = 15,
    SIGKILL = 9,
    SIGSTOP = 19,
    SIGCONT = 18,
    // ...
}
```

#### 4. 缺少共享内存支持

**建议**：增加共享内存消息

```rust
pub enum MemoryRequest {
    // 现有变体...

    // 新增：共享内存
    CreateShared {
        size: usize,
        key: u64,
    },
    AttachShared {
        shmid: u32,
        addr: VirtAddr,
    },
    DetachShared {
        shmid: u32,
    },
}
```

#### 5. 硬件层错误处理不一致

**问题**：不同硬件组件使用不同的错误类型

**建议**：统一错误处理

```rust
// 统一的硬件错误类型
pub enum HardwareError {
    Memory(MemError),
    Disk(DiskError),
    MMU(MMUError),
    CPU(CPUError),
    Device(String),  // 设备错误消息
}
```

#### 6. 缺少权限控制

**建议**：增加基于权限的访问控制

```rust
pub struct ProcessCapabilities {
    pub can_create_processes: bool,
    pub can_access_network: bool,
    pub can_access_hardware: bool,
    pub max_memory: usize,
}

pub enum ProcessRequest {
    // 现有变体...

    // 新增：权限管理
    SetCapabilities {
        pid: Pid,
        caps: ProcessCapabilities,
    },
}
```

---

## 使用示例

### 示例 1：处理系统调用

```rust
use genshin_os::{KernelMsg, Syscall, MessageBus};
use std::sync::Arc;

struct SyscallHandler {
    bus: Arc<MessageBus>,
    receiver: Receiver<KernelMsg>,
}

impl SyscallHandler {
    fn new(bus: Arc<MessageBus>) -> Self {
        let receiver = bus.subscribe();
        Self { bus, receiver }
    }

    fn run(&self) {
        loop {
            if let Ok(msg) = self.receiver.recv() {
                if let KernelMsg::Syscall(syscall) = msg {
                    self.handle_syscall(syscall);
                }
            }
        }
    }

    fn handle_syscall(&self, syscall: Syscall) {
        match syscall {
            Syscall::CreateProcess { executable, args } => {
                // 创建新进程
                // ...
            }
            Syscall::Read { fd, buf, size } => {
                // 读取文件
                // 注意：buf 是虚拟地址，需要通过 MMU 转换
            }
            // ... 其他系统调用
        }
    }
}
```

### 示例 2：处理硬件中断

```rust
struct InterruptHandler {
    bus: Arc<MessageBus>,
    receiver: Receiver<KernelMsg>,
}

impl InterruptHandler {
    fn handle_interrupt(&self, interrupt: Interrupt) {
        match interrupt {
            Interrupt::Timer => {
                // 触发进程调度
                self.schedule_next_process();
            }
            Interrupt::PageFault { addr, access_type } => {
                // 处理缺页异常
                self.handle_page_fault(addr, access_type);
            }
            Interrupt::IoComplete { device_id } => {
                // 唤醒等待 I/O 的进程
                self.wakeup_io_waiter(device_id);
            }
            // ... 其他中断
        }
    }
}
```

### 示例 3：使用 MMU 进行地址转换

```rust
use genshin_os::{MMU, Pid, VirtAddr};

fn copy_from_user(
    mmu: &MMU,
    pid: Pid,
    src: VirtAddr,
    dst: &mut [u8],
) -> Result<(), MMUError> {
    for i in 0..dst.len() {
        let byte = mmu.read_u8(pid, src + i as u64)?;
        dst[i] = byte;
    }
    Ok(())
}

fn copy_to_user(
    mmu: &MMU,
    pid: Pid,
    dst: VirtAddr,
    src: &[u8],
) -> Result<(), MMUError> {
    for i in 0..src.len() {
        mmu.write_u8(pid, dst + i as u64, src[i])?;
    }
    Ok(())
}
```

### 示例 4：进程上下文切换

```rust
use genshin_os::{VirtualCPU, CPUState};

struct Process {
    pid: Pid,
    cpu_state: CPUState,
    // ... 其他进程状态
}

struct Scheduler {
    current_process: Option<Process>,
    ready_queue: Vec<Process>,
    cpu: VirtualCPU,
}

impl Scheduler {
    fn context_switch(&mut self, next_pid: Pid) {
        // 保存当前进程状态
        if let Some(ref mut current) = self.current_process {
            current.cpu_state = self.cpu.save_state();
        }

        // 查找下一个进程
        let next_idx = self.ready_queue
            .iter()
            .position(|p| p.pid == next_pid)
            .expect("Process not found");

        let mut next = self.ready_queue.remove(next_idx);

        // 恢复下一个进程状态
        self.cpu.restore_state(next.cpu_state);
        self.cpu.set_pid(next.pid);

        self.current_process = Some(next);
    }
}
```

---

## 扩展指南

### 添加新的系统调用

1. 在 `Syscall` 枚举中添加新变体
2. 在系统调用处理器中添加匹配分支
3. 更新文档

```rust
// 1. 添加变体
pub enum Syscall {
    // ... 现有变体

    // 新增：获取进程 ID
    GetPid,
}

// 2. 处理
fn handle_syscall(&self, syscall: Syscall) {
    match syscall {
        Syscall::GetPid => {
            let pid = current_process.pid();
            cpu.write_register(Register::R0, pid);
        }
        // ... 其他处理
    }
}
```

### 添加新的硬件组件

1. 创建新的硬件模块
2. 实现 `dump_state()` 方法
3. 通过 `KernelMsg::Interrupt` 上报异常
4. 在 `hardware/mod.rs` 中导出

```rust
// src/hardware/network.rs
pub struct NetworkCard {
    bus: Arc<dyn MessageBus>,
    // ...
}

impl NetworkCard {
    pub fn new(bus: Arc<dyn MessageBus>) -> Self {
        // ...
    }

    pub fn send_packet(&self, data: &[u8]) -> Result<(), NetworkError> {
        // 发送数据包
        // 完成后发送 KernelMsg::Interrupt(IoComplete { device_id })
    }

    pub fn dump_state(&self) -> NetworkState {
        // ...
    }
}
```

### 添加新的服务层组件

1. 定义新的 `KernelMsg` 变体
2. 实现服务处理器
3. 订阅消息总线
4. 实现状态管理

```rust
// 1. 定义消息
pub enum KernelMsg {
    // ... 现有变体
    Network(NetworkRequest),
}

pub enum NetworkRequest {
    Connect { addr: String, port: u16 },
    Send { socket: u32, data: Vec<u8> },
    Recv { socket: u32, buf: VirtAddr, size: usize },
}

// 2. 实现服务
struct NetworkService {
    receiver: Receiver<KernelMsg>,
    // ...
}

impl NetworkService {
    fn run(&self) {
        loop {
            if let Ok(msg) = self.receiver.recv() {
                if let KernelMsg::Network(req) = msg {
                    self.handle_request(req);
                }
            }
        }
    }
}
```

---

## 类型别名速查

```rust
// 进程/线程标识
pub type Pid = u64;  // Process ID
pub type Tid = u64;  // Thread ID

// 地址类型
pub type VirtAddr = u64;  // Virtual Address
pub type PhysAddr = u64;  // Physical Address

// 文件描述符
pub type Fd = u32;

// 设备 ID
pub type DeviceId = u32;
```

---

## 错误处理最佳实践

1. **硬件层**：使用 `Result<T, Error>`，不要 panic
2. **服务层**：捕获硬件错误，转换为 `KernelMsg`
3. **错误传播**：通过消息总线上报，不直接返回

```rust
// 好的做法
fn handle_page_fault(&self, addr: VirtAddr) {
    match self.mmu.translate(pid, addr, AccessType::Read) {
        Err(MMUError::PageNotPresent { .. }) => {
            // 发送消息给内存服务
            let msg = KernelMsg::Memory(MemoryRequest::PageFaultHandler {
                pid,
                faulting_addr: addr,
                access_type: AccessType::Read,
            });
            let _ = self.bus.send(msg);
        }
        Err(e) => {
            // 其他错误：记录日志，可能终止进程
            eprintln!("MMU error: {:?}", e);
        }
        _ => {}
    }
}

// 不好的做法
fn handle_page_fault(&self, addr: VirtAddr) {
    // 不要在服务层直接 panic
    panic!("Page fault!");
}
```

---

## 总结

本文档定义了 genshin-OS 硬件模拟层与内核服务层之间的完整接口契约。主要特点：

- ✅ 清晰的分层架构
- ✅ 统一的通信机制
- ✅ 类型安全的接口
- ✅ 完整的错误处理
- ✅ 易于扩展的设计

**内核服务层开发者应该**：
1. 熟悉 `KernelMsg` 的所有变体
2. 理解硬件层的公共接口
3. 使用消息总线进行所有通信
4. 实现服务的状态管理
5. 处理硬件上报的异常

**禁止事项**：
- ❌ 跨层直接调用函数
- ❌ 绕过消息总线通信
- ❌ 在服务层直接访问硬件
- ❌ 忽略错误处理

---

**最后更新**：2026-03-23
**维护者**：genshin-OS 架构组
