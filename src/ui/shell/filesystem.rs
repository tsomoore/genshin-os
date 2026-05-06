// Simple virtual filesystem for shell builtin commands
//
// This provides a basic in-memory filesystem to make shell commands like
// cd, pwd, and ls actually functional.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Virtual file system node
#[derive(Debug, Clone)]
pub enum VNode {
    /// Directory node with children
    Directory {
        children: HashSet<String>,
        metadata: DirMetadata,
    },
    /// File node with content
    File {
        content: String,
        metadata: FileMetadata,
    },
}

/// Directory metadata
#[derive(Debug, Clone)]
pub struct DirMetadata {
    pub permissions: u32,
    pub owner: u32,
    pub group: u32,
    pub created: std::time::SystemTime,
}

/// File metadata
#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub permissions: u32,
    pub size: usize,
    pub owner: u32,
    pub group: u32,
    pub created: std::time::SystemTime,
    pub modified: std::time::SystemTime,
}

/// Simple virtual filesystem
#[derive(Debug)]
pub struct VirtualFileSystem {
    /// Root directory and all subdirectories
    nodes: HashMap<PathBuf, VNode>,
    /// Current working directory
    cwd: PathBuf,
}

impl VirtualFileSystem {
    /// Create a new virtual filesystem with basic structure
    pub fn new() -> Self {
        let mut nodes = HashMap::new();

        // Create root directory
        nodes.insert(
            PathBuf::from("/"),
            VNode::Directory {
                children: {
                    let mut set = HashSet::new();
                    set.insert("bin".to_string());
                    set.insert("home".to_string());
                    set.insert("tmp".to_string());
                    set.insert("etc".to_string());
                    set.insert("var".to_string());
                    set
                },
                metadata: DirMetadata {
                    permissions: 0o755,
                    owner: 0,
                    group: 0,
                    created: std::time::SystemTime::now(),
                },
            },
        );

        // Create /bin directory
        nodes.insert(
            PathBuf::from("/bin"),
            VNode::Directory {
                children: {
                    let mut set = HashSet::new();
                    set.insert("ls".to_string());
                    set.insert("cd".to_string());
                    set.insert("echo".to_string());
                    set.insert("cat".to_string());
                    set.insert("mkdir".to_string());
                    set
                },
                metadata: DirMetadata {
                    permissions: 0o755,
                    owner: 0,
                    group: 0,
                    created: std::time::SystemTime::now(),
                },
            },
        );

        // Create /home directory
        nodes.insert(
            PathBuf::from("/home"),
            VNode::Directory {
                children: {
                    let mut set = HashSet::new();
                    set.insert("user".to_string());
                    set
                },
                metadata: DirMetadata {
                    permissions: 0o755,
                    owner: 0,
                    group: 0,
                    created: std::time::SystemTime::now(),
                },
            },
        );

        // Create /home/user directory
        nodes.insert(
            PathBuf::from("/home/user"),
            VNode::Directory {
                children: HashSet::new(),
                metadata: DirMetadata {
                    permissions: 0o755,
                    owner: 1000,
                    group: 1000,
                    created: std::time::SystemTime::now(),
                },
            },
        );

        // Create /tmp directory
        nodes.insert(
            PathBuf::from("/tmp"),
            VNode::Directory {
                children: HashSet::new(),
                metadata: DirMetadata {
                    permissions: 0o777,
                    owner: 0,
                    group: 0,
                    created: std::time::SystemTime::now(),
                },
            },
        );

        // Create /etc directory
        nodes.insert(
            PathBuf::from("/etc"),
            VNode::Directory {
                children: {
                    let mut set = HashSet::new();
                    set.insert("passwd".to_string());
                    set.insert("hosts".to_string());
                    set
                },
                metadata: DirMetadata {
                    permissions: 0o755,
                    owner: 0,
                    group: 0,
                    created: std::time::SystemTime::now(),
                },
            },
        );

        // Create /var directory
        nodes.insert(
            PathBuf::from("/var"),
            VNode::Directory {
                children: {
                    let mut set = HashSet::new();
                    set.insert("log".to_string());
                    set
                },
                metadata: DirMetadata {
                    permissions: 0o755,
                    owner: 0,
                    group: 0,
                    created: std::time::SystemTime::now(),
                },
            },
        );

        Self {
            nodes,
            cwd: PathBuf::from("/"),
        }
    }

    /// Get current working directory
    pub fn pwd(&self) -> String {
        self.cwd.display().to_string()
    }

    /// Change directory
    pub fn cd(&mut self, path: &str) -> Result<(), String> {
        let new_path = self.resolve_path(path)?;

        // Check if the path exists and is a directory
        if let Some(VNode::Directory { .. }) = self.nodes.get(&new_path) {
            self.cwd = new_path;
            Ok(())
        } else {
            Err(format!("cd: {}: No such directory", path))
        }
    }

    /// List directory contents
    pub fn ls(&self, path: Option<&str>) -> Result<Vec<String>, String> {
        let target_path = if let Some(p) = path {
            self.resolve_path(p)?
        } else {
            self.cwd.clone()
        };

        if let Some(node) = self.nodes.get(&target_path) {
            match node {
                VNode::Directory { children, .. } => {
                    let mut entries: Vec<String> = children.iter().cloned().collect();
                    entries.sort();
                    Ok(entries)
                }
                VNode::File { .. } => {
                    // If it's a file, just return the filename
                    Ok(vec![target_path.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string()])
                }
            }
        } else {
            Err(format!("ls: cannot access '{}': No such file or directory",
                path.unwrap_or(".")))
        }
    }

    /// Create a directory
    pub fn mkdir(&mut self, path: &str) -> Result<(), String> {
        let new_path = self.resolve_path(path)?;

        // Check if parent directory exists
        if let Some(parent) = new_path.parent() {
            if !parent.as_os_str().is_empty() && !self.nodes.contains_key(parent) {
                return Err(format!("mkdir: cannot create directory '{}': No such file or directory", path));
            }
        }

        // Check if already exists
        if self.nodes.contains_key(&new_path) {
            return Err(format!("mkdir: cannot create directory '{}': File exists", path));
        }

        // Create the directory
        self.nodes.insert(
            new_path.clone(),
            VNode::Directory {
                children: HashSet::new(),
                metadata: DirMetadata {
                    permissions: 0o755,
                    owner: 1000,
                    group: 1000,
                    created: std::time::SystemTime::now(),
                },
            },
        );

        // Add to parent's children
        if let Some(parent) = new_path.parent() {
            if let Some(VNode::Directory { children, .. }) = self.nodes.get_mut(parent) {
                if let Some(name) = new_path.file_name() {
                    children.insert(name.to_string_lossy().to_string());
                }
            }
        }

        Ok(())
    }

    /// Resolve a path to absolute path
    fn resolve_path(&self, path: &str) -> Result<PathBuf, String> {
        let path_obj = Path::new(path);

        if path_obj.is_absolute() {
            Ok(path_obj.to_path_buf())
        } else {
            // Relative path - resolve against cwd
            let mut resolved = self.cwd.clone();
            for component in path_obj.components() {
                use std::path::Component;
                match component {
                    Component::ParentDir => {
                        resolved.pop();
                    }
                    Component::Normal(c) => {
                        resolved.push(c);
                    }
                    _ => {}
                }
            }
            Ok(resolved)
        }
    }

    /// Check if a path exists
    pub fn exists(&self, path: &str) -> bool {
        let resolved = self.resolve_path(path).unwrap_or_else(|_| PathBuf::from(path));
        self.nodes.contains_key(&resolved)
    }

    /// Get node at path
    pub fn get_node(&self, path: &str) -> Option<&VNode> {
        let resolved = self.resolve_path(path).ok()?;
        self.nodes.get(&resolved)
    }
}

impl Default for VirtualFileSystem {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filesystem_creation() {
        let fs = VirtualFileSystem::new();
        assert_eq!(fs.pwd(), "/");
    }

    #[test]
    fn test_cd_absolute() {
        let mut fs = VirtualFileSystem::new();
        assert!(fs.cd("/tmp").is_ok());
        assert_eq!(fs.pwd(), "/tmp");
    }

    #[test]
    fn test_cd_relative() {
        let mut fs = VirtualFileSystem::new();
        assert!(fs.cd("/home").is_ok());
        assert!(fs.cd("user").is_ok());
        assert_eq!(fs.pwd(), "/home/user");
    }

    #[test]
    fn test_cd_parent() {
        let mut fs = VirtualFileSystem::new();
        assert!(fs.cd("/home/user").is_ok());
        assert!(fs.cd("..").is_ok());
        assert_eq!(fs.pwd(), "/home");
    }

    #[test]
    fn test_cd_nonexistent() {
        let mut fs = VirtualFileSystem::new();
        assert!(fs.cd("/nonexistent").is_err());
    }

    #[test]
    fn test_ls_root() {
        let fs = VirtualFileSystem::new();
        let result = fs.ls(None);
        assert!(result.is_ok());
        let entries = result.unwrap();
        assert!(entries.contains(&"bin".to_string()));
        assert!(entries.contains(&"home".to_string()));
        assert!(entries.contains(&"tmp".to_string()));
    }

    #[test]
    fn test_ls_directory() {
        let fs = VirtualFileSystem::new();
        let result = fs.ls(Some("/bin"));
        assert!(result.is_ok());
        let entries = result.unwrap();
        assert!(entries.contains(&"ls".to_string()));
        assert!(entries.contains(&"cd".to_string()));
    }

    #[test]
    fn test_mkdir() {
        let mut fs = VirtualFileSystem::new();
        assert!(fs.mkdir("/tmp/newdir").is_ok());
        assert!(fs.exists("/tmp/newdir"));

        let entries = fs.ls(Some("/tmp")).unwrap();
        assert!(entries.contains(&"newdir".to_string()));
    }

    #[test]
    fn test_mkdir_nonexistent_parent() {
        let mut fs = VirtualFileSystem::new();
        assert!(fs.mkdir("/nonexistent/dir").is_err());
    }

    #[test]
    fn test_resolve_absolute_path() {
        let fs = VirtualFileSystem::new();
        let resolved = fs.resolve_path("/home/user").unwrap();
        assert_eq!(resolved, PathBuf::from("/home/user"));
    }

    #[test]
    fn test_resolve_relative_path() {
        let mut fs = VirtualFileSystem::new();
        fs.cd("/home").unwrap();
        let resolved = fs.resolve_path("user").unwrap();
        assert_eq!(resolved, PathBuf::from("/home/user"));
    }

    #[test]
    fn test_resolve_parent_reference() {
        let mut fs = VirtualFileSystem::new();
        fs.cd("/home/user").unwrap();
        let resolved = fs.resolve_path("..").unwrap();
        assert_eq!(resolved, PathBuf::from("/home"));
    }
}
