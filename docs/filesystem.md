# Genshin-OS 文件系统实现与进程文件访问

## 1. 总体架构

```
Shell (用户态)
  │  fork(1) → exec(N, program, [path]) → wait(N)
  │
  ▼
ProcessService (内核态 — 系统调用处理)
  │  handle_file_syscall(r0, r1, r2)
  │  路径从进程内存 0x100 读取
  │  数据从进程内存 0x200 读取
  │  fd 写回 CPU R1 寄存器
  │
  ▼  KernelMsg::File(FileRequest) 通过消息总线
  │
Kernel (消息路由)
  │  File → file_tx / Memory → memory_tx / Process → process_tx
  │
  ▼
FileService (文件服务)
  ├── VirtualFileSystem (目录树、inode 管理)
  ├── FileDescriptorManager (每进程 fd 表)
  ├── OpenFiles (打开文件缓存, File 对象)
  └── VirtualDisk (物理磁盘, .genshin-disk.img)
```

所有文件操作都走 CPU 的系统调用（INT 0x80），参数通过寄存器传递。

## 2. VFS 节点结构

```rust
VFSNode {
    inode: u64,                    // 唯一编号
    name: String,                  // 文件/目录名
    node_type: NodeType,           // File | Directory
    parent: Option<u64>,           // 父目录 inode
    children: HashMap<String, u64>,// 子项: name → inode
    blocks: Vec<u64>,              // 文件数据扇区列表
    size: u64,                     // 文件大小
    ref_count: u32,                // 引用计数 (0=可删除)
}
```

**目录树的组织方式:**

```
/ (inode 0, Directory)
├── children["bin"] = 1      → bin (inode 1, Directory)
├── children["programs"] = 7 → programs (inode 7, Directory)
│   ├── children["ls.asm"] = 8 → ls.asm (inode 8, File)
│   │   blocks = [4]  → 文件数据在磁盘扇区 4
│   │   size = 109
│   └── children["cat.asm"] = 9 → cat.asm (inode 9, File)
│       blocks = [5]
│       size = 264
├── children["home"] = 2     → home (inode 2, Directory)
├── children["tmp"] = 3      → tmp (inode 3, Directory)
├── children["etc"] = 4      → etc (inode 4, Directory)
├── children["var"] = 5      → var (inode 5, Directory)
└── children["examples"] = 6 → examples (inode 6, Directory)
```

## 3. 具体实例：cat /programs/mkdir.asm 全流程

### 3.1 Shell 层

```rust
// shell/mod.rs — execute_command
"cat" => {
    let p = self.resolve_path("mkdir.asm");  // → "/programs/mkdir.asm"
    self.fork_exec_wait("cat", &[&p])        // fork + exec + wait
}
```

`fork_exec_wait` 内部:
```rust
fn fork_exec_wait(&self, prog: &str, args: &[&str]) -> Result<(), String> {
    // 1. fork: 从 init (PID 1) 创建子进程
    let fork_msg = ForkProcess { parent_pid: 1 };
    let child_pid = self.send_and_wait(fork_msg)?.get_pid()?;  // → PID 6

    // 2. exec: 加载 cat.asm, 把路径写入子进程内存
    let exec_msg = ExecProcess {
        pid: 6,
        executable: "cat".into(),
        args: vec!["/programs/mkdir.asm".to_string()],
    };
    self.send_and_wait(exec_msg)?;

    // 3. wait: 阻塞等子进程退出
    let wait_msg = WaitChild { pid: 1, child_pid: Some(6) };
    self.send_and_wait(wait_msg)?;

    Ok(())
}
```

### 3.2 fork — 创建子进程

```
fork_impl(parent_pid=1):
  1. 分配 child_pid = 6
  2. 遍历 PID 1 的页表 (MMU.get_page_entries(1))
     ├─ AllocFrame → 新物理帧
     ├─ MapPage(6, vaddr, new_frame) → 映射到子进程
     └─ 逐字节复制: mmu.read_u8(1, va) → mmu.write_u8(6, va, b)
  3. 克隆 CPU 状态: child_cpu.set_pc(st.pc), child_cpu.set_sp(st.sp)
  4. child_cpu.write_register(R0, 0)  // fork 返回值: 子进程 R0=0
  5. 创建 PCB, state = Ready
  6. 不加入调度队列 (exec 会负责)
  7. 返回 child_pid = 6
```

此时 PID 6 是 PID 1 的完整副本（内存、CPU 寄存器），但未进入调度队列。

### 3.3 exec — 加载程序

```
exec_impl(pid=6, executable="cat", args=["/programs/mkdir.asm"]):
  1. 验证 PID 6 存在
  2. 加载 cat 程序: load_program("cat")
     → 优先找 programs/cat.asm → 找到! 汇编 → 机器码
     → 回退 gen_builtin_program("cat")  (如果 .asm 不存在)

  cat.asm 汇编:
    MOV R0, #10    ; 0x01 0x00 0x01 0x00 0x0A 0x00 0x00 0x00
    INT 0x80       ; 0x80 0x00 0x00 0x00 0x80 0x00 0x00 0x00
    MOV R0, #12    ; ...
    MOV R2, #16
    INT 0x80
    MOV R0, #11
    INT 0x80
    HALT           ; MOV R0,#0 + INT 0x80

  3. UnmapPage 旧页 (PID 6 从 PID 1 克隆来的页)
  4. AllocFrame → MapPage(6, 0x0000, new_frame)  // 程序代码
  5. write_slice_virt(6, 0x0000, &code)          // 把机器码写入内存
  6. write_slice_virt(6, 0x0100, "/programs/mkdir.asm")  // 把路径写入 0x100
  7. 重置 CPU: set_pc(0), set_sp(0xFFFF), halted = false
  8. PCB 更新: name="cat", state=Ready
  9. handle_schedule(6, 1)  → 加入调度队列
```

此时 PID 6 的内存布局:
```
0x0000 - 0x0037: cat.asm 的机器码 (8 字节/指令 × 7 条 = 56 字节)
  +0x00: MOV R0, #10    (0x01 0x00 0x01 0x00 0x0A 0x00 0x00 0x00)
  +0x08: INT 0x80       (0x80 0x00 0x00 0x00 0x80 0x00 0x00 0x00)
  +0x10: MOV R0, #12    (0x01 0x00 0x01 0x00 0x0C 0x00 0x00 0x00)
  +0x18: MOV R2, #16    (0x01 0x02 0x01 0x00 0x10 0x00 0x00 0x00)
  +0x20: INT 0x80       (0x80 ...)
  +0x28: MOV R0, #11
  +0x30: INT 0x80
  +0x38: HALT (MOV R0,#0 + INT 0x80, 16 字节)

0x0100 - 0x0115: "/programs/mkdir.asm" (路径参数, 以 \0 结尾)
```

### 3.4 CPU 执行 — 定时器驱动

```
handle_timer_interrupt:
  scheduler.schedule() → Run { pid: 6, tid: 1 }

  cpu.step() × 3 条指令:

  Step 0: PC=0x00
    fetch_instruction() → mmu.read_u8(6, 0x00) → 0x01 (MOV)
    → 继续读 7 字节 → 完整指令: MOV R0, #10
    execute: registers[0] = 10
    PC += 8 → PC=0x08

  Step 1: PC=0x08
    fetch_instruction() → 0x80 (INT)
    execute: handle_software_interrupt(0x80)
      → cpu.syscall_pending = true
      → cpu.syscall_regs = [10, 0, 0, 0]  // R0=10 (open)
    PC += 8 → PC=0x10

    定时器中断循环:
      if cpu.syscall_pending:
        handle_file_syscall(cpu, r0=10, r1=0, r2=0)
          → 进入系统调用处理 (见 3.5)

  Step 2: PC=0x10
    fetch_instruction() → 0x01 (MOV R0, #12)
    execute: registers[0] = 12
    PC += 8 → PC=0x18
```

### 3.5 系统调用处理 — handle_file_syscall

```
handle_file_syscall(cpu, r0=10, r1=0, r2=0):
  pid = cpu.pid()  // = 6
  path = read_string_virt(6, 0x100)  // → "/programs/mkdir.asm"
    内部: for i in 0..256 { mmu.read_u8(6, 0x100 + i) }
    读到 \0 停止, 返回 "/programs/mkdir.asm"

  match r0 {
    10 => open:
      bus.send_request(KernelMsg::File(
        FileRequest::Open {
          path: "/programs/mkdir.asm",
          flags: read_only,
        }
      ))
      rx.recv_timeout(100ms)  // 阻塞等 FileService 响应
      fd = resp.fd             // → 3
      cpu.write_register(R1, 3)  // R1 = 3

    12 => read:
      fd = r1 = 3
      // 循环读取直到 EOF:
      loop {
        bus.send_request(KernelMsg::File(
          FileRequest::Read {
            fd: 3,
            offset: offset,    // 0, 256, 512, ...
            size: 256,
          }
        ))
        resp = rx.recv_timeout(50ms)
        data = resp.bytes
        if data.is_empty() { break; }
        print!("{}", String::from_utf8_lossy(&data))
        offset += data.len()
      }

    11 => close:
      bus.send(KernelMsg::File(
        FileRequest::Close { fd: 3 }
      ))
  }
```

### 3.6 消息路由 — Kernel

```
Kernel::route(envelope):
  msg = &envelope.message

  match msg {
    KernelMsg::File(request) => {
      self.file_tx.send(envelope)  // → FileService 的 receiver 通道
    }
    ...
  }
```

### 3.7 FileService — 文件操作

#### 3.7.1 Open

```
handle_open(pid=0, path="/programs/mkdir.asm", flags=read_only):

  1. VFS 路径查找:
     vfs.lookup_path("/programs/mkdir.asm")
       → 从 root (inode 0) 开始
       → root.children["programs"] = 7  → 找到 programs 目录
       → lookup(7).children["mkdir.asm"] = 10  → 找到 mkdir.asm
       → 返回 inode 10 的 VFSNode

  2. 创建 File 对象:
     File::new(inode=10, name="/programs/mkdir.asm", pid=0, mode=read)

  3. 从磁盘加载文件内容:
     vfs_node.blocks = [5]      // 文件占用扇区 5
     file.start_sector = Some(5)
     file.sector_count = 1
     file.metadata.size = 96    // VFS JSON 里保存的大小
     file.load_from_disk(&disk)
       → disk.read_sector(5)              // 从 .genshin-disk.img 读 512 字节
       → data.truncate(metadata.size)     // 截断到 96 字节
       → file.data = "; mkdir.asm — create directory...MOV R0, #14..."

  4. 分配 fd:
     fd_manager.get_table(0).allocate()
       → fd = 3
       → table[3] = OpenFile { fd: 3, owner: 0, file: Arc<Mutex<File>> }

  5. 响应:
     respond_success(ResponseData::Fd(3))  // 通过 response_channel 返回
```

#### 3.7.2 Read

```
handle_read(pid=0, fd=3, size=256):

  1. 查找 fd:
     fd_manager.get(0, 3) → OpenFile { file: Arc<Mutex<File>> }

  2. 读数据:
     open_file.read(256) → file.read(256)
       → data = file.data[position..position+256]
       → position += 256
       → 返回 256 字节 (或到文件末尾)

  3. 响应:
     respond_success(ResponseData::Bytes(read_data))
```

#### 3.7.3 Write (以 write hello.txt world 为例)

```
handle_write(pid=0, fd=3, data="world"):

  1. open_file.write("world")
     → file.data = "world"    // 写入内存缓存
     → file.dirty = true

  2. 同步到磁盘:
     file.sync_to_disk(&disk):
       → disk.allocate_sectors(1)  // 分配扇区 6
       → disk.write_sector(6, pad_to_512("world"))  // 写入 .genshin-disk.img
       → file.start_sector = Some(6)
       → file.sector_count = 1
       → file.dirty = false

  3. 更新 VFS 元数据:
     vfs_node.size = 5                    // 5 字节
     vfs_node.blocks = [6]                // 记录扇区号

  4. 持久化:
     VFS 自动保存到 .genshin-vfs.json:
       [..., [10, "File", "hello.txt", 0, 5, {}, [6]], ...]
                                                ↑
                                          blocks 字段
```

#### 3.7.4 Close

```
handle_close(pid=0, fd=3):

  1. fd_manager.get_table(0).release(3)
     → 从进程 fd 表中移除

  2. 如果 File 对象的引用计数降为 0:
     → 从 open_files 缓存中移除
```

## 4. VirtualDisk — 物理磁盘

```
.genshin-disk.img (1MB 真实文件)
┌────────────────────────────────────────┐
│ Sector 0: 分配位图                     │
│   64-bit words, 每位标记一个扇区状态    │
│   1=已用, 0=空闲                       │
│   扇区 0-3 保留 (位图+超级块)          │
├────────────────────────────────────────┤
│ Sector 1-3: 保留                       │
├────────────────────────────────────────┤
│ Sector 4: hello.txt 的文件数据         │
│   "world\0\0\0...\0" (512 bytes)      │
├────────────────────────────────────────┤
│ Sector 5: mkdir.asm 的文件数据         │
│   "; mkdir.asm — create..." (96 bytes) │
├────────────────────────────────────────┤
│ Sector 6+: 空闲                        │
└────────────────────────────────────────┘

读操作:
  disk.read_sector(5)
    → file.seek(5 * 512)
    → file.read_exact(512 bytes)
    → 返回 [0x3B, 0x20, 0x6D, ...]

写操作:
  disk.write_sector(6, buf)
    → file.seek(6 * 512)
    → file.write_all(buf)
    → file.flush()  // 立即同步到磁盘
```

## 5. 持久化机制

```
启动时:
  1. FileService::new()
     → VirtualFileSystem::load_from_file(".genshin-vfs.json")
       → 反序列化 JSON → 重建所有 VFSNode

  2. 如果文件不存在 (首次启动):
     → VirtualFileSystem::new()
       → 创建 root (inode 0) + 标准目录 (bin, home, tmp, etc, var, examples, programs)
       → 在 programs 下创建占位 .asm 文件

  3. import_host_files()
     → 遍历 host 的 programs/ 目录
     → 读取每个 .asm 的源码内容
     → 写入选中的 VFS 文件 → sync_to_disk
     → 这样 cat programs/ls.asm 能读到真实的汇编源码

运行时:
  每次文件修改后:
    FileService::run() 主循环
      → handle_envelope() 处理请求
      → vfs.save_to_file(".genshin-vfs.json")  // 自动保存

    handle_write() 写文件:
      → file.sync_to_disk(&disk)  // 同步到磁盘镜像
      → 更新 vfs_node.blocks       // 记录扇区号
```

## 6. 系统调用速查

| R0 | 操作 | R1 | R2 | 说明 |
|----|------|----|----|------|
| 0 | halt | - | - | 停止进程 (PID 1 除外) |
| 10 | open | flags (0=读,1=创建) | - | 返回 fd 写入 R1 |
| 11 | close | fd | - | 关闭文件 |
| 12 | read | fd | 最大字节 | 循环读取直到 EOF, 打印 |
| 13 | write | fd | 字节数 | 从 0x200 读数据写入文件 |
| 14 | mkdir | - | - | 从 0x100 读路径, 创建目录 |
| 16 | unlink | - | - | 从 0x100 读路径, 删除文件 |
| 17 | stat | - | - | 从 0x100 读路径, 打印信息 |
| 18 | listdir | - | - | 从 0x100 读路径, 列出目录 |
| 100 | fork | - | - | 克隆当前进程, 父 R0=child_pid, 子 R0=0 |
| 101 | exec | - | - | 从 0x100 读程序名, 替换当前进程 |
| 102 | tree | - | - | 从 0x100 读路径, 递归打印目录树 |
