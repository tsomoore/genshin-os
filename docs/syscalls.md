# 系统调用（Syscall）设计与实现文档

> 全面解析 genshin-OS 系统调用的架构设计、实现细节、权衡分析与代码位置。

---

## 一、架构概览：两层译码模型

系统调用的完整路径经过**两层译码**，分别由不同层次的组件负责：

```
进程 .asm 代码                 CPU（硬件层）              ProcessService（内核层）
────────────────────────────────────────────────────────────────────────────────
MOV R0, #18
MOV R1, #0
INT 0x80          →   ① 译码"指令"：opcode=0x80     →   ② 译码"调用号"：R0=18
                        保存寄存器（syscall_regs）         match r0 {
                        设置 syscall_pending=true             18 => syscall_listdir()
                        停机等待内核处理                           → 发消息给 FileService
                                                              }
```

**关键设计原则**：CPU 只知道"发生了 INT 0x80"，不知道 R0 的值代表什么。R0 的含义由 ProcessService 解释。这保持了硬件层和内核层的清晰边界。

> **代码位置**：CPU 译码 `cpu.rs:660-695`，ProcessService 分发 `service.rs:1450-1486`

---

## 二、系统调用号分配表

| R0 | 系统调用 | 分类 | 输入 | 输出 |
|:--:|---------|------|------|------|
| 0 | `exit` | 生命周期 | R1=退出码 | 释放内存、变 Zombie |
| 1 | `print_char` | 控制台 | R1=字符 | 打印到 stdout |
| 2 | `print_str` | 控制台 | R1=虚拟地址, R2=长度 | 打印字符串 |
| 10 | `open` | 文件 | 路径@0x100, R1=flags | R1=文件描述符 |
| 11 | `close` | 文件 | R1=fd | — |
| 12 | `read` | 文件 | R1=fd, R2=最大字节 | 数据@0x200, R2=实际字节 |
| 13 | `write` | 文件 | R1=fd, 数据@0x200, R2=长度 | — |
| 14 | `mkdir` | 文件 | 路径@0x100 | — |
| 16 | `unlink` | 文件 | 路径@0x100 | — |
| 17 | `stat` | 文件 | 路径@0x100 | — |
| 18 | `listdir` | 文件 | 路径@0x100 | 文件名@0x200, R2=字节数 |
| 100 | `fork` | 进程 | — | R0=子进程 PID（父） / 0（子） |
| 101 | `exec` | 进程 | 程序名@0x100 | 替换当前进程 |
| 102 | `tree` | 进程 | 路径@0x100 | 树形文本@0x200, R2=字节数 |
| 200 | `sem_create` | 同步 | — | R1=sem_id |
| 201 | `sem_wait` | 同步 | R1=sem_id | 可能阻塞当前进程 |
| 202 | `sem_signal` | 同步 | R1=sem_id | 可能唤醒等待者 |
| 203 | `lock_create` | 同步 | — | R1=lock_id |
| 204 | `lock_acquire` | 同步 | R1=lock_id | 可能阻塞当前进程 |
| 205 | `lock_release` | 同步 | R1=lock_id | 可能唤醒等待者 |
| 208 | `device_open` | 设备 | — | R1=device_id |
| 209 | `device_close` | 设备 | — | — |
| 210 | `device_read` | 设备 | R1=最大字节 | 数据@0x200, R2=字节数 |
| 211 | `device_write` | 设备 | 数据@0x200, R2=长度 | — |

---

## 三、系统调用入口：从 CPU 到 ProcessService

### 3.1 CPU 侧：INT 0x80 指令

```rust
// 文件: src/hardware/cpu.rs:660-669
fn handle_software_interrupt(&mut self, vector: u8) -> Result<(), CPUError> {
    if vector == InterruptVector::Syscall.as_u8() {  // 0x80
        self.syscall_regs = self.registers;          // 保存 R0-R3
        self.syscall_pending = true;                 // 设标志
        Ok(())
    }
}
```

CPU 只做三件事：
1. 保存当前寄存器到 `syscall_regs`
2. 设置 `syscall_pending = true`
3. 返回 Ok，不抛异常

> **代码位置**：`src/hardware/cpu.rs:660-695`

### 3.2 ProcessService 侧：定时器中断中处理

```rust
// 文件: src/services/process/service.rs:631-645（简化）
for _ in 0..3 {                    // 每 tick 执行最多 3 条指令
    cpu.step();                    // 执行一条指令
    if cpu.syscall_pending {       // 有系统调用待处理
        cpu.syscall_pending = false;
        self.handle_file_syscall(  // 分发系统调用
            cpu, cpu.syscall_regs[0], cpu.syscall_regs[1], cpu.syscall_regs[2]
        );
    }
}
```

**为什么在定时器中断中处理？** 这是性能优化——避免每条 INT 0x80 都走消息总线（总线广播开销大）。直接在当前上下文中调用 `handle_file_syscall`，省去 `KernelMsg::Interrupt(SyscallTrap)` 的序列化/路由开销。

> **代码位置**：`src/services/process/service.rs:631-670`

---

## 四、系统调用分发表

```rust
// 文件: src/services/process/service.rs:1450-1486
fn handle_file_syscall(&self, cpu: &mut VirtualCPU, r0: u64, r1: u64, r2: u64) {
    let pid = cpu.pid();
    match r0 {
        // ── 生命周期 ──
        0   => self.syscall_exit(cpu, pid, r1),
        // ── 控制台 IO ──
        1   => self.syscall_print_char(cpu, pid, r1),
        2   => self.syscall_print_str(cpu, pid, r1, r2),
        // ── 文件 → 转发给 FileService ──
        10  => self.syscall_open(cpu, pid, r1),
        11  => { self.bus.send(KernelMsg::File(Close{fd})); }
        12  => self.syscall_read(cpu, pid, r1, r2),
        13  => self.syscall_write(cpu, pid, r1, r2),
        14  => self.syscall_mkdir(cpu, pid),
        16  => self.syscall_unlink(cpu, pid),
        17  => self.syscall_stat(cpu, pid),
        18  => self.syscall_listdir(cpu, pid),
        // ── 进程 ──
        100 => self.syscall_fork(cpu, pid),
        101 => self.syscall_exec(cpu, pid),
        102 => self.syscall_tree(cpu, pid),
        // ── 同步 ──
        200 => self.syscall_sem_create(cpu, pid),
        201 => self.syscall_sem_wait(cpu, pid, r1),
        202 => self.syscall_sem_signal(cpu, pid, r1),
        203 => self.syscall_lock_create(cpu, pid),
        204 => self.syscall_lock_acquire(cpu, pid, r1),
        205 => self.syscall_lock_release(cpu, pid, r1),
        // ── 设备 ──
        208 => self.syscall_device_open(cpu, pid),
        209 => self.syscall_device_close(cpu, pid),
        210 => self.syscall_device_read(cpu, pid, r1),
        211 => self.syscall_device_write(cpu, pid, r1, r2),
        _ => {}
    }
}
```

24 个系统调用，17 行的 dispatch 表。每个 handler 是独立的 `syscall_xxx()` 方法。

---

## 五、各类系统调用的实现模式

### 5.1 控制台 IO（R0=1, 2）

**当前实现**：直接 `println!`。这是唯一"破坏抽象"的类别——理论上应该写回进程内存让进程自己输出，但由于历史原因（syncdemo 的 `print '['` 需要立即可见），保留了直接打印。

```rust
// service.rs:1520-1528
fn syscall_print_char(&self, _cpu: &mut VirtualCPU, _pid: u64, r1: u64) {
    println!("[PRINT] {}", r1 as i64);
}
fn syscall_print_str(&self, _cpu: &mut VirtualCPU, pid: u64, r1: u64, r2: u64) {
    let data = self.read_bytes_virt(pid, r1, r2 as usize);
    let s = String::from_utf8_lossy(&data);
    println!("{}", s);
}
```

### 5.2 文件操作（R0=10-18）

**设计原则**：ProcessService **不直接操作文件**，只做消息转发。所有文件操作通过 `self.bus.send(KernelMsg::File(...))` 发给 FileService。

```rust
// service.rs:1574-1584 — 典型的"纯转发"模式
fn syscall_mkdir(&self, _cpu: &mut VirtualCPU, pid: u64) {
    let path = self.get_syscall_path(pid);   // 从进程内存 0x100 读路径
    self.bus.send(KernelMsg::File(FileRequest::CreateDirectory { path })).ok();
}
```

对于需要返回结果的操作（open、read、listdir），流程是：

```
1. 从进程虚拟内存 0x100 读参数（路径等）
2. self.bus.send_request(KernelMsg::File(...))  → 发请求给 FileService
3. rx.recv_timeout(10ms)                       → 等待响应
4. 将响应数据写入进程虚拟内存 0x200            → 结果传给进程
5. cpu.write_register(R2, data_len)            → 通知进程数据长度
```

```rust
// service.rs:1592-1610 — listdir 完整流程
fn syscall_listdir(&self, cpu: &mut VirtualCPU, pid: u64) {
    let path = self.get_syscall_path(pid);
    if let Ok(rx) = self.bus.send_request(KernelMsg::File(FileRequest::ListDir { path })) {
        if let Ok(resp) = rx.recv_timeout(Duration::from_millis(10)) {
            if let Some(ResponseData::StringList(entries)) = resp.data() {
                let data = entries.join("\n");
                let bytes = data.as_bytes();
                let len = std::cmp::min(bytes.len(), 4096);
                for (i, &b) in bytes[..len].iter().enumerate() {
                    let _ = self._mmu.write_u8(pid, 0x200 + i as u64, b);
                }
                cpu.write_register(Register::R2, len as u64);
            }
        }
    }
}
```

> **代码位置**：`service.rs:1534-1610`（get_syscall_path, open, read, write, mkdir, unlink, stat, listdir）

### 5.3 进程操作（R0=100-102）

直接操作进程数据结构，不经消息总线：

```rust
// service.rs:1614-1630
fn syscall_fork(&self, cpu: &mut VirtualCPU, pid: u64) {
    match self.fork_impl(pid) {
        Ok(child_pid) => { cpu.write_register(Register::R0, child_pid); }
        Err(_)         => { cpu.write_register(Register::R0, 0); }
    }
}
fn syscall_exec(&self, _cpu: &mut VirtualCPU, pid: u64) {
    let prog = self.read_string_virt(pid, 0x100);
    let _ = self.exec_impl(pid, prog, vec![]);
}
```

> **代码位置**：`service.rs:1614-1631`

### 5.4 同步原语（R0=200-205）

直接操作 `SyncManager` 中的信号量和互斥锁。这些是本地数据结构，不经过消息总线：

```rust
// service.rs:1648-1666 — sem_wait 关键流程
fn syscall_sem_wait(&self, cpu: &mut VirtualCPU, pid: u64, sem_id: u64) {
    let blocked = {
        let Ok(sync) = self.sync_manager.lock() else { return; };
        if let Some(sem) = sync.get_semaphore(sem_id) {
            sem.wait() != SemaphoreResult::Acquired
        } else { false }
    };
    if blocked {
        // 设置 PCB → Blocked
        // scheduler.block(pid, 1)  → 移出就绪队列
        // cpu.halt()               → CPU 停机
    }
}
```

> **代码位置**：`service.rs:1642-1752`（sem_create, sem_wait, sem_signal, lock_create, lock_acquire, lock_release）

### 5.5 设备操作（R0=208-211）

通过消息总线转发给 DeviceService：

```rust
// service.rs:1767-1780
fn syscall_device_read(&self, cpu: &mut VirtualCPU, pid: u64, max_size: u64) {
    if let Ok(rx) = self.bus.send_request(KernelMsg::Device(
        DeviceRequest::ClipboardGet { max_size: max_size as usize }
    )) {
        if let Ok(resp) = rx.recv_timeout(Duration::from_millis(200)) {
            if let Some(ResponseData::Bytes(data)) = resp.data() {
                for (i, &b) in data.iter().enumerate() {
                    let _ = self._mmu.write_u8(pid, 0x200 + i as u64, b);
                }
                cpu.write_register(Register::R2, data.len() as u64);
            }
        }
    }
}
```

> **代码位置**：`service.rs:1758-1786`

---

## 六、设计权衡分析

### 权衡 1：直接调用 vs 消息总线

| 方式 | 系统调用 | 理由 |
|------|:---:|------|
| 直接调用 | 进程（fork/exec）、同步（sem/lock）、控制台 | 操作本地数据结构，走总线是浪费 |
| 消息总线 | 文件（open/read/write/listdir）、设备 | 跨服务调用，必须遵守微内核规则 |

### 权衡 2：写内存 vs 直接打印

| 方式 | 系统调用 | 理由 |
|------|:---:|------|
| 写进程内存 0x200 | 文件（read/listdir/tree）、设备（clipboardRead） | 遵守抽象边界，进程自己决定如何输出 |
| 直接 println! | 控制台（print_char/str） | 历史原因，.asm 程序的 `[PRINT]` 需要实时可见 |

**未来改进方向**：控制台 IO 也可以走"写内存→进程读取→打印"的模式，彻底消除 ProcessService 中的 println!。代价是需要更复杂的 .asm 程序（需要循环读取缓冲区）。

### 权衡 3：路径参数传递

所有文件系统调用从进程虚拟内存 `0x100` 读取路径，从 `0x200` 读写数据。这是固定的 ABI 约定：

- `0x100`：exec 时写入的程序参数（路径/文件名）
- `0x200`：系统调用返回的数据缓冲区

优点：简单、一致，.asm 程序不需要复杂的栈操作。
缺点：固定地址，灵活性差（无法传递多个参数或大缓冲区）。

### 权衡 4：同步调用 vs 异步调用

所有系统调用都是**同步**的——CPU 执行 INT 0x80 后停机等待，直到 handler 返回。这是为了简化 .asm 程序的编写（不需要处理异步回调）。

真正的微内核（如 seL4）使用异步 IPC，但我们的 .asm 程序太简单，异步会增加不必要的复杂度。

---

## 七、如何添加新系统调用

1. **分配调用号**：在本文档的"调用号分配表"中找到合适的范围
2. **写 handler**：在 `service.rs` 中添加 `fn syscall_xxx()` 方法
3. **注册 dispatch**：在 `handle_file_syscall` 的 match 中加一行
4. **写 .asm 程序**：在 `programs/` 下创建测试程序
5. **测试**：`rm .genshin-* && cargo run` 后运行新命令

示例——添加"获取当前时间"系统调用（R0=103）：

```rust
// service.rs
fn syscall_time(&self, cpu: &mut VirtualCPU, _pid: u64) {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    cpu.write_register(Register::R0, t);
}
```

```rust
// handle_file_syscall 的 match 中加：
103 => self.syscall_time(cpu, pid),
```

---

## 八、关键代码位置速查表

| 组件 | 文件 | 行号 | 说明 |
|------|------|:---:|------|
| **CPU INT 0x80** | `src/hardware/cpu.rs` | 660-669 | 保存寄存器，设 pending |
| **syscall_pending 检查** | `src/services/process/service.rs` | 631-670 | 在定时器 step loop 中 |
| **Dispatch 表** | `src/services/process/service.rs` | 1450-1486 | 17 行 match 分发 |
| **syscall_exit** | `src/services/process/service.rs` | 1491-1518 | 进程退出 |
| **syscall_print_char/str** | `src/services/process/service.rs` | 1520-1528 | 控制台输出 |
| **get_syscall_path** | `src/services/process/service.rs` | 1534-1536 | 读 0x100 路径 |
| **syscall_open** | `src/services/process/service.rs` | 1538-1549 | 打开文件 |
| **syscall_read** | `src/services/process/service.rs` | 1551-1566 | 读文件→0x200 |
| **syscall_write** | `src/services/process/service.rs` | 1568-1572 | 写文件 |
| **syscall_mkdir** | `src/services/process/service.rs` | 1574-1578 | 建目录 |
| **syscall_unlink** | `src/services/process/service.rs` | 1580-1584 | 删文件 |
| **syscall_stat** | `src/services/process/service.rs` | 1586-1590 | 文件信息 |
| **syscall_listdir** | `src/services/process/service.rs` | 1592-1610 | 列目录→0x200 |
| **syscall_fork** | `src/services/process/service.rs` | 1614-1619 | 克隆进程 |
| **syscall_exec** | `src/services/process/service.rs` | 1621-1624 | 加载程序 |
| **syscall_tree** | `src/services/process/service.rs` | 1626-1635 | 目录树→0x200 |
| **syscall_sem_create** | `src/services/process/service.rs` | 1642-1646 | 创建信号量 |
| **syscall_sem_wait** | `src/services/process/service.rs` | 1648-1666 | P 操作 |
| **syscall_sem_signal** | `src/services/process/service.rs` | 1668-1693 | V 操作 + TOCTOU |
| **syscall_lock_create** | `src/services/process/service.rs` | 1699-1703 | 创建互斥锁 |
| **syscall_lock_acquire** | `src/services/process/service.rs` | 1705-1721 | 获取锁 |
| **syscall_lock_release** | `src/services/process/service.rs` | 1723-1752 | 释放锁 + TOCTOU |
| **syscall_device_open** | `src/services/process/service.rs` | 1758-1761 | 打开设备 |
| **syscall_device_close** | `src/services/process/service.rs` | 1763-1765 | 关闭设备 |
| **syscall_device_read** | `src/services/process/service.rs` | 1767-1780 | 读设备→0x200 |
| **syscall_device_write** | `src/services/process/service.rs` | 1782-1786 | 写设备 |
| **built-in 程序** | `src/services/process/service.rs` | 1390-1420 | gen_builtin_program() |
| **.asm 程序** | `programs/*.asm` | — | 用户态测试程序 |
