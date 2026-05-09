// File and Directory Module
//
// 曾国藩曰：
// "文书档案，各有其类。"
// 文件和目录模块管理文件的具体操作和元数据。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use crate::messaging::Pid;
use crate::hardware::VirtualDisk;
use crate::{GenshinResult, GenshinError, ServiceError};

/// File permissions (Unix-style)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilePermissions {
    /// Owner permissions
    pub owner: u8,

    /// Group permissions
    pub group: u8,

    /// Others permissions
    pub others: u8,
}

impl FilePermissions {
    /// Create new permissions
    pub fn new(owner: u8, group: u8, others: u8) -> Self {
        Self { owner, group, others }
    }

    /// Create default permissions (rw-r--r--)
    pub fn default() -> Self {
        Self::new(6, 4, 4)
    }

    /// Convert to u16 (Unix-style)
    pub fn to_u16(&self) -> u16 {
        ((self.owner as u16) << 6) | ((self.group as u16) << 3) | (self.others as u16)
    }

    /// Check if owner can read
    pub fn owner_can_read(&self) -> bool {
        (self.owner & 4) != 0
    }

    /// Check if owner can write
    pub fn owner_can_write(&self) -> bool {
        (self.owner & 2) != 0
    }

    /// Check if owner can execute
    pub fn owner_can_execute(&self) -> bool {
        (self.owner & 1) != 0
    }
}

/// File metadata
#[derive(Debug, Clone)]
pub struct FileMetadata {
    /// File size
    pub size: u64,

    /// Creation time
    pub created: u64,

    /// Modified time
    pub modified: u64,

    /// Accessed time
    pub accessed: u64,

    /// Owner process ID
    pub owner: Pid,

    /// Permissions
    pub permissions: FilePermissions,

    /// File type
    pub file_type: FileType,
}

/// File type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    RegularFile,
    Directory,
    SymLink,
    BlockDevice,
    CharDevice,
    FIFO,
    Socket,
}

/// File - represents an open file
#[derive(Debug)]
pub struct File {
    /// Inode number
    pub inode: u64,

    /// File name
    pub name: String,

    /// File metadata
    pub metadata: FileMetadata,

    /// File position (cursor)
    pub position: u64,

    /// Open mode (read/write/append)
    pub mode: OpenMode,

    /// Data blocks
    pub data: Vec<u8>,

    /// Dirty flag (data modified but not synced)
    pub dirty: bool,

    /// EOF flag
    pub eof: bool,

    /// Starting sector on disk (None = not on disk yet)
    pub start_sector: Option<u32>,

    /// Number of sectors allocated on disk
    pub sector_count: u32,
}

/// Open mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenMode {
    Read,
    Write,
    ReadWrite,
    Append,
}

impl File {
    /// Create a new file
    pub fn new(inode: u64, name: String, owner: Pid, mode: OpenMode) -> Self {
        Self {
            inode,
            name,
            metadata: FileMetadata {
                size: 0,
                created: 0,
                modified: 0,
                accessed: 0,
                owner,
                permissions: FilePermissions::default(),
                file_type: FileType::RegularFile,
            },
            position: 0,
            mode,
            data: Vec::new(),
            dirty: false,
            eof: false,
            start_sector: None,
            sector_count: 0,
        }
    }

    /// Read from file
    pub fn read(&mut self, count: usize) -> GenshinResult<Vec<u8>> {
        if self.mode == OpenMode::Write {
            return Err(GenshinError::Service(ServiceError::PermissionDenied {
                operation: "read".to_string(),
                reason: "File opened for write only".to_string(),
            }));
        }

        let start = self.position as usize;
        let end = std::cmp::min(start + count, self.data.len());

        if start >= self.data.len() {
            self.eof = true;
            return Ok(Vec::new());
        }

        let data = self.data[start..end].to_vec();
        self.position += data.len() as u64;
        self.eof = end == self.data.len();

        self.metadata.accessed = 0; // TODO: Get actual time

        Ok(data)
    }

    /// Write to file
    pub fn write(&mut self, data: &[u8]) -> GenshinResult<usize> {
        if self.mode == OpenMode::Read {
            return Err(GenshinError::Service(ServiceError::PermissionDenied {
                operation: "write".to_string(),
                reason: "File opened for read only".to_string(),
            }));
        }

        let write_pos = if self.mode == OpenMode::Append {
            self.data.len() as u64
        } else {
            self.position
        } as usize;

        // Ensure capacity
        let required_size = write_pos + data.len();
        if required_size > self.data.len() {
            self.data.resize(required_size, 0);
        }

        // Write data
        self.data[write_pos..write_pos + data.len()].copy_from_slice(data);
        self.position = (write_pos + data.len()) as u64;
        self.dirty = true;

        // Update metadata
        self.metadata.size = self.data.len() as u64;
        self.metadata.modified = 0; // TODO: Get actual time

        Ok(data.len())
    }

    /// Seek to position
    pub fn seek(&mut self, position: u64) -> GenshinResult<u64> {
        let old_position = self.position;
        self.position = position;
        self.eof = false;
        Ok(old_position)
    }

    /// Get file size
    pub fn size(&self) -> u64 {
        self.metadata.size
    }

    /// Truncate file
    pub fn truncate(&mut self, new_size: u64) -> GenshinResult<()> {
        if new_size < self.data.len() as u64 {
            self.data.truncate(new_size as usize);
        } else {
            self.data.resize(new_size as usize, 0);
        }

        self.metadata.size = new_size;
        self.dirty = true;

        Ok(())
    }

    /// Sync file data to disk sectors
    pub fn sync_to_disk(&mut self, disk: &VirtualDisk) -> GenshinResult<()> {
        if !self.dirty && self.start_sector.is_some() { return Ok(()); }
        let needed = ((self.data.len() + 511) / 512) as u32;
        if let Some(old) = self.start_sector {
            if self.sector_count > 0 { let _ = disk.free_sectors(old, self.sector_count); }
        }
        if needed == 0 { self.start_sector = None; self.sector_count = 0; self.dirty = false; return Ok(()); }
        let start = disk.allocate_sectors(needed)
            .map_err(|e| GenshinError::Service(ServiceError::Io {
                operation: "disk alloc".into(), details: format!("{:?}", e) }))?;
        for i in 0..needed {
            let offset = (i as usize) * 512;
            let end = std::cmp::min(offset + 512, self.data.len());
            let mut buf = vec![0u8; 512];
            buf[..end - offset].copy_from_slice(&self.data[offset..end]);
            disk.write_sector(start + i, &buf).map_err(|e| GenshinError::Service(ServiceError::Io {
                operation: "disk write".into(), details: format!("{:?}", e) }))?;
        }
        self.start_sector = Some(start);
        self.sector_count = needed;
        self.dirty = false;
        Ok(())
    }

    /// Load file data from disk sectors
    pub fn load_from_disk(&mut self, disk: &VirtualDisk) -> GenshinResult<()> {
        let start = match self.start_sector { Some(s) => s, None => { self.data.clear(); return Ok(()); } };
        if self.sector_count == 0 { self.data.clear(); return Ok(()); }
        let mut data = Vec::with_capacity(self.sector_count as usize * 512);
        for i in 0..self.sector_count {
            let sd = disk.read_sector(start + i).map_err(|e| GenshinError::Service(ServiceError::Io {
                operation: "disk read".into(), details: format!("{:?}", e) }))?;
            data.extend_from_slice(&sd);
        }
        data.truncate(self.metadata.size as usize);
        self.data = data;
        self.dirty = false;
        self.eof = false;
        self.position = 0;
        Ok(())
    }

    /// Simple sync (no-op for backward compat, use sync_to_disk for real I/O)
    pub fn sync(&mut self) -> GenshinResult<()> {
        self.dirty = false;
        Ok(())
    }

    /// Check if file is dirty
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Check if at EOF
    pub fn is_eof(&self) -> bool {
        self.eof
    }
}

/// Directory
#[derive(Debug)]
pub struct Directory {
    /// Inode number
    pub inode: u64,

    /// Directory name
    pub name: String,

    /// Entries (name -> inode)
    pub entries: HashMap<String, u64>,

    /// Metadata
    pub metadata: FileMetadata,
}

impl Directory {
    /// Create a new directory
    pub fn new(inode: u64, name: String, owner: Pid) -> Self {
        Self {
            inode,
            name,
            entries: HashMap::new(),
            metadata: FileMetadata {
                size: 0,
                created: 0,
                modified: 0,
                accessed: 0,
                owner,
                permissions: FilePermissions::new(7, 5, 5), // rwxr-xr-x
                file_type: FileType::Directory,
            },
        }
    }

    /// Add entry to directory
    pub fn add_entry(&mut self, name: String, inode: u64) -> GenshinResult<()> {
        if self.entries.contains_key(&name) {
            return Err(GenshinError::Service(ServiceError::InvalidArguments {
                param: "name".to_string(),
                reason: "Entry already exists".to_string(),
            }));
        }

        self.entries.insert(name, inode);
        self.metadata.modified = 0; // TODO: Get actual time
        Ok(())
    }

    /// Remove entry from directory
    pub fn remove_entry(&mut self, name: &str) -> GenshinResult<u64> {
        self.entries.remove(name)
            .ok_or_else(|| GenshinError::Service(ServiceError::NotFound {
                resource_type: "Directory entry".to_string(),
                id: name.to_string(),
            }))
    }

    /// Look up entry
    pub fn lookup(&self, name: &str) -> Option<u64> {
        self.entries.get(name).copied()
    }

    /// List all entries
    pub fn list(&self) -> Vec<String> {
        self.entries.keys().cloned().collect()
    }

    /// Get entry count
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_permissions() {
        let perms = FilePermissions::default();

        assert!(perms.owner_can_read());
        assert!(perms.owner_can_write());
        assert!(!perms.owner_can_execute());
    }

    #[test]
    fn test_file_creation() {
        let file = File::new(1, "test.txt".to_string(), 100, OpenMode::ReadWrite);

        assert_eq!(file.inode, 1);
        assert_eq!(file.name, "test.txt");
        assert_eq!(file.position, 0);
        assert!(!file.is_eof());
        assert!(!file.is_dirty());
    }

    #[test]
    fn test_file_write_read() {
        let mut file = File::new(1, "test.txt".to_string(), 100, OpenMode::ReadWrite);

        let data = b"Hello, World!";
        let written = file.write(data).unwrap();

        assert_eq!(written, data.len());
        assert_eq!(file.size(), data.len() as u64);
        assert!(file.is_dirty());

        // Seek to beginning
        file.seek(0).unwrap();

        let read_data = file.read(data.len()).unwrap();
        assert_eq!(read_data, data.to_vec());
    }

    #[test]
    fn test_file_append() {
        let mut file = File::new(1, "test.txt".to_string(), 100, OpenMode::Append);

        file.write(b"Hello").unwrap();
        file.write(b" World").unwrap();

        assert_eq!(file.size(), 11);
        assert_eq!(file.data, b"Hello World".to_vec());
    }

    #[test]
    fn test_file_truncate() {
        let mut file = File::new(1, "test.txt".to_string(), 100, OpenMode::ReadWrite);

        file.write(b"Hello, World!").unwrap();
        file.truncate(5).unwrap();

        assert_eq!(file.size(), 5);
        assert_eq!(file.data, b"Hello".to_vec());
    }

    #[test]
    fn test_file_read_only() {
        let mut file = File::new(1, "test.txt".to_string(), 100, OpenMode::Read);

        let result = file.write(b"test");
        assert!(result.is_err());
    }

    #[test]
    fn test_file_write_only() {
        let mut file = File::new(1, "test.txt".to_string(), 100, OpenMode::Write);

        let result = file.read(10);
        assert!(result.is_err());
    }

    #[test]
    fn test_file_seek() {
        let mut file = File::new(1, "test.txt".to_string(), 100, OpenMode::ReadWrite);

        file.write(b"Hello, World!").unwrap();

        let old_pos = file.seek(7).unwrap();
        assert_eq!(old_pos, 13);
        assert_eq!(file.position, 7);

        let data = file.read(5).unwrap();
        assert_eq!(data, b"World".to_vec());
    }

    #[test]
    fn test_file_eof() {
        let mut file = File::new(1, "test.txt".to_string(), 100, OpenMode::ReadWrite);

        file.write(b"Hello").unwrap();
        file.seek(0).unwrap();

        file.read(5).unwrap();
        assert!(file.is_eof());

        let data = file.read(10).unwrap();
        assert_eq!(data.len(), 0);
    }

    #[test]
    fn test_directory_creation() {
        let dir = Directory::new(0, "testdir".to_string(), 100);

        assert_eq!(dir.inode, 0);
        assert_eq!(dir.name, "testdir");
        assert_eq!(dir.entry_count(), 0);
    }

    #[test]
    fn test_directory_operations() {
        let mut dir = Directory::new(0, "testdir".to_string(), 100);

        dir.add_entry("file1.txt".to_string(), 1).unwrap();
        dir.add_entry("file2.txt".to_string(), 2).unwrap();

        assert_eq!(dir.entry_count(), 2);
        assert!(dir.lookup("file1.txt").is_some());
        assert!(dir.lookup("file1.txt").unwrap() == 1);

        let entries = dir.list();
        assert_eq!(entries.len(), 2);

        let removed = dir.remove_entry("file1.txt").unwrap();
        assert_eq!(removed, 1);
        assert_eq!(dir.entry_count(), 1);
    }

    #[test]
    fn test_duplicate_entry() {
        let mut dir = Directory::new(0, "testdir".to_string(), 100);

        dir.add_entry("file.txt".to_string(), 1).unwrap();

        let result = dir.add_entry("file.txt".to_string(), 2);
        assert!(result.is_err());
    }

    #[test]
    fn test_remove_nonexistent_entry() {
        let mut dir = Directory::new(0, "testdir".to_string(), 100);

        let result = dir.remove_entry("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_file_sync() {
        let mut file = File::new(1, "test.txt".to_string(), 100, OpenMode::ReadWrite);

        file.write(b"Hello").unwrap();
        assert!(file.is_dirty());

        file.sync().unwrap();
        assert!(!file.is_dirty());
    }
}
