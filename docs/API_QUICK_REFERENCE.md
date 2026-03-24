# genshin-OS API 快速参考

> **内核服务层开发者速查手册**

## 📦 消息类型 (KernelMsg)

```rust
pub enum KernelMsg {
    Syscall(Syscall),         // 用户系统调用
    Interrupt(Interrupt),     // 硬件中断
    Process(ProcessRequest),  // 进程服务请求
    Memory(MemoryRequest),    // 内存服务请求
    File(FileRequest),        // 文件服务请求
    Device(DeviceRequest),    // 设备服务请求
}
```

## 🖥️ 硬件层 API

### PhysicalMemory

```rust
PhysicalMemory::new(size: usize) -> Self
.size() -> usize
.read_u8(addr: usize) -> Result<u8, MemError>
.write_u8(addr: usize, value: u8) -> Result<(), MemError>
.read_u32(addr: usize) -> Result<u32, MemError>
.write_u32(addr: usize, value: u32) -> Result<(), MemError>
.read_slice(addr: usize, buf: &mut [u8]) -> Result<(), MemError>
.write_slice(addr: usize, buf: &[u8]) -> Result<(), MemError>
.dump_state() -> MemoryState
```

### VirtualDisk

```rust
VirtualDisk::new(total_sectors: u32) -> Self
.size_bytes() -> u64
.total_sectors() -> u32
.read_sector(sector: u32) -> Result<Vec<u8>, DiskError>
.write_sector(sector: u32, buf: &[u8]) -> Result<(), DiskError>
.read_sectors(start: u32, count: u32) -> Result<Vec<u8>, DiskError>
.write_sectors(start: u32, buf: &[u8]) -> Result<(), DiskError>
.dump_state() -> DiskState
```

### MMU

```rust
MMU::new(memory: PhysicalMemory, page_size: usize) -> Self
.create_page_table(pid: Pid)
.remove_page_table(pid: Pid)
.map_page(pid: Pid, vaddr: VirtAddr, paddr: PhysAddr, flags: PageFlags) -> Result<(), MMUError>
.unmap_page(pid: Pid, vaddr: VirtAddr) -> Result<(), MMUError>
.translate(pid: Pid, vaddr: VirtAddr, access: AccessType) -> Result<PhysAddr, MMUError>
.read_u8(pid: Pid, vaddr: VirtAddr) -> Result<u8, MMUError>
.write_u8(pid: Pid, vaddr: VirtAddr, value: u8) -> Result<(), MMUError>
.dump_state(pid: Pid) -> MMUState
```

### Timer

```rust
Timer::new(bus: Arc<MessageBus>, config: TimerConfig) -> Self
.start()
.stop()
.pause()
.resume()
.is_running() -> bool
.tick_count() -> u64
.reset_counter()
.dump_state() -> TimerSnapshot
```

### VirtualCPU

```rust
VirtualCPU::new(mmu: Arc<MMU>, bus: Arc<MessageBus>, pid: Pid) -> Self
.pid() -> Pid
.set_pid(pid: Pid)
.read_register(reg: Register) -> u64
.write_register(reg: Register, value: u64)
.pc() -> u64
.set_pc(pc: u64)
.sp() -> u64
.set_sp(sp: u64)
.flags() -> CPUFlags
.is_halted() -> bool
.halt()
.reset()
.step() -> Result<(), CPUError>
.save_state() -> CPUState
.restore_state(state: CPUState)
.dump_state() -> CPUState
```

## 🔄 典型使用流程

### 1. 处理系统调用

```rust
// 订阅消息
let receiver = bus.subscribe();

// 接收消息
loop {
    if let Ok(KernelMsg::Syscall(syscall)) = receiver.recv() {
        match syscall {
            Syscall::CreateProcess { executable, args } => {
                // 处理进程创建
            }
            Syscall::Read { fd, buf, size } => {
                // 处理文件读取
                // 注意：buf 是虚拟地址，需用 MMU 转换
            }
            // ...
        }
    }
}
```

### 2. 处理硬件中断

```rust
loop {
    if let Ok(KernelMsg::Interrupt(interrupt)) = receiver.recv() {
        match interrupt {
            Interrupt::Timer => {
                // 触发调度
            }
            Interrupt::PageFault { addr, access_type } => {
                // 处理缺页
            }
            // ...
        }
    }
}
```

### 3. 地址转换

```rust
// 从用户空间复制数据
let data = mmu.read_u32(pid, user_vaddr)?;

// 向用户空间写入数据
mmu.write_u32(pid, user_vaddr, value)?;
```

### 4. 进程切换

```rust
// 保存当前进程状态
let saved_state = cpu.save_state();

// 切换到新进程
cpu.set_pid(new_pid);
cpu.restore_state(new_process_state);
```

## 📝 类型别名

```rust
Pid = u64         // 进程 ID
Tid = u64         // 线程 ID
VirtAddr = u64    // 虚拟地址
PhysAddr = u64    // 物理地址
Fd = u32          // 文件描述符
DeviceId = u32    // 设备 ID
```

## ⚠️ 重要注意事项

1. **所有虚拟地址必须通过 MMU 转换**
2. **不要跨层直接调用函数**
3. **使用消息总线进行所有通信**
4. **硬件错误通过 `KernelMsg::Interrupt` 上报**
5. **fire-and-forget 模式：发送后不等待响应**

## 🔗 相关文档

- [完整接口文档](./INTERFACE_REVIEW.md)
- [架构设计](../CLAUDE.md)
