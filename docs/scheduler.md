# 进程调度状态机设计文档

## 一、概述

Genshin-OS 实现了一个 **SMP（对称多处理）感知的 Round-Robin 调度器**。调度器每 10ms（100Hz）被硬件 Timer 中断唤醒一次，为每个 CPU 核心独立做出调度决策。

### 核心设计原则

1. **每核独立计时**：每个 CPU 有自己的 `current` 进程和 `ticks` 计数器，互不干扰
2. **共享就绪队列**：所有 CPU 从同一个 FIFO 队列中取进程
3. **同 tick 去重**：同一时钟周期内，两个 CPU 不能运行同一个进程
4. **时间片轮转**：每个进程最多运行 3 ticks（30ms），超时放回队尾

---

## 二、数据结构

### 2.1 Scheduler 核心字段

```
Scheduler {
    ready_queue: VecDeque<Entry>    ← 共享就绪队列 (FIFO)
    
    cpu_current: [Option<PID>; N]  ← 每核当前运行的进程
    cpu_ticks:   [u64; N]          ← 每核当前时间片已消耗 ticks
    
    time_slice: 3                   ← 时间片大小 (ticks)
}
```

### 2.2 进程状态（PCB）

```
                    fork_impl(0)
  ┌──────────┐  ───────────────→  ┌──────────┐
  │ CREATING │                    │  READY   │ ←───────── unblock / quantum expire
  └──────────┘                    └────┬─────┘
                                      │ schedule() 选入 CPU
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

---

## 三、调度算法

### 3.1 schedule(cpu_id) 伪代码

```
fn schedule(cpu_id):
    // 1. 检查当前进程时间片是否耗尽
    if cpu_current[cpu_id] is Some(pid):
        cpu_ticks[cpu_id] += 1
        if cpu_ticks[cpu_id] < time_slice:
            return Run(pid)           ← 继续运行
        
        // 时间片耗尽：放回队尾
        ready_queue.push_back(pid)
        cpu_current[cpu_id] = None
        cpu_ticks[cpu_id] = 0
    
    // 2. 从就绪队列取下一个
    if ready_queue is not empty:
        next = ready_queue.pop_front()
        cpu_current[cpu_id] = Some(next)
        return Run(next)
    
    // 3. 无可运行进程
    return Idle
```

### 3.2 handle_timer_interrupt 主循环

```
fn handle_timer_interrupt():
    assigned = HashSet::new()        ← 本 tick 已分配的 PID
    
    for cpu_id in 0..cpu_count:
        decision = scheduler.schedule(cpu_id)
        
        // 去重：如果该 PID 已被另一个 CPU 占用
        if decision is Run(pid) and pid in assigned:
            decision = scheduler.yield_current(cpu_id)  ← 让出，取下一个
        
        assigned.insert(pid)
        
        // 更新 PCB 状态
        if last_running[cpu_id] ≠ pid:
            PCB[last_running[cpu_id]].state = Ready     ← 旧进程 → Ready
            PCB[pid].state = Running                    ← 新进程 → Running
            last_running[cpu_id] = pid
        
        // 执行 3 条指令
        for _ in 0..3:
            cpu.step()
            // 处理系统调用、缺页等
    

    else (Idle):
        // 回收 Zombie 进程
        for each Zombie process:
            reap_process(pid)
```

---

## 四、双核调度示例

以 2 个 CPU、4 个 loop 进程（PID 2-5）为例，时间片=3 ticks：

```
Tick 1:  CPU0 → schedule(0) → PID 2 (队头)     CPU1 → schedule(1) → PID 3 (下一个)
         assigned = {2, 3}
         step ×3: CPU0[2], CPU1[3]

Tick 2:  cpu_ticks[0]=1 < 3 → PID 2 继续        cpu_ticks[1]=1 < 3 → PID 3 继续
         step ×3: CPU0[2], CPU1[3]

Tick 3:  cpu_ticks[0]=2 < 3 → PID 2 继续        cpu_ticks[1]=2 < 3 → PID 3 继续
         step ×3: CPU0[2], CPU1[3]

Tick 4:  cpu_ticks[0]=3 ≥ 3 → PID 2 回队尾      cpu_ticks[1]=3 ≥ 3 → PID 3 回队尾
         取 PID 4 (队头)                          取 PID 5 (下一个)
         ready_queue = [2, 3]                    ready_queue = [2, 3]
         step ×3: CPU0[4], CPU1[5]

Tick 5:  cpu_ticks[0]=1 → PID 4 继续             cpu_ticks[1]=1 → PID 5 继续

Tick 6:  cpu_ticks[0]=2 → PID 4 继续             cpu_ticks[1]=2 → PID 5 继续

Tick 7:  PID 4/5 到期 → 回队尾
         取 PID 2, PID 3                          ← 循环回到开始
```

**关键性质**：每个进程每 6 ticks（60ms）获得一次 3-tick（30ms）的执行窗口。

---

## 五、去重机制

### 问题

如果就绪队列只有一个进程（例如刚启动时），两个 CPU 都会调度到同一个进程，导致同一个进程在两个 CPU 上同时执行——这违反了单线程语义。

### 解决

`handle_timer_interrupt` 维护一个 `HashSet<Pid>` 记录本 tick 已分配的 PID。如果 `schedule(cpu_id)` 返回的 PID 已在集合中，调用 `yield_current(cpu_id)`：

```
fn yield_current(cpu_id):
    pid = cpu_current[cpu_id]
    ready_queue.push_back(pid)     ← 放回队尾
    cpu_current[cpu_id] = None
    cpu_ticks[cpu_id] = 0
    return dequeue_next(cpu_id)    ← 从队头取新的
```

这样 CPU1 会从就绪队列中取下一个不同的进程。如果队列中只有同一个进程，CPU1 返回 Idle（空闲）。

---

## 六、PCB 状态转换

### 每核独立的 last_running

```
last_running: Vec<Option<Pid>>    ← 每核独立追踪上一个进程
```

每次调度决策后：

```
if decision is Run(new_pid):
    prev = last_running[cpu_id]
    if prev ≠ new_pid:
        PCB[prev].state = Ready      ← 旧进程归还
        PCB[new_pid].state = Running  ← 新进程标记
    last_running[cpu_id] = new_pid

if decision is Idle:
    PCB[last_running[cpu_id]].state = Ready
    last_running[cpu_id] = None
```

### 状态转换触发器

| 事件 | 旧状态 | 新状态 | 触发位置 |
|------|--------|--------|----------|
| 被调度选中 | Ready/Creating | Running | handle_timer_interrupt |
| 时间片耗尽 | Running | Ready | schedule() 内部 |
| 被抢占（另一进程选中）| Running | Ready | handle_timer_interrupt |
| 调用 sem_wait 阻塞 | Running | Blocked | handle_file_syscall |
| 调用 exit() | Running | Zombie | handle_file_syscall |
| HALT 指令 | Running | Zombie | handle_timer_interrupt |
| 父进程 wait | Zombie | (收割) | reap_process |

---

## 七、关键不变量

1. **单进程单核**：同一时刻，一个 PID 最多在一个 CPU 的 `cpu_current` 中出现
2. **队列完整性**：`ready_queue` 中的每个 PID 在 PCB 中状态为 Ready
3. **时间片公平**：每个进程每轮获得恰好 `time_slice` ticks 的 CPU 时间
4. **init 不死**：PID 1 永远不会被标记为 Zombie 或收割
5. **阻塞=空闲**：Blocked 进程不出现在队列中，`cpu.halted=true` 让出 CPU
