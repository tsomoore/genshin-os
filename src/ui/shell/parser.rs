// Command parser for the shell
//
// Parses user input into structured commands with arguments and options.

/// Parsed command structure
#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    /// Command name
    pub name: String,
    /// Command arguments
    pub args: Vec<String>,
    /// Command options (flags)
    pub options: Vec<String>,
}

impl Command {
    /// Create a new command
    pub fn new(name: String) -> Self {
        Self {
            name,
            args: Vec::new(),
            options: Vec::new(),
        }
    }

    /// Create a command with arguments
    pub fn with_args(name: String, args: Vec<String>) -> Self {
        Self {
            name,
            args,
            options: Vec::new(),
        }
    }

    /// Check if an option/flag is present
    pub fn has_option(&self, option: &str) -> bool {
        self.options.iter().any(|o| o == option)
    }

    /// Get an argument by index
    pub fn get_arg(&self, index: usize) -> Option<&String> {
        self.args.get(index)
    }
}

/// Shell command parser
pub struct ShellParser {
    /// Command separator
    separator: char,
}

impl ShellParser {
    /// Create a new parser
    pub fn new() -> Self {
        Self {
            separator: ' ',
        }
    }

    /// Parse a command line into a Command structure
    pub fn parse(&self, line: &str) -> Option<Command> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }

        // Handle quoted strings and escape sequences
        let tokens = self.tokenize(line)?;
        if tokens.is_empty() {
            return None;
        }

        let name = tokens[0].clone();
        let mut args = Vec::new();
        let mut options = Vec::new();

        for token in &tokens[1..] {
            if token.starts_with('-') {
                options.push(token.clone());
            } else {
                args.push(token.clone());
            }
        }

        Some(Command { name, args, options })
    }

    /// Tokenize a command line, handling quotes and escapes
    fn tokenize(&self, line: &str) -> Option<Vec<String>> {
        let mut tokens = Vec::new();
        let mut current = String::new();
        let mut in_quotes = false;
        let mut escape_next = false;
        let mut quote_char = ' ';

        for ch in line.chars() {
            if escape_next {
                current.push(ch);
                escape_next = false;
                continue;
            }

            if ch == '\\' {
                escape_next = true;
                continue;
            }

            if ch == '"' || ch == '\'' {
                if !in_quotes {
                    in_quotes = true;
                    quote_char = ch;
                } else if ch == quote_char {
                    in_quotes = false;
                    quote_char = ' ';
                } else {
                    current.push(ch);
                }
                continue;
            }

            if ch == self.separator && !in_quotes {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
                continue;
            }

            current.push(ch);
        }

        if !current.is_empty() {
            tokens.push(current);
        }

        if tokens.is_empty() {
            None
        } else {
            Some(tokens)
        }
    }

    /// Parse multiple commands separated by pipes or semicolons
    pub fn parse_pipeline(&self, line: &str) -> Vec<Command> {
        let mut commands = Vec::new();
        let mut current_cmd = String::new();

        for ch in line.chars() {
            if ch == ';' || ch == '|' {
                if let Some(cmd) = self.parse(&current_cmd) {
                    commands.push(cmd);
                }
                current_cmd.clear();
            } else {
                current_cmd.push(ch);
            }
        }

        if let Some(cmd) = self.parse(&current_cmd) {
            commands.push(cmd);
        }

        commands
    }
}

impl Default for ShellParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_command() {
        let parser = ShellParser::new();
        let cmd = parser.parse("ls");
        assert_eq!(cmd, Some(Command::new("ls".to_string())));
    }

    #[test]
    fn test_parse_command_with_args() {
        let parser = ShellParser::new();
        let cmd = parser.parse("ls -l /tmp").unwrap();
        assert_eq!(cmd.name, "ls");
        assert_eq!(cmd.args, vec!["/tmp"]);
        assert_eq!(cmd.options, vec!["-l"]);
    }

    #[test]
    fn test_parse_quoted_string() {
        let parser = ShellParser::new();
        let cmd = parser.parse("echo \"hello world\"").unwrap();
        assert_eq!(cmd.name, "echo");
        assert_eq!(cmd.args, vec!["hello world"]);
    }

    #[test]
    fn test_parse_single_quoted_string() {
        let parser = ShellParser::new();
        let cmd = parser.parse("echo 'hello world'").unwrap();
        assert_eq!(cmd.name, "echo");
        assert_eq!(cmd.args, vec!["hello world"]);
    }

    #[test]
    fn test_parse_escape_sequence() {
        let parser = ShellParser::new();
        let cmd = parser.parse("echo hello\\ world").unwrap();
        assert_eq!(cmd.name, "echo");
        assert_eq!(cmd.args, vec!["hello world"]);
    }

    #[test]
    fn test_parse_empty_line() {
        let parser = ShellParser::new();
        let cmd = parser.parse("");
        assert_eq!(cmd, None);
    }

    #[test]
    fn test_parse_whitespace_only() {
        let parser = ShellParser::new();
        let cmd = parser.parse("   \t  ");
        assert_eq!(cmd, None);
    }

    #[test]
    fn test_parse_multiple_options() {
        let parser = ShellParser::new();
        let cmd = parser.parse("ls -l -a -h").unwrap();
        assert_eq!(cmd.name, "ls");
        assert_eq!(cmd.options, vec!["-l", "-a", "-h"]);
    }

    #[test]
    fn test_parse_mixed_args_and_options() {
        let parser = ShellParser::new();
        let cmd = parser.parse("ls -l /tmp -a /home").unwrap();
        assert_eq!(cmd.name, "ls");
        assert_eq!(cmd.args, vec!["/tmp", "/home"]);
        assert_eq!(cmd.options, vec!["-l", "-a"]);
    }

    #[test]
    fn test_has_option() {
        let parser = ShellParser::new();
        let cmd = parser.parse("ls -l -a").unwrap();
        assert!(cmd.has_option("-l"));
        assert!(cmd.has_option("-a"));
        assert!(!cmd.has_option("-h"));
    }

    #[test]
    fn test_get_arg() {
        let parser = ShellParser::new();
        let cmd = parser.parse("echo hello world").unwrap();
        assert_eq!(cmd.get_arg(0), Some(&"hello".to_string()));
        assert_eq!(cmd.get_arg(1), Some(&"world".to_string()));
        assert_eq!(cmd.get_arg(2), None);
    }

    #[test]
    fn test_parse_pipeline() {
        let parser = ShellParser::new();
        let cmds = parser.parse_pipeline("ls ; echo hello");
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0].name, "ls");
        assert_eq!(cmds[1].name, "echo");
    }

    #[test]
    fn test_parse_empty_pipeline() {
        let parser = ShellParser::new();
        let cmds = parser.parse_pipeline("   ;  ");
        assert_eq!(cmds.len(), 0);
    }
}
