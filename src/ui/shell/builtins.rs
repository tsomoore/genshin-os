// Built-in commands for the shell
//
// These commands are executed directly by the shell without sending
// messages to the kernel. They are used for shell-internal operations.

use crate::ui::shell::parser::Command;
use crate::ui::UIContext;
use std::collections::HashMap;

/// Built-in command handler
pub struct BuiltinCommand {
    /// Environment variables
    env: HashMap<String, String>,
}

impl BuiltinCommand {
    /// Create a new built-in command handler
    pub fn new() -> Self {
        let mut env = HashMap::new();
        // Set default environment variables
        env.insert("PATH".to_string(), "/bin:/usr/bin".to_string());
        env.insert("HOME".to_string(), "/root".to_string());
        env.insert("USER".to_string(), "root".to_string());
        env.insert("SHELL".to_string(), "/bin/chao-sh".to_string());
        env.insert("TERM".to_string(), "xterm-256color".to_string());

        Self { env }
    }

    /// Execute a built-in command
    ///
    /// Returns Some(result) if the command was handled, None if it should
    /// be passed to the kernel.
    pub fn execute(&mut self, _context: &UIContext, command: &Command) -> Option<Result<(), String>> {
        match command.name.as_str() {
            "echo" => Some(self.cmd_echo(command)),
            "export" => Some(self.cmd_export(command)),
            "unset" => Some(self.cmd_unset(command)),
            "env" => Some(self.cmd_env()),
            "set" => Some(self.cmd_set()),
            "clear" => Some(self.cmd_clear()),
            "history" => Some(self.cmd_history()),
            "alias" => Some(self.cmd_alias(command)),
            "unalias" => Some(self.cmd_unalias(command)),
            "true" => Some(Ok(())),
            "false" => Some(Err("".to_string())),
            ":" => Some(Ok(())), // Null command
            _ => None,
        }
    }

    /// echo: Print arguments to stdout
    fn cmd_echo(&self, command: &Command) -> Result<(), String> {
        let output = command.args.join(" ");
        println!("{}", output);
        Ok(())
    }

    /// export: Set environment variable
    fn cmd_export(&mut self, command: &Command) -> Result<(), String> {
        for arg in &command.args {
            if let Some(eq_pos) = arg.find('=') {
                let key = arg[..eq_pos].to_string();
                let value = arg[eq_pos + 1..].to_string();
                self.env.insert(key, value);
            } else {
                // Just display the variable if no assignment
                if let Some(value) = self.env.get(arg) {
                    println!("export {}=\"{}\"", arg, value);
                } else {
                    return Err(format!("export: {}: undefined variable", arg));
                }
            }
        }
        Ok(())
    }

    /// unset: Unset environment variable
    fn cmd_unset(&mut self, command: &Command) -> Result<(), String> {
        for arg in &command.args {
            if self.env.remove(arg).is_none() {
                return Err(format!("unset: {}: undefined variable", arg));
            }
        }
        Ok(())
    }

    /// env: Print all environment variables
    fn cmd_env(&self) -> Result<(), String> {
        for (key, value) in &self.env {
            println!("{}={}", key, value);
        }
        Ok(())
    }

    /// set: Print all shell variables
    fn cmd_set(&self) -> Result<(), String> {
        self.cmd_env()
    }

    /// clear: Clear the screen
    fn cmd_clear(&self) -> Result<(), String> {
        // ANSI escape code to clear screen
        print!("\x1b[2J\x1b[H");
        use std::io::Write;
        std::io::stdout().flush().map_err(|e| e.to_string())?;
        Ok(())
    }

    /// history: Display command history (placeholder)
    fn cmd_history(&self) -> Result<(), String> {
        println!("Command history not yet implemented");
        Ok(())
    }

    /// alias: Create command alias (placeholder)
    fn cmd_alias(&self, _command: &Command) -> Result<(), String> {
        println!("Command alias not yet implemented");
        Ok(())
    }

    /// unalias: Remove command alias (placeholder)
    fn cmd_unalias(&self, _command: &Command) -> Result<(), String> {
        println!("Command alias not yet implemented");
        Ok(())
    }

    /// Get an environment variable
    pub fn get_env(&self, key: &str) -> Option<&String> {
        self.env.get(key)
    }

    /// Set an environment variable
    pub fn set_env(&mut self, key: String, value: String) {
        self.env.insert(key, value);
    }
}

impl Default for BuiltinCommand {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_context() -> UIContext {
        use crate::messaging::LockedBus;
        use std::sync::Arc;
        let bus = Arc::new(LockedBus::new());
        UIContext::new(bus)
    }

    #[test]
    fn test_echo() {
        let mut builtins = BuiltinCommand::new();
        let context = make_context();
        let cmd = Command::with_args("echo".to_string(), vec!["hello".to_string(), "world".to_string()]);

        let result = builtins.execute(&context, &cmd);
        assert!(result.is_some());
        assert!(result.unwrap().is_ok());
    }

    #[test]
    fn test_export() {
        let mut builtins = BuiltinCommand::new();
        let context = make_context();
        let cmd = Command::with_args("export".to_string(), vec!["TEST=value".to_string()]);

        let result = builtins.execute(&context, &cmd);
        assert!(result.is_some());
        assert!(result.unwrap().is_ok());
        assert_eq!(builtins.get_env("TEST"), Some(&"value".to_string()));
    }

    #[test]
    fn test_unset() {
        let mut builtins = BuiltinCommand::new();
        let context = make_context();
        builtins.set_env("TEMP_VAR".to_string(), "value".to_string());

        let cmd = Command::with_args("unset".to_string(), vec!["TEMP_VAR".to_string()]);
        let result = builtins.execute(&context, &cmd);
        assert!(result.is_some());
        assert!(result.unwrap().is_ok());
        assert_eq!(builtins.get_env("TEMP_VAR"), None);
    }

    #[test]
    fn test_true_command() {
        let mut builtins = BuiltinCommand::new();
        let context = make_context();
        let cmd = Command::new("true".to_string());

        let result = builtins.execute(&context, &cmd);
        assert!(result.is_some());
        assert!(result.unwrap().is_ok());
    }

    #[test]
    fn test_false_command() {
        let mut builtins = BuiltinCommand::new();
        let context = make_context();
        let cmd = Command::new("false".to_string());

        let result = builtins.execute(&context, &cmd);
        assert!(result.is_some());
        assert!(result.unwrap().is_err());
    }

    #[test]
    fn test_colon_command() {
        let mut builtins = BuiltinCommand::new();
        let context = make_context();
        let cmd = Command::new(":".to_string());

        let result = builtins.execute(&context, &cmd);
        assert!(result.is_some());
        assert!(result.unwrap().is_ok());
    }

    #[test]
    fn test_unknown_command() {
        let mut builtins = BuiltinCommand::new();
        let context = make_context();
        let cmd = Command::new("unknown_command".to_string());

        let result = builtins.execute(&context, &cmd);
        assert!(result.is_none());
    }
}
