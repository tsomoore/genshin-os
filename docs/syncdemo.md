# syncdemo.asm 代码详解与信号量同步演示

## 一、演示目标

通过 `dual syncdemo` 创建两个进程，使用全局信号量（semaphore 0）实现临界区互斥。直观展示：两个进程并发运行，但同一时刻只有一个进入临界区。

---

## 二、信号量机制

全局信号量 0 在系统启动时预创建（`SyncManager::new()`），初始值 = 1（二元信号量，相当于互斥锁）。

| 系统调用 | R0 | R1 | 语义 |
|---------|-----|-----|------|
| `sem_wait(sem_id)` | 201 | sem_id | P 操作：count=0 则阻塞，count>0 则 count-- |
| `sem_signal(sem_id)` | 202 | sem_id | V 操作：有等待者则转移所有权，无等待者则 count++ |

**所有权转移（TOCTOU 修复）**：`sem_signal` 不会简单 count++ 然后让调用者抢回去，而是直接把信号量交给阻塞的等待者。这避免了"信号-立即重入"竞争。

---

## 三、syncdemo.asm 逐行解释

```
地址      指令              含义
──────────────────────────────────────────────────
0x00      MOV R3, #5         循环计数器 = 5 轮
0x08      MOV R1, #0         sem_id = 0（全局信号量）
0x10      MOV R0, #201       sem_wait(0)
0x18      INT 0x80           → 触发系统调用，进入临界区
0x20      MOV R0, #1         print '['
0x28      MOV R1, #0x5B      字符 '[' = 0x5B
0x30      INT 0x80           → 输出 '['
0x38      MOV R2, #10        自旋延迟计数器
0x40      SUB R2, #1         延迟循环：递减
0x48      CMP R2, #0         
0x50      JNZ 0x40           跳回 SUB（让调度可见）
0x58      MOV R1, #0         sem_id = 0
0x60      MOV R0, #202       sem_signal(0)
0x68      INT 0x80           → 退出临界区，唤醒等待者
0x70      MOV R0, #1         print ']'
0x78      MOV R1, #0x5D      字符 ']' = 0x5D
0x80      INT 0x80           → 输出 ']'
0x88      SUB R3, #1         循环计数器--
0x90      CMP R3, #0         
0x98      JNZ 0x08           跳回 MOV R1,#0（继续下一轮）
0xA0      MOV R0, #0         exit(0)
0xA8      MOV R1, #0         
0xB0      INT 0x80           → 退出进程，释放信号量
```

### 关键点

1. **临界区 = 0x10 到 0x68**：从 `sem_wait` 到 `sem_signal`。同一时刻只有一个进程在此区域内。
2. **临界区外 = 0x70 到 0x80**：打印 `]`。两个进程可以同时在这里（都 Running）。
3. **自旋延迟（0x38-0x50）**：故意让调度可见，否则临界区太短（几微秒），肉眼看不到交替。
4. **exit(0)（0xA0-0xB0）**：进程结束时释放所有资源，包括信号量——防止另一个进程永久阻塞。

---

## 四、双核 SMP 下的执行时序

### 理想情况（第一轮）

```
Tick 1:  CPU0 调度 PID3          CPU1 调度 PID4
         PID3: sem_wait(0)       PID4: sem_wait(0)
               count 1→0, 获锁         count=0, 阻塞!
               print '['               [HALTED]
         
Tick 2:  PID3: 自旋延迟          CPU1: 调度 loop（空闲）
         PID3: sem_signal(0)
               → 转移所有权给 PID4
               → PID4 unblock, Ready
         
Tick 3:  PID3: print ']'        CPU1: 调度 PID4
         PID3: sem_wait(0)      PID4: print '['
               count=0, 阻塞!          (PID4 此时在临界区)
               [HALTED]
         
Tick 4:  CPU0: 调度 PID4（继续）  CPU1: 调度 loop
         PID4: 自旋延迟
         PID4: sem_signal(0)
               → 转移给 PID3
```

### 关键观察

- **PID3 和 PID4 都 Running？→ 正常！** PID3 在临界区外打印 `]`，PID4 在临界区内打印 `[`。
- **互斥证据**：verbose 输出中，绝不会出现两个 `[PRINT] 91`（即 `[`）中间没有 `]`。
- **pmon 中看到一个 Blocked 一个 Running** → 信号量正在工作。

---

## 五、演示步骤

```
rm .genshin-disk.img .genshin-vfs.json
cargo run

> verbose on
> dual syncdemo
```

观察点：

| 现象 | 说明 |
|------|------|
| `[PRINT] 91` 和 `[PRINT] 93` 交替出现 | `[` = 0x5B = 91, `]` = 0x5D = 93 |
| 不会出现连续两个 `[PRINT] 91` | 互斥生效 |
| pmon 中一个 Running 一个 Blocked | 信号量阻塞 |
| 偶尔两个都 Running | SMP 并发——一个临界区内，一个临界区外 |
| 5 轮后进程消失 | exit(0) → Zombie → reaper 回收 |

---

## 六、答辩问答准备

**Q: 为什么两个进程能同时 Running？**

A: "Running 是 PCB 状态，表示进程被分配到了某个 CPU 上执行。双核 SMP 下两个 CPU 各自独立调度。信号量保证的是临界区互斥——同一时刻只有一个进程在 `sem_wait` 和 `sem_signal` 之间。临界区外（打印 `]` 和循环回 `sem_wait` 之间）两个进程可以并发。这就是为什么 pmon 里偶尔看到两个 Running——一个刚释放锁，一个刚拿到锁。"

**Q: 信号量怎么实现阻塞和唤醒？**

A: "`sem_wait` 发现 count=0 时，把进程 PCB 设为 Blocked，从调度器就绪队列移除，CPU 停机（cpu.halt）。`sem_signal` 扫描进程表找到等待该信号量的进程，设为 Ready 并重新加入调度队列。所有权转移机制确保信号量不会在释放瞬间被调用者自己抢回去。"

**Q: 为什么用自旋延迟？**

A: "sem_wait + print + sem_signal 在真实 CPU 上只需几微秒。汇编里每条指令 1 个 CPU 周期，不加延迟的话双方 5 轮 100 微秒就跑完了，肉眼和 pmon 都观察不到。10 次自旋让临界区延长到 ~100 微秒，让调度切换可见。"

**Q: 进程结束后另一个会不会永久卡死？**

A: "不会。exit(0) 系统调用会释放进程持有的信号量，唤醒等待者。即使进程因 HALT 指令意外终止（未调 exit），Zombie 检查也会自动释放信号量。"

---

## 七、关键数据结构速查

```
信号量 Semaphore {
    id: 0,              // 全局信号量 ID
    owner_pid: 0,       // 创建者（0=kernel）
    value: AtomicU64,   // 当前计数（1=可用, 0=被占用）
    wait_count: u64,    // 等待者数量
}

PCB 阻塞状态:
ProcessState::Blocked(BlockReason::WaitingForLock { lock_addr: 0 })
                                     ↑ sem_id = 0
```
