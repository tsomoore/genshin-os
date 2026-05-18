// CLI Shell for Genshin-OS
//
// All file operations go through FileService via the message bus.
// No local VFS — single source of truth.

pub mod parser;
pub mod builtins;
use crate::messaging::{MessageBus, KernelMsg, Pid, Response, ResponseData, ProcessRequest};
use crate::hardware::Timer;
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
    timer: Arc<Timer>,
}

impl Shell {
    pub fn new(bus: Arc<dyn MessageBus>, timer: Arc<Timer>) -> Self {
        let context = UIContext::new(bus);
        Self {
            context,
            config: ShellConfig::default(),
            parser: ShellParser::new(),
            builtins: BuiltinCommand::new(),
            cwd: "/".to_string(),
            running: false,
            timer,
        }
    }

    pub fn with_config(bus: Arc<dyn MessageBus>, config: ShellConfig, timer: Arc<Timer>) -> Self {
        let context = UIContext::new(bus);
        Self {
            context,
            config,
            parser: ShellParser::new(),
            builtins: BuiltinCommand::new(),
            cwd: "/".to_string(),
            running: false,
            timer,
        }
    }

    /// Start the interactive shell
    pub fn run_interactive(&mut self) {
        self.running = true;

        // No startup processes — both CPUs free for user demos
        
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

    /// fork + exec: start a background process (no wait)
    fn fork_exec_detach(&self, prog: &str, args: &[&str]) -> Result<u64, String> {
        let fork_msg = KernelMsg::Process(ProcessRequest::ForkProcess { parent_pid: 1 });
        let child_pid = match self.send_and_wait(fork_msg) {
            Ok(r) => if let Some(ResponseData::Pid(p)) = r.data() { *p } else { return Err("fork failed".into()); },
            Err(e) => return Err(e),
        };
        let a: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let exec_msg = KernelMsg::Process(ProcessRequest::ExecProcess {
            pid: child_pid, executable: prog.into(), args: a, path: None,
        });
        self.send_and_wait(exec_msg)?;
        Ok(child_pid)
    }

    /// fork + exec + wait: the Unix way (foreground)
    fn fork_exec_wait(&self, prog: &str, args: &[&str]) -> Result<(), String> {
        let child_pid = self.fork_exec_detach(prog, args)?;
        let wait_msg = KernelMsg::Process(ProcessRequest::WaitChild { pid: 1, child_pid: Some(child_pid) });
        self.send_and_wait(wait_msg)?;
        Ok(())
    }

    /// Send a request via the message bus and wait for the response
    /// Mini debugger: step through a program instruction by instruction
    fn run_minigdb(&self, prog: &str) -> Result<(), String> {
        use crate::hardware::{PhysicalMemory, MMU, VirtualCPU, PageFlags};
        use crate::messaging::LockedBus;
        use std::io::{BufRead, Write};

        // Load and assemble the program
        let path = format!("programs/{}", prog);
        let asm_path = if prog.ends_with(".asm") { path.clone() } else { format!("{}.asm", path) };
        let asm_code = std::fs::read_to_string(&asm_path)
            .map_err(|e| format!("Cannot read {}: {}", asm_path, e))?;

        // Assemble
        let code = crate::services::process::assembler::assemble(&asm_code)
            .map_err(|e| format!("Assemble error: {}", e))?;

        // Setup hardware
        let mem = PhysicalMemory::new(64 * 1024);
        let mmu = Arc::new(MMU::new(mem.clone(), 4096));
        let bus: Arc<dyn MessageBus> = Arc::new(LockedBus::new());
        let mut cpu = VirtualCPU::new(mmu.clone(), bus, 0);

        // Map and write program
        mmu.map_page(0, 0, 0, PageFlags { present: true, writable: true, user_accessible: true }).ok();
        for (i, &b) in code.iter().enumerate() {
            mmu.write_u8(0, i as u64, b).ok();
        }
        cpu.set_pc(0);
        cpu.set_sp(0xFFFF);

        println!("minigdb: loaded {} ({} bytes, {} instrs)", prog, code.len(), code.len()/8);
        println!("Commands: s(step) r(regs) c(continue) q(quit)");
        print!("(gdb) "); std::io::stdout().flush().ok();

        let stdin = std::io::stdin();
        let mut lines = stdin.lock().lines();
        let mut running = true;
        let mut instr_count = 0;

        while running {
            let line = match lines.next() {
                Some(Ok(l)) => l.trim().to_string(),
                _ => break,
            };

            match line.as_str() {
                "s" | "step" | "" => {
                    if cpu.is_halted() {
                        println!("[HALTED]");
                        break;
                    }
                    let pc_before = cpu.pc();
                    match cpu.step() {
                        Ok(()) => {
                            instr_count += 1;
                            let st = cpu.dump_state();
                            let inst_bytes = &code[pc_before as usize..std::cmp::min(pc_before as usize + 8, code.len())];
                            println!("  #{:<3} 0x{:04x}: {:02x?}  | R0={:<5} R1={:<5} R2={:<5} R3={:<5} | PC=0x{:04x} SP=0x{:04x} Z={} S={}",
                                instr_count, pc_before, inst_bytes,
                                st.registers[0] as i64, st.registers[1] as i64,
                                st.registers[2] as i64, st.registers[3] as i64,
                                st.pc, st.sp,
                                st.flags.zero as u8, st.flags.sign as u8);
                            if cpu.is_halted() {
                                println!("[HALTED after {} instructions]", instr_count);
                            }
                        }
                        Err(e) => println!("Error: {:?}", e),
                    }
                }
                "r" | "regs" => {
                    let st = cpu.dump_state();
                    println!("  R0=0x{:016x} ({})", st.registers[0], st.registers[0] as i64);
                    println!("  R1=0x{:016x} ({})", st.registers[1], st.registers[1] as i64);
                    println!("  R2=0x{:016x} ({})", st.registers[2], st.registers[2] as i64);
                    println!("  R3=0x{:016x} ({})", st.registers[3], st.registers[3] as i64);
                    println!("  PC=0x{:04x}  SP=0x{:04x}", st.pc, st.sp);
                    println!("  Z={} S={} O={} C={}",
                        st.flags.zero as u8, st.flags.sign as u8,
                        st.flags.overflow as u8, st.flags.carry as u8);
                }
                "c" | "continue" => {
                    while !cpu.is_halted() {
                        if cpu.step().is_err() { cpu.halt(); break; }
                        instr_count += 1;
                    }
                    println!("Ran to HALT ({} total instructions)", instr_count);
                }
                "q" | "quit" | "exit" => {
                    running = false;
                }
                _ if !line.is_empty() => {
                    println!("Unknown command: {} (s=step, r=regs, c=continue, q=quit)", line);
                }
                _ => {} // Rust 2024: empty string already covered above
            }
            if running && !cpu.is_halted() {
                print!("(gdb) "); std::io::stdout().flush().ok();
            }
        }
        Ok(())
    }

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
            "uptime" => {
                let ticks = self.timer.tick_count();
                let ms = (ticks as f64 * 10.0) / 1000.0;
                println!("+{} ticks | {:.2}s", ticks, ms);
                Ok(())
            }
            "pmon" | "htop" => {
                let bus = self.context.bus.clone();
                let timer = self.timer.clone();
                println!("Launching process monitor...");
                std::thread::sleep(std::time::Duration::from_millis(300));
                if let Err(e) = crate::ui::monitor::run_monitor(bus, timer) {
                    eprintln!("Monitor error: {}", e);
                }
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
                    Ok(resp) if resp.is_success() => { self.cwd = target; Ok(()) }
                    Ok(_) => Err(format!("cd: {}: Not a directory", path)),
                    Err(_) => Err(format!("cd: {}: No such file or directory", path)),
                }
            }
            "ls" => {
                let p = if let Some(arg) = command.args.get(0) { self.resolve_path(arg) } else { self.cwd.clone() };
                self.fork_exec_wait("ls", &[&p])
            }
            "tree" => {
                let p = if let Some(arg) = command.args.get(0) { self.resolve_path(arg) } else { self.cwd.clone() };
                self.fork_exec_wait("tree", &[&p])
            },
            "mkdir" => {
                let p = self.resolve_path(command.args.get(0).ok_or("mkdir: missing operand")?);
                self.fork_exec_wait("mkdir", &[&p])
            }
            "touch" => {
                let p = self.resolve_path(command.args.get(0).ok_or("touch: missing operand")?);
                self.fork_exec_wait("touch", &[&p])
            }
            "cat" => {
                let p = self.resolve_path(command.args.get(0).ok_or("cat: missing operand")?);
                self.fork_exec_wait("cat", &[&p])
            }
            "rm" => {
                let p = self.resolve_path(command.args.get(0).ok_or("rm: missing operand")?);
                self.fork_exec_wait("rm", &[&p])
            }
            "stat" => {
                let p = self.resolve_path(command.args.get(0).ok_or("stat: missing operand")?);
                self.fork_exec_wait("stat", &[&p])
            }
            "write" => {
                let p = self.resolve_path(command.args.get(0).ok_or("write: missing operand")?);
                let content = if command.args.len() > 1 { command.args[1..].join(" ") } else { String::new() };
                self.fork_exec_wait("write", &[&p, &content])
            }
            "dual" => {
                let prog = command.args.get(0).map(|s| s.as_str()).unwrap_or("busy");
                let pid1 = self.fork_exec_detach(prog, &[])?;
                let pid2 = self.fork_exec_detach(prog, &[])?;
                println!("dual: {} PID {} + PID {}", prog, pid1, pid2);
                Ok(())
            }
            "fork" => {
                let pid: u64 = command.args.get(0).and_then(|s| s.parse().ok()).unwrap_or(1);
                let msg = KernelMsg::Process(ProcessRequest::ExecProcess { pid, executable: "fork".into(), args: vec![], path: None });
                let _ = self.send_and_wait(msg)?;
                println!("fork: PID {} now runs fork program via CPU", pid);
                Ok(())
            }

            "fork" => {
                let pid: u64 = command.args.get(0).and_then(|s| s.parse().ok()).unwrap_or(1);
                // Load fork program into PID, let CPU execute it → INT 0x80 → handle_file_syscall(100) → fork_impl
                let msg = KernelMsg::Process(ProcessRequest::ExecProcess { pid, executable: "fork".into(), args: vec![], path: None });
                let _ = self.send_and_wait(msg)?;
                println!("fork: PID {} now runs fork program via CPU", pid);
                Ok(())
            }
            "fork2" => {
                // Legacy: direct bus request (bypasses CPU)
                let pid: u64 = command.args.get(0).and_then(|s| s.parse().ok()).unwrap_or(1);
                let msg = KernelMsg::Process(ProcessRequest::ForkProcess { parent_pid: pid });
                match self.send_and_wait(msg) {
                    Ok(r) => {
                        if !r.is_error() { if let Some(ResponseData::Pid(c)) = r.data() { println!("fork: child PID = {}", c); } }
                        else { eprintln!("fork: {}", r.service_error().unwrap()); }
                    }
                    Err(e) => eprintln!("fork: {}", e),
                }
                Ok(())
            }
            "exec" => {
                // exec <prog>        → PID 1, program <prog>
                // exec <pid> <prog>  → explicit PID
                let (pid, prog): (u64, String) = match command.args.len() {
                    0 => return Err("exec: missing program".into()),
                    1 => (1, command.args[0].clone()),
                    _ => (
                        command.args[0].parse().unwrap_or(1),
                        command.args[1].clone(),
                    ),
                };
                let a: Vec<String> = if command.args.len() > 2 { command.args[2..].to_vec() } else { vec![] };
                let msg = KernelMsg::Process(ProcessRequest::ExecProcess { pid, executable: prog.clone(), args: a, path: None });
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
                let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                let pid = self.fork_exec_detach(prog, &args_ref)?;
                println!("run: {} (PID {})", prog, pid);
                Ok(())
            }
            "minigdb" => {
                let prog = command.args.get(0).ok_or("minigdb: missing program")?;
                self.run_minigdb(prog)
            }
            "ps"|"pstree" => {
                let msg = KernelMsg::Process(crate::messaging::ProcessRequest::ListProcesses);
                match self.send_and_wait(msg) {
                    Ok(resp) => {
                        if let Some(ResponseData::String(s)) = resp.data() {
                            print!("{}", s);
                        }
                    }
                    Err(e) => eprintln!("pstree: {}", e),
                }
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
        println!("  Monitoring:");
        println!("    pmon | htop            Live TUI process/memory/disk monitor (q to quit)");
        println!("    uptime                 Show hardware timer ticks");
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
