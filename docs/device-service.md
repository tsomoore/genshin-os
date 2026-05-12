# DeviceService 实现原理

## 1. 操作系统原理：设备管理

真实 OS 中，设备管理分四层：

```
用户进程 (open/read/write/close)
    │
    ▼ 系统调用
内核 VFS/设备层 (统一接口)
    │
    ▼ 驱动接口
设备驱动 (键盘驱动、磁盘驱动、剪贴板驱动...)
    │
    ▼ I/O 指令
硬件设备 (键盘、磁盘、剪贴板...)
```

genshin-os 的 DeviceService 对应**内核设备层**——统一管理所有设备的注册、打开、读写和释放。

## 2. 架构：消息总线 + 设备管理器

```
┌──────────────────────────────────────────────────┐
│                  进程 (CPU 执行)                   │
│  MOV R0, #208  ; device_open                     │
│  INT 0x80      ; → handle_file_syscall(208)      │
└────────────────────┬─────────────────────────────┘
                     │ bus.send_request(
                     │   KernelMsg::Device(
                     │     DeviceRequest::...
                     │   ))
                     ▼
┌──────────────────────────────────────────────────┐
│              Kernel (消息路由)                     │
│  KernelMsg::Device(_) → {}  // 忽略!              │
│  Device 消息不需要 Kernel 转发                     │
└──────────────────────────────────────────────────┘
                     │ bus 广播到所有订阅者
                     ▼
┌──────────────────────────────────────────────────┐
│            DeviceService (直接订阅 bus)            │
│                                                  │
│  ┌──────────────────────────────────────┐        │
│  │         DeviceManager                │        │
│  │  devices: HashMap<DeviceId, Device>  │        │
│  │  管理设备注册/注销/查找               │        │
│  └──────────────────────────────────────┘        │
│                                                  │
│  ┌──────────────────────────────────────┐        │
│  │         DriverManager                │        │
│  │  drivers: Vec<Driver>               │        │
│  │  驱动匹配和加载 (目前未使用)          │        │
│  └──────────────────────────────────────┘        │
│                                                  │
│  ┌──────────────────────────────────────┐        │
│  │         Clipboard Buffer             │        │
│  │  clipboard: Arc<Mutex<String>>       │        │
│  │  剪贴板数据 (绕过设备框架直接操作)    │        │
│  └──────────────────────────────────────┘        │
└──────────────────────────────────────────────────┘
```

## 3. 关键设计决策：直接订阅 bus

DeviceService **不通过 Kernel 转发**，而是直接调用 `bus.subscribe()` 订阅消息总线。

```
其他服务:             DeviceService:
Kernel → channel →    直接订阅 bus
  Process/Memory/       ↑
  File Service         KernelMsg::Device
```

这意味着 Device 消息不需要 Kernel 参与路由。Kernel 收到 `Device` 消息时执行空操作 `{}`——因为 DeviceService 已经直接从 bus 收到了。

**好处**: Device 消息延迟最低（少一跳）
**代价**: 架构不一致（其他服务都走 Kernel 通道）

## 4. 剪贴板实现

### 4.1 消息类型

```rust
// messaging/msg.rs
pub enum DeviceRequest {
    // 通用设备操作
    RegisterDevice { device_type, name },
    Read { device_id, buf, size },
    Write { device_id, buf, size },
    // ...

    // 剪贴板专用
    ClipboardSet { data: Vec<u8> },   // 写入剪贴板
    ClipboardGet { max_size: usize }, // 读取剪贴板
}
```

### 4.2 DeviceService 处理

```rust
// service.rs — handle_device_request_with_response
DeviceRequest::ClipboardSet { data } => {
    self.clipboard.lock().unwrap().clear();
    self.clipboard.lock().unwrap()
        .push_str(&String::from_utf8_lossy(&data));
    envelope.respond_success(ResponseData::Void);
}

DeviceRequest::ClipboardGet { max_size } => {
    let clip = self.clipboard.lock().unwrap();
    let data = clip.as_bytes();
    let len = std::cmp::min(max_size, data.len());
    envelope.respond_success(
        ResponseData::Bytes(data[..len].to_vec())
    );
}
```

### 4.3 数据存储

剪贴板数据存在 `Arc<Mutex<String>>` 中——简单高效，不需要走 DeviceManager 的复杂框架。

```
DeviceService.clipboard = ""
    │
    │ ClipboardSet { data: "hello" }
    │ → clipboard = "hello"
    │
    │ ClipboardGet { max_size: 256 }
    │ → Bytes("hello")
    ▼
```

### 4.4 完整写入流程

```
clipwrite.asm:
  STORE [0x200], 'H'      ; 把 "HELLO" 写入进程内存 0x200
  STORE [0x201], 'E'
  STORE [0x202], 'L'
  STORE [0x203], 'L'
  STORE [0x204], 'O'

  MOV R0, #208            ; device_open
  INT 0x80                ; → println("[DEVICE] pid=2 requests clipboard")

  MOV R0, #211            ; clipboard_write
  MOV R2, #5              ; 5 字节
  INT 0x80                ; → ProcessService
                            │
                            ▼
                          handle_file_syscall(211):
                            data = read_bytes_virt(pid, 0x200, 5)
                            // data = "HELLO"
                            bus.send_request(
                              KernelMsg::Device(
                                DeviceRequest::ClipboardSet {
                                  data: vec![72,69,76,76,79]
                                }))
                              │
                              ▼ (bus 广播)
                            DeviceService:
                              clipboard = "HELLO"

  MOV R0, #209            ; device_close
  INT 0x80                ; → println("[DEVICE] pid=2 releases clipboard")
```

### 4.5 完整读取流程

```
clipread.asm:
  MOV R0, #208            ; device_open
  INT 0x80

  MOV R0, #210            ; clipboard_read
  MOV R1, #32             ; max 32 bytes
  INT 0x80                ; → ProcessService
                            │
                            ▼
                          handle_file_syscall(210):
                            bus.send_request(
                              KernelMsg::Device(
                                DeviceRequest::ClipboardGet { max_size: 32 }))
                              │
                              ▼
                            DeviceService:
                              clipboard = "HELLO" (5 bytes)
                              → respond Bytes("HELLO")
                              │
                              ▼
                            ProcessService:
                              write_bytes_virt(pid, 0x200, "HELLO")
                              cpu.write_register(R2, 5)

  MOV R0, #2              ; print_str syscall
  MOV R1, #0x200          ; buffer at 0x200
  MOV R2, R2              ; length = 5
  INT 0x80                ; → println!("HELLO")

  MOV R0, #209            ; device_close
  INT 0x80
```

## 5. 系统调用速查

| R0 | 操作 | 说明 |
|----|------|------|
| 208 | device_open | 申请剪贴板，日志输出 |
| 209 | device_close | 释放剪贴板，日志输出 |
| 210 | clipboard_read | R1=max_size，数据到 0x200，R2=实际长度 |
| 211 | clipboard_write | R2=长度，从 0x200 读数据写入剪贴板 |

## 6. 输出演示

```
run clipwrite
  [DEVICE] pid=2 requests clipboard    ← 进程申请设备
  [DEVICE] pid=2 releases clipboard    ← 进程释放设备

paste
  HELLO                                ← 剪贴板数据验证

run clipread
  [DEVICE] pid=3 requests clipboard    ← 申请设备
  HELLO                                ← 读取设备数据
  [DEVICE] pid=3 releases clipboard    ← 释放设备
```

## 7. 当前局限

| 问题 | 说明 |
|------|------|
| 只支持剪贴板 | DeviceManager/DriverManager 框架已有但未使用 |
| 无独占性 | 多进程可同时"打开"剪贴板，无互斥保护 |
| 绕过 Kernel | 架构不一致（其他服务走 Kernel 通道） |
| 无中断处理 | DeviceRequest::EnableInterrupt 等未实现 |
