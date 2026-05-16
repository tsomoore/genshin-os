# Genshin-OS 总体设计说明书

> 版本 0.2.0 | 最后更新 2026-05

## 一、项目概述

Genshin-OS 是一个用 Rust 编写的微内核操作系统模拟。核心设计原则：

1. **消息总线通信**: 所有模块间通信必须通过 `KernelMsg` 消息总线，禁止直接函数调用
2. **CPU 模拟执行**: 用户程序运行在 `VirtualCPU` 上，通过 `INT 0x80` 触发系统调用
3. **fork/exec/wait 进程模型**: 仿 Unix 的进程生命周期管理
4. **文件持久化**: VFS JSON + VirtualDisk 磁盘镜像双重持久化

## 二、总体架构

```
┌─────────────────────────────────────────────────────────────┐
│                      Shell (用户界面)                        │
│  ls / mkdir / touch / cat / write / rm / tree / pstree     │
│  minigdb / dual / run / pmon / fork / exec / uptime        │
│                                                             │
│  dual/run: fire-and-forget（发完即返，进程 Timer 驱动）     │
│  文件命令: fork(1) → exec(child, program, args) → wait(N)  │
│  pmon: TUI 实时进程/内存/磁盘监控面板 (ratatui)             │
└────────────────────────────┬────────────────────────────────┘
                             │ KernelMsg::Process / File / ...
                             ▼
┌─────────────────────────────────────────────────────────────┐
│                   Kernel (消息总线中枢)                       │
│                                                             │
│  LockedBus: Mutex + Vec<Sender<Envelope>>                   │
│  唯一的总线订阅者。按消息类型路由到各服务通道:               │
│    Process/Syscall → process_tx                             │
│    Memory          → memory_tx                              │
│    File            → file_tx                                │
│    Interrupt       → intr_tx                                │
│    Device          → {} (DeviceService 直接订阅 bus)        │
└──┬──────────┬──────────┬──────────┬────────────────────────┘
   │          │          │          │
   ▼          ▼          ▼          ▼
┌──────┐ ┌──────┐ ┌──────┐ ┌──────────────┐
│Process│ │Memory│ │File  │ │DeviceService │
│Service│ │Service│ │Service│ │(直接订阅bus) │
└──┬───┘ └──┬───┘ └──┬───┘ └──────┬───────┘
   │        │        │             │
   ▼        ▼        ▼             ▼
┌─────────────────────────────────────────────────────────────┐
│                    硬件模拟层                               │
│  VirtualCPU │ MMU │ PhysicalMemory │ VirtualDisk │ Timer ★ │
│                                                             │
│  Timer (100Hz): 每 10ms 发 Interrupt::Timer，              │
│  ProcessService 通过 select! 阻塞等待 → 驱动调度器          │
└─────────────────────────────────────────────────────────────┘
```

## 三、数据结构设计

### 3.1 进程控制块 (PCB)

```
PCB {
    pid: Pid,              // 进程 ID
    name: String,          // 程序名
    state: ProcessState,   // Creating | Ready | Running | Blocked | Zombie
    parent_pid: Option<Pid>,
    threads: Vec<TCB>,     // 线程 (当前未使用)
    priority: u8,
    args: Vec<String>,
    mmu_state: Option<MMUState>,
    signals: Vec<SignalType>,
}
```

**进程状态转换图:**

```
                    fork_impl(0)
  ┌──────────┐  ───────────────→  ┌──────────┐
  │ CREATING │                    │  READY   │ ←───────── unblock
  └──────────┘                    └────┬─────┘
                                      │ schedule()
                                 ┌────▼─────┐
                                 │ RUNNING  │
                                 └────┬─────┘
                        halt/exit │    │ time slice expire
                              ┌───▼──┐ │
                              │ZOMBIE│ │ → READY
                              └───┬──┘
                           wait │ │ reap
                              ┌─▼──▼─┐
                              │ FREED │
                              └──────┘
```

### 3.2 虚拟文件系统节点 (VFSNode)

```
VFSNode {
    inode: u64,                        // 唯一编号
    name: String,                      // 文件名
    node_type: File | Directory | SymLink | BlockDevice | CharDevice,
    parent: Option<u64>,               // 父目录 inode
    children: HashMap<String, u64>,    // name → child_inode
    blocks: Vec<u64>,                  // 磁盘扇区列表
    size: u64,                         // 文件大小 (字节)
    permissions: u16,                  // Unix 权限位
    owner: Pid,
    ref_count: u32,                    // 引用计数 (0=可删除)
    deleted: bool,
}
```

**目录树组织:**

```
/ (inode 0, Directory)
├── bin (inode 1)    /  home (inode 2)  /  tmp (inode 3)
├── etc (inode 4)    /  var (inode 5)   /  examples (inode 6)
└── programs (inode 7, Directory)
    ├── ls.asm (inode 8, File)    blocks=[4]    size=109
    ├── cat.asm (inode 9, File)   blocks=[5]    size=264
    └── rwlock.asm (inode 10, File) blocks=[6] size=600
```

### 3.3 页表项 (PageTableEntry)

```
PageTableEntry {
    frame: PhysAddr,           // 物理帧地址 (4KB 对齐)
    flags: PageFlags {
        present: bool,         // 页是否在内存中
        writable: bool,
        user_accessible: bool,
    }
}
```

**地址转换:**

```
虚拟地址 → MMU.translate(pid, vaddr)
  page_vaddr = vaddr & 0xFFFF_F000   // 对齐到 4KB
  offset     = vaddr & 0x0000_0FFF
  entry      = page_tables[pid][page_vaddr]
  paddr      = entry.frame + offset
```

### 3.4 物理内存帧 (Frame)

```
Frame {
    number: u64,         // 帧号
    address: PhysAddr,   // 物理地址
    size: usize,         // 4096
    allocated: bool,
    owner: Option<Pid>,
}

FrameAllocator {
    frames: Vec<Frame>,        // 所有帧
    free_queue: VecDeque<u64>, // 空闲帧队列 (FIFO)
    total_frames: u64,         // 总帧数 (默认 1024 = 4MB)
    free_count: u64,
}
```

### 3.5 交换槽 (SwapSlot)

```
SwapSlot {
    number: u64,         // 槽号
    size: usize,         // 4096
    occupied: bool,
    owner: Option<Pid>,
    vpn: Option<u64>,    // 虚拟页号
}

SwapManager {
    slots: Vec<SwapSlot>,              // 所有槽
    free_queue: VecDeque<u64>,         // 空闲槽队列
    process_slots: HashMap<Pid, Vec>,  // 每进程的槽
    disk: VirtualDisk,                 // .genshin-swap.img
}
```

### 3.6 虚拟 CPU

```
VirtualCPU {
    registers: [u64; 4],     // R0-R3
    pc: u64,                 // 程序计数器
    sp: u64,                 // 栈指针
    flags: CPUFlags {        // 标志寄存器
        zero: bool,          // Z: 结果为零
        sign: bool,          // S: 结果为负
        overflow: bool,      // O: 有符号溢出
        carry: bool,         // C: 无符号进位
    },
    current_pid: Pid,        // 所属进程
    halted: bool,            // 停机标志
    instruction_count: u64,  // 已执行指令数
    syscall_pending: bool,   // 待处理系统调用
    syscall_regs: [u64; 4],  // 系统调用时的寄存器
    mmu: Arc<MMU>,           // 内存管理单元
    bus: Arc<dyn MessageBus>,// 消息总线
}
```

### 3.7 消息总线

```
KernelMsg 枚举 (所有通信的统一类型):
  Process(ProcessRequest)    // 进程管理: fork, exec, wait, signal, spawn, etc.
  Syscall(Syscall)           // 系统调用: CreateProcess, ExitProcess
  Memory(MemoryRequest)      // 内存管理: AllocFrame, FreeFrame, MapPage, PageFaultHandler
  File(FileRequest)          // 文件系统: Open, Close, Read, Write, Mkdir, ListDir, Stat
  Device(DeviceRequest)      // 设备管理: ClipboardGet, ClipboardSet, Read, Write
  Interrupt(Interrupt)       // 硬件中断: Timer, PageFault, SyscallTrap

Envelope (消息信封):
  message: KernelMsg         // 消息体
  request_id: Option<u64>    // 请求 ID (用于响应匹配)
  response_channel: Option<Sender<Response>>  // 响应通道
```

## 四、功能模块划分

### 4.1 模块功能

| 模块 | 文件 | 职责 |
|------|------|------|
| **Kernel** | `services/kernel.rs` | 消息路由中枢，唯一的总线订阅者 |
| **ProcessService** | `services/process/service.rs` | 进程生命周期、调度、系统调用处理 |
| **MemoryService** | `services/memory/service.rs` | 物理内存分配回收、页表映射、缺页处理、交换 |
| **FileService** | `services/file/service.rs` | VFS 目录树、文件描述符、磁盘读写、持久化 |
| **DeviceService** | `services/device/service.rs` | 设备管理、剪贴板 |
| **Shell** | `ui/shell/mod.rs` | 命令行界面、命令解析、fork+exec+wait 封装 |

### 4.2 模块间关系

```
                  ┌──────────┐
                  │  Shell   │  (用户输入 → fork+exec+wait)
                  └────┬─────┘
                       │ send_and_wait() → ProcessRequest::ForkProcess
                       │                → ProcessRequest::ExecProcess
                       │                → ProcessRequest::WaitChild
                       ▼
┌──────────────────────────────────────────────────────────────┐
│                        Kernel                                │
│                                                              │
│  Process → process_tx     Memory → memory_tx                 │
│  File    → file_tx        Interrupt → intr_tx               │
│  Device  → {} (直接忽略)                                     │
└──┬───────────┬───────────┬──────────────────────────────────┘
   │           │           │
   ▼           ▼           ▼
┌──────┐  ┌──────┐  ┌──────────────┐
│Process│  │Memory│  │FileService   │
│Service│  │Service│  │              │
│       │  │       │  │ VFS + Disk   │
│ fork  │  │ alloc │  │ open/read/   │
│ exec  │  │ map   │  │ write/close  │
│ exit  │  │ swap  │  │ persist      │
│ sched │  │ pfault│  └──────┬───────┘
└──┬───┘  └──┬───┘          │
   │         │               │
   │    ┌────┴────┐     ┌────┴────┐
   │    │ Shared  │     │ Shared  │
   │    │  MMU    │     │  MMU    │
   │    └─────────┘     └─────────┘
   │
   ▼
┌──────────────┐     ┌──────────────┐
│DeviceService │     │  .asm 程序   │
│ (直接订阅bus)│     │  (汇编代码)  │
│  clipboard   │     │  programs/   │
└──────────────┘     └──────────────┘
```

### 4.3 模块间接口 (消息类型)

**进程管理消息:**
```
ProcessRequest::ForkProcess { parent_pid }     → 创建子进程
ProcessRequest::ExecProcess { pid, executable, args, path }
                                                → 替换进程代码
ProcessRequest::WaitChild { pid, child_pid }    → 等待子进程
ProcessRequest::Spawn { program, params }       → 直接生成进程
ProcessRequest::ListProcesses                   → pstree
ProcessRequest::Signal { pid, signal }          → 发送信号
```

**内存管理消息:**
```
MemoryRequest::AllocFrame { count }             → 分配物理帧
MemoryRequest::FreeFrame { paddr }              → 释放物理帧
MemoryRequest::MapPage { pid, virt, phys, prot }→ 建立页表映射
MemoryRequest::UnmapPage { pid, virt }          → 解除页表映射
MemoryRequest::PageFaultHandler { pid, faulting_addr, access_type }
                                                → 缺页处理
```

**文件管理消息:**
```
FileRequest::Open { path, flags }               → 打开文件/创建
FileRequest::Close { fd }                       → 关闭文件
FileRequest::Read { fd, offset, buf, size }     → 读取文件
FileRequest::WriteData { fd, data }             → 写入文件
FileRequest::CreateDirectory { path }           → 创建目录
FileRequest::Unlink { path }                    → 删除文件
FileRequest::ListDir { path }                   → 列出目录
FileRequest::Stat { path }                      → 文件信息
```

**设备管理消息:**
```
DeviceRequest::ClipboardGet { max_size }        → 读取剪贴板
DeviceRequest::ClipboardSet { data }            → 写入剪贴板
DeviceRequest::RegisterDevice { device_type, name }
                                                → 注册设备
```

## 五、关键流程

### 5.0 Timer 驱动调度 (select! 事件循环)

```
Timer 硬件 (100Hz)
  │
  └─→ bus.send(KernelMsg::Interrupt(Interrupt::Timer))
       │
       └─→ Kernel 路由 → intr_tx → ProcessService.intr_rx
                                            │
                                            ▼
  select! {
      recv(intr_rx) → handle_timer_interrupt()  ←─ 调度、页错误、回收
      recv(receiver) → handle_envelope()         ←─ fork/exec/wait/文件
  双重就绪: select! 随机选择 → 公平调度
```

**系统监控**: `pmon` 命令通过 `ProcessRequest::GetStats` / `MemoryRequest::GetStats` / `FileRequest::DiskInfo` 查询系统状态，每 250ms 刷新一次 TUI 面板。

### 5.1 进程创建 (统一 fork+exec 流水线)

所有进程创建均通过 `fork_impl(0)` + `exec_impl(pid, name, args)` 两条指令:
- `fork_impl(0)` → 分配 PID、创建 CPU、PCB，子进程 R0=0
- `exec_impl` → 加载程序 (.asm 汇编或内置)、建立页表映射、重置 PC/SP
- `handle_schedule` → 加入就绪队列，Timer 驱动调度

调用方:
- Shell `ls`/`cat` 等: fork_exec_wait → ForkProcess + ExecProcess + WaitChild
- Shell `run`/`dual`: Spawn (fire-and-forget) → ProcessService 内部 fork_impl + exec_impl
- CPU INT 0x80 R0=100: fork_impl 直接调用（同步，子进程立即调度）

### 5.2 文件命令流程 (fork+exec+wait)

```
Shell: ls
  │
  ├─ fork(1) ──────────────────────────────────────────┐
  │   ProcessService.fork_impl(1):                     │
  │     1. 分配 child_pid                              │
  │     2. 遍历父进程页表 (MMU.get_page_entries(1))     │
  │        ├─ AllocFrame → 新物理帧                     │
  │        ├─ MapPage(child, vaddr, new_frame)          │
  │        └─ 逐字节复制页内容                          │
  │     3. 克隆 CPU 状态 (PC, SP, 寄存器)                │
  │     4. child.write_register(R0, 0)  // fork 返回值  │
  │     5. 创建 PCB, state=Ready                        │
  │     6. 不加入调度队列 (等 exec)                      │
  │     7. 返回 child_pid = N                           │
  │                                                     │
  ├─ exec(N, "ls", ["/"]):                              │
  │   ProcessService.exec_impl(N, "ls", ["/"]):         │
  │     1. load_program("ls") → programs/ls.asm         │
  │     2. UnmapPage 旧页                               │
  │     3. AllocFrame + MapPage 新页                     │
  │     4. write_slice_virt(0x0000, code)    // 程序代码 │
  │     5. write_slice_virt(0x0100, "/")     // 路径参数 │
  │     6. 重置 CPU: PC=0, SP=0xFFFF, halted=false      │
  │     7. handle_schedule(N) → 加入就绪队列             │
  │                                                     │
  ├─ wait(1, N):                                        │
  │   ProcessService.handle_wait_child:                  │
  │     1. 验证 N 是 1 的子进程                          │
  │     2. 如果 N 已是 Zombie → 收割, 返回 exit_code     │
  │     3. 否则 → 存储 waiting_parents[N] = (1, channel) │
  │                                                     │
  └─ [定时器驱动 PID N 执行 ls]                          │
      CPU.step(): MOV R0,#18 → INT 0x80 → listdir → HALT │
      → Zombie → 通知 waiting_parents → wait 返回        │
      → reap_process → 清理                              │
```

### 5.3 文件写入流程

```
write /hello.txt world
  │
  ├─ fork(1) → child PID N
  ├─ exec(N, "write", ["/hello.txt", "world"])
  │   └─ write_slice_virt(0x0100, "/hello.txt")  // 路径
  │   └─ write_slice_virt(0x0200, "world")       // 内容
  │
  └─ [CPU 执行 write.asm]
      MOV R1,#1 → MOV R0,#10 → INT 0x80    // open(fd=flags_create)
        → FileService: vfs.create_file("/hello.txt") → fd=3
      MOV R0,#13 → MOV R2,#5 → INT 0x80    // write(fd=3, 5 bytes)
        → ProcessService: read_bytes_virt(pid, 0x200, 5) → "world"
        → FileService: open_file.write("world")
          → file.dirty = true
          → file.sync_to_disk(&disk)
            → disk.allocate_sectors(1) → sector X
            → disk.write_sector(X, "world\0...\0")
          → vfs_node.blocks = [X], vfs_node.size = 5
      MOV R0,#11 → INT 0x80               // close(fd=3)
      MOV R0,#0 → INT 0x80                // exit(0)
        → UnmapPage + FreeFrame + Zombie
```

### 5.4 信号量互斥 (dual rwlock)

```
dual rwlock → 创建 2 个进程，共享全局信号量 ID=0

CPU0 执行 PID 2:                 CPU1 执行 PID 3:
  sem_wait(0)                     sem_wait(0)
    count: 1→0                     count: 0 → 阻塞!
    进入临界区                     cpu.halted=true
    打印 '['                       (等待)
    打印 '['
  sem_signal(0)                  
    count: 0→1                     count: 1→0
    打印 ']'                       进入临界区
                                    打印 '['
                                  sem_signal(0)
                                    count: 0→1
                                    打印 ']'

输出: [ [ ] ] [ [ ] ] ...  (不会出现 [ [ 同时)
```

## 六、持久化设计

```
启动:
  FileService::new()
    → VirtualFileSystem::load_from_file(".genshin-vfs.json")
      └─ 反序列化 JSON → 重建所有 VFSNode
    → import_host_files("programs", "/programs")
      └─ 读取 host programs/*.asm → 写入 VFS 文件

运行时:
  每次文件请求后:
    → vfs.save_to_file(".genshin-vfs.json")  // 保存元数据

  write 操作:
    → file.sync_to_disk(&disk)               // 写入 .genshin-disk.img
    → vfs_node.blocks 记录扇区号             // 元数据标记

退出:
  → JSON + 磁盘镜像 保留在文件系统
  → 下次 cargo run 自动加载
```

## 七、系统调用完整表

| R0 | 名称 | R1 | R2 | 说明 |
|----|------|----|----|------|
| 0 | exit | exit_code | - | 退出进程 (清理页+帧) |
| 1 | print | value | - | 打印整数 |
| 2 | print_str | addr | len | 打印字符串 |
| 10 | open | flags | - | 打开文件, fd→R1 |
| 11 | close | fd | - | 关闭文件 |
| 12 | read | fd | max_size | 循环读文件到 EOF |
| 13 | write | fd | size | 从 0x200 写数据到文件 |
| 14 | mkdir | - | - | 从 0x100 读路径, 创建目录 |
| 16 | unlink | - | - | 从 0x100 读路径, 删除文件 |
| 17 | stat | - | - | 打印文件信息 |
| 18 | listdir | - | - | 从 0x100 读路径, 列出目录 |
| 100 | fork | - | - | 克隆进程 (异步) |
| 101 | exec | - | - | 从 0x100 读程序名, 替换进程 |
| 102 | tree | - | - | 递归目录树 |
| 200 | sem_create | - | - | 创建信号量, id→R1 |
| 201 | sem_wait | sem_id | - | P 操作, 计数=0 则阻塞 |
| 202 | sem_signal | sem_id | - | V 操作 |
| 203 | lock_create | - | - | 创建互斥锁, id→R1 |
| 204 | lock_acquire | lock_id | - | 加锁, 已锁则阻塞 |
| 205 | lock_release | lock_id | - | 解锁 |
| 208 | device_open | - | - | 申请剪贴板设备 |
| 209 | device_close | - | - | 释放剪贴板设备 |
| 210 | clipboard_read | max_size | - | 读剪贴板到 0x200, 长度→R2 |
| 211 | clipboard_write | - | size | 从 0x200 读数据写剪贴板 |
