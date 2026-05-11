// Virtual File System Module
//
// 曾国藩曰：
// "治大国如烹小鲜，治文书如理丝麻。"
// 虚拟文件系统统一管理各类文件系统，提供统一接口。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use crate::messaging::Pid;
use crate::{GenshinResult, GenshinError, ServiceError};
use crate::vprintln;
/// Node type in the file system
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum NodeType {
    File,
    Directory,
    SymLink,
    BlockDevice,
    CharDevice,
}

/// VFS node - represents a file or directory
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct VFSNode {
    /// Node ID (inode number)
    pub inode: u64,

    /// Node type
    pub node_type: NodeType,

    /// Node name
    pub name: String,

    /// Parent directory inode
    pub parent: Option<u64>,

    /// Permissions
    pub permissions: u16, // Unix-style permissions

    /// Owner process ID
    pub owner: Pid,

    /// Size in bytes
    pub size: u64,

    /// Creation time
    pub created: u64,

    /// Modified time
    pub modified: u64,

    /// Data blocks (for files)
    pub blocks: Vec<u64>,

    /// Children (for directories)
    pub children: HashMap<String, u64>,

    /// Reference count
    pub ref_count: u32,

    /// Whether this node is deleted
    pub deleted: bool,
}

impl VFSNode {
    /// Create a new VFS node
    pub fn new(inode: u64, name: String, node_type: NodeType, parent: Option<u64>, owner: Pid) -> Self {
        Self {
            inode,
            node_type,
            name,
            parent,
            permissions: 0o644, // Default: rw-r--r--
            owner,
            size: 0,
            created: 0,
            modified: 0,
            blocks: Vec::new(),
            children: HashMap::new(),
            ref_count: 0,
            deleted: false,
        }
    }

    /// Check if this is a directory
    pub fn is_directory(&self) -> bool {
        self.node_type == NodeType::Directory
    }

    /// Check if this is a file
    pub fn is_file(&self) -> bool {
        self.node_type == NodeType::File
    }

    /// Check if this is a symbolic link
    pub fn is_symlink(&self) -> bool {
        self.node_type == NodeType::SymLink
    }

    /// Add a child to this directory
    pub fn add_child(&mut self, name: String, inode: u64) -> GenshinResult<()> {
        if !self.is_directory() {
            return Err(GenshinError::Service(ServiceError::InvalidArguments {
                param: "node".to_string(),
                reason: "Not a directory".to_string(),
            }));
        }

        if self.children.contains_key(&name) {
            return Err(GenshinError::Service(ServiceError::InvalidArguments {
                param: "name".to_string(),
                reason: "Child already exists".to_string(),
            }));
        }

        self.children.insert(name, inode);
        self.modified = 0; // TODO: Get actual time
        Ok(())
    }

    /// Remove a child from this directory
    pub fn remove_child(&mut self, name: &str) -> GenshinResult<u64> {
        if !self.is_directory() {
            return Err(GenshinError::Service(ServiceError::InvalidArguments {
                param: "node".to_string(),
                reason: "Not a directory".to_string(),
            }));
        }

        self.children.remove(name)
            .ok_or_else(|| GenshinError::Service(ServiceError::NotFound {
                resource_type: "Child".to_string(),
                id: name.to_string(),
            }))
    }

    /// Get child inode by name
    pub fn get_child(&self, name: &str) -> Option<u64> {
        self.children.get(name).copied()
    }

    /// List all children
    pub fn list_children(&self) -> Vec<String> {
        self.children.keys().cloned().collect()
    }

    /// Increment reference count
    pub fn inc_ref(&mut self) {
        self.ref_count += 1;
    }

    /// Decrement reference count
    pub fn dec_ref(&mut self) -> bool {
        if self.ref_count > 0 {
            self.ref_count -= 1;
        }
        self.ref_count == 0 && self.deleted
    }

    /// Mark node as deleted
    pub fn mark_deleted(&mut self) {
        self.deleted = true;
    }

    /// Check if node can be deleted
    pub fn can_delete(&self) -> bool {
        self.ref_count == 0 && !self.is_directory() || (self.is_directory() && self.children.is_empty())
    }
}

/// Virtual File System
#[derive(Debug)]
pub struct VirtualFileSystem {
    /// All nodes (inode -> VFSNode)
    nodes: HashMap<u64, Arc<Mutex<VFSNode>>>,

    /// Next available inode number
    next_inode: u64,

    /// Root inode
    root_inode: u64,

    /// Mount points
    mounts: HashMap<String, u64>,
}

impl VirtualFileSystem {
    /// Create a new VFS
    pub fn new() -> Self {
        let root = Arc::new(Mutex::new(VFSNode::new(
            0, "/".to_string(), NodeType::Directory, None, 0)));
        let mut nodes = HashMap::new();
        nodes.insert(0, root);

        let mut vfs = Self { nodes, next_inode: 1, root_inode: 0, mounts: HashMap::new() };

        // Create standard directories
        for name in &["bin", "home", "tmp", "etc", "var", "examples"] {
            let _ = vfs.create_directory(0, name.to_string(), 0);
        }

        vfs
    }

    /// Create a new file
    pub fn create_file(&mut self, parent: u64, name: String, owner: Pid) -> GenshinResult<u64> {
        let inode = self.next_inode;
        self.next_inode += 1;

        let name_clone = name.clone();

        let node = Arc::new(Mutex::new(VFSNode::new(
            inode,
            name,
            NodeType::File,
            Some(parent),
            owner,
        )));

        self.nodes.insert(inode, node);

        // Add to parent directory
        if let Some(parent_node) = self.nodes.get(&parent) {
            let mut parent = parent_node.lock().map_err(|e| {
                GenshinError::Service(ServiceError::Other {
                    code: 1,
                    msg: format!("Mutex poisoned: {}", e),
                })
            })?;
            parent.add_child(name_clone, inode)?;
        }

        Ok(inode)
    }

    /// Create a new directory
    pub fn create_directory(&mut self, parent: u64, name: String, owner: Pid) -> GenshinResult<u64> {
        let inode = self.next_inode;
        self.next_inode += 1;

        let name_clone = name.clone();

        let node = Arc::new(Mutex::new(VFSNode::new(
            inode,
            name,
            NodeType::Directory,
            Some(parent),
            owner,
        )));

        self.nodes.insert(inode, node);

        // Add to parent directory
        if let Some(parent_node) = self.nodes.get(&parent) {
            let mut parent_node = parent_node.lock().map_err(|e| {
                GenshinError::Service(ServiceError::Other {
                    code: 2,
                    msg: format!("Mutex poisoned: {}", e),
                })
            })?;
            parent_node.add_child(name_clone, inode)?;
        }

        Ok(inode)
    }

    /// Look up a node by inode
    pub fn lookup(&self, inode: u64) -> Option<Arc<Mutex<VFSNode>>> {
        self.nodes.get(&inode).cloned()
    }

    /// Look up a node by path
    pub fn lookup_path(&self, path: &str) -> GenshinResult<Arc<Mutex<VFSNode>>> {
        if path.is_empty() || path == "/" {
            // Empty or root: return root node
            return self.lookup(self.root_inode).ok_or_else(|| GenshinError::Service(ServiceError::NotFound {
                resource_type: "Root".to_string(),
                id: self.root_inode.to_string(),
            }));
        }

        let components: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut current_inode = self.root_inode;

        for component in components {
            let current_node = self.lookup(current_inode)
                .ok_or_else(|| GenshinError::Service(ServiceError::NotFound {
                    resource_type: "Inode".to_string(),
                    id: current_inode.to_string(),
                }))?;

            let current = current_node.lock().map_err(|e| {
                GenshinError::Service(ServiceError::Other {
                    code: 3,
                    msg: format!("Mutex poisoned: {}", e),
                })
            })?;

            if let Some(child_inode) = current.get_child(component) {
                current_inode = child_inode;
            } else {
                return Err(GenshinError::Service(ServiceError::NotFound {
                    resource_type: "Path component".to_string(),
                    id: component.to_string(),
                }));
            }
        }

        self.lookup(current_inode)
            .ok_or_else(|| GenshinError::Service(ServiceError::NotFound {
                resource_type: "Inode".to_string(),
                id: current_inode.to_string(),
            }))
    }

    /// Delete a node
    pub fn delete(&mut self, inode: u64) -> GenshinResult<()> {
        let node = self.lookup(inode)
            .ok_or_else(|| GenshinError::Service(ServiceError::NotFound {
                resource_type: "Inode".to_string(),
                id: inode.to_string(),
            }))?;

        let mut node = node.lock().map_err(|e| {
            GenshinError::Service(ServiceError::Other {
                code: 4,
                msg: format!("Mutex poisoned: {}", e),
            })
        })?;

        if !node.can_delete() {
            return Err(GenshinError::Service(ServiceError::InvalidArguments {
                param: "inode".to_string(),
                reason: "Node cannot be deleted".to_string(),
            }));
        }

        // Remove from parent directory
        if let Some(parent_inode) = node.parent {
            if let Some(parent_node) = self.lookup(parent_inode) {
                let mut parent = parent_node.lock().map_err(|e| {
                    GenshinError::Service(ServiceError::Other {
                        code: 5,
                        msg: format!("Mutex poisoned: {}", e),
                    })
                })?;
                let _ = parent.remove_child(&node.name);
            }
        }

        node.mark_deleted();

        // Actually remove from nodes map if ref_count is 0
        if node.ref_count == 0 {
            self.nodes.remove(&inode);
        }

        Ok(())
    }

    /// Get root inode
    pub fn root(&self) -> u64 {
        self.root_inode
    }

    /// Get node count
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Save VFS to JSON file
    pub fn save_to_file(&self, path: &str) -> GenshinResult<()> {
        let mut nodes_vec = Vec::new();
        for (_, node_arc) in &self.nodes {
            let node = node_arc.lock().map_err(|_| GenshinError::Service(ServiceError::Other {
                code: 99, msg: "vfs lock".into(),
            }))?;
            nodes_vec.push((node.inode, node.node_type, node.name.clone(), node.parent, node.size, node.children.clone(), node.blocks.clone()));
        }
        let json = serde_json::to_string_pretty(&nodes_vec)
            .map_err(|e| GenshinError::Service(ServiceError::Other { code: 98, msg: format!("json: {}", e) }))?;
        std::fs::write(path, json)
            .map_err(|e| GenshinError::Service(ServiceError::Other { code: 97, msg: format!("write: {}", e) }))?;
        vprintln!("VFS: saved {} nodes to {}", nodes_vec.len(), path);
        Ok(())
    }

    /// Load VFS from JSON file, returns new VFS
    pub fn load_from_file(path: &str) -> Option<Self> {
        let json = std::fs::read_to_string(path).ok()?;
        let nodes_vec: Vec<(u64, NodeType, String, Option<u64>, u64, HashMap<String, u64>, Vec<u64>)> = serde_json::from_str(&json).ok()?;
        let mut vfs = Self::new();
        // First pass: create all nodes
        for (inode, ntype, name, parent, size, _children, blocks) in &nodes_vec {
            if *inode == 0 { continue; } // root already exists
            let node = Arc::new(Mutex::new(VFSNode {
                inode: *inode, node_type: *ntype, name: name.clone(),
                parent: *parent, permissions: 0o644, owner: 0,
                size: *size, created: 0, modified: 0,
                blocks: blocks.clone(), children: HashMap::new(),
                ref_count: 0, deleted: false,
            }));
            vfs.nodes.insert(*inode, node);
            if *inode >= vfs.next_inode { vfs.next_inode = *inode + 1; }
        }
        // Second pass: restore children
        for (inode, _, _, _, _, children, _) in &nodes_vec {
            if let Some(node_arc) = vfs.nodes.get(inode) {
                let mut node = node_arc.lock().ok()?;
                node.children = children.clone();
            }
        }
        println!("VFS: loaded {} nodes from {}", nodes_vec.len(), path);
        Some(vfs)
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
    fn test_vfs_creation() {
        let vfs = VirtualFileSystem::new();
        assert_eq!(vfs.root(), 0);
        assert_eq!(vfs.node_count(), 1);
    }

    #[test]
    fn test_create_file() {
        let mut vfs = VirtualFileSystem::new();
        let inode = vfs.create_file(0, "test.txt".to_string(), 100);
        assert!(inode.is_ok());
        assert_eq!(inode.unwrap(), 1);
        assert_eq!(vfs.node_count(), 2);
    }

    #[test]
    fn test_create_directory() {
        let mut vfs = VirtualFileSystem::new();
        let inode = vfs.create_directory(0, "testdir".to_string(), 100);
        assert!(inode.is_ok());
        assert_eq!(inode.unwrap(), 1);
    }

    #[test]
    fn test_lookup() {
        let mut vfs = VirtualFileSystem::new();
        let inode = vfs.create_file(0, "test.txt".to_string(), 100).unwrap();

        let node = vfs.lookup(inode);
        assert!(node.is_some());

        let node = node.unwrap();
        let node = node.lock().unwrap();
        assert_eq!(node.name, "test.txt");
        assert!(node.is_file());
    }

    #[test]
    fn test_lookup_path() {
        let mut vfs = VirtualFileSystem::new();

        // Create directory
        let dir_inode = vfs.create_directory(0, "dir".to_string(), 100).unwrap();

        // Create file in directory
        let file_inode = vfs.create_file(dir_inode, "file.txt".to_string(), 100).unwrap();

        // Look up via path
        let node = vfs.lookup_path("/dir/file.txt");
        assert!(node.is_ok());

        let node = node.unwrap();
        let node = node.lock().unwrap();
        assert_eq!(node.name, "file.txt");
        assert_eq!(node.inode, file_inode);
    }

    #[test]
    fn test_directory_children() {
        let mut vfs = VirtualFileSystem::new();

        // Create files in root
        vfs.create_file(0, "file1.txt".to_string(), 100).unwrap();
        vfs.create_file(0, "file2.txt".to_string(), 100).unwrap();

        // List root directory
        let root = vfs.lookup(0).unwrap();
        let root = root.lock().unwrap();
        let children = root.list_children();

        assert_eq!(children.len(), 2);
        assert!(children.contains(&"file1.txt".to_string()));
        assert!(children.contains(&"file2.txt".to_string()));
    }

    #[test]
    fn test_delete_file() {
        let mut vfs = VirtualFileSystem::new();

        // Create file
        let inode = vfs.create_file(0, "test.txt".to_string(), 100).unwrap();

        // Delete file
        let result = vfs.delete(inode);
        assert!(result.is_ok());
    }

    #[test]
    fn test_delete_directory_with_children() {
        let mut vfs = VirtualFileSystem::new();

        // Create directory
        let dir_inode = vfs.create_directory(0, "dir".to_string(), 100).unwrap();

        // Create file in directory
        vfs.create_file(dir_inode, "file.txt".to_string(), 100).unwrap();

        // Try to delete directory (should fail)
        let result = vfs.delete(dir_inode);
        assert!(result.is_err());
    }

    #[test]
    fn test_reference_counting() {
        let mut vfs = VirtualFileSystem::new();

        let inode = vfs.create_file(0, "test.txt".to_string(), 100).unwrap();

        let node = vfs.lookup(inode).unwrap();
        let mut node = node.lock().unwrap();

        assert_eq!(node.ref_count, 0);

        node.inc_ref();
        assert_eq!(node.ref_count, 1);

        node.dec_ref();
        assert_eq!(node.ref_count, 0);
    }

    #[test]
    fn test_node_types() {
        let vfs = VirtualFileSystem::new();

        let root = vfs.lookup(0).unwrap();
        let root = root.lock().unwrap();

        assert!(root.is_directory());
        assert!(!root.is_file());
        assert!(!root.is_symlink());
    }

    #[test]
    fn test_invalid_path() {
        let vfs = VirtualFileSystem::new();

        // Try to look up non-existent path
        let result = vfs.lookup_path("/nonexistent/path");
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_path() {
        let vfs = VirtualFileSystem::new();

        // Try to look up empty path
        let result = vfs.lookup_path("");
        assert!(result.is_err());
    }

    #[test]
    fn test_add_duplicate_child() {
        let mut vfs = VirtualFileSystem::new();

        vfs.create_file(0, "test.txt".to_string(), 100).unwrap();

        // Try to create duplicate file
        let result = vfs.create_file(0, "test.txt".to_string(), 100);
        assert!(result.is_err());
    }
}
