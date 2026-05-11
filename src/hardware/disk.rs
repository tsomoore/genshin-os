// Virtual Disk Simulation — file-backed
//
// 曾国藩曰：
// "凡事之需逐日检点者，文书是也。"
// 磁盘乃持久存储之所，读写不可不慎，记录不可不勤。

use std::sync::{Arc, Mutex};
use std::io::{Seek, SeekFrom, Read, Write};
use std::fs::{File, OpenOptions};
use crate::error::DiskError;

pub const SECTOR_SIZE: usize = 512;

/// File-backed virtual disk
#[derive(Clone)]
pub struct VirtualDisk {
    file: Arc<Mutex<File>>,
    total_sectors: u32,
    /// Allocation bitmap (bit=1 means used), synced to sector 0
    bitmap: Arc<Mutex<Vec<u64>>>,
    path: String,
}

impl VirtualDisk {
    pub fn new(total_sectors: u32, path: &str) -> Self {
        assert!(total_sectors >= 8, "Disk must have at least 8 sectors");

        let bitmap_words = ((total_sectors as usize) + 63) / 64;

        // Try to open existing disk image, or create new one
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)
            .expect("Failed to open disk image");

        let file_len = file.metadata().map(|m| m.len()).unwrap_or(0);
        let expected_len = total_sectors as u64 * SECTOR_SIZE as u64;

        let bitmap = if file_len >= SECTOR_SIZE as u64 {
            // Load bitmap from sector 0
            let mut buf = vec![0u8; SECTOR_SIZE];
            let mut f = file.try_clone().expect("clone");
            f.seek(SeekFrom::Start(0)).ok();
            f.read_exact(&mut buf).ok();
            let mut bitmap = vec![0u64; bitmap_words];
            for (i, chunk) in buf.chunks(8).enumerate() {
                if i < bitmap.len() {
                    let mut bytes = [0u8; 8];
                    bytes[..chunk.len()].copy_from_slice(chunk);
                    bitmap[i] = u64::from_le_bytes(bytes);
                }
            }
            bitmap
        } else {
            // New disk: pre-allocate file
            file.set_len(expected_len).expect("Failed to pre-allocate disk image");
            let mut bitmap = vec![0u64; bitmap_words];
            // Reserve sectors 0-3 for metadata (bitmap + superblock)
            Self::mark_used_range(&mut bitmap, 0, 3);
            bitmap
        };

        Self {
            file: Arc::new(Mutex::new(file)),
            total_sectors,
            bitmap: Arc::new(Mutex::new(bitmap)),
            path: path.to_string(),
        }
    }

    fn check_sector(&self, sector: u32) -> Result<(), DiskError> {
        if sector >= self.total_sectors {
            Err(DiskError::InvalidSector { sector, max_sector: self.total_sectors - 1 })
        } else {
            Ok(())
        }
    }

    pub fn total_sectors(&self) -> u32 { self.total_sectors }
    pub fn size_bytes(&self) -> u64 { self.total_sectors as u64 * SECTOR_SIZE as u64 }

    /// Read a single sector from the file
    pub fn read_sector(&self, sector: u32) -> Result<Vec<u8>, DiskError> {
        self.check_sector(sector)?;
        let mut buf = vec![0u8; SECTOR_SIZE];
        let mut f = self.file.lock().map_err(|_| DiskError::Busy)?;
        f.seek(SeekFrom::Start(sector as u64 * SECTOR_SIZE as u64))
            .map_err(|e| DiskError::IoFailed { operation: "seek".into(), sector })?;
        f.read_exact(&mut buf)
            .map_err(|e| DiskError::IoFailed { operation: "read".into(), sector })?;
        Ok(buf)
    }

    pub fn read_sectors(&self, start_sector: u32, count: u32) -> Result<Vec<u8>, DiskError> {
        if count == 0 { return Ok(Vec::new()); }
        let end_sector = start_sector + count - 1;
        self.check_sector(end_sector)?;
        let mut buf = vec![0u8; count as usize * SECTOR_SIZE];
        let mut f = self.file.lock().map_err(|_| DiskError::Busy)?;
        f.seek(SeekFrom::Start(start_sector as u64 * SECTOR_SIZE as u64))
            .map_err(|e| DiskError::IoFailed { operation: "seek".into(), sector: start_sector })?;
        f.read_exact(&mut buf)
            .map_err(|e| DiskError::IoFailed { operation: "read".into(), sector: start_sector })?;
        Ok(buf)
    }

    /// Write a single sector to the file
    pub fn write_sector(&self, sector: u32, buf: &[u8]) -> Result<(), DiskError> {
        self.check_sector(sector)?;
        if buf.len() != SECTOR_SIZE {
            return Err(DiskError::IoFailed { operation: "write_sector".to_string(), sector });
        }
        let mut f = self.file.lock().map_err(|_| DiskError::Busy)?;
        f.seek(SeekFrom::Start(sector as u64 * SECTOR_SIZE as u64))
            .map_err(|e| DiskError::IoFailed { operation: "seek".into(), sector })?;
        f.write_all(buf)
            .map_err(|e| DiskError::IoFailed { operation: "write".into(), sector })?;
        f.flush().ok();
        Ok(())
    }

    pub fn write_sectors(&self, start_sector: u32, buf: &[u8]) -> Result<(), DiskError> {
        let count = (buf.len() + SECTOR_SIZE - 1) / SECTOR_SIZE;
        if count == 0 { return Ok(()); }
        self.check_sector(start_sector + count as u32 - 1)?;
        let mut f = self.file.lock().map_err(|_| DiskError::Busy)?;
        f.seek(SeekFrom::Start(start_sector as u64 * SECTOR_SIZE as u64))
            .map_err(|e| DiskError::IoFailed { operation: "seek".into(), sector: start_sector })?;
        f.write_all(buf)
            .map_err(|e| DiskError::IoFailed { operation: "write".into(), sector: start_sector })?;
        f.flush().ok();
        Ok(())
    }

    /// Flush bitmap to sector 0
    pub fn flush_bitmap(&self) -> Result<(), DiskError> {
        let bitmap = self.bitmap.lock().map_err(|_| DiskError::Busy)?;
        let mut buf = vec![0u8; SECTOR_SIZE];
        for (i, word) in bitmap.iter().enumerate() {
            let bytes = word.to_le_bytes();
            let start = i * 8;
            if start + 8 <= SECTOR_SIZE {
                buf[start..start+8].copy_from_slice(&bytes);
            }
        }
        drop(bitmap);
        self.write_sector(0, &buf)
    }

    // ── Sector allocation ──

    fn mark_used(bitmap: &mut [u64], sector: u32) {
        let word = (sector / 64) as usize;
        let bit = (sector % 64) as u64;
        if word < bitmap.len() { bitmap[word] |= 1 << bit; }
    }

    fn mark_free(bitmap: &mut [u64], sector: u32) {
        let word = (sector / 64) as usize;
        let bit = (sector % 64) as u64;
        if word < bitmap.len() { bitmap[word] &= !(1 << bit); }
    }

    fn is_free(bitmap: &[u64], sector: u32) -> bool {
        let word = (sector / 64) as usize;
        let bit = (sector % 64) as u64;
        word >= bitmap.len() || (bitmap[word] & (1 << bit)) == 0
    }

    fn mark_used_range(bitmap: &mut [u64], start: u32, count: u32) {
        for s in start..start + count {
            Self::mark_used(bitmap, s);
        }
    }

    pub fn alloc_sector(&self) -> Option<u32> {
        let mut bitmap = self.bitmap.lock().ok()?;
        for s in 4..self.total_sectors {
            if Self::is_free(&bitmap, s) {
                Self::mark_used(&mut bitmap, s);
                drop(bitmap);
                self.flush_bitmap().ok();
                return Some(s);
            }
        }
        None
    }

    pub fn free_sector(&self, sector: u32) -> Result<(), DiskError> {
        if sector < 4 { return Ok(()); } // metadata sectors
        let mut bitmap = self.bitmap.lock().map_err(|_| DiskError::Busy)?;
        Self::mark_free(&mut bitmap, sector);
        drop(bitmap);
        self.flush_bitmap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_disk() -> VirtualDisk {
        let _ = fs::remove_file("/tmp/test-disk.img");
        VirtualDisk::new(100, "/tmp/test-disk.img")
    }

    #[test]
    fn test_read_write_sector() {
        let disk = make_disk();
        let mut data = vec![0u8; SECTOR_SIZE];
        data[0] = 0xAB;
        disk.write_sector(5, &data).unwrap();
        let read = disk.read_sector(5).unwrap();
        assert_eq!(read[0], 0xAB);
        let _ = fs::remove_file("/tmp/test-disk.img");
    }

    #[test]
    fn test_alloc_free() {
        let disk = make_disk();
        let s = disk.alloc_sector().unwrap();
        assert!(s >= 4);
        assert!(!disk.alloc_sector().is_none());
        disk.free_sector(s).unwrap();
        let _ = fs::remove_file("/tmp/test-disk.img");
    }

    #[test]
    fn test_persistence() {
        let _ = fs::remove_file("/tmp/test-persist.img");
        let data = vec![0x42u8; SECTOR_SIZE];
        {
            let disk = VirtualDisk::new(100, "/tmp/test-persist.img");
            disk.write_sector(8, &data).unwrap();
        }
        {
            let disk = VirtualDisk::new(100, "/tmp/test-persist.img");
            let read = disk.read_sector(8).unwrap();
            assert_eq!(read[0], 0x42);
        }
        let _ = fs::remove_file("/tmp/test-persist.img");
    }
}

// ── Bulk operations (used by File sync_to_disk) ──

impl VirtualDisk {
    /// Allocate contiguous sectors
    pub fn allocate_sectors(&self, count: u32) -> Result<u32, DiskError> {
        let mut bitmap = self.bitmap.lock().map_err(|_| DiskError::Busy)?;
        let mut run_start = 4u32;
        let mut run_len = 0u32;
        for s in 4..self.total_sectors {
            if Self::is_free(&bitmap, s) {
                if run_len == 0 { run_start = s; }
                run_len += 1;
                if run_len >= count {
                    for i in run_start..run_start + count {
                        Self::mark_used(&mut bitmap, i);
                    }
                    drop(bitmap);
                    self.flush_bitmap().ok();
                    return Ok(run_start);
                }
            } else {
                run_len = 0;
            }
        }
        Err(DiskError::OutOfSpace)
    }

    /// Free a range of sectors
    pub fn free_sectors(&self, start: u32, count: u32) -> Result<(), DiskError> {
        let mut bitmap = self.bitmap.lock().map_err(|_| DiskError::Busy)?;
        for s in start..start + count {
            if s >= 4 { Self::mark_free(&mut bitmap, s); }
        }
        drop(bitmap);
        self.flush_bitmap()
    }

    /// Count used sectors
    pub fn used_sectors_count(&self) -> usize {
        self.bitmap.lock().map(|b| {
            b.iter().map(|w| w.count_ones() as usize).sum()
        }).unwrap_or(0)
    }
}

/// Disk state snapshot
#[derive(Debug, Clone)]
pub struct DiskState {
    pub total_sectors: u32,
    pub total_bytes: u64,
}

impl VirtualDisk {
    pub fn dump_state(&self) -> DiskState {
        DiskState {
            total_sectors: self.total_sectors,
            total_bytes: self.size_bytes(),
        }
    }
}

impl std::fmt::Debug for VirtualDisk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VirtualDisk")
            .field("total_sectors", &self.total_sectors)
            .field("path", &self.path)
            .finish()
    }
}
