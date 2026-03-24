// Virtual Disk Simulation
//
// 曾国藩曰：
// "凡事之需逐日检点者，文书是也。"
// 磁盘乃持久存储之所，读写不可不慎，记录不可不勤。

use std::sync::{Arc, Mutex};
use std::fmt;

use crate::error::DiskError;

/// Standard disk sector size (512 bytes)
pub const SECTOR_SIZE: usize = 512;

/// Virtual disk simulation
///
/// Simulates a block storage device with 512-byte sectors.
/// Used as the backing store for swap space and file system.
///
/// 曾国藩曰：
/// "储粮于仓，须防鼠雀；储文于盘，须防损坏。"
/// 磁盘乃数据之粮仓，当以谨慎之心待之。
#[derive(Clone)]
pub struct VirtualDisk {
    /// Disk data organized by sector
    data: Arc<Mutex<Vec<Vec<u8>>>>,
    /// Total number of sectors
    total_sectors: u32,
}

impl VirtualDisk {
    /// Create a new virtual disk with specified sector count
    ///
    /// # Arguments
    /// * `total_sectors` - Number of 512-byte sectors
    pub fn new(total_sectors: u32) -> Self {
        if total_sectors == 0 {
            panic!("Disk must have at least one sector");
        }

        let mut sectors = Vec::with_capacity(total_sectors as usize);
        for _ in 0..total_sectors {
            sectors.push(vec![0u8; SECTOR_SIZE]);
        }

        Self {
            data: Arc::new(Mutex::new(sectors)),
            total_sectors,
        }
    }

    /// Get total disk size in bytes
    pub fn size_bytes(&self) -> u64 {
        self.total_sectors as u64 * SECTOR_SIZE as u64
    }

    /// Get total number of sectors
    pub fn total_sectors(&self) -> u32 {
        self.total_sectors
    }

    /// Read a single sector
    ///
    /// 曾国藩曰：
    /// "读书之法，在循序而渐进；读盘之法，亦当按扇区而读之。"
    /// 每次读写必以扇区为单位，不可紊乱。
    pub fn read_sector(&self, sector: u32) -> Result<Vec<u8>, DiskError> {
        self.check_sector(sector)?;

        let data = self.data.lock()
            .map_err(|_| DiskError::Busy)?;

        Ok(data[sector as usize].clone())
    }

    /// Read multiple sectors
    pub fn read_sectors(&self, start_sector: u32, count: u32) -> Result<Vec<u8>, DiskError> {
        if count == 0 {
            return Ok(Vec::new());
        }

        // Check last sector
        let end_sector = start_sector.checked_add(count - 1)
            .ok_or(DiskError::InvalidSector { sector: u32::MAX, max_sector: self.total_sectors })?;
        self.check_sector(end_sector)?;

        let data = self.data.lock()
            .map_err(|_| DiskError::Busy)?;

        let mut result = Vec::with_capacity(count as usize * SECTOR_SIZE);
        for i in 0..count {
            let sector_idx = (start_sector + i) as usize;
            result.extend_from_slice(&data[sector_idx]);
        }

        Ok(result)
    }

    /// Write a single sector
    ///
    /// 曾国藩曰：
    /// "落笔纸笺，当思此字传之后世，不可草率。"
    /// 写入磁盘亦当如此，每一扇区皆当认真对待。
    pub fn write_sector(&self, sector: u32, buf: &[u8]) -> Result<(), DiskError> {
        self.check_sector(sector)?;

        if buf.len() != SECTOR_SIZE {
            return Err(DiskError::IoFailed {
                operation: "write_sector".to_string(),
                sector,
            });
        }

        let mut data = self.data.lock()
            .map_err(|_| DiskError::Busy)?;

        data[sector as usize].copy_from_slice(buf);
        Ok(())
    }

    /// Write multiple sectors
    pub fn write_sectors(&self, start_sector: u32, buf: &[u8]) -> Result<(), DiskError> {
        if buf.len() % SECTOR_SIZE != 0 {
            return Err(DiskError::IoFailed {
                operation: "write_sectors".to_string(),
                sector: start_sector,
            });
        }

        let sector_count = (buf.len() / SECTOR_SIZE) as u32;
        if sector_count == 0 {
            return Ok(());
        }

        let end_sector = start_sector.checked_add(sector_count - 1)
            .ok_or(DiskError::InvalidSector { sector: u32::MAX, max_sector: self.total_sectors })?;
        self.check_sector(end_sector)?;

        let mut data = self.data.lock()
            .map_err(|_| DiskError::Busy)?;

        for (i, chunk) in buf.chunks(SECTOR_SIZE).enumerate() {
            let sector_idx = (start_sector + i as u32) as usize;
            data[sector_idx].copy_from_slice(chunk);
        }

        Ok(())
    }

    /// Zero out a sector
    pub fn zero_sector(&self, sector: u32) -> Result<(), DiskError> {
        self.check_sector(sector)?;

        let mut data = self.data.lock()
            .map_err(|_| DiskError::Busy)?;

        for byte in data[sector as usize].iter_mut() {
            *byte = 0;
        }

        Ok(())
    }

    /// Zero out multiple sectors
    pub fn zero_sectors(&self, start_sector: u32, count: u32) -> Result<(), DiskError> {
        let end_sector = start_sector.checked_add(count - 1)
            .ok_or(DiskError::InvalidSector { sector: u32::MAX, max_sector: self.total_sectors })?;
        self.check_sector(end_sector)?;

        let mut data = self.data.lock()
            .map_err(|_| DiskError::Busy)?;

        for i in 0..count {
            let sector_idx = (start_sector + i) as usize;
            for byte in data[sector_idx].iter_mut() {
                *byte = 0;
            }
        }

        Ok(())
    }

    /// Dump disk state for debugging/TUI display
    ///
    /// 曾国藩曰：
    /// "每日检点仓库，知其盈虚，方能理财。"
    /// 磁盘状态亦当定期检查，方能知己知彼。
    pub fn dump_state(&self) -> DiskState {
        let data = self.data.lock()
            .unwrap();

        // Count non-zero sectors (used sectors)
        let used_sectors = data.iter()
            .filter(|sector| sector.iter().any(|&b| b != 0))
            .count();

        DiskState {
            total_sectors: self.total_sectors,
            used_sectors,
            total_bytes: self.size_bytes(),
        }
    }

    /// Check if sector number is valid
    fn check_sector(&self, sector: u32) -> Result<(), DiskError> {
        if sector >= self.total_sectors {
            return Err(DiskError::InvalidSector { sector, max_sector: self.total_sectors });
        }
        Ok(())
    }
}

impl fmt::Debug for VirtualDisk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VirtualDisk")
            .field("total_sectors", &self.total_sectors)
            .field("size_bytes", &self.size_bytes())
            .finish()
    }
}

/// Disk state snapshot for TUI display
#[derive(Debug, Clone)]
pub struct DiskState {
    pub total_sectors: u32,
    pub used_sectors: usize,
    pub total_bytes: u64,
}

impl DiskState {
    pub fn utilization_percent(&self) -> f64 {
        if self.total_sectors == 0 {
            return 0.0;
        }
        (self.used_sectors as f64 / self.total_sectors as f64) * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disk_creation() {
        let disk = VirtualDisk::new(1024);
        assert_eq!(disk.total_sectors(), 1024);
        assert_eq!(disk.size_bytes(), 1024 * 512);
    }

    #[test]
    fn test_read_write_sector() {
        let disk = VirtualDisk::new(100);

        // Write a sector
        let mut sector_data = vec![0xAB; SECTOR_SIZE];
        sector_data[0] = 0xDE;
        sector_data[1] = 0xAD;
        disk.write_sector(5, &sector_data).unwrap();

        // Read it back
        let read_data = disk.read_sector(5).unwrap();
        assert_eq!(read_data.len(), SECTOR_SIZE);
        assert_eq!(read_data[0], 0xDE);
        assert_eq!(read_data[1], 0xAD);
    }

    #[test]
    fn test_invalid_sector() {
        let disk = VirtualDisk::new(100);
        assert!(matches!(
            disk.read_sector(100),
            Err(DiskError::InvalidSector { .. })
        ));
    }

    #[test]
    fn test_multiple_sectors() {
        let disk = VirtualDisk::new(100);

        // Write 2 sectors
        let data = vec![0xCD; SECTOR_SIZE * 2];
        disk.write_sectors(10, &data).unwrap();

        // Read them back
        let read_data = disk.read_sectors(10, 2).unwrap();
        assert_eq!(read_data.len(), SECTOR_SIZE * 2);
        assert!(read_data.iter().all(|&b| b == 0xCD));
    }

    #[test]
    fn test_zero_sector() {
        let disk = VirtualDisk::new(100);

        // Write some data
        let data = vec![0xFF; SECTOR_SIZE];
        disk.write_sector(5, &data).unwrap();

        // Zero it out
        disk.zero_sector(5).unwrap();

        // Verify
        let read_data = disk.read_sector(5).unwrap();
        assert!(read_data.iter().all(|&b| b == 0));
    }

    #[test]
    fn test_dump_state() {
        let disk = VirtualDisk::new(1000);

        // Use some sectors
        let data = vec![0xAA; SECTOR_SIZE];
        disk.write_sector(0, &data).unwrap();
        disk.write_sector(10, &data).unwrap();

        let state = disk.dump_state();
        assert_eq!(state.total_sectors, 1000);
        assert_eq!(state.used_sectors, 2);
    }
}
