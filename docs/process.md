# Genshin-OS 进程管理

## 1. 进程生命周期

```
                    ┌──────────┐
     fork_impl(0)    │  CREATING │  (PID 0 → init)
  ─────────────────→ │          │
                    └────┬─────┘
                         │ handle_schedule (或 exec 调度)
                    ┌────▼─────┐
       scheduler    │  READY   │ ←──────────────┐
    ──────────────→ │          │    unblock     │
                    └────┬─────┘                │
                         │ scheduler.schedule() │
                    ┌────▼─────┐          ┌────┴─────┐
       timer tick   │ RUNNING  │  block   │  BLOCKED  │
    ←────────────── │          │ ───────→ │           │
       (3 ticks)    └────┬─────┘          └──────────┘
                         │ halt / exit
                    ┌────▼─────┐
                    │  ZOMBIE  │
                    │          │
                    └────┬─────┘
                         │ wait() 或 init reaper
                    ┌────▼─────┐
                    │  FREED   │  (从 process_table 移除)
                    └──────────┘
```

## 2. 进程创建

### 2.1 系统启动 — init 进程 (PID 1)

```rust
// ProcessService::run()
match self.fork_impl(0) {  // PID 0 = 内核
    Ok(pid) => println!("PS: Init PID = {}", pid),  // → 1
    ...
}
```

`fork_impl(0)` 从"无"创建进程：
- 分配 PID
- 创建 VirtualCPU (PC=0, SP=0xFFFF, 空寄存器)
- 创建 PCB (name="init", state=Ready)
- 不分配内存页 (等 exec 加载程序)
- 不加入调度队列 (无代码，exec 后才调度)

### 2.2 fork — 克隆进程

```rust
fork_impl(parent_pid=1):
  1. 验证父进程存在
  2. 分配 child_pid
  3. 遍历父进程页表 (MMU.get_page_entries(parent_pid)):
     ├─ AllocFrame → 新物理帧 (bus 消息 → MemoryService)
     ├─ MapPage(child, vaddr, new_frame) → 映射子进程页
     └─ 逐字节复制: mmu.read_u8(parent) → mmu.write_u8(child)
  4. 克隆 CPU 状态:
     child_cpu.set_pc(st.pc)
     child_cpu.set_sp(st.sp)
     child_cpu.write_register(R0, 0)  // 子进程 fork 返回 0
  5. 创建 PCB, state=Ready, parent_pid=Some(parent_pid)
  6. 建立父子关系: parent_children[parent].push(child)
  7. 不加入调度队列 (exec 会调度)
  8. 返回 child_pid
```

### 2.3 exec — 替换进程

```rust
exec_impl(pid, executable, args):
  1. 加载程序: load_program(name) → .asm 或 gen_builtin_program
  2. UnmapPage 所有旧页
  3. AllocFrame → MapPage 新页 → write_slice_virt 写代码
  4. write_slice_virt(0x100, args[0])  // 写路径参数
  5. write_slice_virt(0x200, args[1])  // 写第二参数 (write 用)
  6. 重置 CPU: set_pc(0), set_sp(0xFFFF), halted=false
  7. PCB 更新: name=executable, state=Ready
  8. handle_schedule(pid) → 加入调度队列
```

### 2.4 fork+exec+wait — Shell 命令的标准模式

```rust
fn fork_exec_wait(prog, args):
  fork_msg = ForkProcess { parent_pid: 1 }
  child_pid = send_and_wait(fork_msg).get_pid()   // → N

  exec_msg = ExecProcess { pid: N, executable: prog, args }
  send_and_wait(exec_msg)                          // 加载代码

  wait_msg = WaitChild { pid: 1, child_pid: N }
  send_and_wait(wait_msg)                          // 阻塞等子进程退出
```

## 3. 进程撤销

### 3.1 正常退出 — HALT 系统调用

```
MOV R0, #0    ; halt syscall
INT 0x80      ; → handle_file_syscall(0)
                → if pid != 1 { cpu.halt() }  // init 永不停止
```

### 3.2 Zombie 标记 — 定时器检测

```rust
// handle_timer_interrupt
if pid != 1 && cpu.is_halted() {
    scheduler.remove(pid, 1);           // 移出调度队列
    pcb.state = Zombie { exit_code: 0 }  // 标记僵尸

    // 通知等待的父进程
    if let Some(tx) = waiting_parents[pid] {
        tx.send(Response::success(exit_code))
    }
}
```

### 3.3 wait — 父进程回收

```rust
handle_wait_child_with_response(pid=1, child_pid=N):
  1. 验证 child_pid 是 pid 的子进程
  2. 检查子进程状态:
     ├─ 已是 Zombie → 立即收割, 返回 exit_code
     └─ 仍存活 → 存储 waiting_parents[N] = (1, response_channel)
                 父进程不阻塞 (异步 wait)
```

### 3.4 init reaper — 孤儿收割

```rust
// handle_timer_interrupt 空闲分支
if scheduler 返回 Idle:
  for each Zombie in process_table:
    reap_process(pid)
      → cpus.remove(pid)
      → process_table.remove(pid)
      // 注意: 不释放内存页 (等待 wait 或后续完善)
```

## 4. 进程调度

### 4.1 Round-Robin 时间片轮转

```
配置: Scheduler::round_robin(3)  // 3 个 timer tick = ~9 条指令
```

```rust
schedule_round_robin():
  if self.current == Some(pid, tid):
    self.time_used += 1            // 每个 tick 计数+1
    if self.time_used >= self.time_slice:  // >= 3
      self.time_used = 0
      self.ready(pid, tid, 128)   // 重新入队尾
      self.current = None
      return switch_to_next()     // 取队首进程
    return Run { pid, tid }       // 继续执行当前进程

  return get_next()               // 从就绪队列取下一个
```

### 4.2 定时器驱动

```rust
// ProcessService::run() 主循环
loop {
    match receiver.try_recv() {
        Ok(envelope) => handle_envelope(envelope),
        Err(Empty)   => sleep(10ms),
    }
    process_pending_forks();      // 异步 fork
    handle_timer_interrupt();     // 驱动调度
}
```

### 4.3 调度数据结构

```
Scheduler {
    policy: RoundRobin { quantum: 3 },
    current: Option<(Pid, Tid)>,     // 当前运行进程
    time_used: u64,                  // 当前时间片已用 tick 数
    ready_queue: VecDeque<ReadyQueueEntry>,  // FIFO 就绪队列
    priority_queue: Vec<ReadyQueueEntry>,    // 优先级队列 (未使用)
    context_switches: u64,           // 上下文切换计数
}
```

### 4.4 当前不足

| 问题 | 说明 |
|------|------|
| 优先级未使用 | priority_queue 存在但 round-robin 只查 ready_queue |
| 单 vCPU | 所有进程共享一个 VirtualCPU, 非真正的并行 |
| 无抢占优先级 | 时间片到才切换, 高优先级不能抢占低优先级 |
| 无 CPU 亲和性 | 所有进程在同一个 vCPU 上 |

## 5. 内存分配与回收

### 5.1 页式内存管理

```
页面大小: 4096 字节
物理内存: PhysicalMemory (4MB, 1024 个帧)

MMU (Memory Management Unit):
  page_tables: HashMap<Pid, HashMap<VirtAddr, PageTableEntry>>

  PageTableEntry:
    frame: PhysAddr      // 物理帧地址
    flags: PageFlags {
      present: bool       // 页是否在内存中
      writable: bool
      user_accessible: bool
    }
```

### 5.2 分配流程

```
alloc_frames(count):
  1. bus.send_request(AllocFrame { count })  → MemoryService
  2. rx.recv_timeout(2s)                     → 同步等待响应
  3. 返回 [phys_addr, phys_addr+4096, ...]

MemoryService 内部:
  PhysicalMemoryManager.allocate_frames(count)
    → 扫描帧位图, 找连续空闲帧
    → 标记为已用
    → 返回帧地址列表
```

### 5.3 映射流程

```
MapPage(pid, virt, phys, prot):
  bus.send_request(MapPage { pid, virt, phys, prot })  → MemoryService
  rx.recv_timeout(2s)

MemoryService:
  mmu.map_page(pid, virt, phys, flags)
    → page_tables[pid][virt] = PageTableEntry { frame: phys, flags }
```

### 5.4 回收流程

```
UnmapPage(pid, virt):
  bus.send(UnmapPage { pid, virt })  → MemoryService (fire-and-forget)
  mmu.page_tables[pid].remove(virt)

FreeFrame(paddr):
  bus.send(FreeFrame { paddr })  → MemoryService
  PhysicalMemoryManager.free_frame(paddr)
```

### 5.5 缺页处理

```
CPU 执行 fetch_instruction → mmu.read_u8(pid, vaddr)
  → translate() 失败 → PageNotPresent
  → cpu.pagefault_pending = true
  → bus.send(Interrupt::PageFault { addr: vaddr })

定时器中断循环:
  if cpu.pagefault_pending:
    bus.send_request(PageFaultHandler { pid, faulting_addr })
    → MemoryService 分配新帧 → map_page → 响应
    cpu.pagefault_pending = false
```

### 5.6 当前不足

| 问题 | 说明 |
|------|------|
| 无 COW (写时复制) | fork 总是完整复制所有页, 浪费内存和时间 |
| 无页面置换 | 物理内存满时没有 swap out 机制 |
| 分配/映射同步阻塞 | fork_impl 里逐页等待 MemoryService 响应, 延迟高 |
| 回收不完整 | Zombie 进程的 UnmapPage/FreeFrame 未执行 (reaper 跳过了) |
| 无内存配额 | 进程可以无限分配内存 |
| 缺页处理器丢弃响应 | `let _ = send_request(...)` 丢掉了 receiver |

## 6. fork 异步执行机制

fork 系统调用 (R0=100) 不直接在定时器里执行——那会卡住整个调度器。

```
CPU 执行 INT 0x80:
  handle_file_syscall(100):
    pcb.state = Blocked           // 进程进入睡眠
    scheduler.block(pid, 1)       // 移出就绪队列
    pending_forks.push(pid)       // 加入待办

主循环 (定时器外部):
  process_pending_forks():
    fork_impl(pid)               // 这里做重活 (分配内存、复制页表)
    cpu.write_register(R0, child_pid)  // 写返回值
    pcb.state = Ready            // 唤醒父进程
    scheduler.ready(pid, 1, 128) // 重新加入就绪队列
```

## 7. 进程状态检查 (pstree)

```
pstree 命令:
  → ProcessRequest::ListProcesses
  → handle_list_processes_with_response()
    → 遍历 process_table
    → 读取每个 PCB: pid, name, state, parent_pid
    → 递归构建树: 找根节点 → 按 parent_pid 分组 → 缩进打印

输出:
  └── PID 1 [Ready] init
      ├── PID 2 [Zombie] cat
      └── PID 3 [Ready] ls
```
