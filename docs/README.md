# genshin-OS 文档中心

> 硬件模拟层与内核服务层接口文档

欢迎使用 genshin-OS 微内核操作系统文档中心。本文档为负责内核服务层开发的同学提供完整的接口说明和协作指南。

---

## 🚀 快速开始

### 我是内核服务层开发者 - 必读文档

按以下顺序阅读，快速上手：

1. ⭐ **[硬件层与内核服务层协作指南](./HARDWARE_SERVICE_COORDINATION.md)** - **最重要！**
   - 📖 理解消息总线如何工作
   - 📖 了解硬件层提供了哪些接口
   - 📖 学习服务层实现框架
   - 📖 查看完整的消息传递流程图

2. ⭐ **[IPC 消息格式文档](./IPC_MESSAGE_FORMAT.md)**
   - 📖 掌握进程间通信的所有协议
   - 📖 消息传递、共享内存、同步原语
   - 📖 完整的代码示例和使用模式

3. ⭐ **[File System 和 Device Manager 硬件支持指南](./FILE_SYSTEM_DEVICE_MANAGER_GUIDE.md)** - **新增！**
   - 📖 块设备接口（BlockDevice trait）
   - 📖 磁盘分区支持
   - 📖 通用设备接口（Device trait）
   - 📖 设备注册表（DeviceRegistry）
   - 📖 完整的文件系统和设备管理器消息协议

4. 📗 **[API 快速参考](./API_QUICK_REFERENCE.md)**
   - 📖 常用类型和函数速查表
   - 📖 快速代码示例

### 我是架构设计者

1. 📘 **[硬件层与内核服务层协作指南](./HARDWARE_SERVICE_COORDINATION.md)** - "架构概览"章节
2. 📙 **[设计审查报告](./DESIGN_REVIEW.md)** - 了解设计决策和改进建议
3. 📗 **[完整接口文档](./INTERFACE_REVIEW.md)** - 查看所有接口的详细设计

---

## 📚 完整文档目录

### 核心文档

| 文档 | 说明 | 重要性 |
|------|------|--------|
| **[硬件层与内核服务层协作指南](./HARDWARE_SERVICE_COORDINATION.md)** | 消息总线工作原理、消息传递流程、接口说明、实现框架 | ⭐⭐⭐ |
| **[IPC 消息格式文档](./IPC_MESSAGE_FORMAT.md)** | 进程间通信协议、消息类型、同步原语、使用示例 | ⭐⭐⭐ |
| **[File System 和 Device Manager 硬件支持指南](./FILE_SYSTEM_DEVICE_MANAGER_GUIDE.md)** | 块设备、分区、通用设备接口、文件系统和设备管理器消息协议 | ⭐⭐⭐ |
| **[API 快速参考](./API_QUICK_REFERENCE.md)** | 常用 API 速查、快速代码示例 | ⭐⭐ |
| **[完整接口文档](./INTERFACE_REVIEW.md)** | 所有接口的详细说明、数据结构、扩展指南 | ⭐⭐ |
| **[设计审查报告](./DESIGN_REVIEW.md)** | 架构设计评估、优缺点分析、改进建议 | ⭐ |
| **[接口检查清单](./INTERFACE_CHECKLIST.md)** | 接口完整性检查、实现进度跟踪 | ⭐ |

### 示例代码

| 示例 | 说明 |
|------|------|
| **[IPC 消息演示](../examples/ipc_messages_demo.rs)** | 展示所有 IPC 消息类型的使用 |
| **[块设备和设备管理器演示](../examples/block_device_demo.rs)** | 展示块设备和设备管理器的使用 |
| **[响应机制示例](../examples/response_example.rs)** | RequestWithResponse 使用方法 |

---

## 🏗️ 架构概览

genshin-OS 采用 4 层微内核架构：

```
┌─────────────────────────────────────────────────────┐
│              用户交互层 (UI)                         │
│  • CLI Shell (用户命令)                             │
│  • TUI 系统监控器                                   │
└────────────────────┬────────────────────────────────┘
                     │
┌────────────────────┴────────────────────────────────┐
│            交换层 (Exchange Layer)                  │
│  • MessageBus (crossbeam channel)                  │
│  • KernelMsg (统一消息枚举)                         │
└────────────────────┬────────────────────────────────┘
                     │
┌────────────────────┴────────────────────────────────┐
│              服务层 (Service Layer)                 │
│  • ProcessService (进程管理、IPC)                   │
│  • MemoryService (内存分配、分页)                   │
│  • FileService (文件系统)                           │
│  • DeviceService (设备管理)                         │
└────────────────────┬────────────────────────────────┘
                     │
┌────────────────────┴────────────────────────────────┐
│             硬件层 (Hardware Layer)                 │
│  • VirtualCPU, MMU, Timer, Disk                    │
│  • BlockDevice (块设备接口)                         │
│  • Device (通用设备接口)                            │
│  • PhysicalMemory (物理内存)                        │
└─────────────────────────────────────────────────────┘
```

**核心设计原则**：
- ✅ **所有通信必须通过 MessageBus** - 无跨层直接调用
- ✅ **异步 Fire-and-Forget** - 发送消息不等待响应
- ✅ **统一消息格式** - 使用 `KernelMsg` 枚举
- ✅ **清晰的职责分离** - 硬件报告异常，服务做决策

---

## 🔌 核心类型

### 基础类型

| 类型 | 定义 | 说明 |
|------|------|------|
| `Pid` | `u64` | 进程 ID |
| `Tid` | `u64` | 线程 ID |
| `VirtAddr` | `u64` | 虚拟地址 |
| `PhysAddr` | `u64` | 物理地址 |

### 消息类型 (KernelMsg)

```rust
pub enum KernelMsg {
    Syscall(Syscall),           // 用户系统调用
    Interrupt(Interrupt),       // 硬件中断
    Process(ProcessRequest),    // 进程服务请求 (IPC、生命周期)
    Memory(MemoryRequest),      // 内存服务请求
    File(FileRequest),          // 文件服务请求
    Device(DeviceRequest),      // 设备服务请求
}
```

### 硬件组件

| 组件 | 文件 | 主要功能 |
|------|------|---------|
| `PhysicalMemory` | src/hardware/memory.rs | 物理内存访问 |
| `VirtualDisk` | src/hardware/disk.rs | 虚拟磁盘 |
| `BlockDevice` | src/hardware/block.rs | 块设备接口 trait |
| `PhysicalBlockDevice` | src/hardware/block.rs | 块设备实现 |
| `PartitionDevice` | src/hardware/block.rs | 分区设备 |
| `Device` | src/hardware/device.rs | 通用设备接口 trait |
| `DeviceRegistry` | src/hardware/device.rs | 设备注册表 |
| `MMU` | src/hardware/mmu.rs | 地址转换 |
| `Timer` | src/hardware/timer.rs | 定时器中断 |
| `VirtualCPU` | src/hardware/cpu.rs | 指令执行 |
| `IVT` | src/hardware/ivt.rs | 中断向量表 |

---

## 💻 开发环境设置

### 前置要求
- Rust 1.70+
- Git

### 快速开始

```bash
# 1. 克隆仓库
git clone <repository-url>
cd genshin-os

# 2. 构建
cargo build

# 3. 运行测试
cargo test

# 4. 运行 IPC 示例
cargo run --example ipc_messages_demo

# 5. 运行块设备和设备管理器示例
cargo run --example block_device_demo
```

### 导入类型

所有类型从 `genshin_os` crate 导出：

```rust
use genshin_os::{
    // 核心类型
    KernelMsg, MessageBus, LockedBus,

    // 基础类型
    Pid, Tid, VirtAddr, PhysAddr,

    // 消息类型
    ProcessRequest, IPCMessage, SignalType,
    MemoryRequest, FileRequest, DeviceRequest,
    FileSystemType, SeekWhence, DeviceClass, DeviceConfig,

    // 硬件层 - 基础
    VirtualCPU, MMU, PhysicalMemory, Timer, VirtualDisk,

    // 硬件层 - 块设备（文件系统支持）
    BlockDevice, PhysicalBlockDevice, PartitionDevice,
    Partition, PartitionType, PartitionLayout,

    // 硬件层 - 通用设备（设备管理器支持）
    Device, DeviceType, DeviceStatus, DeviceRegistry,
    KeyboardDevice, SerialDevice, NetworkDevice,

    // 错误处理
    GenshinError, GenshinResult,

    // 响应机制
    RequestWithResponse, Response, ResponseData,
};
```

---

## 🔧 常用命令

```bash
# 构建项目
cargo build

# 检查代码（不构建）
cargo check

# 运行所有测试
cargo test

# 运行特定测试
cargo test test_memory_creation

# 运行示例
cargo run --example ipc_messages_demo
cargo run --example block_device_demo

# 格式化代码
cargo fmt

# 代码检查
cargo clippy

# 生成文档
cargo doc --open
```

---

## 💡 快速提示

### 订阅消息

```rust
let receiver = bus.subscribe();
loop {
    if let Ok(msg) = receiver.recv() {
        // 处理消息
    }
}
```

### 发送消息

```rust
let msg = KernelMsg::Process(ProcessRequest::SendMessage {
    from_pid: 100,
    to_pid: 200,
    msg: IPCMessage::Text { data: "Hello!".to_string() },
});
bus.send(msg)?;
```

### 需要响应时

```rust
let (req, rx) = RequestWithResponse::new(msg);
bus.send(req.message)?;
let response = rx.recv()?;
```

### 虚拟地址转换

```rust
let data = mmu.read_u32(pid, user_vaddr)?;
```

### 查看硬件状态

```rust
let state = cpu.dump_state();
println!("CPU: {:#?}", state);
```

---

## 🔗 相关链接

- **[项目主 README](../README.md)** - 项目概述
- **[CLAUDE.md](../CLAUDE.md)** - 架构指南和开发规范
- **[源代码](../src/)** - 完整源代码

---

## 📝 文档更新记录

| 日期 | 版本 | 更新内容 |
|------|------|---------|
| 2026-03-24 | 3.0 | 添加 File System 和 Device Manager 硬件支持指南、块设备和设备管理器示例 |
| 2026-03-24 | 2.0 | 添加硬件层与内核服务层协作指南、IPC 消息格式文档 |
| 2026-03-23 | 1.0 | 初始版本，完整接口文档 |

---

**维护者**：genshin-OS 架构组
**最后更新**：2026-03-24

**祝开发顺利！** 🚀
