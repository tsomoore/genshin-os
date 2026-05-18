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
