# 组员及分工

| 姓名 | 学号 | 角色 |
|------|------|------|
| 邵汉超 | 2023210932 | 组长 |
| 唐博艺 | 2023210942 | 组员 |

---

## 邵汉超（2023210932）—— 组长

负责微内核核心基础设施：消息总线、内核路由、进程管理、调度器、同步原语、内存管理、CPU 与定时器模拟。

### 消息总线与内核路由（`src/messaging/`、`src/services/kernel.rs`）

- 设计并实现 `MessageBus` trait，定义 `send()`（fire-and-forget）、`send_request()`（请求-响应）、`subscribe()` 三个核心接口
- 实现 `LockedBus`：基于 `Arc<Mutex<Vec<Sender<Envelope>>>>` 的广播总线，订阅者通过 `unbounded` channel 接收消息，发送者通过 `try_send` 非阻塞广播
- 设计 `Envelope` 结构体：封装 `KernelMsg` + `request_id` + `response_channel`，支持 `fire_and_forget()` 和 `with_response()` 两种构造模式
- 设计 `KernelMsg` 枚举（7 个变体）：`Syscall`、`Interrupt`、`Process`、`Memory`、`File`、`Device`，统一所有跨模块通信
- 实现 `Kernel` 路由器：唯一订阅总线的组件，按消息类型分发到 ProcessService / MemoryService / FileService 的专用 channel
- 设计 `DirectBus` 点对点通道和 `Response` / `ResponseData` 响应机制

### 进程管理服务（`src/services/process/`）

- 设计并实现 `ProcessService`：消息驱动的事件循环，统一处理 `ProcessRequest` 和 `Syscall` 两类消息
- **进程生命周期**：`fork_impl()`（克隆父进程内存与 CPU 状态）、`exec_impl()`（加载程序、替换页表、重置 CPU）、`handle_exit()`（释放页帧、标记 Zombie、通知父进程）、`reap_process()`（回收 Zombie 进程）
- **SMP 调度器**（`scheduler.rs`）：支持 RoundRobin / FIFO / Priority / SJF / MLFQ 五种策略，每 CPU 独立调度，`dequeue_next()` 跳过已在其他 CPU 运行的 PID，`schedule(cpu_id)` 带量子检查
- **进程控制块 PCB**（`pcb.rs`）：管理进程状态机（Creating → Ready → Running → Blocked → Zombie），支持多线程 TCB
- **定时器中断处理**（`handle_timer_interrupt()`）：每 tick 遍历 SMP CPU，调度 → 状态机转换 → CPU 步进（最多 3 条指令/CPU/tick）→ 系统调用处理 → zombie 检查 → reaper 回收
- **系统调用分发表**（24 个 syscall）：重构为 `handle_file_syscall()` 分派到 17 个独立 `syscall_xxx()` 方法，按 Lifecycle / Console I/O / File / Process / Sync / Device 六大类组织
- **IPC 管理器**（`ipc.rs`）：实现进程间消息队列 `MessageQueue`（send/receive/peek）、共享内存 `SharedMemoryRegion`（create/attach/detach，引用计数管理）

### 同步原语（`src/services/process/sync.rs`）

- **信号量 Semaphore**：基于 `AtomicU32` 的 CAS 无锁实现，`wait()`（P 操作）和 `signal()`（V 操作），支持有界/无界模式
- **互斥锁 MutexLock**：基于 `AtomicU64` 的 owner 跟踪，支持递归锁，`try_acquire()` / `release()` 用 CAS 原子操作
- **TOCTOU 所有权转移**：`sem_signal` 不直接 count++，而是扫描进程表找到 `Blocked(WaitingForLock)` 的等待者，直接将信号量转移给等待者，避免「释放后立即被调用者抢回」的多核竞态
- **SyncManager**：集中管理所有信号量和互斥锁，预创建全局信号量 0 和互斥锁 0 供 syncdemo 使用

### 内存管理服务（`src/services/memory/`）

- 实现 `MemoryService`：处理 `MemoryRequest`（AllocFrame / FreeFrame / MapPage / UnmapPage / PageFaultHandler / SwapOut / SwapIn）
- 实现 `FrameAllocator` 物理帧分配器、`PageTable` / `PageTableManager` 页表管理
- 实现 `SwapManager` 换页机制：支持 FIFO / LRU 策略，将不活跃页面换出到虚拟磁盘

### 硬件模拟层（`src/hardware/cpu.rs`、`src/hardware/timer.rs`、`src/hardware/mmu.rs`、`src/hardware/memory.rs`）

- **VirtualCPU**：实现 20+ 条 Mock ISA 指令（MOV/ADD/SUB/MUL/DIV/CMP/JMP/JZ/JNZ/LOAD/STORE/INT/HALT），取指→解码→执行三阶段流水线，支持 CPUFlags（Zero/Sign/Carry/Overflow），`syscall_pending` 机制
- **MMU**：虚拟地址→物理地址转换，页表管理（`map_page` / `unmap_page`），权限检查
- **Timer**：100Hz 硬件定时器，通过消息总线发送 `Interrupt::Timer`，驱动进程调度
- **PhysicalMemory**：4MB 物理内存模拟

---

## 唐博艺（2023210942）

负责文件系统、设备管理、用户交互界面。

### 文件服务与虚拟文件系统（`src/services/file/`）

- 设计并实现 `FileService`：消息驱动的事件循环，处理 `FileRequest`（Open / Close / Read / Write / CreateDirectory / Unlink / ListDir / Stat / Seek / Dup）
- **VFS 虚拟文件系统**（`vfs.rs`）：
  - 设计 `VFSNode` 结构体：inode 编号、节点类型（File/Directory/SymLink/BlockDevice）、权限位、父子关系、数据块列表、引用计数
  - 实现 `VirtualFileSystem`：`HashMap<inode, VFSNode>` 的 inode 表，自增 `next_inode` 分配器，`lookup_path()` 递归路径解析，`create_file()` / `create_directory()` 节点创建
  - JSON 持久化：启动时 `load_from_file(".genshin-vfs.json")` 恢复 VFS 树，每次操作后 `save_to_file()` 写回 JSON
- **文件抽象**（`file.rs`）：`File` 结构体封装 `Vec<u8>` 内存缓冲 + 磁盘扇区映射（`start_sector` + `sector_count`），支持 Read / Write / ReadWrite / Append 模式，`read()` / `write()` 含权限检查和光标管理
- **文件描述符管理**（`descriptor.rs`）：`FileDescriptorManager` 维护每进程的 fd 表，`alloc()` 分配 fd 编号，`dealloc()` 回收，`OpenFile` 关联 File 与 fd
- **主机文件导入**（`import_host_files()`）：启动时扫描 `programs/` 目录，将 .asm / .txt 文件导入 VFS，含去重逻辑（已有磁盘块的跳过）

### 虚拟磁盘（`src/hardware/disk.rs`）

- 实现 `VirtualDisk`：基于文件的块设备，512B 扇区，`read_sector()` / `write_sector()` 通过 `seek` + `read` / `write` 操作 `.genshin-disk.img` 文件
- 支持 `PhysicalBlockDevice` 和 `PartitionDevice` 分区层

### 设备服务（`src/services/device/`）

- 实现 `DeviceService`：直接订阅消息总线（不经过 Kernel 路由），处理 `DeviceRequest`
- **设备抽象**（`device.rs`）：`Device` trait 定义 `open()` / `close()` / `read()` / `write()` / `ioctl()`，`DeviceRegistry` 注册表管理设备实例
- **剪贴板设备**：`Arc<Mutex<String>>` 共享缓冲区，支持 `ClipboardGet` / `ClipboardSet`，集成到 syscall 208-211
- **驱动管理**（`driver.rs`）：`DriverManager` 管理设备驱动实例

### Shell 与用户交互（`src/ui/shell/`）

- 实现 `Shell` 交互式命令行：解析用户输入（`parser.rs`），执行命令（`mod.rs`），内置命令（`builtins.rs`）
- **命令实现**：`ls` / `tree` / `mkdir` / `touch` / `cat` / `write` / `rm` / `stat`（文件操作，通过 `fork_exec_wait` 走完整的 fork+exec+wait 流程）、`ps` / `pstree`（进程树显示）、`kill`（发送信号）、`dual`（双进程演示启动器）、`cd` / `pwd`（目录导航）、`verbose`（调试开关）、`uptime`（定时器计数）
- **pmon 进程监控器**（`pmon.rs`）：TUI 实时显示进程状态、内存使用、磁盘信息
- **minigdb 调试器**（`mod.rs:153-232`）：独立 CPU+MMU 的单步调试器，支持 `step` / `regs` / `continue` / `quit`，不从 VFS 加载程序（直接读 `programs/` 目录）
- `UIContext` 封装消息总线接口，`send_and_wait()` 实现请求-响应的同步等待

### 用户态测试程序（`programs/`）

- 编写 syncdemo.asm（信号量互斥演示）、schedemo.asm（纯调度演示）、cmpdemo.asm（条件跳转演示）
- 编写 ls.asm / tree.asm / cat.asm / write.asm / mkdir.asm / stat.asm / rm.asm 等文件系统操作测试程序
- 编写 clipwrite.asm / clipread.asm 设备操作测试程序
