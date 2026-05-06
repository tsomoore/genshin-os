# genshin-OS 文档索引

本文档提供所有 genshin-OS 文档的快速索引。

---

## 📋 文档列表

### 🆕 ProcessService 设计与实现文档 ⭐ 新增
**路径**: `docs/PROCESS_SERVICE.md`

**说明**: ProcessService 的完整设计文档，涵盖架构、组件、API、调度器和线程安全设计。

**包含内容**:
- 📐 架构概览与组件结构图
- 🔄 进程生命周期状态机
- 📨 消息处理流程与路由
- 📋 全部 ProcessRequest 消息类型
- ⏱️ 调度器设计（FIFO/RR/Priority）
- 🔒 同步原语（信号量/互斥锁）
- 🔧 线程安全与锁层级设计
- 🧪 测试覆盖说明（64 tests）

**适合**: 内核服务层开发者、架构设计者

---

### 1. 文档中心 (README.md)
**路径**: `docs/README.md`

**说明**: 文档导航中心，为不同角色提供阅读路径建议。

**包含内容**:
- 快速开始指南
- 按角色的阅读建议
- 架构概览
- 核心类型速查
- 开发环境设置
- 常用命令

**适合**: 所有开发者，首次阅读

---

### 2. 硬件层与内核服务层协作指南 ⭐ 必读
**路径**: `docs/HARDWARE_SERVICE_COORDINATION.md`

**说明**: 最重要的文档，详细说明硬件层和内核服务层如何协作。

**包含内容**:
- 📐 架构概览和设计原则
- 🔄 MessageBus 工作原理
- 📊 消息传递流程详解（3个场景）
- 📡 KernelMsg 协议说明
- 🔌 硬件层提供的所有接口
  - VirtualCPU
  - MMU
  - Timer
  - VirtualDisk
  - PhysicalMemory
- 🔧 内核服务层实现指南
- 📊 完整的消息传递流程图
- 🎯 关键协作点说明

**适合**: 内核服务层开发者，必读

---

### 3. IPC 消息格式文档 ⭐ 必读
**路径**: `docs/IPC_MESSAGE_FORMAT.md`

**说明**: 进程间通信的完整协议说明和使用示例。

**包含内容**:
- 消息传递 (SendMessage, ReceiveMessage, PeekMessage)
- 共享内存 (CreateSharedMemory, Attach, Detach)
- 同步原语 (Semaphore, Mutex)
- 信号机制 (各种信号类型)
- 进程生命周期管理 (Fork, Exec, Wait)
- 完整使用示例
- 内核服务层实现要点

**适合**: ProcessService 开发者，必读

---

### 4. API 快速参考
**路径**: `docs/API_QUICK_REFERENCE.md`

**说明**: 常用 API 的速查表。

**包含内容**:
- 类型定义
- 函数签名
- 快速代码示例
- 常见使用模式

**适合**: 所有开发者，作为参考手册

---

### 5. 完整接口文档
**路径**: `docs/INTERFACE_REVIEW.md`

**说明**: 所有接口的详细设计文档。

**包含内容**:
- KernelMsg 完整定义
- 硬件层接口详细说明
- 服务层接口详细说明
- 使用示例
- 扩展指南

**适合**: 需要深入理解接口的开发者

---

### 6. 接口检查清单
**路径**: `docs/INTERFACE_CHECKLIST.md`

**说明**: 接口完整性检查和实现进度跟踪。

**包含内容**:
- 接口完整性检查项
- 实现状态
- 待办事项

**适合**: 项目管理者、架构设计者

---

### 7. 设计审查报告
**路径**: `docs/DESIGN_REVIEW.md`

**说明**: 架构设计的评估和改进建议。

**包含内容**:
- 设计优缺点分析
- 改进建议
- 设计决策理由
- 最佳实践

**适合**: 架构设计者

---

## 🎓 按学习路径阅读

### 路径 1: 内核服务层开发者

**目标**: 快速上手实现服务层功能

1. **[文档中心](README.md)** - 了解项目结构
2. **[硬件层与内核服务层协作指南](HARDWARE_SERVICE_COORDINATION.md)** - 理解协作机制
3. **[IPC 消息格式文档](IPC_MESSAGE_FORMAT.md)** - 掌握 IPC 协议
4. **[API 快速参考](API_QUICK_REFERENCE.md)** - 查阅 API

### 路径 2: 架构设计者

**目标**: 理解整体架构和设计决策

1. **[文档中心](README.md)** - "架构概览"章节
2. **[硬件层与内核服务层协作指南](HARDWARE_SERVICE_COORDINATION.md)** - 消息传递机制
3. **[设计审查报告](DESIGN_REVIEW.md)** - 设计评估
4. **[完整接口文档](INTERFACE_REVIEW.md)** - 详细设计

### 路径 3: 快速参考

**目标**: 查找特定信息

- 需要查找 API? → **[API 快速参考](API_QUICK_REFERENCE.md)**
- 需要了解 IPC? → **[IPC 消息格式文档](IPC_MESSAGE_FORMAT.md)**
- 需要理解消息流? → **[硬件层与内核服务层协作指南](HARDWARE_SERVICE_COORDINATION.md)** "消息传递流程"章节

---

## 📊 文档统计

| 文档 | 大小 | 页数估算 | 重要性 |
|------|------|---------|--------|
| 协作指南 | 23KB | ~8 页 | ⭐⭐⭐ |
| IPC 消息格式 | 5.2KB | ~2 页 | ⭐⭐⭐ |
| 完整接口文档 | 24KB | ~8 页 | ⭐⭐ |
| 设计审查 | 13KB | ~5 页 | ⭐⭐ |
| 接口检查清单 | 9.9KB | ~4 页 | ⭐ |
| API 快速参考 | 4.4KB | ~2 页 | ⭐⭐ |

---

## 🔍 快速查找

### 按主题查找

#### 消息总线
- **工作原理**: [协作指南 § 消息传递机制](HARDWARE_SERVICE_COORDINATION.md#消息传递机制)
- **消息流详解**: [协作指南 § 消息流详解](HARDWARE_SERVICE_COORDINATION.md#消息流详解)
- **订阅和发送**: [README § 快速提示](README.md#快速提示)

#### IPC 协议
- **所有 IPC 消息类型**: [IPC 消息格式文档](IPC_MESSAGE_FORMAT.md)
- **消息传递示例**: [examples/ipc_messages_demo.rs](../examples/ipc_messages_demo.rs)
- **共享内存**: [IPC 消息格式文档 § 共享内存](IPC_MESSAGE_FORMAT.md#2-共享内存-shared-memory)

#### 硬件接口
- **VirtualCPU**: [协作指南 § VirtualCPU](HARDWARE_SERVICE_COORDINATION.md#1-virtualcpu)
- **MMU**: [协作指南 § MMU](HARDWARE_SERVICE_COORDINATION.md#2-mmu)
- **Timer**: [协作指南 § Timer](HARDWARE_SERVICE_COORDINATION.md#3-timer)
- **其他组件**: [协作指南 § 硬件层提供的接口](HARDWARE_SERVICE_COORDINATION.md#-硬件层提供的接口)

#### 服务层实现
- **基本框架**: [协作指南 § 内核服务层实现指南](HARDWARE_SERVICE_COORDINATION.md#-内核服务层实现指南)
- **响应机制**: [协作指南 § 响应机制使用](HARDWARE_SERVICE_COORDINATION.md#响应机制使用)
- **错误处理**: [协作指南 § 错误处理](HARDWARE_SERVICE_COORDINATION.md#错误处理)

---

## 📝 文档维护

### 更新原则
- 当添加新的消息类型时，更新 [IPC 消息格式文档](IPC_MESSAGE_FORMAT.md)
- 当添加新的硬件接口时，更新 [协作指南](HARDWARE_SERVICE_COORDINATION.md)
- 定期检查 [接口检查清单](INTERFACE_CHECKLIST.md)，更新实现状态

### 贡献指南
- 保持文档与代码同步
- 使用清晰的章节结构
- 提供代码示例
- 添加流程图和表格说明

---

## 🔗 外部资源

- **[Rust 文档](https://doc.rust-lang.org/)**
- **[crossbeam 文档](https://docs.rs/crossbeam/)**
- **[项目主 README](../README.md)**
- **[CLAUDE.md](../CLAUDE.md)**

---

**最后更新**: 2026-03-24
