// File Descriptor Management Module
//


use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use crate::messaging::Pid;
use crate::{GenshinResult, GenshinError, ServiceError};
use super::file::{File, OpenMode};

/// File descriptor number
pub type Fd = u32;

/// Open file entry
#[derive(Debug)]
pub struct OpenFile {
    /// File descriptor number
    pub fd: Fd,

    /// Process that owns this file descriptor
    pub owner: Pid,

    /// The actual file
    pub file: Arc<Mutex<File>>,

    /// Open flags
    pub flags: u32,

    /// Close-on-exec flag
    pub close_on_exec: bool,
}

impl OpenFile {
    /// Create a new open file entry
    pub fn new(fd: Fd, owner: Pid, file: Arc<Mutex<File>>, flags: u32) -> Self {
        Self {
            fd,
            owner,
            file,
            flags,
            close_on_exec: false,
        }
    }

    /// Read from file
    pub fn read(&self, count: usize) -> GenshinResult<Vec<u8>> {
        let mut file = self.file.lock().map_err(|e| {
            GenshinError::Service(ServiceError::Other {
                code: 1,
                msg: format!("Mutex poisoned: {}", e),
            })
        })?;
        file.read(count)
    }

    /// Write to file
    pub fn write(&self, data: &[u8]) -> GenshinResult<usize> {
        let mut file = self.file.lock().map_err(|e| {
            GenshinError::Service(ServiceError::Other {
                code: 2,
                msg: format!("Mutex poisoned: {}", e),
            })
        })?;
        file.write(data)
    }

    /// Seek in file
    pub fn seek(&self, position: u64) -> GenshinResult<u64> {
        let mut file = self.file.lock().map_err(|e| {
            GenshinError::Service(ServiceError::Other {
                code: 3,
                msg: format!("Mutex poisoned: {}", e),
            })
        })?;
        file.seek(position)
    }

    /// Sync file
    pub fn sync(&self) -> GenshinResult<()> {
        let mut file = self.file.lock().map_err(|e| {
            GenshinError::Service(ServiceError::Other {
                code: 4,
                msg: format!("Mutex poisoned: {}", e),
            })
        })?;
        file.sync()
    }

    /// Get file size
    pub fn size(&self) -> GenshinResult<u64> {
        let file = self.file.lock().map_err(|e| {
            GenshinError::Service(ServiceError::Other {
                code: 5,
                msg: format!("Mutex poisoned: {}", e),
            })
        })?;
        Ok(file.size())
    }
}

/// File table - manages all open files for a process
#[derive(Debug)]
pub struct FileTable {
    /// Open file descriptors (fd -> OpenFile)
    descriptors: HashMap<Fd, Arc<OpenFile>>,

    /// Next available file descriptor
    next_fd: Fd,

    /// Maximum file descriptors
    max_fds: Fd,
}

impl FileTable {
    /// Create a new file table
    pub fn new(max_fds: Fd) -> Self {
        Self {
            descriptors: HashMap::new(),
            next_fd: 3, // Start from 3 (0=stdin, 1=stdout, 2=stderr)
            max_fds,
        }
    }

    /// Allocate a new file descriptor
    pub fn allocate(&mut self, owner: Pid, file: Arc<Mutex<File>>, flags: u32) -> GenshinResult<Fd> {
        // Find next available fd
        let fd = self.next_fd;

        if fd >= self.max_fds {
            return Err(GenshinError::Service(ServiceError::ResourceExhausted {
                resource: "File descriptors".to_string(),
                available: 0,
                requested: 1,
            }));
        }

        self.next_fd += 1;

        let open_file = Arc::new(OpenFile::new(fd, owner, file, flags));
        self.descriptors.insert(fd, open_file);

        Ok(fd)
    }

    /// Get file descriptor
    pub fn get(&self, fd: Fd) -> Option<Arc<OpenFile>> {
        self.descriptors.get(&fd).cloned()
    }

    /// Close file descriptor
    pub fn close(&mut self, fd: Fd) -> GenshinResult<()> {
        self.descriptors.remove(&fd)
            .ok_or_else(|| GenshinError::Service(ServiceError::NotFound {
                resource_type: "File descriptor".to_string(),
                id: fd.to_string(),
            }))?;

        Ok(())
    }

    /// Duplicate file descriptor
    pub fn dup(&mut self, old_fd: Fd) -> GenshinResult<Fd> {
        let open_file = self.get(old_fd)
            .ok_or_else(|| GenshinError::Service(ServiceError::NotFound {
                resource_type: "File descriptor".to_string(),
                id: old_fd.to_string(),
            }))?;

        let new_fd = self.next_fd;

        if new_fd >= self.max_fds {
            return Err(GenshinError::Service(ServiceError::ResourceExhausted {
                resource: "File descriptors".to_string(),
                available: 0,
                requested: 1,
            }));
        }

        self.next_fd += 1;
        self.descriptors.insert(new_fd, open_file);

        Ok(new_fd)
    }

    /// Get all file descriptors
    pub fn list_fds(&self) -> Vec<Fd> {
        self.descriptors.keys().copied().collect()
    }

    /// Get file descriptor count
    pub fn count(&self) -> usize {
        self.descriptors.len()
    }

    /// List all open file descriptor numbers
    pub fn fd_list(&self) -> Vec<Fd> {
        self.descriptors.keys().cloned().collect()
    }

    /// Check if file descriptor exists
    pub fn contains(&self, fd: Fd) -> bool {
        self.descriptors.contains_key(&fd)
    }
}

/// File descriptor manager for all processes
#[derive(Debug)]
pub struct FileDescriptorManager {
    /// Process file tables (pid -> FileTable)
    process_tables: HashMap<Pid, FileTable>,

    /// Maximum file descriptors per process
    max_fds_per_process: Fd,

    /// Standard file descriptors (shared)
    stdin: Option<Arc<Mutex<File>>>,
    stdout: Option<Arc<Mutex<File>>>,
    stderr: Option<Arc<Mutex<File>>>,
}

impl FileDescriptorManager {
    /// Create a new file descriptor manager
    pub fn new(max_fds_per_process: Fd) -> Self {
        Self {
            process_tables: HashMap::new(),
            max_fds_per_process,
            stdin: None,
            stdout: None,
            stderr: None,
        }
    }

    /// Get or create file table for process
    pub fn get_table(&mut self, pid: Pid) -> &mut FileTable {
        self.process_tables
            .entry(pid)
            .or_insert_with(|| FileTable::new(self.max_fds_per_process))
    }

    /// Allocate file descriptor for process
    pub fn allocate(&mut self, pid: Pid, file: Arc<Mutex<File>>, flags: u32) -> GenshinResult<Fd> {
        let table = self.get_table(pid);
        table.allocate(pid, file, flags)
    }

    /// Get file descriptor for process
    pub fn get(&self, pid: Pid, fd: Fd) -> Option<Arc<OpenFile>> {
        self.process_tables.get(&pid)
            .and_then(|table| table.get(fd))
    }

    /// Close file descriptor for process
    pub fn close(&mut self, pid: Pid, fd: Fd) -> GenshinResult<()> {
        let table = self.process_tables.get_mut(&pid)
            .ok_or_else(|| GenshinError::Service(ServiceError::NotFound {
                resource_type: "Process file table".to_string(),
                id: pid.to_string(),
            }))?;

        table.close(fd)
    }

    /// Duplicate file descriptor for process
    pub fn dup(&mut self, pid: Pid, old_fd: Fd) -> GenshinResult<Fd> {
        let table = self.process_tables.get_mut(&pid)
            .ok_or_else(|| GenshinError::Service(ServiceError::NotFound {
                resource_type: "Process file table".to_string(),
                id: pid.to_string(),
            }))?;

        table.dup(old_fd)
    }

    /// Remove all file descriptors for process
    pub fn remove_process(&mut self, pid: Pid) -> GenshinResult<()> {
        self.process_tables.remove(&pid)
            .ok_or_else(|| GenshinError::Service(ServiceError::NotFound {
                resource_type: "Process file table".to_string(),
                id: pid.to_string(),
            }))?;

        Ok(())
    }

    /// Clone all file descriptors from one process to another (for fork)
    pub fn clone_fds(&mut self, from_pid: Pid, to_pid: Pid) -> GenshinResult<()> {
        // Ensure target has a file table
        self.get_table(to_pid);
        
        // Collect FD numbers from source
        let fd_list: Vec<Fd> = if let Some(table) = self.process_tables.get(&from_pid) {
            table.fd_list()
        } else {
            return Ok(());
        };
        
        // Clone each FD into target with same FD number
        for fd in &fd_list {
            if let Some(src) = self.get(from_pid, *fd) {
                let file = src.file.clone();
                let flags = src.flags;
                if let Some(target) = self.process_tables.get_mut(&to_pid) {
                    let cloned = OpenFile::new(*fd, to_pid, file, flags);
                    target.descriptors.insert(*fd, Arc::new(cloned));
                    if target.next_fd <= *fd {
                        target.next_fd = *fd + 1;
                    }
                }
            }
        }

        Ok(())
    }
    /// Get file descriptor count for process
    pub fn count(&self, pid: Pid) -> usize {
        self.process_tables.get(&pid)
            .map(|table| table.count())
            .unwrap_or(0)
    }

    /// Set standard input
    pub fn set_stdin(&mut self, file: Arc<Mutex<File>>) {
        self.stdin = Some(file);
    }

    /// Set standard output
    pub fn set_stdout(&mut self, file: Arc<Mutex<File>>) {
        self.stdout = Some(file);
    }

    /// Set standard error
    pub fn set_stderr(&mut self, file: Arc<Mutex<File>>) {
        self.stderr = Some(file);
    }

    /// Get standard input
    pub fn get_stdin(&self) -> Option<&Arc<Mutex<File>>> {
        self.stdin.as_ref()
    }

    /// Get standard output
    pub fn get_stdout(&self) -> Option<&Arc<Mutex<File>>> {
        self.stdout.as_ref()
    }

    /// Get standard error
    pub fn get_stderr(&self) -> Option<&Arc<Mutex<File>>> {
        self.stderr.as_ref()
    }
}

impl Default for FileDescriptorManager {
    fn default() -> Self {
        Self::new(256) // Default: 256 file descriptors per process
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_table_creation() {
        let table = FileTable::new(256);
        assert_eq!(table.count(), 0);
        assert!(!table.contains(0));
    }

    #[test]
    fn test_allocate_fd() {
        let mut table = FileTable::new(256);

        let file = Arc::new(Mutex::new(File::new(
            1,
            "test.txt".to_string(),
            100,
            OpenMode::ReadWrite,
        )));

        let fd = table.allocate(100, file, 0).unwrap();
        assert_eq!(fd, 3); // First available fd
        assert_eq!(table.count(), 1);
        assert!(table.contains(fd));
    }

    #[test]
    fn test_get_fd() {
        let mut table = FileTable::new(256);

        let file = Arc::new(Mutex::new(File::new(
            1,
            "test.txt".to_string(),
            100,
            OpenMode::ReadWrite,
        )));

        let fd = table.allocate(100, file.clone(), 0).unwrap();

        let open_file = table.get(fd);
        assert!(open_file.is_some());

        let open_file = open_file.unwrap();
        assert_eq!(open_file.fd, fd);
    }

    #[test]
    fn test_close_fd() {
        let mut table = FileTable::new(256);

        let file = Arc::new(Mutex::new(File::new(
            1,
            "test.txt".to_string(),
            100,
            OpenMode::ReadWrite,
        )));

        let fd = table.allocate(100, file, 0).unwrap();
        table.close(fd).unwrap();

        assert_eq!(table.count(), 0);
        assert!(!table.contains(fd));
    }

    #[test]
    fn test_dup_fd() {
        let mut table = FileTable::new(256);

        let file = Arc::new(Mutex::new(File::new(
            1,
            "test.txt".to_string(),
            100,
            OpenMode::ReadWrite,
        )));

        let fd1 = table.allocate(100, file.clone(), 0).unwrap();
        let fd2 = table.dup(fd1).unwrap();

        assert_ne!(fd1, fd2);
        assert_eq!(table.count(), 2);

        // Both should point to same file
        let file1 = table.get(fd1).unwrap();
        let file2 = table.get(fd2).unwrap();
        assert!(Arc::ptr_eq(&file1.file, &file2.file));
    }

    #[test]
    fn test_fd_exhaustion() {
        let mut table = FileTable::new(5); // Max 5 fds

        let file = Arc::new(Mutex::new(File::new(
            1,
            "test.txt".to_string(),
            100,
            OpenMode::ReadWrite,
        )));

        // Allocate all available fds
        for _ in 0..5 {
            let result = table.allocate(100, file.clone(), 0);
            if result.is_err() {
                break; // Expected to fail when exhausted
            }
        }

        // Next allocation should fail
        let result = table.allocate(100, file, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_fd_manager() {
        let mut manager = FileDescriptorManager::new(256);

        let file = Arc::new(Mutex::new(File::new(
            1,
            "test.txt".to_string(),
            100,
            OpenMode::ReadWrite,
        )));

        let fd = manager.allocate(100, file, 0).unwrap();
        assert_eq!(manager.count(100), 1);

        let open_file = manager.get(100, fd);
        assert!(open_file.is_some());

        manager.close(100, fd).unwrap();
        assert_eq!(manager.count(100), 0);
    }

    #[test]
    fn test_open_file_operations() {
        let file = Arc::new(Mutex::new(File::new(
            1,
            "test.txt".to_string(),
            100,
            OpenMode::ReadWrite,
        )));

        let open_file = OpenFile::new(3, 100, file.clone(), 0);

        // Write through open file
        open_file.write(b"Hello").unwrap();

        // Read through open file
        file.lock().unwrap().seek(0).unwrap();
        let data = open_file.read(5).unwrap();
        assert_eq!(data, b"Hello".to_vec());
    }

    #[test]
    fn test_multiple_process_tables() {
        let mut manager = FileDescriptorManager::new(256);

        let file = Arc::new(Mutex::new(File::new(
            1,
            "test.txt".to_string(),
            100,
            OpenMode::ReadWrite,
        )));

        // Allocate for different processes
        let fd1 = manager.allocate(100, file.clone(), 0).unwrap();
        let fd2 = manager.allocate(200, file.clone(), 0).unwrap();

        assert_eq!(manager.count(100), 1);
        assert_eq!(manager.count(200), 1);

        // Remove one process
        manager.remove_process(100).unwrap();
        assert_eq!(manager.count(100), 0);
        assert_eq!(manager.count(200), 1);
    }

    #[test]
    fn test_close_nonexistent_fd() {
        let mut table = FileTable::new(256);

        let result = table.close(999);
        assert!(result.is_err());
    }

    #[test]
    fn test_get_nonexistent_fd() {
        let table = FileTable::new(256);

        let result = table.get(999);
        assert!(result.is_none());
    }
}
