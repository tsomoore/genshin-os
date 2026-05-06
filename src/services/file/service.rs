// File Service - Main file management service
//
// 曾国藩曰：
// "文书档案，乃治世之基。"
// 文件服务管理所有文件系统操作，提供统一的文件访问接口。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use crossbeam_channel::{Receiver, Sender};
use crate::messaging::{
    KernelMsg, FileRequest, Pid, OpenFlags, SeekWhence,
    MessageBus, Response, ResponseData, ServiceError as MessagingServiceError,
};
use crate::messaging::bus::Envelope;
use crate::{GenshinResult, GenshinError, ServiceError};
use crate::hardware::VirtualDisk;

// File descriptor type
pub type Fd = u32;

// Import file service components
use super::vfs::{VirtualFileSystem, VFSNode, NodeType};
use super::file::{File, Directory, OpenMode};
use super::descriptor::{FileDescriptorManager, OpenFile};

/// File Service - Main file management service
///
/// 曾国藩曰：
/// "统管文书，当知其详。"
/// 文件服务统筹文件系统、文件操作和文件描述符管理。
pub struct FileService {
    /// Message bus
    bus: Arc<dyn MessageBus>,

    /// Receiver for message bus
    receiver: Receiver<Envelope>,

    /// Virtual file system
    vfs: Arc<Mutex<VirtualFileSystem>>,

    /// File descriptor manager
    fd_manager: Arc<Mutex<FileDescriptorManager>>,

    /// Open files (inode -> File)
    open_files: Arc<Mutex<HashMap<u64, Arc<Mutex<File>>>>>,

    /// Virtual disk reference
    disk: Arc<Mutex<VirtualDisk>>,
}

impl FileService {
    /// Create a new file service
    pub fn new(
        bus: Arc<dyn MessageBus>,
        max_fds_per_process: u32,
        disk_size: usize,
    ) -> Self {
        let receiver = bus.subscribe();

        let vfs = Arc::new(Mutex::new(VirtualFileSystem::new()));
        let fd_manager = Arc::new(Mutex::new(FileDescriptorManager::new(max_fds_per_process)));
        let open_files = Arc::new(Mutex::new(HashMap::new()));
        let disk = Arc::new(Mutex::new(VirtualDisk::new(disk_size as u32)));

        Self {
            bus,
            receiver,
            vfs,
            fd_manager,
            open_files,
            disk,
        }
    }

    /// Run the file service (main loop)
    pub fn run(&self) {
        println!("FileService starting...");

        loop {
            match self.receiver.recv() {
                Ok(envelope) => {
                    if let Err(e) = self.handle_envelope(envelope) {
                        eprintln!("FileService error: {}", e);
                    }
                }
                Err(_) => {
                    eprintln!("Message bus disconnected");
                    break;
                }
            }
        }
    }

    /// Handle incoming envelope
    fn handle_envelope(&self, envelope: Envelope) -> GenshinResult<()> {
        // Handle the message based on envelope type
        let result = match &envelope.message {
            KernelMsg::File(req) => {
                if envelope.expects_response() {
                    self.handle_file_request_with_response(req.clone(), &envelope)
                } else {
                    self.handle_file_request(req.clone())
                }
            }
            KernelMsg::Interrupt(int) => self.handle_interrupt(int.clone()),
            _ => {
                // Ignore other messages
                Ok(())
            }
        };

        // Log errors but don't fail the service
        if let Err(e) = result {
            eprintln!("FileService error handling message: {}", e);

            // If this was a request, send error response
            if envelope.expects_response() {
                let _ = envelope.respond_error(MessagingServiceError::Other {
                    code: 1,
                    msg: e.to_string(),
                });
            }
        }

        Ok(())
    }

    /// Handle hardware interrupt
    fn handle_interrupt(&self, interrupt: crate::messaging::Interrupt) -> GenshinResult<()> {
        match interrupt {
            crate::messaging::Interrupt::HardwareFailure { component } => {
                eprintln!("FileService: Hardware failure in {}", component);
            }
            _ => {
                println!("FileService: Received interrupt {:?}", interrupt);
            }
        }
        Ok(())
    }

    /// Handle file service request
    fn handle_file_request(&self, req: FileRequest) -> GenshinResult<()> {
        // For now, we'll use pid 0 as default since FileRequest doesn't have pid
        let pid = 0;

        match req {
            // ========== File Operations ==========
            FileRequest::Open { path, flags } => {
                self.handle_open(pid, path, flags)?;
            }

            FileRequest::Close { fd } => {
                self.handle_close(pid, fd)?;
            }

            FileRequest::Read { fd, offset, buf, size } => {
                self.handle_read(pid, fd, size)?;
            }

            FileRequest::Write { fd, offset, buf, size } => {
                // For now, we'll create a dummy data vector
                let data = vec![0u8; size];
                self.handle_write(pid, fd, data)?;
            }

            FileRequest::Unlink { path } => {
                self.handle_delete(pid, path)?;
            }

            FileRequest::Stat { path } => {
                self.handle_stat(pid, path)?;
            }

            // ========== Directory Operations ==========
            FileRequest::CreateDirectory { path } => {
                self.handle_mkdir(pid, path, 0o755)?;
            }

            FileRequest::RemoveDirectory { path } => {
                self.handle_rmdir(pid, path)?;
            }

            FileRequest::OpenDirectory { path } => {
                // For now, treat as list directory
                self.handle_listdir(pid, path)?;
            }

            FileRequest::ReadDirectory { dir_fd } => {
                // TODO: Implement read directory entries
                println!("FileService: Read directory entries for fd {}", dir_fd);
            }

            FileRequest::CloseDirectory { dir_fd } => {
                // TODO: Implement close directory
                println!("FileService: Close directory fd {}", dir_fd);
            }

            _ => {
                println!("FileService: Unhandled file request");
            }
        }

        Ok(())
    }

    /// Handle file service request with response
    fn handle_file_request_with_response(&self, req: FileRequest, envelope: &Envelope) -> GenshinResult<()> {
        // For now, we'll use pid 0 as default since FileRequest doesn't have pid
        let pid = 0;

        match req {
            // ========== File Operations ==========
            FileRequest::Open { path, flags } => {
                self.handle_open_with_response(pid, path, flags, envelope)?;
            }

            FileRequest::Close { fd } => {
                self.handle_close_with_response(pid, fd, envelope)?;
            }

            FileRequest::Read { fd, offset: _, buf: _, size } => {
                self.handle_read_with_response(pid, fd, size, envelope)?;
            }

            FileRequest::Write { fd, offset: _, buf: _, size } => {
                // For now, we'll create a dummy data vector
                let data = vec![0u8; size];
                self.handle_write_with_response(pid, fd, data, envelope)?;
            }

            FileRequest::Unlink { path } => {
                self.handle_delete(pid, path)?;
                let _ = envelope.respond_success(ResponseData::Void);
            }

            FileRequest::Stat { path } => {
                self.handle_stat(pid, path)?;
                let _ = envelope.respond_success(ResponseData::Void);
            }

            // ========== Directory Operations ==========
            FileRequest::CreateDirectory { path } => {
                self.handle_mkdir(pid, path, 0o755)?;
                let _ = envelope.respond_success(ResponseData::Void);
            }

            FileRequest::RemoveDirectory { path } => {
                self.handle_rmdir(pid, path)?;
                let _ = envelope.respond_success(ResponseData::Void);
            }

            FileRequest::OpenDirectory { path } => {
                // For now, treat as list directory
                self.handle_listdir(pid, path)?;
                let _ = envelope.respond_success(ResponseData::Void);
            }

            FileRequest::Unlink { path } => {
                self.handle_delete(pid, path)?;
                let _ = envelope.respond_success(ResponseData::Void);
            }

            _ => {
                println!("FileService: Unhandled file request");
                let _ = envelope.respond_success(ResponseData::Void);
            }
        }

        Ok(())
    }

    // ========== File Operations Handlers ==========

    fn handle_open(&self, pid: Pid, path: String, flags: OpenFlags) -> GenshinResult<()> {
        let vfs = Self::lock_mutex(&self.vfs)?;

        // Look up file by path
        let node = vfs.lookup_path(&path)?;

        let node = Self::lock_mutex(&node)?;
        if !node.is_file() {
            return Err(GenshinError::Service(ServiceError::InvalidArguments {
                param: "path".to_string(),
                reason: "Not a file".to_string(),
            }));
        }

        let inode = node.inode;
        drop(node);

        // Get or create file
        let file = if let Some(file) = Self::lock_mutex(&self.open_files)?.get(&inode) {
            file.clone()
        } else {
            let new_file = Arc::new(Mutex::new(File::new(
                inode,
                path.clone(),
                pid,
                Self::open_flags_to_mode(flags)?,
            )));

            Self::lock_mutex(&self.open_files)?.insert(inode, new_file.clone());
            new_file
        };

        // Allocate file descriptor
        let mut fd_manager = Self::lock_mutex(&self.fd_manager)?;
        let fd = fd_manager.allocate(pid, file, 0)?; // TODO: Convert flags to u32

        println!("FileService: Opened file {} for pid {}, fd={}", path, pid, fd);

        // TODO: Send response with fd
        Ok(())
    }

    fn handle_open_with_response(&self, pid: Pid, path: String, flags: OpenFlags, envelope: &Envelope) -> GenshinResult<()> {
        let vfs = Self::lock_mutex(&self.vfs)?;

        // Look up file by path
        let node = vfs.lookup_path(&path);

        let node = match node {
            Ok(n) => n,
            Err(e) => {
                let _ = envelope.respond_error(MessagingServiceError::NotFound {
                    resource: "File".to_string(),
                    id: path,
                });
                return Err(e);
            }
        };

        let node = Self::lock_mutex(&node)?;
        if !node.is_file() {
            let _ = envelope.respond_error(MessagingServiceError::InvalidArguments {
                msg: "Not a file".to_string(),
            });
            return Err(GenshinError::Service(ServiceError::InvalidArguments {
                param: "path".to_string(),
                reason: "Not a file".to_string(),
            }));
        }

        let inode = node.inode;
        drop(node);
        drop(vfs);

        // Get or create file
        let file = if let Some(file) = Self::lock_mutex(&self.open_files)?.get(&inode) {
            file.clone()
        } else {
            let new_file = Arc::new(Mutex::new(File::new(
                inode,
                path.clone(),
                pid,
                Self::open_flags_to_mode(flags)?,
            )));

            Self::lock_mutex(&self.open_files)?.insert(inode, new_file.clone());
            new_file
        };

        // Allocate file descriptor
        let mut fd_manager = Self::lock_mutex(&self.fd_manager)?;
        let fd = fd_manager.allocate(pid, file, 0)?; // TODO: Convert flags to u32

        println!("FileService: Opened file {} for pid {}, fd={}", path, pid, fd);

        // Send response with fd
        let _ = envelope.respond_success(ResponseData::Fd(fd));
        Ok(())
    }

    fn handle_close(&self, pid: Pid, fd: Fd) -> GenshinResult<()> {
        let mut fd_manager = Self::lock_mutex(&self.fd_manager)?;
        fd_manager.close(pid, fd)?;

        println!("FileService: Closed fd {} for pid {}", fd, pid);
        Ok(())
    }

    fn handle_close_with_response(&self, pid: Pid, fd: Fd, envelope: &Envelope) -> GenshinResult<()> {
        let mut fd_manager = Self::lock_mutex(&self.fd_manager)?;
        fd_manager.close(pid, fd)?;

        println!("FileService: Closed fd {} for pid {}", fd, pid);
        let _ = envelope.respond_success(ResponseData::Void);
        Ok(())
    }

    fn handle_read(&self, pid: Pid, fd: Fd, count: usize) -> GenshinResult<()> {
        let fd_manager = Self::lock_mutex(&self.fd_manager)?;

        let open_file = fd_manager.get(pid, fd)
            .ok_or_else(|| GenshinError::Service(ServiceError::NotFound {
                resource_type: "File descriptor".to_string(),
                id: fd.to_string(),
            }))?;

        let data = open_file.read(count)?;

        println!("FileService: Read {} bytes from fd {} for pid {}", data.len(), fd, pid);

        // TODO: Send response with data
        Ok(())
    }

    fn handle_read_with_response(&self, pid: Pid, fd: Fd, count: usize, envelope: &Envelope) -> GenshinResult<()> {
        let fd_manager = Self::lock_mutex(&self.fd_manager)?;

        let open_file = fd_manager.get(pid, fd)
            .ok_or_else(|| {
                let _ = envelope.respond_error(MessagingServiceError::NotFound {
                    resource: "File descriptor".to_string(),
                    id: fd.to_string(),
                });
                GenshinError::Service(ServiceError::NotFound {
                    resource_type: "File descriptor".to_string(),
                    id: fd.to_string(),
                })
            })?;

        let data = open_file.read(count)?;

        println!("FileService: Read {} bytes from fd {} for pid {}", data.len(), fd, pid);

        // Send response with bytes processed
        let _ = envelope.respond_success(ResponseData::BytesProcessed(data.len()));
        Ok(())
    }

    fn handle_write(&self, pid: Pid, fd: Fd, data: Vec<u8>) -> GenshinResult<()> {
        let fd_manager = Self::lock_mutex(&self.fd_manager)?;

        let open_file = fd_manager.get(pid, fd)
            .ok_or_else(|| GenshinError::Service(ServiceError::NotFound {
                resource_type: "File descriptor".to_string(),
                id: fd.to_string(),
            }))?;

        let written = open_file.write(&data)?;

        println!("FileService: Wrote {} bytes to fd {} for pid {}", written, fd, pid);

        // TODO: Send response with bytes written
        Ok(())
    }

    fn handle_write_with_response(&self, pid: Pid, fd: Fd, data: Vec<u8>, envelope: &Envelope) -> GenshinResult<()> {
        let fd_manager = Self::lock_mutex(&self.fd_manager)?;

        let open_file = fd_manager.get(pid, fd)
            .ok_or_else(|| {
                let _ = envelope.respond_error(MessagingServiceError::NotFound {
                    resource: "File descriptor".to_string(),
                    id: fd.to_string(),
                });
                GenshinError::Service(ServiceError::NotFound {
                    resource_type: "File descriptor".to_string(),
                    id: fd.to_string(),
                })
            })?;

        let written = open_file.write(&data)?;

        println!("FileService: Wrote {} bytes to fd {} for pid {}", written, fd, pid);

        // Send response with bytes written
        let _ = envelope.respond_success(ResponseData::BytesProcessed(written));
        Ok(())
    }

    fn handle_seek(&self, pid: Pid, fd: Fd, offset: i64, whence: SeekWhence) -> GenshinResult<()> {
        let fd_manager = Self::lock_mutex(&self.fd_manager)?;

        let open_file = fd_manager.get(pid, fd)
            .ok_or_else(|| GenshinError::Service(ServiceError::NotFound {
                resource_type: "File descriptor".to_string(),
                id: fd.to_string(),
            }))?;

        // Calculate new position
        let current_position = {
            let file = Self::lock_mutex(&open_file.file)?;
            file.position
        };

        let new_position = match whence {
            SeekWhence::Set => offset as u64,
            SeekWhence::Cur => (current_position as i64 + offset) as u64,
            SeekWhence::End => {
                let file = Self::lock_mutex(&open_file.file)?;
                (file.size() as i64 + offset) as u64
            }
        };

        open_file.seek(new_position)?;

        println!("FileService: Seeked fd {} to position {} for pid {}", fd, new_position, pid);

        // TODO: Send response with new position
        Ok(())
    }

    // ========== File Management Handlers ==========

    fn handle_create(&self, pid: Pid, path: String, permissions: u16) -> GenshinResult<()> {
        let mut vfs = Self::lock_mutex(&self.vfs)?;

        // Get parent directory and file name
        let (parent_path, file_name) = Self::split_path(&path)?;

        let parent_node = vfs.lookup_path(&parent_path)?;
        let parent = Self::lock_mutex(&parent_node)?;

        if !parent.is_directory() {
            return Err(GenshinError::Service(ServiceError::InvalidArguments {
                param: "path".to_string(),
                reason: "Parent is not a directory".to_string(),
            }));
        }

        let parent_inode = parent.inode;
        drop(parent);

        // Create file
        let inode = vfs.create_file(parent_inode, file_name, pid)?;

        println!("FileService: Created file {} (inode {}) for pid {}", path, inode, pid);

        // TODO: Send response with inode
        Ok(())
    }

    fn handle_delete(&self, pid: Pid, path: String) -> GenshinResult<()> {
        let mut vfs = Self::lock_mutex(&self.vfs)?;

        let node = vfs.lookup_path(&path)?;
        let node = Self::lock_mutex(&node)?;

        let inode = node.inode;
        drop(node);

        vfs.delete(inode)?;

        println!("FileService: Deleted file {} for pid {}", path, pid);

        // TODO: Send response
        Ok(())
    }

    fn handle_stat(&self, pid: Pid, path: String) -> GenshinResult<()> {
        let vfs = Self::lock_mutex(&self.vfs)?;

        let node = vfs.lookup_path(&path)?;
        let node = Self::lock_mutex(&node)?;

        println!("FileService: Stat file {} for pid {}: size={}", path, pid, node.size);

        // TODO: Send response with metadata
        Ok(())
    }

    // ========== Directory Operations Handlers ==========

    fn handle_mkdir(&self, pid: Pid, path: String, permissions: u16) -> GenshinResult<()> {
        let mut vfs = Self::lock_mutex(&self.vfs)?;

        // Get parent directory and directory name
        let (parent_path, dir_name) = Self::split_path(&path)?;

        let parent_node = vfs.lookup_path(&parent_path)?;
        let parent = Self::lock_mutex(&parent_node)?;

        if !parent.is_directory() {
            return Err(GenshinError::Service(ServiceError::InvalidArguments {
                param: "path".to_string(),
                reason: "Parent is not a directory".to_string(),
            }));
        }

        let parent_inode = parent.inode;
        drop(parent);

        // Create directory
        let inode = vfs.create_directory(parent_inode, dir_name, pid)?;

        println!("FileService: Created directory {} (inode {}) for pid {}", path, inode, pid);

        // TODO: Send response with inode
        Ok(())
    }

    fn handle_rmdir(&self, pid: Pid, path: String) -> GenshinResult<()> {
        let mut vfs = Self::lock_mutex(&self.vfs)?;

        let node = vfs.lookup_path(&path)?;
        let node = Self::lock_mutex(&node)?;

        if !node.is_directory() {
            return Err(GenshinError::Service(ServiceError::InvalidArguments {
                param: "path".to_string(),
                reason: "Not a directory".to_string(),
            }));
        }

        if !node.children.is_empty() {
            return Err(GenshinError::Service(ServiceError::InvalidArguments {
                param: "path".to_string(),
                reason: "Directory not empty".to_string(),
            }));
        }

        let inode = node.inode;
        drop(node);

        vfs.delete(inode)?;

        println!("FileService: Removed directory {} for pid {}", path, pid);

        // TODO: Send response
        Ok(())
    }

    fn handle_listdir(&self, pid: Pid, path: String) -> GenshinResult<()> {
        let vfs = Self::lock_mutex(&self.vfs)?;

        let node = vfs.lookup_path(&path)?;
        let node = Self::lock_mutex(&node)?;

        if !node.is_directory() {
            return Err(GenshinError::Service(ServiceError::InvalidArguments {
                param: "path".to_string(),
                reason: "Not a directory".to_string(),
            }));
        }

        let entries = node.list_children();

        println!("FileService: Listed directory {} for pid {}: {} entries", path, pid, entries.len());

        // TODO: Send response with entries
        Ok(())
    }

    // ========== File Descriptor Operations Handlers ==========

    fn handle_dup(&self, pid: Pid, old_fd: Fd) -> GenshinResult<()> {
        let mut fd_manager = Self::lock_mutex(&self.fd_manager)?;
        let new_fd = fd_manager.dup(pid, old_fd)?;

        println!("FileService: Duplicated fd {} to {} for pid {}", old_fd, new_fd, pid);

        // TODO: Send response with new_fd
        Ok(())
    }

    fn handle_dup2(&self, pid: Pid, old_fd: Fd, new_fd: Fd) -> GenshinResult<()> {
        // TODO: Implement dup2 (duplicate to specific fd)
        println!("FileService: Dup2 fd {} to {} for pid {}", old_fd, new_fd, pid);

        // TODO: Send response
        Ok(())
    }

    // ========== Helper Methods ==========

    /// Helper function to lock mutex and convert poison errors
    fn lock_mutex<T>(mutex: &Mutex<T>) -> GenshinResult<std::sync::MutexGuard<T>> {
        mutex.lock().map_err(|e| {
            GenshinError::Service(ServiceError::InvalidArguments {
                param: "mutex".to_string(),
                reason: format!("Mutex poisoned: {}", e)
            })
        })
    }

    /// Split path into parent and name
    fn split_path(path: &str) -> GenshinResult<(String, String)> {
        if path.is_empty() {
            return Err(GenshinError::Service(ServiceError::InvalidArguments {
                param: "path".to_string(),
                reason: "Path cannot be empty".to_string(),
            }));
        }

        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

        if parts.is_empty() {
            return Ok(("/".to_string(), "/".to_string()));
        }

        let file_name = parts.last().unwrap().to_string();
        let parent_path = if parts.len() > 1 {
            format!("/{}", parts[..parts.len()-1].join("/"))
        } else {
            "/".to_string()
        };

        Ok((parent_path, file_name))
    }

    /// Convert OpenFlags to OpenMode
    fn open_flags_to_mode(flags: OpenFlags) -> GenshinResult<OpenMode> {
        if flags.read && flags.write {
            Ok(OpenMode::ReadWrite)
        } else if flags.write {
            Ok(OpenMode::Write)
        } else if flags.append {
            Ok(OpenMode::Append)
        } else {
            Ok(OpenMode::Read)
        }
    }

    // ========== Query Methods ==========

    /// Get VFS statistics
    pub fn vfs_stats(&self) -> VfsStats {
        let vfs = Self::lock_mutex(&self.vfs).unwrap();
        VfsStats {
            total_nodes: vfs.node_count(),
        }
    }

    /// Get file descriptor count for process
    pub fn fd_count(&self, pid: Pid) -> usize {
        let fd_manager = Self::lock_mutex(&self.fd_manager).unwrap();
        fd_manager.count(pid)
    }
}

/// VFS statistics
#[derive(Debug, Clone)]
pub struct VfsStats {
    pub total_nodes: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::LockedBus;

    #[test]
    fn test_file_service_creation() {
        let bus = Arc::new(LockedBus::new());
        let service = FileService::new(bus, 256, 1024 * 1024);

        // Service should be created successfully
        assert_eq!(service.vfs_stats().total_nodes, 1); // Root node
    }

    #[test]
    fn test_split_path() {
        let (parent, name) = FileService::split_path("/test/file.txt").unwrap();
        assert_eq!(parent, "/test");
        assert_eq!(name, "file.txt");

        let (parent, name) = FileService::split_path("/file.txt").unwrap();
        assert_eq!(parent, "/");
        assert_eq!(name, "file.txt");
    }

    #[test]
    fn test_split_path_empty() {
        let result = FileService::split_path("");
        assert!(result.is_err());
    }

    #[test]
    fn test_open_flags_to_mode() {
        let flags = OpenFlags::read_only();

        let mode = FileService::open_flags_to_mode(flags).unwrap();
        assert_eq!(mode, OpenMode::Read);

        let mut flags = OpenFlags::read_only();
        flags.write = true;
        let mode = FileService::open_flags_to_mode(flags).unwrap();
        assert_eq!(mode, OpenMode::ReadWrite);
    }
}
