// CLI Shell for genshin-os
//
// Provides an interactive command-line interface for users to interact
// with the kernel through the message bus.

pub mod parser;
pub mod builtins;
pub mod filesystem;

use crate::messaging::{MessageBus, KernelMsg, Pid};
use crate::ui::UIContext;
use std::sync::Arc;
use parser::{Command, ShellParser};
use builtins::BuiltinCommand;
use filesystem::VirtualFileSystem;

/// Shell configuration
#[derive(Debug, Clone)]
pub struct ShellConfig {
    /// Prompt string
    pub prompt: String,
    /// Current process ID (for command execution context)
    pub current_pid: Pid,
    /// Echo commands before execution
    pub echo: bool,
    /// Show welcome message
    pub show_welcome: bool,
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            prompt: "chao-os> ".to_string(),
            current_pid: 1, // Shell runs as PID 1
            echo: false,
            show_welcome: true,
        }
    }
}

/// Main shell structure
pub struct Shell {
    /// UI context
    context: UIContext,
    /// Shell configuration
    config: ShellConfig,
    /// Command parser
    parser: ShellParser,
    /// Built-in commands
    builtins: BuiltinCommand,
    /// Virtual filesystem for shell operations
    filesystem: VirtualFileSystem,
    /// Running state
    running: bool,
}

impl Shell {
    /// Create a new shell
    pub fn new(bus: Arc<dyn MessageBus>) -> Self {
        let context = UIContext::new(bus);
        let config = ShellConfig::default();
        let parser = ShellParser::new();
        let builtins = BuiltinCommand::new();
        let filesystem = VirtualFileSystem::new();

        Self {
            context,
            config,
            parser,
            builtins,
            filesystem,
            running: false,
        }
    }

    /// Create a new shell with custom configuration
    pub fn with_config(bus: Arc<dyn MessageBus>, config: ShellConfig) -> Self {
        let context = UIContext::new(bus);
        let parser = ShellParser::new();
        let builtins = BuiltinCommand::new();
        let filesystem = VirtualFileSystem::new();

        Self {
            context,
            config,
            parser,
            builtins,
            filesystem,
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
            // Display prompt and read input
            let input = self.read_line();

            // Handle EOF (Ctrl+D)
            if input.is_empty() {
                println!(); // Print newline before exit
                break;
            }

            // Skip empty lines (but not EOF)
            if input.trim().is_empty() {
                continue;
            }

            // Echo command if enabled
            if self.config.echo {
                println!("{}", input);
            }

            // Parse and execute command
            if let Err(err) = self.execute_line(&input) {
                eprintln!("Error: {}", err);
            }
        }
    }

    /// Execute a single command line
    pub fn execute_line(&mut self, line: &str) -> Result<(), String> {
        // Parse the command
        let command = self.parser.parse(line)
            .ok_or_else(|| format!("Failed to parse command: {}", line))?;

        // Execute it
        self.execute_command(&command)
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
            "pwd" => {
                // Use real filesystem
                println!("{}", self.filesystem.pwd());
                Ok(())
            }
            "cd" => {
                // Use real filesystem
                let default_path = "/".to_string();
                let path = command.args.get(0).unwrap_or(&default_path);
                self.filesystem.cd(path)
            }
            "ls" => {
                // Use real filesystem
                let path = command.args.first().map(|s| s.as_str());
                match self.filesystem.ls(path) {
                    Ok(entries) => {
                        for entry in entries {
                            println!("{}", entry);
                        }
                        Ok(())
                    }
                    Err(err) => Err(err)
                }
            }
            "mkdir" => {
                // Use real filesystem
                let path = command.args.get(0).ok_or_else(|| "mkdir: missing operand".to_string())?;
                self.filesystem.mkdir(path)
            }
            _ => {
                // Try built-in commands first
                if let Some(result) = self.builtins.execute(&self.context, command) {
                    result
                } else {
                    // Send as kernel message
                    self.execute_kernel_command(command)
                }
            }
        }
    }

    /// Execute a command via the kernel message bus
    fn execute_kernel_command(&self, command: &Command) -> Result<(), String> {
        // Convert command to kernel message
        let msg = self.command_to_message(command)
            .ok_or_else(|| format!("Unknown command: {}", command.name))?;

        // Send to message bus
        self.context.send(msg);
        Ok(())
    }

    /// Convert shell command to kernel message
    fn command_to_message(&self, command: &Command) -> Option<KernelMsg> {
        use crate::messaging::{ProcessRequest, MemoryRequest, FileRequest, DeviceRequest};

        match command.name.as_str() {
            "ps" => Some(KernelMsg::Process(ProcessRequest::ListProcesses)),
            "mount" => {
                let device_str = command.args.get(0)?;
                let mount_point = command.args.get(1)?.clone();
                let fs_type = command.args.get(2)
                    .and_then(|t| Self::parse_fs_type(t))
                    .unwrap_or(crate::messaging::FileSystemType::FAT32);

                // Try to parse device ID from string
                let device_id = device_str.parse::<u32>().unwrap_or(0);

                Some(KernelMsg::File(FileRequest::Mount {
                    device_id,
                    mount_point,
                    fs_type,
                }))
            }
            _ => None,
        }
    }

    /// Parse filesystem type from string
    fn parse_fs_type(s: &str) -> Option<crate::messaging::FileSystemType> {
        match s.to_lowercase().as_str() {
            "fat32" => Some(crate::messaging::FileSystemType::FAT32),
            "ext4" => Some(crate::messaging::FileSystemType::EXT4),
            "simple" | "simplefs" => Some(crate::messaging::FileSystemType::SimpleFS),
            _ => None,
        }
    }

    /// Read a line from stdin
    fn read_line(&self) -> String {
        use std::io::{self, Write};

        print!("{}", self.config.prompt);
        io::stdout().flush().unwrap_or(());

        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(0) => {
                // EOF (Ctrl+D) - return empty string to trigger exit
                String::new()
            }
            Ok(_) => {
                input.trim_end().to_string()
            }
            Err(_) => {
                String::new()
            }
        }
    }

    /// Print welcome message
    fn print_welcome(&self) {
        println!("╔════════════════════════════════════════════════════════════╗");
        println!("║            Welcome to Chao-OS Microkernel Shell            ║");
        println!("║                     Version 0.1.0                           ║");
        println!("╠════════════════════════════════════════════════════════════╣");
        println!("║  Type 'help' for available commands or 'exit' to quit.     ║");
        println!("╚════════════════════════════════════════════════════════════╝");
        println!();
    }

    /// Show help information
    fn show_help(&self) {
        println!("Available commands:");
        println!();
        println!("  File System:");
        println!("    ls [path]   List directory contents");
        println!("    cd <path>   Change directory");
        println!("    pwd         Print working directory");
        println!("    mkdir <dir> Create a directory");
        println!();
        println!("  Process Management:");
        println!("    ps          List all processes");
        println!();
        println!("  System:");
        println!("    echo <text> Print text to stdout");
        println!("    clear       Clear the screen");
        println!("    help        Show this help message");
        println!("    exit        Exit the shell");
        println!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::LockedBus;

    #[test]
    fn test_shell_creation() {
        let bus = Arc::new(LockedBus::new());
        let shell = Shell::new(bus);
        assert_eq!(shell.config.prompt, "chao-os> ".to_string());
    }

    #[test]
    fn test_shell_with_config() {
        let bus = Arc::new(LockedBus::new());
        let config = ShellConfig {
            prompt: "test> ".to_string(),
            ..Default::default()
        };
        let shell = Shell::with_config(bus, config);
        assert_eq!(shell.config.prompt, "test> ".to_string());
    }

    #[test]
    fn test_parse_simple_command() {
        let parser = ShellParser::new();
        let command = parser.parse("ls").unwrap();
        assert_eq!(command.name, "ls");
        assert!(command.args.is_empty());
    }

    #[test]
    fn test_parse_command_with_args() {
        let parser = ShellParser::new();
        let command = parser.parse("mkdir /tmp/test").unwrap();
        assert_eq!(command.name, "mkdir");
        assert_eq!(command.args, vec!["/tmp/test"]);
    }
}
