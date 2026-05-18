# 进程调度与同步演示文档

## 一、两个 Demo 的分工

| Demo | 创建命令 | 展示什么 | 关键特征 |
|------|---------|---------|---------|
| **schedemo** | `dual schedemo` | 纯调度：时间片轮转 | 无限 CPU 循环，无锁，可同时 Running |
| **syncdemo** | `dual syncdemo` | 信号量互斥 | 无限循环，sem_wait/sem_signal，绝不同时 Running |

**同台对比**：同时运行两类 demo，pmon 中 schedemo 两个都 Running，syncdemo 一个 Running 一个 Blocked——直观展示「调度」和「同步」的区别。

---

## 二、信号量机制（syncdemo 使用）

全局信号量 0 在系统启动时预创建（`SyncManager::new()`，`src/services/process/sync.rs:409-416`），初始值 = 1（二元信号量）。

| 系统调用 | R0 | R1 | 语义 |
|---------|-----|-----|------|
| `sem_wait(sem_id)` | 201 | sem_id | P 操作：count=0 则阻塞，count>0 则 count-- |
| `sem_signal(sem_id)` | 202 | sem_id | V 操作：有等待者则转移所有权，无等待者则 count++ |

**所有权转移（TOCTOU 修复）**：`sem_signal` 直接把信号量交给阻塞的等待者，不 count++。避免调用者立即抢回去。代码：`src/services/process/service.rs:1592-1619`。

---

## 三、schedemo.asm（纯调度演示）

**文件**：`programs/schedemo.asm`

```
地址      指令              含义
────────────────────────────────────
0x00      MOV R0, #0         counter = 0
0x08      MOV R1, #255       max value

0x10      ADD R0, #1         counter++       ← 循环体
0x18      CMP R0, R1         比较
0x20      JNZ 0x10           跳回 ADD

0x28      MOV R0, #0         counter 归零
0x30      JMP 0x10           无限循环
```

**特点**：纯 CPU 计算，无任何系统调用。多个 schedemo 进程同时运行时，调度器按时间片（3 ticks = 30ms）轮转。pmon 中可以看到所有 schedemo 进程都是 Ready/Running 状态，自由切换。

---

## 四、syncdemo.asm（信号量互斥演示）

**文件**：`programs/syncdemo.asm`

```
地址      指令              含义
────────────────────────────────────
0x00      MOV R1, #0         sem_id = 0
0x08      MOV R0, #201       sem_wait(0)
0x10      INT 0x80           → 进临界区

0x18      MOV R0, #1         print '['
0x20      MOV R1, #0x5B      字符 '[' = 91
0x28      INT 0x80

0x30      MOV R2, #50        自旋延迟
0x38      SUB R2, #1
0x40      CMP R2, #0
0x48      JNZ 0x38

0x50      MOV R1, #0         sem_signal(0)
0x58      MOV R0, #202       → 出临界区，唤醒等待者
0x60      INT 0x80

0x68      MOV R0, #1         print ']'
0x70      MOV R1, #0x5D      字符 ']' = 93
0x78      INT 0x80

0x80      JMP 0x00           无限循环
```

**关键点**：

1. **临界区 = 0x08 到 0x58**：从 `sem_wait` 到 `sem_signal`。同时只有一个进程在此。
2. **临界区外 = 0x68 到 0x78**：打印 `]`。两个进程可以同时在这里（都是 Running）。
3. **自旋延迟**：50 次循环让临界区延长，使调度切换肉眼可见。
4. **无限循环**：`JMP 0x00`，永不退出。用 `kill` 停止。

---

## 五、双核 SMP 执行时序

### 5.1 schedemo（无锁，自由调度）

```
CPU0: PID2(schedemo)  ADD R0,#1 → CMP → JNZ → ...  (3 ticks 后切换)
CPU1: PID3(schedemo)  ADD R0,#1 → CMP → JNZ → ...

时间片到期: CPU0 换 PID4, CPU1 换 PID5
→ 4 个进程全部 Ready/Running，自由轮转
```

### 5.2 syncdemo（信号量互斥）

```
Tick 1:  CPU0=PID2              CPU1=PID3
         sem_wait(0) → 获锁     sem_wait(0) → 阻塞! [HALTED]
         print '['
         
Tick N:  PID2: sem_signal(0)
               → 转移所有权给 PID3
               → PID3 Ready, 进就绪队列
         PID2: print ']', JMP 0x00 → sem_wait(0)
               → count=0 → 阻塞! [HALTED]
               
Tick N+1: CPU0=从队列取 PID3    CPU1=空闲
         PID3: print '['        (PID3 在临界区内)
         PID3: sem_signal(0) → 转移给 PID2
```

**关键**：PID2 和 PID3 交替。永远不会同时 Running——pmon 看到「两个都 Running」是因为瞬态：一个刚释放锁在打印 `]`（临界区外），另一个刚拿到锁在打印 `[`（临界区内）。

---

## 六、同台对比演示

```
rm .genshin-disk.img .genshin-vfs.json
cargo run

> dual syncdemo       ← PID 2, 3：信号量互斥，永不退出
> dual schedemo       ← PID 4, 5：纯调度
> dual schedemo       ← PID 6, 7：再加两个
> pmon
```

pmon 中同时看到：

```
PID 2  Running   syncdemo    ← 获锁，临界区内
PID 3  Blocked   syncdemo    ← 等锁  ← 绝不同时 Running！
PID 4  Running   schedemo    ← 自由调度
PID 5  Ready     schedemo    ← 等时间片
PID 6  Running   schedemo    ← 自由调度
PID 7  Ready     schedemo    ← 等时间片
```

**对比一目了然**：schedemo 可以两个都 Running，syncdemo 绝不可能——信号量互斥 vs 无锁并发。

---

## 七、答辩问答

**Q: 为什么 syncdemo 的两个进程偶尔都显示 Running？**

A: "Running 是 PCB 状态，表示进程被分配到了 CPU 上。两个 CPU 各自独立调度。信号量保证的是临界区互斥——同一时刻只有一个进程在 `sem_wait` 和 `sem_signal` 之间。临界区外两个进程可以同时 Running。pmon 中看到两个都 Running 的瞬间，一个是刚释放锁在打印 `]`，一个是刚拿到锁在打印 `[`。互斥的证据是：verbose 输出中绝不会出现连续两个 `[PRINT] 91` 中间没有 `]`。"

**Q: 为什么 schedemo 能两个都 Running，syncdemo 不能？**

A: "schedemo 没有锁，纯 CPU 计算，调度器自由分配。syncdemo 有 `sem_wait`/`sem_signal`，信号量保证临界区互斥——一个进临界区，另一个就必须等。这就是同步机制的作用。"

**Q: 信号量怎么实现阻塞和唤醒？**

A: "`sem_wait` 发现 count=0 时，PCB 设为 Blocked，从调度器就绪队列移除，CPU 停机。`sem_signal` 扫描进程表找到等待该信号量的进程，设为 Ready 并重新加入调度队列。所有权转移机制确保信号量不会在释放瞬间被调用者抢回去。"

---

## 八、关键代码位置

| 组件 | 文件 | 行号 |
|------|------|------|
| schedemo.asm | `programs/schedemo.asm` | 全文 |
| syncdemo.asm | `programs/syncdemo.asm` | 全文 |
| 信号量 0 预创建 | `src/services/process/sync.rs` | 409-416 |
| sem_wait 处理 | `src/services/process/service.rs` | 1573-1591 |
| sem_signal 处理（TOCTOU）| `src/services/process/service.rs` | 1592-1619 |
| 调度器 schedule() | `src/services/process/scheduler.rs` | 127-142 |
| handle_timer_interrupt | `src/services/process/service.rs` | 571-720 |
| 调度文档 | `docs/scheduler.md` | 全文 |
