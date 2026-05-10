// CLI Shell for Genshin-OS
//
// All file operations go through FileService via the message bus.
// No local VFS — single source of truth.

pub mod parser;
pub mod builtins;

use crate::messaging::{MessageBus, KernelMsg, Pid, Response, ResponseData, ProcessRequest};
use crate::ui::UIContext;
use std::sync::Arc;
use std::time::Duration;
use std::path::{Path, PathBuf};
use parser::{Command, ShellParser};
use builtins::BuiltinCommand;

/// Shell configuration
#[derive(Debug, Clone)]
pub struct ShellConfig {
    pub prompt: String,
    pub current_pid: Pid,
    pub echo: bool,
    pub show_welcome: bool,
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            prompt: "genshin-os> ".to_string(),
            current_pid: 1,
            echo: false,
            show_welcome: true,
        }
    }
}

/// Main shell structure
pub struct Shell {
    context: UIContext,
    config: ShellConfig,
    parser: ShellParser,
    builtins: BuiltinCommand,
    cwd: String,
    running: bool,
}

impl Shell {
    pub fn new(bus: Arc<dyn MessageBus>) -> Self {
        let context = UIContext::new(bus);
        Self {
            context,
            config: ShellConfig::default(),
            parser: ShellParser::new(),
            builtins: BuiltinCommand::new(),
            cwd: "/".to_string(),
            running: false,
        }
    }

    pub fn with_config(bus: Arc<dyn MessageBus>, config: ShellConfig) -> Self {
        let context = UIContext::new(bus);
        Self {
            context,
            config,
            parser: ShellParser::new(),
            builtins: BuiltinCommand::new(),
            cwd: "/".to_string(),
            running: false,
        }
    }

    /// Start the interactive shell
    pub fn run_interactive(&mut self) {
        self.running = true;

        if self.config.show_welcome {
            self.print_welcome();
        }

        while self.running {
            let input = self.read_line();
            if input.is_empty() {
                println!();
                break;
            }
            if input.trim().is_empty() {
                continue;
            }
            if self.config.echo {
                println!("{}", input);
            }
            if let Err(err) = self.execute_line(&input) {
                eprintln!("Error: {}", err);
            }
        }
    }

    /// Execute a single command line
    pub fn execute_line(&mut self, line: &str) -> Result<(), String> {
        let command = self.parser.parse(line)
            .ok_or_else(|| format!("Failed to parse command: {}", line))?;
        self.execute_command(&command)
    }

    /// Resolve a path (relative to cwd or absolute)
    fn resolve_path(&self, path: &str) -> String {
        let p = Path::new(path);
        if p.is_absolute() {
            path.to_string()
        } else {
            let mut base = PathBuf::from(&self.cwd);
            for comp in p.components() {
                match comp {
                    std::path::Component::ParentDir => { base.pop(); }
                    std::path::Component::Normal(c) => { base.push(c); }
                    _ => {}
                }
            }
            base.to_string_lossy().to_string()
        }
    }

    /// Send a request via the message bus and wait for the response
    fn send_and_wait(&self, msg: KernelMsg) -> Result<Response, String> {
        let rx = self.context.send_request(msg)
            .map_err(|e| format!("Bus error: {}", e))?;
        rx.recv_timeout(Duration::from_secs(3))
            .map_err(|e| format!("No response from service: {}", e))
    }


    /// Execute a parsed command
    fn execute_command(&mut self, command: &Command) -> Result<(), String> {
        match command.name.as_str() {
            "exit" | "quit" => {
                self.running = false;
                println!("Goodbye!");
                Ok(())
            }
            "help" => {
                self.show_help();
                Ok(())
            }
            "verbose" => {
                let on = command.args.first().map(|s| s.as_str()).unwrap_or("on");
                crate::verbose::set_verbose(on == "on");
                println!("verbose: {}", if on == "on" { "on" } else { "off" });
                Ok(())
            }
            "pwd" => {
                println!("{}", self.cwd);
                Ok(())
            }
            "cd" => {
                let path = command.args.get(0).map(|s| s.as_str()).unwrap_or("/");
                let target = self.resolve_path(path);
                let msg = KernelMsg::File(crate::messaging::FileRequest::Stat {
                    path: target.clone(),
                });
                match self.send_and_wait(msg) {
                    Ok(_) => { self.cwd = target; Ok(()) }
                    Err(_) => Err(format!("cd: {}: No such directory", path)),
                }
            }
            "ls" => {
                let path = command.args.first().map(|s| s.as_str()).unwrap_or(".");
                let target = self.resolve_path(path);
                let msg = KernelMsg::Process(ProcessRequest::Spawn { program: "ls".into(), params: target.as_bytes().to_vec() });
                let _ = self.send_and_wait(msg)?;
                Ok(())
            }
            "tree" => {
                let path = command.args.first().map(|s| s.as_str()).unwrap_or("/");
                let target = self.resolve_path(path);
                let root = PathBuf::from(&target);
                println!("{}", root.display());
                self.print_tree_recursive(&root, "");
                Ok(())
            }
            "cd" => {
                let path = command.args.get(0).map(|s| s.as_str()).unwrap_or("/");
                let target = self.resolve_path(path);
                let msg = KernelMsg::File(crate::messaging::FileRequest::Stat { path: target.clone() });
                match self.send_and_wait(msg) {
                    Ok(_) => { self.cwd = target; Ok(()) }
                    Err(_) => Err(format!("cd: {}: No such directory", path)),
                }
            }
            "mkdir" => {
                let path = command.args.get(0).ok_or_else(|| "mkdir: missing operand".to_string())?;
                let target = self.resolve_path(path);
                let msg = KernelMsg::Process(ProcessRequest::Spawn { program: "mkdir".into(), params: target.as_bytes().to_vec() });
                let _ = self.send_and_wait(msg)?;
                Ok(())
            }
            "touch" => {
                let path = command.args.get(0).ok_or_else(|| "touch: missing operand".to_string())?;
                let target = self.resolve_path(path);
                let msg = KernelMsg::Process(ProcessRequest::Spawn { program: "touch".into(), params: target.as_bytes().to_vec() });
                let _ = self.send_and_wait(msg)?;
                Ok(())
            }
            "cat" => {
                let path = command.args.get(0).ok_or_else(|| "cat: missing operand".to_string())?;
                let target = self.resolve_path(path);
                let msg = KernelMsg::Process(ProcessRequest::Spawn { program: "cat".into(), params: target.as_bytes().to_vec() });
                let _ = self.send_and_wait(msg)?;
                Ok(())
            }
            "write" => {
                let path = command.args.get(0).ok_or_else(|| "write: missing operand".to_string())?;
                let content = if command.args.len() > 1 { command.args[1..].join(" ") } else { String::new() };
                let target = self.resolve_path(path);
                let params = { let mut p = target.as_bytes().to_vec(); p.push(0); p.extend_from_slice(content.as_bytes()); p };
                let msg = KernelMsg::Process(ProcessRequest::Spawn { program: "write".into(), params });
                let _ = self.send_and_wait(msg)?;
                Ok(())
            }
            "rm" => {
                let path = command.args.get(0).ok_or_else(|| "rm: missing operand".to_string())?;
                let target = self.resolve_path(path);
                let msg = KernelMsg::Process(ProcessRequest::Spawn { program: "rm".into(), params: target.as_bytes().to_vec() });
                let _ = self.send_and_wait(msg)?;
                Ok(())
            }
            "stat" => {
                let path = command.args.get(0).ok_or_else(|| "stat: missing operand".to_string())?;
                let target = self.resolve_path(path);
                let msg = KernelMsg::Process(ProcessRequest::Spawn { program: "stat".into(), params: target.as_bytes().to_vec() });
                let _ = self.send_and_wait(msg)?;
                println!("stat: '{}'", path);
                Ok(())
            }
            "dual" => {
                let msg = KernelMsg::Process(ProcessRequest::Spawn { program: "dual".into(), params: vec![] });
                let _ = self.send_and_wait(msg)?;
                Ok(())
            }
            "fork" => {
                let pid: u64 = command.args.get(0).and_then(|s| s.parse().ok()).unwrap_or(1);
                let msg = KernelMsg::Process(ProcessRequest::ForkProcess { parent_pid: pid });
                match self.send_and_wait(msg) {
                    Ok(r) => {
                        if r.is_error() { eprintln!("fork: {}", r.service_error().unwrap()); }
                        else if let Some(ResponseData::Pid(c)) = r.data() { println!("fork: child PID = {}", c); }
                    }
                    Err(e) => eprintln!("fork: {}", e),
                }
                Ok(())
            }
            "exec" => {
                let pid: u64 = command.args.get(0).and_then(|s| s.parse().ok()).unwrap_or(1);
                let prog = command.args.get(1).cloned().unwrap_or_default();
                let a: Vec<String> = command.args.iter().skip(2).cloned().collect();
                let msg = KernelMsg::Process(ProcessRequest::ExecProcess { pid, executable: prog.clone(), args: a });
                let _ = self.send_and_wait(msg)?;
                println!("exec: PID {} now '{}'", pid, prog);
                Ok(())
            }
            "copy" => {
                let text = command.args.join(" ");
                let msg = KernelMsg::Device(crate::messaging::DeviceRequest::ClipboardSet { data: text.as_bytes().to_vec() });
                self.context.send(msg);
                println!("Copied: {}", text);
                Ok(())
            }
            "paste" => {
                let msg = KernelMsg::Device(crate::messaging::DeviceRequest::ClipboardGet { max_size: 4096 });
                match self.send_and_wait(msg) {
                    Ok(resp) => { if let Some(ResponseData::Bytes(data)) = resp.data() { println!("{}", String::from_utf8_lossy(data)); } }
                    Err(e) => eprintln!("paste: {}", e),
                }
                Ok(())
            }
            "disk" => {
                let msg = KernelMsg::File(crate::messaging::FileRequest::DiskInfo);
                match self.send_and_wait(msg) {
                    Ok(resp) => {
                        if let Some(ResponseData::DiskStats { total_sectors, used_sectors, total_bytes }) = resp.data() {
                            let u = *used_sectors; let t = *total_sectors as f64;
                            let pct = if t > 0.0 { (u as f64 / t) * 100.0 } else { 0.0 };
                            println!("Disk: {}/{} sectors ({:.1}%), {} bytes", u, total_sectors, pct, total_bytes);
                        }
                        Ok(())
                    }
                    Err(e) => Err(format!("disk: {}", e)),
                }
            }
            "cpu" => {
                use crate::hardware::{PhysicalMemory, MMU, VirtualCPU, PageFlags};
                use crate::messaging::LockedBus;
                println!("╔════════════════════════════════════════╗");
                println!("║     VirtualCPU Instruction Demo        ║");
                println!("╚════════════════════════════════════════╝");
                let mem = PhysicalMemory::new(64 * 1024);
                let mmu = Arc::new(MMU::new(mem.clone(), 4096));
                let bus: Arc<dyn MessageBus> = Arc::new(LockedBus::new());
                let flags = PageFlags { present: true, writable: true, user_accessible: true };
                mmu.map_page(0, 0x0000, 0x0000, flags).map_err(|e| format!("MMU: {}", e))?;
                let program: Vec<u8> = vec![
                    0x01,0x00,0x01, 0x00, 0x0A,0x00,0x00,0x00,
                    0x01,0x01,0x01, 0x00, 0x14,0x00,0x00,0x00,
                    0x02,0x02,0x00, 0x00, 0x00,0x00,0x00,0x00,
                    0x02,0x02,0x00, 0x00, 0x01,0x00,0x00,0x00,
                    0x03,0x03,0x00, 0x00, 0x02,0x00,0x00,0x00,
                    0xFF,0x00,0x00, 0x00, 0x00,0x00,0x00,0x00,
                ];
                mem.write_slice(0x100, &program).map_err(|e| format!("Mem: {}", e))?;
                let mut cpu = VirtualCPU::new(mmu, bus, 0);
                cpu.set_pc(0x100);
                println!("  PC   │  R0     R1     R2     R3   │Z S O C│ #");
                println!("───────┼──────────────────────────────┼───────┼───");
                while !cpu.is_halted() {
                    let before = cpu.dump_state();
                    match cpu.step() {
                        Ok(()) => {
                            let after = cpu.dump_state();
                            print!("{:#06x}→{:#06x}│", before.pc, after.pc);
                            for i in 0..4 { print!("{:<7}", after.registers[i] as i64); }
                            print!("│{} {} {} {}│",
                                after.flags.zero as u8, after.flags.sign as u8,
                                after.flags.overflow as u8, after.flags.carry as u8);
                            println!(" {}", after.instruction_count);
                        }
                        Err(e) => { println!("     ! {}", e); break; }
                    }
                }
                println!("───────┴──────────────────────────────┴───────┴───");
                println!("✓ {} instructions, halted: {}", cpu.dump_state().instruction_count, cpu.is_halted());
                Ok(())
            }
            "run" => {
                let prog = command.args.get(0).ok_or_else(|| "run: missing program name")?;
                let args: Vec<String> = command.args.iter().skip(1).cloned().collect();
                let msg = KernelMsg::Syscall(crate::messaging::Syscall::CreateProcess {
                    executable: prog.clone(),
                    args,
                });
                let _ = self.send_and_wait(msg)?;
                println!("run: started '{}'", prog);
                Ok(())
            }
            "ps" => {
                let msg = KernelMsg::Process(crate::messaging::ProcessRequest::ListProcesses);
                self.context.send(msg);
                Ok(())
            }
            _ => {
                if let Some(result) = self.builtins.execute(&self.context, command) {
                    result
                } else {
                    self.execute_kernel_command(command)
                }
            }
        }
    }

    /// Convert shell command to kernel message (legacy support)
    fn execute_kernel_command(&self, command: &Command) -> Result<(), String> {
        let msg = self.command_to_message(command)
            .ok_or_else(|| format!("Unknown command: {}", command.name))?;
        self.context.send(msg);
        Ok(())
    }

    fn command_to_message(&self, command: &Command) -> Option<KernelMsg> {
        use crate::messaging::{ProcessRequest, Syscall, SignalType, IPCMessage, OpenFlags, FileRequest};
        match command.name.as_str() {
            "run" => {
                let executable = command.args.get(0)?.clone();
                let args: Vec<String> = command.args.iter().skip(1).cloned().collect();
                Some(KernelMsg::Syscall(Syscall::CreateProcess { executable, args }))
            }
            "ps" => Some(KernelMsg::Process(ProcessRequest::ListProcesses)),
            "kill" => {
                let pid: u64 = command.args.get(0)?.parse().ok()?;
                let signal = match command.args.get(1).map(|s| s.as_str()).unwrap_or("term") {
                    "kill" | "sigkill" => SignalType::Kill,
                    "stop" | "sigstop" => SignalType::Stop,
                    _ => SignalType::Terminate,
                };
                Some(KernelMsg::Process(ProcessRequest::Signal { pid, signal }))
            }
            "send" => {
                let from_pid: u64 = command.args.get(0)?.parse().ok()?;
                let to_pid: u64 = command.args.get(1)?.parse().ok()?;
                let text = command.args.get(2)?.clone();
                Some(KernelMsg::Process(ProcessRequest::SendMessage {
                    from_pid, to_pid, msg: IPCMessage::Text { data: text },
                }))
            }
            "mount" => {
                let device_id = command.args.get(0)?.parse::<u32>().unwrap_or(0);
                let mount_point = command.args.get(1)?.clone();
                let fs_type = crate::messaging::FileSystemType::SimpleFS;
                Some(KernelMsg::File(FileRequest::Mount { device_id, mount_point, fs_type }))
            }
            _ => None,
        }
    }

    fn read_line(&self) -> String {
        use std::io::{self, Write};
        print!("genshin-os:{}> ", self.cwd);
        io::stdout().flush().unwrap_or(());
        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(0) => String::new(),
            Ok(_) => input.trim_end().to_string(),
            Err(_) => String::new(),
        }
    }

    fn print_welcome(&self) {
        println!("╔════════════════════════════════════════════════════════════╗");
        println!("║           Welcome to Genshin-OS Microkernel Shell          ║");
        println!("║                     Version 0.2.0                          ║");
        println!("╠════════════════════════════════════════════════════════════╣");
        println!("║  Type 'help' for available commands or 'exit' to quit.     ║");
        println!("╚════════════════════════════════════════════════════════════╝");
        println!();
    }

    fn show_help(&self) {
        println!("Available commands:");
        println!();
        println!("  File System:");
        println!("    ls [path]              List directory contents");
        println!("    cd <path>              Change directory");
        println!("    pwd                    Print working directory");
        println!("    mkdir <dir>            Create a directory");
        println!("    touch <file>           Create a file");
        println!("    cat <file>             Read file contents");
        println!("    write <file> <text>    Write text to file");
        println!("    rm <file>              Delete file");
        println!("    stat <file>            Show file info");
        println!("    disk                   Show disk usage");
        println!("  Process Management:");
        println!("    run <name> [args...]   Run program from programs/<name>.asm");
        println!("    ps                     List all processes");
        println!("    kill <pid> [sig]       Send signal (term/kill/stop)");
        println!("    send <from> <to> <msg> Send IPC message");
        println!("  Hardware:");
        println!("    cpu                    CPU instruction demo");
        println!("    mem                    PhysicalMemory hex dump");
        println!("  System:");
        println!("    echo <text>            Print text to stdout");
        println!("    clear                  Clear the screen");
        println!("    help                   Show this help message");
        println!("    exit                   Exit the shell");
        println!();
    }

    /// Recursively print directory tree via FileService
    fn print_tree_recursive(&self, path: &Path, prefix: &str) {
        let path_str = path.to_string_lossy().to_string();
        let msg = KernelMsg::File(crate::messaging::FileRequest::ListDir { path: path_str });
        if let Ok(resp) = self.send_and_wait(msg) {
            if let Some(ResponseData::StringList(entries)) = resp.data() {
                let count = entries.len();
                for (i, child) in entries.iter().enumerate() {
                    let is_last = i == count - 1;
                    let connector = if is_last { "└── " } else { "├── " };
                    let child_path = path.join(child);
                    // Check if it's a directory by trying to list it
                    let child_str = child_path.to_string_lossy().to_string();
                    let dir_msg = KernelMsg::File(crate::messaging::FileRequest::ListDir { path: child_str.clone() });
                    if self.send_and_wait(dir_msg).is_ok() {
                        println!("{}{}{}/", prefix, connector, child);
                        let new_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });
                        self.print_tree_recursive(&child_path, &new_prefix);
                    } else {
                        println!("{}{}{}", prefix, connector, child);
                    }
                }
            }
        }
    }
}
