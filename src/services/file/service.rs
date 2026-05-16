use crate::vprintln;
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
        receiver: Receiver<Envelope>,
    ) -> Self {
        let vfs = Arc::new(Mutex::new(VirtualFileSystem::load_from_file(".genshin-vfs.json").unwrap_or_else(VirtualFileSystem::new)));
        let fd_manager = Arc::new(Mutex::new(FileDescriptorManager::new(max_fds_per_process)));
        let open_files = Arc::new(Mutex::new(HashMap::new()));
        let sector_count = (disk_size / 512) as u32;
        let disk = Arc::new(Mutex::new(VirtualDisk::new(sector_count, ".genshin-disk.img")));

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

        // Import host .asm files into VFS on first boot
        self.import_host_files("programs", "/programs");

        loop {
            match self.receiver.recv() {
                Ok(envelope) => {
                    if let Err(e) = self.handle_envelope(envelope) {
                        eprintln!("FileService error: {}", e);
                    }
                    // Auto-save VFS
                    if let Ok(vfs) = self.vfs.lock() {
                        let _ = vfs.save_to_file(".genshin-vfs.json");
                    }
                }
                Err(_) => {
                    eprintln!("Message bus disconnected");
                    break;
                }
            }
        }
    }

    /// Import host files into VFS (only if file doesn't exist or is empty)
    fn import_host_files(&self, host_dir: &str, vfs_dir: &str) {
        let host_path = std::path::Path::new(host_dir);
        if !host_path.is_dir() { return; }
        let entries = match std::fs::read_dir(host_path) {
            Ok(e) => e, Err(_) => return,
        };
        for entry in entries.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            if !fname.ends_with(".asm") && !fname.ends_with(".txt") { continue; }
            let content = match std::fs::read_to_string(entry.path()) {
                Ok(c) => c, Err(_) => continue,
            };
            let vfs_path = format!("{}/{}", vfs_dir, fname);
            let mut vfs = match self.vfs.lock() { Ok(v) => v, Err(_) => return, };
            let node = match vfs.lookup_path(&vfs_path) {
                Ok(n) => n,
                Err(_) => {
                    // File doesn't exist in VFS yet — create it
                    let parent_path = vfs_dir.to_string();
                    if let Ok(parent_node) = vfs.lookup_path(&parent_path) {
                        let parent_inode = parent_node.lock().unwrap().inode;
                        drop(parent_node);
                        if vfs.create_file(parent_inode, fname.clone(), 0).is_ok() {
                            match vfs.lookup_path(&vfs_path) {
                                Ok(n) => n,
                                Err(_) => continue,
                            }
                        } else { continue; }
                    } else { continue; }
                }
            };
            let inode = { node.lock().unwrap().inode };
            drop(vfs);
            use crate::services::file::file::File;
            use crate::services::file::file::OpenMode;
            let mut f = File::new(inode, vfs_path.clone(), 0, OpenMode::Write);
            f.write(content.as_bytes()).ok();
            if let Ok(disk) = self.disk.lock() {
                f.sync_to_disk(&disk).ok();
                if let Some(start) = f.start_sector {
                    if let Ok(vfs) = self.vfs.lock() {
                        if let Some(vnode) = vfs.lookup(inode) {
                            let mut vn = vnode.lock().unwrap();
                            vn.size = content.len() as u64;
                            vn.blocks = (0..f.sector_count).map(|i: u32| (start + i) as u64).collect();
                        }
                    }
                }
            }
            vprintln!("FileService: imported {}", fname);
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
                // ignore
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

            FileRequest::Read { fd, offset: _, buf: _, size } => {
                self.handle_read(pid, fd, size)?;
            }

            FileRequest::Write { fd, offset: _, buf: _, size } => {
                // For now, we'll create a dummy data vector
                let data = vec![0u8; size];
                self.handle_write(pid, fd, data)?;
            }

            FileRequest::WriteData { fd, data } => {
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
                vprintln!("FileService: Read directory entries for fd {}", dir_fd);
            }

            FileRequest::CloseDirectory { dir_fd } => {
                // TODO: Implement close directory
                vprintln!("FileService: Close directory fd {}", dir_fd);
            }

            FileRequest::CloneFds { from_pid, to_pid } => {
                let mut fd_mgr = self.fd_manager.lock().unwrap();
                fd_mgr.clone_fds(from_pid, to_pid)?;
            }

            _ => {
                vprintln!("FileService: Unhandled file request");
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

            FileRequest::WriteData { fd, data } => {
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

            FileRequest::DiskInfo => {
                let disk = self.disk.lock().map_err(|_| {
                    GenshinError::Service(ServiceError::Other { code: 40, msg: "disk lock".into() })
                })?;
                let state = disk.dump_state();
                let _ = envelope.respond_success(ResponseData::DiskStats {
                    total_sectors: state.total_sectors,
                    used_sectors: disk.used_sectors_count(),
                    total_bytes: state.total_bytes,
                });
            }

            FileRequest::ListDir { path } => {
                match self.handle_listdir_with_response(pid, &path) {
                    Ok(entries) => {
                        let _ = envelope.respond_success(ResponseData::StringList(entries));
                    }
                    Err(e) => {
                        let _ = envelope.respond_error(MessagingServiceError::Other {
                            code: 50, msg: e.to_string(),
                        });
                    }
                }
            }

            FileRequest::CloneFds { from_pid, to_pid } => {
                let mut fd_mgr = self.fd_manager.lock().unwrap();
                fd_mgr.clone_fds(from_pid, to_pid)?;
                let _ = envelope.respond_success(ResponseData::Void);
            }

            _ => {
                vprintln!("FileService: Unhandled file request");
                let _ = envelope.respond_success(ResponseData::Void);
            }
        }

        Ok(())
    }

    // ========== File Operations Handlers ==========

    fn handle_open(&self, pid: Pid, path: String, flags: OpenFlags) -> GenshinResult<()> {
        let mut vfs = Self::lock_mutex(&self.vfs)?;

        // Look up file by path, create if flags.create is set
        let node = match vfs.lookup_path(&path) {
            Ok(node) => node,
            Err(_) if flags.create => {
                // Extract parent directory and filename
                let (parent_path, name) = match path.rfind('/') {
                    Some(pos) if pos == 0 => ("/".to_string(), path[1..].to_string()),
                    Some(pos) => (path[..pos].to_string(), path[pos+1..].to_string()),
                    None => return Err(GenshinError::Service(ServiceError::InvalidArguments {
                        param: "path".to_string(),
                        reason: "Cannot create root as file".to_string(),
                    })),
                };
                // Look up parent directory inode
                let parent = vfs.lookup_path(&parent_path).map_err(|_| {
                    GenshinError::Service(ServiceError::NotFound {
                        resource_type: "Parent directory".to_string(),
                        id: parent_path.clone(),
                    })
                })?;
                let parent_inode = parent.lock().map_err(|e| {
                    GenshinError::Service(ServiceError::Other {
                        code: 20,
                        msg: format!("Mutex poisoned: {}", e),
                    })
                })?.inode;
                vfs.create_file(parent_inode, name.clone(), pid)?;
                vfs.lookup_path(&path)?
            }
            Err(e) => return Err(e),
        };

        let node = Self::lock_mutex(&node)?;
        if !node.is_file() {
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
            // Update mode for cached file to match new open flags
            {
                let mut f = file.lock().map_err(|e| {
                    GenshinError::Service(ServiceError::Other {
                        code: 20,
                        msg: format!("Mutex poisoned: {}", e),
                    })
                })?;
                f.mode = Self::open_flags_to_mode(flags)?;
                f.position = 0;
                f.eof = false;
            }
            file.clone()
        } else {
            let new_file = Arc::new(Mutex::new(File::new(
                inode,
                path.clone(),
                pid,
                Self::open_flags_to_mode(flags)?,
            )));
            // Load existing file content from disk
            {
                let vfs = Self::lock_mutex(&self.vfs)?;
                if let Some(vnode) = vfs.lookup(inode) {
                    let vn = vnode.lock().unwrap();
                    if !vn.blocks.is_empty() {
                        let mut f = new_file.lock().unwrap();
                        f.start_sector = Some(vn.blocks[0] as u32);
                        f.sector_count = vn.blocks.len() as u32;
                        f.metadata.size = vn.size;
                        let disk = self.disk.lock().unwrap();
                        f.load_from_disk(&disk).ok();
                        drop(disk);
                    }
                }
                drop(vfs);
            }
            Self::lock_mutex(&self.open_files)?.insert(inode, new_file.clone());
            new_file
        };

        // Allocate file descriptor
        let mut fd_manager = Self::lock_mutex(&self.fd_manager)?;
        let fd = fd_manager.allocate(pid, file, 0)?; // TODO: Convert flags to u32

        vprintln!("FileService: Opened file {} for pid {}, fd={}", path, pid, fd);

        // TODO: Send response with fd
        Ok(())
    }

    fn handle_open_with_response(&self, pid: Pid, path: String, flags: OpenFlags, envelope: &Envelope) -> GenshinResult<()> {
        let mut vfs = Self::lock_mutex(&self.vfs)?;

        let node = match vfs.lookup_path(&path) {
            Ok(n) => n,
            Err(_) if flags.create => {
                let (parent_path, name) = match path.rfind('/') {
                    Some(pos) if pos == 0 => ("/".to_string(), path[1..].to_string()),
                    Some(pos) => (path[..pos].to_string(), path[pos+1..].to_string()),
                    None => {
                        let _ = envelope.respond_error(MessagingServiceError::InvalidArguments {
                            msg: "Cannot create root as file".to_string(),
                        });
                        return Err(GenshinError::Service(ServiceError::InvalidArguments {
                            param: "path".to_string(),
                            reason: "Cannot create root as file".to_string(),
                        }));
                    }
                };
                let parent = match vfs.lookup_path(&parent_path) {
                    Ok(p) => p,
                    Err(_) => {
                        let _ = envelope.respond_error(MessagingServiceError::NotFound {
                            resource: "Parent directory".to_string(),
                            id: parent_path.clone(),
                        });
                        return Err(GenshinError::Service(ServiceError::NotFound {
                            resource_type: "Parent directory".to_string(),
                            id: parent_path.to_string(),
                        }));
                    }
                };
                let parent_inode = parent.lock().map_err(|e| {
                    GenshinError::Service(ServiceError::Other { code: 20, msg: format!("Mutex poisoned: {}", e) })
                })?.inode;
                vfs.create_file(parent_inode, name.clone(), pid).map_err(|e| {
                    let _ = envelope.respond_error(MessagingServiceError::Other {
                        code: 21, msg: e.to_string(),
                    });
                    e
                })?;
                vfs.lookup_path(&path).map_err(|e| {
                    let _ = envelope.respond_error(MessagingServiceError::NotFound {
                        resource: "File".to_string(),
                        id: path.clone(),
                    });
                    e
                })?
            }
            Err(e) => {
                let _ = envelope.respond_error(MessagingServiceError::NotFound {
                    resource: "File".to_string(),
                    id: path.clone(),
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
            // Update mode for cached file to match new open flags
            {
                let mut f = file.lock().map_err(|e| {
                    GenshinError::Service(ServiceError::Other {
                        code: 20,
                        msg: format!("Mutex poisoned: {}", e),
                    })
                })?;
                f.mode = Self::open_flags_to_mode(flags)?;
                f.position = 0;
                f.eof = false;
            }
            file.clone()
        } else {
            let new_file = Arc::new(Mutex::new(File::new(
                inode,
                path.clone(),
                pid,
                Self::open_flags_to_mode(flags)?,
            )));
            // Load existing file content from disk
            {
                let vfs = Self::lock_mutex(&self.vfs)?;
                if let Some(vnode) = vfs.lookup(inode) {
                    let vn = vnode.lock().unwrap();
                    if !vn.blocks.is_empty() {
                        let mut f = new_file.lock().unwrap();
                        f.start_sector = Some(vn.blocks[0] as u32);
                        f.sector_count = vn.blocks.len() as u32;
                        f.metadata.size = vn.size;
                        let disk = self.disk.lock().unwrap();
                        f.load_from_disk(&disk).ok();
                        drop(disk);
                    }
                }
                drop(vfs);
            }
            Self::lock_mutex(&self.open_files)?.insert(inode, new_file.clone());
            new_file
        };

        // Allocate file descriptor
        let mut fd_manager = Self::lock_mutex(&self.fd_manager)?;
        let fd = fd_manager.allocate(pid, file, 0)?; // TODO: Convert flags to u32

        vprintln!("FileService: Opened file {} for pid {}, fd={}", path, pid, fd);

        // Send response with fd
        let _ = envelope.respond_success(ResponseData::Fd(fd));
        Ok(())
    }

    fn handle_close(&self, pid: Pid, fd: Fd) -> GenshinResult<()> {
        let mut fd_manager = Self::lock_mutex(&self.fd_manager)?;
        fd_manager.close(pid, fd)?;

        vprintln!("FileService: Closed fd {} for pid {}", fd, pid);
        Ok(())
    }

    fn handle_close_with_response(&self, pid: Pid, fd: Fd, envelope: &Envelope) -> GenshinResult<()> {
        // Sync file data to disk before closing
        {
            let fd_manager = Self::lock_mutex(&self.fd_manager)?;
            if let Some(open_file) = fd_manager.get(pid, fd) {
                let mut file = open_file.file.lock().map_err(|e| {
                    GenshinError::Service(ServiceError::Other {
                        code: 30, msg: format!("Mutex: {}", e)
                    })
                })?;
                if file.is_dirty() {
                    let disk_guard = self.disk.lock().map_err(|e| {
                        GenshinError::Service(ServiceError::Other { code: 29, msg: format!("Disk: {}", e) })
                    })?;
                    if let Err(e) = file.sync_to_disk(&disk_guard) {
                        eprintln!("FileService: sync failed for fd {}: {}", fd, e);
                    }
                    vprintln!("FileService: Synced fd {} to disk ({} sectors, start={:?})",
                        fd, file.sector_count, file.start_sector);
                }
            }
        }

        let mut fd_manager = Self::lock_mutex(&self.fd_manager)?;
        fd_manager.close(pid, fd)?;

        vprintln!("FileService: Closed fd {} for pid {}", fd, pid);
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

        vprintln!("FileService: Read {} bytes from fd {} for pid {}", data.len(), fd, pid);

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

        vprintln!("FileService: Read {} bytes from fd {} for pid {}", data.len(), fd, pid);

        // Send response with actual data
        let _ = envelope.respond_success(ResponseData::Bytes(data));
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

        // Sync to disk for persistence
        {
            let inode = { open_file.file.lock().unwrap().inode };
            let mut f = open_file.file.lock().unwrap();
            let disk = self.disk.lock().unwrap();
            if f.sync_to_disk(&disk).is_ok() {
                if let Some(start) = f.start_sector {
                    let mut vfs = self.vfs.lock().unwrap();
                    if let Some(vnode) = vfs.lookup(inode) {
                        let mut vn = vnode.lock().unwrap();
                        vn.size = f.data.len() as u64;
                        vn.blocks = (0..f.sector_count).map(|i: u32| (start + i) as u64).collect();
                    }
                }
            }
        }

        vprintln!("FileService: Wrote {} bytes to fd {} for pid {}", written, fd, pid);

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

        // Sync to disk for persistence
        {
            let inode = { open_file.file.lock().unwrap().inode };
            let mut f = open_file.file.lock().unwrap();
            let disk = self.disk.lock().unwrap();
            if f.sync_to_disk(&disk).is_ok() {
                if let Some(start) = f.start_sector {
                    let mut vfs = self.vfs.lock().unwrap();
                    if let Some(vnode) = vfs.lookup(inode) {
                        let mut vn = vnode.lock().unwrap();
                        vn.size = f.data.len() as u64;
                        vn.blocks = (0..f.sector_count).map(|i: u32| (start + i) as u64).collect();
                    }
                }
            }
        }

        vprintln!("FileService: Wrote {} bytes to fd {} for pid {}", written, fd, pid);

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

        vprintln!("FileService: Seeked fd {} to position {} for pid {}", fd, new_position, pid);

        // TODO: Send response with new position
        Ok(())
    }

    // ========== File Management Handlers ==========

    fn handle_create(&self, pid: Pid, path: String, _permissions: u16) -> GenshinResult<()> {
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

        vprintln!("FileService: Created file {} (inode {}) for pid {}", path, inode, pid);

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

        vprintln!("FileService: Deleted file {} for pid {}", path, pid);

        // TODO: Send response
        Ok(())
    }

    fn handle_stat(&self, pid: Pid, path: String) -> GenshinResult<()> {
        let vfs = Self::lock_mutex(&self.vfs)?;

        let node = vfs.lookup_path(&path)?;
        let node = Self::lock_mutex(&node)?;

        vprintln!("FileService: Stat file {} for pid {}: size={}", path, pid, node.size);

        // TODO: Send response with metadata
        Ok(())
    }

    // ========== Directory Operations Handlers ==========

    fn handle_mkdir(&self, pid: Pid, path: String, _permissions: u16) -> GenshinResult<()> {
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

        vprintln!("FileService: Created directory {} (inode {}) for pid {}", path, inode, pid);

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

        vprintln!("FileService: Removed directory {} for pid {}", path, pid);

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

        vprintln!("FileService: Listed directory {} for pid {}: {} entries", path, pid, entries.len());

        // TODO: Send response with entries
        Ok(())
    }

    /// List directory and return entries
    fn handle_listdir_with_response(&self, pid: Pid, path: &str) -> GenshinResult<Vec<String>> {
        let vfs = Self::lock_mutex(&self.vfs)?;
        let node = vfs.lookup_path(path)?;
        let node = Self::lock_mutex(&node)?;
        if !node.is_directory() {
            return Err(GenshinError::Service(ServiceError::InvalidArguments {
                param: "path".to_string(), reason: "Not a directory".to_string(),
            }));
        }
        let mut entries: Vec<String> = node.children.keys().cloned().collect();
        entries.sort();
        vprintln!("FileService: Listed {} for pid {}: {} entries", path, pid, entries.len());
        Ok(entries)
    }

    // ========== File Descriptor Operations Handlers ==========
    // ========== File Descriptor Operations Handlers ==========

    fn handle_dup(&self, pid: Pid, old_fd: Fd) -> GenshinResult<()> {
        let mut fd_manager = Self::lock_mutex(&self.fd_manager)?;
        let new_fd = fd_manager.dup(pid, old_fd)?;

        vprintln!("FileService: Duplicated fd {} to {} for pid {}", old_fd, new_fd, pid);

        // TODO: Send response with new_fd
        Ok(())
    }

    fn handle_dup2(&self, pid: Pid, old_fd: Fd, new_fd: Fd) -> GenshinResult<()> {
        // TODO: Implement dup2 (duplicate to specific fd)
        vprintln!("FileService: Dup2 fd {} to {} for pid {}", old_fd, new_fd, pid);

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

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::messaging::LockedBus;
    use crate::messaging::OpenFlags;
    use std::thread;
    use std::time::Duration;

    fn wait_ms(ms: u64) {
        thread::sleep(Duration::from_millis(ms));
    }

    /// Helper: create a FileService with a listening bus subscriber
    fn create_service() -> (Arc<LockedBus>, FileService) {
        let bus = Arc::new(LockedBus::new());
        let service = FileService::new(bus.clone(), 256, 1024 * 1024);
        (bus.clone(), service)
    }

    /// Helper: send file request and process it synchronously
    fn send_and_process(bus: &Arc<LockedBus>, msg: KernelMsg, service: &FileService) {
        bus.send(msg).unwrap();
        wait_ms(50);
        // Process one message
        if let Ok(env) = service.receiver.try_recv() {
            let _ = service.handle_envelope(env);
        }
    }

    #[test]
    fn test_create_and_list_directory() {
        let (bus, service) = create_service();

        // Create a directory
        let msg = KernelMsg::File(FileRequest::CreateDirectory {
            path: "/testdir".to_string(),
        });
        send_and_process(&bus, msg, &service);

        // Verify it exists via Stat
        let stat = KernelMsg::File(FileRequest::Stat {
            path: "/testdir".to_string(),
        });
        send_and_process(&bus, stat, &service);

        // List root directory
        let ls = KernelMsg::File(FileRequest::OpenDirectory {
            path: "/".to_string(),
        });
        send_and_process(&bus, ls, &service);

        // Verify VFS has the directory
        let vfs = service.vfs.lock().unwrap();
        let root = vfs.lookup(0).unwrap();
        let root_node = root.lock().unwrap();
        assert!(root_node.children.contains_key("testdir"));
    }

    #[test]
    fn test_create_nested_directories() {
        let (bus, service) = create_service();

        send_and_process(&bus, KernelMsg::File(FileRequest::CreateDirectory {
            path: "/a".to_string(),
        }), &service);
        send_and_process(&bus, KernelMsg::File(FileRequest::CreateDirectory {
            path: "/b".to_string(),
        }), &service);

        let vfs = service.vfs.lock().unwrap();
        let root = vfs.lookup(0).unwrap();
        let root_node = root.lock().unwrap();
        assert!(root_node.children.contains_key("a"));
        assert!(root_node.children.contains_key("b"));
        assert_eq!(root_node.children.len(), 2);
    }

    #[test]
    fn test_stat_directory() {
        let (bus, service) = create_service();

        send_and_process(&bus, KernelMsg::File(FileRequest::CreateDirectory {
            path: "/mydir".to_string(),
        }), &service);

        let vfs = service.vfs.lock().unwrap();
        // mydir should exist in root's children
        let root = vfs.lookup(0).unwrap();
        let root_node = root.lock().unwrap();
        let mydir_inode = root_node.children.get("mydir").unwrap();
        let mydir = vfs.lookup(*mydir_inode).unwrap();
        let mydir_node = mydir.lock().unwrap();
        assert_eq!(mydir_node.name, "mydir");
    }

    #[test]
    fn test_delete_node() {
        let (bus, service) = create_service();

        // Create then delete
        send_and_process(&bus, KernelMsg::File(FileRequest::CreateDirectory {
            path: "/tempdir".to_string(),
        }), &service);

        send_and_process(&bus, KernelMsg::File(FileRequest::Unlink {
            path: "/tempdir".to_string(),
        }), &service);

        // Root should no longer have tempdir
        let vfs = service.vfs.lock().unwrap();
        let root = vfs.lookup(0).unwrap();
        let root_node = root.lock().unwrap();
        assert!(!root_node.children.contains_key("tempdir"));
    }

    #[test]
    fn test_unlink_nonexistent_graceful() {
        let (bus, service) = create_service();
        let msg = KernelMsg::File(FileRequest::Unlink {
            path: "/nonexistent".to_string(),
        });
        bus.send(msg).unwrap();
        wait_ms(100);
        // Verify VFS is still in a valid state
        let vfs = service.vfs.lock().unwrap();
        assert!(vfs.lookup(0).is_some());
    }

    #[test]
    fn test_create_many_directories() {
        let (bus, service) = create_service();
        for i in 0..10 {
            send_and_process(&bus, KernelMsg::File(FileRequest::CreateDirectory {
                path: format!("/dir{}", i),
            }), &service);
        }
        let vfs = service.vfs.lock().unwrap();
        let root = vfs.lookup(0).unwrap();
        let root_node = root.lock().unwrap();
        assert_eq!(root_node.children.len(), 10);
    }

    #[test]
    fn test_vfs_inode_sequence() {
        let (bus, service) = create_service();

        send_and_process(&bus, KernelMsg::File(FileRequest::CreateDirectory {
            path: "/first".to_string(),
        }), &service);
        send_and_process(&bus, KernelMsg::File(FileRequest::CreateDirectory {
            path: "/second".to_string(),
        }), &service);

        let vfs = service.vfs.lock().unwrap();
        let root = vfs.lookup(0).unwrap();
        let root_node = root.lock().unwrap();
        let first = root_node.children.get("first").unwrap();
        let second = root_node.children.get("second").unwrap();
        assert_eq!(*first, 1);
        assert_eq!(*second, 2);
    }
}
