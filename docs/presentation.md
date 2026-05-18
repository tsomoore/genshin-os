# Genshin-OS 答辩讲解文档

> 讲解顺序：架构 → CPU/内存本质 → 指令集 → 进程宇宙

---

## 一、系统架构：微内核 + 消息总线

### 1.1 为什么选择微内核

传统宏内核将所有服务（文件系统、网络、驱动）跑在内核态，一个模块崩溃整个系统挂。微内核只保留最核心的功能（进程调度、内存管理、IPC）在内核态，其他服务跑在用户态，通过消息通信。

我们的四层架构：

```
用户交互层： Shell / pmon TUI
    ↕ KernelMsg::Process / File / Memory / Device
交换层：    Kernel（消息总线中枢，LockedBus）
    ↕ process_tx / memory_tx / file_tx / intr_tx
服务层：    ProcessService / MemoryService / FileService / DeviceService
    ↕
硬件模拟层： VirtualCPU / MMU / PhysicalMemory / VirtualDisk / Timer
```

**核心代码位置：**

| 组件 | 文件 | 行号 |
|------|------|------|
| 主入口，四层组装 | `src/main.rs` | 13-69 |
| Kernel 消息路由 | `src/services/kernel.rs` | 30-45 |
| 消息总线 (LockedBus) | `src/messaging/bus.rs` | 159-230 |
| 消息枚举 (KernelMsg) | `src/messaging/msg.rs` | 30-45 |
| 架构文档 | `docs/architecture.md` | 全文 |

### 1.2 通信方式：消息总线

**铁律：所有模块间通信必须走消息总线，禁止直接函数调用。**

```rust
// src/messaging/msg.rs:30-45
pub enum KernelMsg {
    Process(ProcessRequest),   // 进程管理：fork, exec, wait, signal
    Memory(MemoryRequest),     // 内存管理：AllocFrame, MapPage, PageFaultHandler
    File(FileRequest),         // 文件系统：Open, Read, Write, Mkdir
    Device(DeviceRequest),     // 设备管理：ClipboardGet, ClipboardSet
    Interrupt(Interrupt),      // 硬件中断：Timer, PageFault, SyscallTrap
    Syscall(Syscall),          // 系统调用
}
```

每种消息类型有对应的 `*Request` 枚举定义具体的操作。Kernel 根据消息类型路由到对应的服务通道（process_tx / memory_tx / file_tx / intr_tx）。

**讲解要点**：跨模块无直接调用 → 模块独立可替换 → 符合微内核哲学。举例：ProcessService 需要分配内存时，不发 `memory_service.alloc()`，而是发 `KernelMsg::Memory(AllocFrame)` 到总线。

---

## 二、CPU 与内存的本质

### 2.1 CPU = 死循环指令解释器

**核心代码**：`src/hardware/cpu.rs`

CPU 就是一个 `while true` 循环，不断做三件事：**取指 → 译码 → 执行**。

```rust
// src/hardware/cpu.rs:371-403
pub fn step(&mut self) -> Result<(), CPUError> {
    if self.halted { return Err(CPUError::Halted); }
    if self.pagefault_pending { return Ok(()); }  // 等待缺页处理
    let saved_pc = self.pc;
    match self.fetch_instruction() {       // ① 取指
        Ok(instr) => {
            match self.execute_instruction(instr) {  // ②③ 译码+执行
                Ok(()) => { self.instruction_count += 1; Ok(()) }
                Err(CPUError::PageFault { vaddr, .. }) => {
                    self.pc = saved_pc;     // 重置 PC，等待重试
                    self.bus.send(Interrupt::PageFault { ... });
                    self.pagefault_pending = true;
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
        // ...
    }
}
```

**关键数据结构**：

```rust
// src/hardware/cpu.rs:248-278
pub struct VirtualCPU {
    registers: [u64; 4],      // R0-R3 通用寄存器
    pc: u64,                   // 程序计数器（下一条指令地址）
    sp: u64,                   // 栈指针
    flags: CPUFlags,           // Z(ero) S(ign) O(verflow) C(arry)
    current_pid: Pid,          // 所属进程
    pub halted: bool,          // 停机标志
    pub syscall_pending: bool, // 待处理的系统调用
    instruction_count: u64,    // 已执行指令数
    mmu: Arc<MMU>,             // 内存管理单元
    bus: Arc<dyn MessageBus>,  // 消息总线（报告异常）
}
```

**讲解要点**：
- PC（程序计数器）就是指向下一条指令的指针
- INT 0x80 设置 `syscall_pending = true`，ProcessService 在量子循环中处理
- 缺页时 PC 回退、发中断、等 MemoryService 处理完再重试

### 2.2 内存 = 大数组 + 两层映射

**核心代码**：`src/hardware/memory.rs`，`src/hardware/mmu.rs`

物理内存就是一维字节数组：

```rust
// src/hardware/memory.rs:19-25
pub struct PhysicalMemory {
    data: Vec<u8>,         // 就是一个大数组！
    size: usize,
}

// src/hardware/memory.rs:122-128
pub fn write_u8(&self, addr: usize, value: u8) -> Result<(), MemoryError> {
    if addr >= self.size { return Err(MemoryError::OutOfBounds { ... }); }
    self.data[addr] = value;  // 就是 arr[addr] = value！
    Ok(())
}
```

**MMU（内存管理单元）= 虚拟地址 → 物理地址翻译器**：

```rust
// src/hardware/mmu.rs:104-110
pub struct MMU {
    page_tables: Mutex<HashMap<Pid, HashMap<VirtAddr, PageTableEntry>>>,
    physical_memory: PhysicalMemory,
    page_size: usize,
}
```

翻译过程：

```
虚拟地址 0x1000  →  MMU.translate(pid, 0x1000)
  ↓ 对齐到 4KB 页
  page_vaddr = 0x1000
  offset = 0x000
  ↓ 查该进程的页表
  page_tables[pid][0x1000] → PageTableEntry { frame: 0x3000, flags: {present, writable} }
  ↓ 物理地址 = 帧基址 + 偏移
  paddr = 0x3000 + 0x000 = 0x3000
```

**页表项**：

```rust
// src/hardware/mmu.rs:84-101
struct PageTableEntry {
    frame: PhysAddr,     // 物理帧地址
    flags: PageFlags {   // 权限标志
        present: bool,        // 页是否在内存中
        writable: bool,
        user_accessible: bool,
    }
}
```

### 2.3 缺页中断（Page Fault）

当 CPU 访问未映射的虚拟地址时：

1. **MMU 返回 PageFault 错误** → CPU 发送 `Interrupt::PageFault` 到总线
2. **ProcessService 接收中断** → 发送 `MemoryRequest::PageFaultHandler` 到 MemoryService
3. **MemoryService 处理**：
   - 从 FrameAllocator 分配空闲物理帧
   - 建立页表映射（MapPage）
   - 如果物理内存不足 → 触发 Swap（换出旧页到磁盘）
4. **CPU 重试指令** → 这次页已映射，成功

**代码位置**：
- 缺页发送：`src/hardware/cpu.rs:383-390`
- 页错误处理轮询：`src/services/process/service.rs:658-678`
- MemoryService 缺页处理：`src/services/memory/service.rs:473-540`

### 2.4 物理帧分配与回收

```rust
// src/services/memory/alloc.rs:76-95
pub struct FrameAllocator {
    frames: Vec<Frame>,          // 所有帧数组
    free_queue: VecDeque<u64>,   // 空闲帧队列（FIFO）
    total_frames: u64,
    free_count: u64,
}

// 分配：从队头取一个空闲帧
pub fn allocate(&mut self, pid: Pid) -> Option<Frame> { ... }

// 回收：放回队尾，标记所有者=None
pub fn free(&mut self, frame_num: u64) -> bool { ... }
```

每个帧 4KB，4MB 物理内存 = 1024 帧。pmon Memory 面板中显示的就是这些帧的分配情况。

### 2.5 共享内存实现

两个进程映射同一个物理帧到自己的虚拟地址空间：

```
PID 3: 0x10000 → frame 0x2000
PID 4: 0x10000 → frame 0x2000   ← 同一个物理帧！
```

通过 `MemoryRequest::MapPage { pid, virt: 0x10000, phys: 0x2000, ... }` 建立映射。一个进程写入，另一个进程立即可见——这就是 `rwlock3` 中 `reader_count` 共享的原理。

---

## 三、自定义指令集

### 3.1 指令编码

每条指令固定 **8 字节**：`[opcode:1][dst:1][src_type:1][pad:1][value:4]`

```rust
// src/hardware/cpu.rs:407-417
fn fetch_instruction(&mut self) -> Result<Instruction, CPUError> {
    let opcode = self.fetch_byte()?;      // 操作码
    let dst_reg = self.fetch_byte()?;     // 目标寄存器
    let src_type = self.fetch_byte()?;    // 源类型（0=寄存器, 1=立即数）
    let _pad = self.fetch_byte()?;        // 填充
    let src_value = self.fetch_qword()?;  // 源值（4字节）
    // 译码...
}
```

### 3.2 指令表

```rust
// src/hardware/cpu.rs:114-187
pub enum Instruction {
    Mov { dst: Register, src: Operand },   // 0x01: 数据传输
    Add { dst: Register, src: Operand },   // 0x02: 加法
    Sub { dst: Register, src: Operand },   // 0x03: 减法
    Mul { dst: Register, src: Operand },   // 0x04: 乘法
    Div { dst: Register, src: Operand },   // 0x05: 除法
    Load { dst: Register, addr: VirtAddr },// 0x06: 从内存加载
    Store { src: Register, addr: VirtAddr },// 0x07: 存储到内存
    Jmp { addr: VirtAddr },               // 0x10: 无条件跳转
    Cmp { dst: Register, src: Operand },   // 0x11: 比较（设置标志位）
    Jz  { addr: VirtAddr },               // 0x12: 零标志跳转
    Jnz { addr: VirtAddr },               // 0x13: 非零标志跳转
    Int,                                    // 0x80: 系统调用陷阱
    Halt,                                   // 0x01 0x00: 停机
}
```

### 3.3 算术运算影响标志位

```rust
// src/hardware/cpu.rs:65-72
pub struct CPUFlags {
    pub zero: bool,      // Z: 结果为零
    pub sign: bool,      // S: 结果为负
    pub overflow: bool,  // O: 有符号溢出
    pub carry: bool,     // C: 无符号进位
}
```

ADD/SUB/MUL/DIV 执行后自动更新 Z/S/O/C，CMP 本质是 SUB 但不写回结果（只更新标志位），JZ/JNZ 根据 Z 标志决定是否跳转。

### 3.4 汇编器

```rust
// src/services/process/assembler.rs
pub fn assemble(asm: &str) -> Result<Vec<u8>, String>
pub fn assemble_file(path: &str) -> Result<(String, Vec<u8>), String>
```

将 .asm 文本编译为二进制字节码。支持 MOV/ADD/SUB/MUL/DIV/LOAD/STORE/JMP/CMP/JZ/JNZ/INT/HALT。

**示例**：`MOV R0, #201` → `[0x01, 0x00, 0x01, 0x00, 0xC9, 0x00, 0x00, 0x00]`

---

## 四、进程宇宙：fork + exec + exit

### 4.1 UNIX 进程模型

```
Shell: fork(1) → child_pid → exec(child, program, args) → wait(1, child_pid)
                                                                    ↓
CPU: 执行 program → exit(status) → Zombie → reaper 回收
```

**代码位置**：
- Shell: `src/ui/shell/mod.rs:128-157`（fork_exec_wait / fork_exec_detach）
- fork_impl: `src/services/process/service.rs:964-1035`
- exec_impl: `src/services/process/service.rs:1092-1140`
- exit 处理器: `src/services/process/service.rs:1433-1487`（R0=0 分支）
- Zombie 收割: `src/services/process/service.rs:704-718`

### 4.2 fork_impl：克隆进程

```rust
// src/services/process/service.rs:964-1035
fn fork_impl(&self, parent_pid: Pid) -> GenshinResult<Pid> {
    // 1. 分配新 PID
    let child_pid = next_pid++;

    // 2. 克隆父进程的页表（逐页复制）
    for (vaddr, paddr, flags) in self._mmu.get_page_entries(parent_pid) {
        AllocFrame → 新物理帧
        MapPage(child, vaddr, new_frame) → 建立子进程页表
        for o in 0..4096 { copy byte by byte } → 逐字节复制
    }

    // 3. 克隆 CPU 状态（PC, SP, 寄存器）
    child_cpu.set_pc(parent.pc);
    child_cpu.set_sp(parent.sp);
    for r in 0..4 { child_cpu.write_register(r, parent.registers[r]); }
    child_cpu.write_register(R0, 0);  // xv6 约定：子进程 fork 返回 0

    // 4. 创建 PCB，加入就绪队列
    self.handle_schedule(child_pid, 1);
    Ok(child_pid)
}
```

### 4.3 exec_impl：替换进程映像

```rust
// src/services/process/service.rs:1092-1140
fn exec_impl(&self, pid, executable, args) -> GenshinResult<()> {
    // 1. 加载程序（.asm 文件 → 汇编 → 二进制）
    let code = self.load_program(&executable);

    // 2. 卸载旧页
    for (vaddr, _, _) in self._mmu.get_page_entries(pid) {
        UnmapPage  // 解除映射
    }

    // 3. 分配新帧，映射，写入代码
    alloc_frames → MapPage(pid, 0, frame) → write_slice_virt(pid, 0, &code)

    // 4. 重置 CPU
    cpu.set_pc(0); cpu.set_sp(0xFFFF); cpu.halted = false;

    // 5. 更新 PCB
    pcb.name = executable; pcb.state = Ready;

    // 6. 加入调度队列
    self.handle_schedule(pid, 1);
}
```

### 4.4 exit：进程终止

```rust
// src/services/process/service.rs:1433-1487
0 => {  // R0 = 0: exit syscall
    let exit_code = r1 as i32;

    // 1. 卸载所有页面 + 释放所有帧
    for (vaddr, paddr, _) in &entries {
        UnmapPage + FreeFrame
    }

    // 2. 标记 Zombie
    p.state = ProcessState::Zombie { exit_code };

    // 3. 从调度器移除
    scheduler.block(pid, 1);

    // 4. 停机
    cpu.halt();

    // 5. 释放信号量 0（防止等待者永久阻塞）
    sync.get_semaphore(0).signal();
}
```

### 4.5 进程生命周期状态机

```
                  fork_impl(0)
  ┌──────────┐  ───────────────→  ┌──────────┐
  │ CREATING │                    │  READY   │ ← unblock / quantum expire
  └──────────┘                    └────┬─────┘
                                      │ schedule()
                                 ┌────▼─────┐
                                 │ RUNNING  │
                                 └────┬─────┘
                        exit/HALT │    │ time slice expire
                              ┌───▼──┐ │
                              │ZOMBIE│ │ → READY
                              └───┬──┘
                           wait │ │ reap
                              ┌─▼──▼─┐
                              │ FREED │
                              └──────┘
```

### 4.6 完整示例：ls 命令的执行路径

```
用户输入 "ls"
  → Shell: fork_exec_wait("ls", &["/"])
    → fork(1)       → PID N
    → exec(N, "ls") → 加载 programs/ls.asm → MOV R0,#18; INT 0x80; HALT
    → wait(1, N)    → 阻塞等子进程退出
    
Timer 驱动:
    → schedule → PID N 获得 CPU
    → CPU.step(): MOV R0,#18 → INT 0x80 → syscall_pending
    → handle_file_syscall(18): listdir → FileService → 打印文件列表
    → CPU.step(): HALT → cpu.halt()
    → 检测到 halted + 非阻塞 → 标记 Zombie
    → 通知 waiting_parents: PID 1（Shell 的 wait 解除）
    → Shell 收到 exit_code=0 → 返回提示符
    → Reaper 回收 Zombie
```

### 4.7 进程树展示

```
pstree 输出:
└── PID 1 [Creating] init          ← 系统启动时创建
    ├── PID 2 [Running] loop       ← 启动 demo 进程（fork+exec）
    ├── PID 3 [Ready] syncdemo     ← dual syncdemo 创建的信号量演示进程
    └── PID 4 [Blocked] syncdemo   ← 语义：正在等待信号量 0
```

---

## 五、演示 checklist

```
□ cargo run → 看到四层架构日志
□ ls → fork+exec+wait 路径
□ dual syncdemo → 两个进程交替信号量互斥
□ verbose on + dual syncdemo → 看 CPU0/CPU1 调度
□ pmon → 进程状态/内存物理帧/磁盘用量
□ pstree → 进程父子关系树
□ 讲 CPU 时展示 cpu.rs:step() 的 while-true 循环
□ 讲内存时展示 PhysicalMemory::write_u8 = arr[addr]=value
□ 讲进程时展示 fork_impl / exec_impl 的代码结构
```


## 六、硬件 Timer：系统心跳

### 6.1 本质

Timer 就是一个独立线程，每隔固定时间（10ms = 100Hz）向消息总线发送一次 `Interrupt::Timer`。

```rust
// src/hardware/timer.rs:51-66
pub struct Timer {
    state: Arc<Mutex<TimerStateInternal>>,
    bus: Arc<dyn MessageBus>,           // 消息总线
    tick_interval: Duration,             // 10ms
    tick_count: Arc<Mutex<u64>>,        // 累计 tick 数
}
```

**核心代码**：

| 组件 | 文件 | 行号 |
|------|------|------|
| Timer 结构体 | `src/hardware/timer.rs` | 51-66 |
| start() 启动线程 | `src/hardware/timer.rs` | 91-146 |
| 死循环发中断 | `src/hardware/timer.rs` | 107-141 |
| tick_count() 查询 | `src/hardware/timer.rs` | 178-180 |
| main.rs 创建 Timer | `src/main.rs` | 22-23 |

### 6.2 工作流程

```
Timer 线程 (独立)
  │
  ├─ thread::sleep(10ms)
  ├─ bus.send(KernelMsg::Interrupt(Interrupt::Timer))
  ├─ tick_count += 1
  └─ loop

消息传递：
  Timer → bus.send() → LockedBus → Kernel → intr_tx → ProcessService.intr_rx
```

### 6.3 验证 Timer 在工作

```bash
cargo run
> uptime
+490 ticks | 4.90s     # ~100 ticks/s
> uptime
+6996 ticks | 69.96s   # 持续增长
```

`uptime` 命令读取 `timer.tick_count()`。每秒增长约 100，证明 Timer 线程在正常运行。


## 七、进程调度：SMP Round-Robin

### 7.1 调度器数据结构

```rust
// src/services/process/scheduler.rs:52-63
pub struct Scheduler {
    ready_queue: VecDeque<ReadyQueueEntry>,  // 共享就绪队列 (FIFO)
    cpu_current: Vec<Option<(Pid, Tid)>>,   // [N] 每核当前进程
    cpu_ticks:   Vec<u64>,                  // [N] 每核已消耗时间片
    time_slice:  u64,                        // 时间片大小 (3 ticks = 30ms)
}
```

**关键设计**：每个 CPU 独立追踪 `cpu_current` 和 `cpu_ticks`。两个 CPU 共享一个就绪队列。

**代码位置**：

| 组件 | 文件 | 行号 |
|------|------|------|
| Scheduler 结构体 | `src/services/process/scheduler.rs` | 52-63 |
| schedule(cpu_id) | `src/services/process/scheduler.rs` | 127-142 |
| ready() 加入就绪队列 | `src/services/process/scheduler.rs` | 90-105 |
| remove() 移除进程 | `src/services/process/scheduler.rs` | 107-116 |
| dequeue_next() 出队(跳过忙碌PID) | `src/services/process/scheduler.rs` | 145-172 |

### 7.2 schedule(cpu_id) 算法

```rust
// src/services/process/scheduler.rs:127-142
pub fn schedule(&mut self, cpu_id: usize) -> SchedulingDecision {
    // ① 检查当前进程时间片是否耗尽
    if let Some((pid, tid)) = self.cpu_current[cpu_id] {
        self.cpu_ticks[cpu_id] += 1;
        if self.cpu_ticks[cpu_id] < self.time_slice {
            return Run(pid);  // 继续运行
        }
        // 时间片耗尽 → 放回队尾
        self.ready(pid, tid, 128);
        self.cpu_current[cpu_id] = None;
    }
    // ② 从就绪队列取下一个 (跳过已在其他 CPU 上的 PID)
    self.dequeue_next(cpu_id)
}
```

### 7.3 时间片轮转示例（2 CPU + 3 进程）

```
Tick 1:  CPU0=schedule(0) → PID 1    CPU1=schedule(1) → PID 2
         ready_queue = [PID 3]

Tick 2:  cpu_ticks[0]=1<3 → keep PID 1
         cpu_ticks[1]=1<3 → keep PID 2

Tick 3:  cpu_ticks[0]=2<3 → keep
         cpu_ticks[1]=2<3 → keep

Tick 4:  cpu_ticks[0]=3≥3 → 到期！
         ① ready(PID 1) → 队尾: [PID 3, PID 1]
         ② dequeue_next(0): busy=[PID 2].
            弹出 PID 3 → 不忙 → CPU0=PID 3 ✓

         cpu_ticks[1]=3≥3 → 到期！
         ① ready(PID 2) → 队尾: [PID 1, PID 2]
         ② dequeue_next(1): busy=[PID 3].
            弹出 PID 1 → 不忙 → CPU1=PID 1 ✓
```


## 八、Timer + 调度状态机：完整流程

### 8.1 主循环（事件驱动）

```rust
// src/services/process/service.rs:110-135
loop {
    // ① 处理 Timer 中断 (最多 10 个/轮，防止饿死 receiver)
    for _ in 0..10 {
        match self.intr_rx.try_recv() {
            Ok(env) => self.handle_timer_interrupt().ok(),
            Err(_) => break,
        }
    }
    // ② 处理进程消息 (fork/exec/wait/文件)
    match self.receiver.try_recv() {
        Ok(envelope) => self.handle_envelope(envelope)?,
        Err(Empty) => sleep(1ms),
        Err(Disconnected) => return,
    }
}
```

### 8.2 handle_timer_interrupt：调度 + 执行 + 回收

```rust
// src/services/process/service.rs:571-720
fn handle_timer_interrupt(&self) {
    for cpu_id in 0..self.cpu_count {
        // ═══ 步骤 1: 调度 ═══
        let decision = scheduler.schedule(cpu_id);

        // ═══ 步骤 2: PCB 状态机 ═══
        if Run(pid) {
            // 过渡上一个进程: Running → Ready
            if last_running[cpu_id] != pid {
                PCB[last_running[cpu_id]].state = Ready;
                last_running[cpu_id] = pid;
            }
            // 标记新进程: Ready/Creating → Running
            PCB[pid].state = Running;
        } else {
            last_running[cpu_id] = None;  // CPU 空闲
        }

        // ═══ 步骤 3: CPU 执行 (3 条指令) ═══
        let cpu = cpus.get_mut(&pid);
        for _ in 0..3 {
            cpu.step();  // 取指→译码→执行
            if syscall_pending {
                handle_file_syscall(cpu, r0, r1, r2);
            }
            if cpu.is_halted() { break; }
        }

        // ═══ 步骤 4: 检查 Zombie ═══
        if !is_blocked && cpu.is_halted() {
            PCB.state = Zombie { exit_code: 0 };
            scheduler.remove(pid);
        }
    }

    // ═══ 步骤 5: 收割 Zombie (每 tick 回收 1 个) ═══
    if let Some(zombie_pid) = find_first_zombie() {
        reap_process(zombie_pid);  // 清理 PCB + CPU + 页表帧
    }
}
```

### 8.3 一条完整链路

```
Timer 线程                     ProcessService 主循环
  │                                  │
  │ bus.send(Timer中断)               │
  └──→ Kernel → intr_tx ──→ intr_rx  │
                              ↓      │
                     try_recv() 收到  │
                              ↓      │
                 handle_timer_interrupt()
                              ↓
                     scheduler.schedule(cpu_id)
                       │
                       ├─ 时间片未用完 → keep
                       └─ 时间片到期 → 队尾 → 取新进程
                              ↓
                     PCB 状态更新 (Ready↔Running)
                              ↓
                     cpu.step() × 3  ← 取指·译码·执行
                              ↓
                     检查 Zombie → 回收
```

### 8.4 关键不变量

1. **单进程单核**：同一时刻，一个 PID 最多在一个 CPU 的 `cpu_current` 中出现
2. **时间片公平**：每个进程每轮获得恰好 `time_slice`（3 ticks = 30ms）的 CPU 时间
3. **init 不死**：PID 1 永远不会被标记为 Zombie
4. **阻塞 = 空闲**：Blocked 进程不在就绪队列中，不消耗 CPU 时间
5. **Reaper 每 tick 运行**：保证 Zombie 及时回收，不堆积

**代码位置汇总**：

| 组件 | 文件 | 行号 |
|------|------|------|
| handle_timer_interrupt | `src/services/process/service.rs` | 571-720 |
| 主循环 (run) | `src/services/process/service.rs` | 110-135 |
| Scheduler 全部方法 | `src/services/process/scheduler.rs` | 52-195 |
| Timer 全部方法 | `src/hardware/timer.rs` | 51-200 |
| 调度文档 | `docs/scheduler.md` | 全文 |


## 九、进程同步：信号量与互斥

### 9.1 信号量数据结构

```rust
// src/services/process/sync.rs:41-52
pub struct Semaphore {
    id: SemaphoreId,              // 信号量 ID
    owner_pid: Pid,               // 创建者
    value: AtomicU64,             // 当前计数（原子操作）
    wait_count: AtomicU64,        // 等待者数量
}
```

全局信号量 0 在系统启动时预创建：
```rust
// src/services/process/sync.rs:409-417
pub fn new() -> Self {
    let mut sm = Self { ... };
    sm.create_semaphore(0, 1);    // 二元信号量，初始值=1
    sm
}
```

### 9.2 系统调用接口

| R0 | 系统调用 | 参数 | 语义 |
|----|---------|------|------|
| 201 | sem_wait(sem_id) | R1=sem_id | P 操作：count=0 则阻塞，count>0 则 count-- |
| 202 | sem_signal(sem_id) | R1=sem_id | V 操作：有等待者转移所有权，无等待者 count++ |

### 9.3 sem_wait 处理器

```rust
// src/services/process/service.rs:1560-1579 (handle_file_syscall 内)
201 => {
    let sem_id = r1;
    let blocked = {
        let sync = self.sync_manager.lock().unwrap();
        let sem = sync.get_semaphore(sem_id);
        sem.wait() != SemaphoreResult::Acquired  // 原子 CAS
    };
    if blocked {
        // 1. PCB 设为 Blocked
        pcb.state = Blocked(WaitingForLock { lock_addr: sem_id });
        // 2. 从调度器就绪队列移除
        scheduler.block(pid, 1);
        // 3. CPU 停机
        cpu.halt();
    }
}
```

### 9.4 sem_signal 处理器（TOCTOU 修复）

```rust
// src/services/process/service.rs:1580-1619
202 => {
    // 扫描进程表，找阻塞在本信号量上的等待者
    let waiter = process_table.iter().find_map(|(&p, pcb)| {
        if pcb.state == Blocked(WaitingForLock { lock_addr: sem_id }) {
            Some(p)
        } else { None }
    });

    if let Some(wpid) = waiter {
        // 转移所有权：直接唤醒等待者
        PCB[wpid].state = Ready;
        scheduler.ready(wpid);
        cpu.halted = false;  // try_lock 安全 unhalt
        // count 不变！调用者下次 sem_wait 会阻塞
    } else {
        sem.signal();  // 无等待者：count++
    }
}
```

**TOCTOU 修复**：有等待者时直接转移所有权，不 count++。避免了调用者在释放瞬间立刻用 sem_wait 抢回去的竞争。

### 9.5 阻塞与唤醒的完整链路

```
进程 A (持有锁)                  进程 B (等待锁)
  │                                │
  │ sem_signal(0)                   │ (Blocked, cpu.halted)
  ├─ 扫描 process_table             │
  ├─ 找到 B: Blocked(lock=0)        │
  ├─ B.state = Ready                │
  ├─ scheduler.ready(B) ──────────→ 加入就绪队列
  ├─ B.cpu.halted = false ────────→ 唤醒 CPU
  │                                │
  │ sem_wait(0)                     │ (下一 tick 被调度)
  │ count=0 → Blocked!              ├─ 进入临界区
  │ cpu.halt()                      │ print '['
  │                                │ sem_signal(0) → 唤醒 A
  └─ [HALTED]                       └─ ...
```

### 9.6 演示：syncdemo 互斥

```
> dual syncdemo     ← 两个进程争用信号量 0
> pmon              ← PID 2 Running, PID 3 Blocked
> verbose on        ← 看输出：93 91 93 91 交替
```

互斥证据：绝不会出现连续两个 `[PRINT] 91`（即 `[`）中间没有 `]`。

**代码位置汇总**：

| 组件 | 文件 | 行号 |
|------|------|------|
| Semaphore 结构体 | `src/services/process/sync.rs` | 41-52 |
| sem_wait 处理器 | `src/services/process/service.rs` | 1560-1579 |
| sem_signal 处理器 (TOCTOU) | `src/services/process/service.rs` | 1580-1619 |
| 信号量 0 预创建 | `src/services/process/sync.rs` | 409-417 |
| 调度器 block/ready | `src/services/process/scheduler.rs` | 107-116, 90-105 |
| Zombie 检查 (unhalt Ready) | `src/services/process/service.rs` | 699-710 |
| syncdemo.asm | `programs/syncdemo.asm` | 全文 |
| syncdemo 文档 | `docs/syncdemo.md` | 全文 |


## 十、系统调用：INT 0x80 全表

### 10.1 系统调用机制

CPU 执行 `INT 0x80` 指令时：
1. 设置 `syscall_pending = true`，保存寄存器快照 `syscall_regs`
2. 返回 `Ok(())`，继续执行后续指令
3. 量子循环检测 `syscall_pending` → 调用 `handle_file_syscall(cpu, r0, r1, r2)`
4. 根据 R0 值分发到对应处理器

```rust
// src/hardware/cpu.rs:659-666 (INT 指令执行)
Instruction::Int { .. } => {
    self.syscall_pending = true;
    self.syscall_regs = [self.registers[0], ..];
    Ok(())
}

// src/services/process/service.rs:644-647 (量子循环中处理)
if cpu.syscall_pending {
    cpu.syscall_pending = false;
    self.handle_file_syscall(cpu, cpu.syscall_regs[0], ...);
}
```

### 10.2 系统调用全表

| R0 | 名称 | 参数 | 说明 | 代码行 |
|----|------|------|------|--------|
| 0 | exit | R1=exit_code | 退出进程 | 1443-1487 |
| 1 | print_int | R1=value | 打印整数 | 1488 |
| 2 | print_str | R1=addr, R2=len | 打印字符串 | 1489-1493 |
| 10 | open | R1=flags | 打开/创建文件 | 1489-1498 |
| 11 | close | R1=fd | 关闭文件 | 1499 |
| 12 | read | R1=fd, R2=size | 循环读文件 | 1500-1520 |
| 13 | write | R1=fd, R2=size | 写文件(从0x200) | 1521-1523 |
| 14 | mkdir | - | 创建目录(路径在0x100) | 1525 |
| 16 | unlink | - | 删除文件(路径在0x100) | 1526 |
| 17 | stat | - | 文件信息 | 1527 |
| 18 | listdir | - | 列目录 | 1528-1534 |
| 100 | fork | - | 克隆进程(异步) | 1549-1560 |
| 101 | exec | - | 替换程序(程序名在0x100) | 1561-1565 |
| 200 | sem_create | - | 创建信号量 | 1567-1572 |
| 201 | sem_wait | R1=sem_id | P 操作 | 1573-1579 |
| 202 | sem_signal | R1=sem_id | V 操作 | 1580-1619 |
| 208-211 | device | - | 剪贴板设备 | 1670-1704 |

**文件**：所有处理器在 `src/services/process/service.rs:1442-1704`（`handle_file_syscall`）。

### 10.3 汇编层面的系统调用

syncdemo.asm 示例——三个系统调用协作：

```asm
MOV R1, #0      ; 参数: sem_id=0
MOV R0, #201    ; 系统调用号: sem_wait
INT 0x80        ; 触发 → ProcessService.handle_file_syscall(cpu, 201, 0, 0)

MOV R0, #1      ; 系统调用号: print_int
MOV R1, #0x5B   ; 参数: '['
INT 0x80        ; 触发 → 打印 '['

MOV R0, #202    ; 系统调用号: sem_signal
INT 0x80        ; 触发 → 释放信号量
```

**关键**：每条 `INT 0x80` 是一条完整的系统调用。ProcessService 的 `handle_file_syscall` 根据 R0 值用 `match` 分发到具体处理器。

### 10.4 代码位置

| 组件 | 文件 | 行号 |
|------|------|------|
| INT 指令执行 | `src/hardware/cpu.rs` | 659-666 |
| 量子循环 syscall 处理 | `src/services/process/service.rs` | 644-647 |
| handle_file_syscall (全表) | `src/services/process/service.rs` | 1442-1704 |


## 十一、文件系统：VFS + 持久化

### 11.1 VFS 树形结构

```rust
// src/services/file/vfs.rs:24-40
pub struct VFSNode {
    inode: u64,                        // 唯一编号
    name: String,                      // 文件名
    node_type: File | Directory | SymLink,
    parent: Option<u64>,               // 父目录 inode
    children: HashMap<String, u64>,    // name → child_inode
    blocks: Vec<u64>,                  // 磁盘扇区列表
    size: u64,                         // 文件大小
    permissions: u16,
}
```

目录树示例：

```
/ (inode 0)
├── bin/    home/    tmp/    etc/
└── programs/ (inode 7)
    ├── ls.asm (inode 8)   blocks=[4]
    ├── cat.asm (inode 9)  blocks=[5]
    └── syncdemo.asm       blocks=[...]
```

### 11.2 双重持久化

```
VFS 元数据 (JSON)             文件内容 (VirtualDisk)
.genshin-vfs.json             .genshin-disk.img
{                              Sector 0: 分配位图
  "root_inode": 0,             Sector 1-3: 目录元数据
  "nodes": [                   Sector 4: ls.asm 内容
    {inode:8, name:"ls.asm",   Sector 5: cat.asm 内容
     blocks:[4], size:109}     ...
  ]                           }
}
```

**代码**：
- 保存：`src/services/file/vfs.rs:389-404`（`save_to_file`）
- 加载：`src/services/file/vfs.rs:406-430`（`load_from_file`）
- 磁盘写入：`src/services/file/file.rs:260-285`（`sync_to_disk`）

### 11.3 文件操作全流程（以 write 为例）

```
Shell: write /docs/hello.txt HelloWorld
  │
  ├─ fork(1) → child PID N
  ├─ exec(N, "write", ["/docs/hello.txt", "HelloWorld"])
  │   └─ 路径写入 0x100，内容写入 0x200
  └─ wait(1, N)
       │
       └─ CPU 执行 write.asm:
            MOV R1,#1; MOV R0,#10; INT 0x80    ← open(create)
            MOV R0,#13; MOV R2,#255; INT 0x80  ← write(data at 0x200)
            MOV R0,#11; INT 0x80               ← close

INT 0x80 → handle_file_syscall:
  R0=10 → FileRequest::Open → FileService → VFS.create_file → 分配 inode
  R0=13 → FileRequest::WriteData → FileService → file.write → sync_to_disk
       → 分配磁盘扇区 → 写入 .genshin-disk.img → vfs_node.blocks 记录扇区号
  R0=11 → FileRequest::Close → 关闭 fd
  
HALT → Zombie → reaper 回收
```

### 11.4 磁盘扇区管理

```rust
// src/hardware/disk.rs:16-25
pub struct VirtualDisk {
    file: File,                  // .genshin-disk.img 文件
    total_sectors: u32,         // 总扇区数 (默认 2048)
    bitmap: Vec<u64>,           // 分配位图 (bit=1 表示已用)
}
```

- **分配**：`allocate_sectors(n)` → 扫描 bitmap 找连续空闲位 → 返回起始扇区号
- **释放**：`free_sectors(start, n)` → 清除 bitmap 对应位
- **读写**：`write_sector(s, buf)` / `read_sector(s)` → 基于文件的 seek+read/write

每个扇区 512 字节。一个 .asm 文件通常占 1 个扇区。

### 11.5 命令与系统调用映射

| Shell 命令 | fork+exec 的程序 | 使用的系统调用 (R0) |
|-----------|-----------------|-------------------|
| ls | ls.asm | 18 (listdir) |
| mkdir | mkdir.asm | 14 (mkdir) |
| touch | touch.asm | 10 (open create) + 11 (close) |
| cat | cat.asm | 10 (open) + 12 (read) + 11 (close) |
| write | write.asm | 10 (open) + 13 (write) + 11 (close) |
| rm | rm.asm | 16 (unlink) |
| stat | stat.asm | 17 (stat) |

所有命令都是 `fork(1) → exec(child, prog, args) → wait(1, child)` 的标准流程。

### 11.6 代码位置

| 组件 | 文件 | 行号 |
|------|------|------|
| VFSNode 结构体 | `src/services/file/vfs.rs` | 24-40 |
| VirtualFileSystem | `src/services/file/vfs.rs` | 173-220 |
| create_file | `src/services/file/vfs.rs` | 223-260 |
| lookup_path | `src/services/file/vfs.rs` | 290-330 |
| save_to_file / load_from_file | `src/services/file/vfs.rs` | 389-430 |
| FileService open | `src/services/file/service.rs` | 481-550 |
| FileService read | `src/services/file/service.rs` | 664-686 |
| FileService write | `src/services/file/service.rs` | 721-760 |
| VirtualDisk | `src/hardware/disk.rs` | 16-40 |
| allocate_sectors | `src/hardware/disk.rs` | 252-280 |
| write_sector | `src/hardware/disk.rs` | 110-122 |
| write.asm 示例 | `programs/write.asm` | 全文 |

### 11.7 增删改查（CRUD）详解

#### CREATE — 创建文件

```
Shell: touch /docs/hello.txt
  → fork_exec_wait("touch", &["/docs/hello.txt"])

touch.asm 执行:
  MOV R1, #1      ; flags = create
  MOV R0, #10     ; open(path, create)
  INT 0x80        → FileService.handle_open_with_response
                      ├─ vfs.lookup_path → 不存在, flags.create=true
                      ├─ 解析父路径 "/docs" → parent_inode
                      ├─ vfs.create_file(parent, "hello.txt", pid)
                      │   ├─ 分配新 inode
                      │   ├─ parent.children["hello.txt"] = new_inode
                      │   └─ vfs_node { type:File, size:0, blocks:[] }
                      ├─ 分配 fd → fd_manager.allocate(pid, file)
                      └─ respond_success(Fd(fd))
  MOV R0, #11     ; close(fd)
  INT 0x80        → FileService.handle_close
  HALT

结果: /docs/hello.txt 出现在 VFS 树中, size=0, blocks=[]
```

**代码**：`src/services/file/service.rs:481-550` (handle_open_with_response)，`src/services/file/vfs.rs:223-260` (create_file)

#### READ — 读取文件

```
Shell: cat /docs/hello.txt
  → fork_exec_wait("cat", &["/docs/hello.txt"])

cat.asm 执行:
  MOV R0, #10     ; open(path, read_only)
  INT 0x80        → fd in R1
  MOV R0, #12     ; read(fd, max=16)
  MOV R2, #0x10
  INT 0x80        → handle_file_syscall(R0=12):
                      loop {
                        FileRequest::Read { fd, offset, size:16 }
                        → FileService.handle_read_with_response
                           ├─ fd_manager.get(pid, fd) → OpenFile
                           ├─ open_file.read(16) → File.read()
                           │   └─ 若 dirty: sync_to_disk(先写回)
                           │   └─ 若未加载: load_from_disk(从磁盘读取扇区)
                           └─ respond_success(Bytes(data))
                        print!("{}", data)  // 打印到终端
                        offset += data.len()
                        if data.len() < 16 { break }
                      }
  MOV R0, #11     ; close(fd)
  INT 0x80
  HALT
```

**代码**：`src/services/process/service.rs:1500-1520` (R0=12 处理器)，`src/services/file/service.rs:664-686` (handle_read_with_response)

#### UPDATE — 写入文件

```
Shell: write /docs/hello.txt HelloWorld
  → fork_exec_wait("write", &["/docs/hello.txt", "HelloWorld"])
  → exec 时写入: 0x100="/docs/hello.txt", 0x200="HelloWorld"

write.asm 执行:
  MOV R1, #1      ; flags = create
  MOV R0, #10     ; open(path, create)
  INT 0x80        → fd in R1
  MOV R0, #13     ; write(fd, size=255) — 从 0x200 读取数据
  MOV R2, #255
  INT 0x80        → handle_file_syscall(R0=13):
                      data = read_bytes_virt(pid, 0x200, 255)
                      FileRequest::WriteData { fd, data }
                      → FileService.handle_write_with_response
                         ├─ open_file.write(data)
                         │   ├─ file.data = data
                         │   └─ file.dirty = true
                         ├─ file.sync_to_disk(&disk)
                         │   ├─ 释放旧扇区 (如有)
                         │   ├─ allocate_sectors(needed)
                         │   └─ write_sector(sector, data)
                         ├─ vfs_node.size = data.len()
                         └─ vfs_node.blocks = [new_sector]
  MOV R0, #11     ; close(fd)
  INT 0x80
  HALT

结果: hello.txt 内容="HelloWorld", size=10, blocks=[X] (扇区 X)
```

**代码**：`src/services/process/service.rs:1521-1523` (R0=13 处理器)，`src/services/file/service.rs:721-760` (handle_write_with_response)，`src/services/file/file.rs:260-285` (sync_to_disk)

#### DELETE — 删除文件

```
Shell: rm /docs/hello.txt
  → fork_exec_wait("rm", &["/docs/hello.txt"])

rm.asm 执行:
  MOV R0, #16     ; unlink
  INT 0x80        → FileRequest::Unlink { path }
                  → FileService.handle_delete
                     ├─ vfs.lookup_path → 找到 VFSNode
                     ├─ disk.free_sectors(node.blocks) — 释放磁盘扇区
                     ├─ 从 parent.children 中移除
                     └─ vfs_node.deleted = true
  HALT

结果: hello.txt 从目录树消失，磁盘扇区回收
```

**代码**：`src/services/file/service.rs:824-853` (handle_delete)，`src/hardware/disk.rs:290-310` (free_sectors)

#### 完整 CRUD 链路总结

```
          Shell                 ProcessService          FileService           VFS/Disk
           │                         │                      │                   │
  CREATE   │─ fork_exec_wait ──────→ │─ exec(touch) ──────→ │─ open(create) ──→ create_file
           │                         │                      │                   ├─ 分配 inode
           │                         │                      │                   └─ save JSON
  READ     │─ fork_exec_wait ──────→ │─ exec(cat) ────────→ │─ open → read ──→ load_from_disk
           │                         │   ┌─ loop read       │   └─ respond      └─ read_sector
           │                         │   └─ print!          │
  UPDATE   │─ fork_exec_wait ──────→ │─ exec(write) ──────→ │─ open → write ──→ sync_to_disk
           │                         │                      │                   ├─ alloc_sectors
           │                         │                      │                   └─ write_sector
  DELETE   │─ fork_exec_wait ──────→ │─ exec(rm) ─────────→ │─ unlink ────────→ free_sectors
           │                         │                      │                   └─ save JSON
```

**关键设计**：
1. 所有文件操作都走 `fork + exec + wait`，统一进程模型
2. 元数据（文件名/大小/inode）存 JSON，文件内容存 VirtualDisk 扇区
3. 每次操作后自动 `save_to_file`，保证崩溃一致性
4. 扇区分配用位图（bitmap），空闲扇区用 FIFO 队列管理
