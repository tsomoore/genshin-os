// Block Device Interface
//
// 曾国藩曰：
// "万丈高楼平地起，根基不牢楼必倾。"
// 块设备乃文件系统之根基，必须稳固可靠。

use std::sync::{Arc, Mutex};
use std::fmt;
use crate::{GenshinError, GenshinResult, HardwareError, DiskError};

/// Block device size constants
pub const SECTOR_SIZE: usize = 512;
pub const BLOCK_SIZE: usize = 4096;  // 8 sectors per block

/// Block device trait
///
/// All block-oriented storage devices must implement this trait.
/// This provides a unified interface for file systems.
pub trait BlockDevice: Send + Sync {
    /// Read a block from the device
    fn read_block(&self, block_id: u64) -> GenshinResult<Vec<u8>>;

    /// Write a block to the device
    fn write_block(&self, block_id: u64, data: &[u8]) -> GenshinResult<()>;

    /// Get total number of blocks
    fn total_blocks(&self) -> u64;

    /// Get block size in bytes
    fn block_size(&self) -> usize;

    /// Flush any cached writes to device
    fn flush(&self) -> GenshinResult<()>;

    /// Get device name/identifier
    fn device_name(&self) -> &str;
}

/// Physical block device wrapper for VirtualDisk
///
/// Wraps a VirtualDisk to implement BlockDevice trait.
pub struct PhysicalBlockDevice {
    disk: Arc<crate::hardware::VirtualDisk>,
    name: String,
}

impl PhysicalBlockDevice {
    /// Create a new physical block device from a virtual disk
    pub fn new(disk: Arc<crate::hardware::VirtualDisk>, name: String) -> Self {
        Self { disk, name }
    }
}

impl BlockDevice for PhysicalBlockDevice {
    fn read_block(&self, block_id: u64) -> GenshinResult<Vec<u8>> {
        // Convert block to sectors (1 block = 8 sectors)
        let start_sector = (block_id * (BLOCK_SIZE / SECTOR_SIZE) as u64) as u32;
        let sector_count = (BLOCK_SIZE / SECTOR_SIZE) as u32;

        let mut data = Vec::with_capacity(BLOCK_SIZE);
        for i in 0..sector_count {
            let sector_data = self.disk.read_sector(start_sector + i)
                .map_err(|e| GenshinError::Hardware(crate::error::HardwareError::Disk(e)))?;
            data.extend_from_slice(&sector_data);
        }

        Ok(data)
    }

    fn write_block(&self, block_id: u64, data: &[u8]) -> GenshinResult<()> {
        if data.len() != BLOCK_SIZE {
            return Err(GenshinError::Hardware(
                crate::error::HardwareError::Disk(DiskError::IoFailed {
                    operation: "write_block".to_string(),
                    sector: block_id as u32,
                })
            ));
        }

        let start_sector = (block_id * (BLOCK_SIZE / SECTOR_SIZE) as u64) as u32;

        for (i, chunk) in data.chunks(SECTOR_SIZE).enumerate() {
            self.disk.write_sector(start_sector + i as u32, chunk)
                .map_err(|e| GenshinError::Hardware(crate::error::HardwareError::Disk(e)))?;
        }

        Ok(())
    }

    fn total_blocks(&self) -> u64 {
        self.disk.total_sectors() as u64 / (BLOCK_SIZE / SECTOR_SIZE) as u64
    }

    fn block_size(&self) -> usize {
        BLOCK_SIZE
    }

    fn flush(&self) -> GenshinResult<()> {
        // VirtualDisk doesn't have explicit flush - data is immediately written
        Ok(())
    }

    fn device_name(&self) -> &str {
        &self.name
    }
}

/// Disk partition information
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Partition {
    /// Partition number (1-based)
    pub number: u8,
    /// Starting block
    pub start_block: u64,
    /// Total blocks
    pub total_blocks: u64,
    /// Partition type
    pub partition_type: PartitionType,
    /// Bootable flag
    pub bootable: bool,
}

/// Partition type (simplified)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitionType {
    /// Empty/unused
    Empty,
    /// FAT32
    FAT32,
    /// ext4
    EXT4,
    /// Linux swap
    LinuxSwap,
    /// Unknown
    Unknown(u8),
}

/// Partition table entry (MBR style)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
struct MBRPartitionEntry {
    boot_indicator: u8,
    starting_chs: [u8; 3],
    partition_type: u8,
    ending_chs: [u8; 3],
    start_sector: u32,
    total_sectors: u32,
}

/// Disk partition layout
#[derive(Debug, Clone)]
pub struct PartitionLayout {
    pub partitions: Vec<Partition>,
}

impl PartitionLayout {
    /// Create a new empty partition layout
    pub fn new() -> Self {
        Self {
            partitions: Vec::new(),
        }
    }

    /// Add a partition
    pub fn add_partition(&mut self, partition: Partition) {
        self.partitions.push(partition);
    }

    /// Get partition by number
    pub fn get_partition(&self, number: u8) -> Option<&Partition> {
        self.partitions.iter().find(|p| p.number == number)
    }
}

/// Partitioned block device
///
/// Represents a single partition on a block device.
pub struct PartitionDevice {
    device: Arc<dyn BlockDevice>,
    partition: Partition,
}

impl PartitionDevice {
    /// Create a new partition device
    pub fn new(device: Arc<dyn BlockDevice>, partition: Partition) -> Self {
        Self { device, partition }
    }

    /// Get partition info
    pub fn partition_info(&self) -> &Partition {
        &self.partition
    }
}

impl BlockDevice for PartitionDevice {
    fn read_block(&self, block_id: u64) -> GenshinResult<Vec<u8>> {
        if block_id >= self.partition.total_blocks {
            return Err(GenshinError::Hardware(
                crate::error::HardwareError::Disk(DiskError::InvalidSector {
                    sector: block_id as u32,
                    max_sector: self.partition.total_blocks as u32,
                })
            ));
        }

        let absolute_block = self.partition.start_block + block_id;
        self.device.read_block(absolute_block)
    }

    fn write_block(&self, block_id: u64, data: &[u8]) -> GenshinResult<()> {
        if block_id >= self.partition.total_blocks {
            return Err(GenshinError::Hardware(
                crate::error::HardwareError::Disk(DiskError::InvalidSector {
                    sector: block_id as u32,
                    max_sector: self.partition.total_blocks as u32,
                })
            ));
        }

        let absolute_block = self.partition.start_block + block_id;
        self.device.write_block(absolute_block, data)
    }

    fn total_blocks(&self) -> u64 {
        self.partition.total_blocks
    }

    fn block_size(&self) -> usize {
        self.device.block_size()
    }

    fn flush(&self) -> GenshinResult<()> {
        self.device.flush()
    }

    fn device_name(&self) -> &str {
        &self.device.device_name()
    }
}

impl fmt::Debug for dyn BlockDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BlockDevice")
            .field("name", &self.device_name())
            .field("block_size", &self.block_size())
            .field("total_blocks", &self.total_blocks())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_constants() {
        assert_eq!(SECTOR_SIZE, 512);
        assert_eq!(BLOCK_SIZE, 4096);
        assert_eq!(BLOCK_SIZE / SECTOR_SIZE, 8);
    }

    #[test]
    fn test_partition_layout() {
        let mut layout = PartitionLayout::new();

        let part1 = Partition {
            number: 1,
            start_block: 0,
            total_blocks: 1024,
            partition_type: PartitionType::EXT4,
            bootable: true,
        };

        layout.add_partition(part1);

        assert_eq!(layout.partitions.len(), 1);
        assert!(layout.get_partition(1).is_some());
        assert!(layout.get_partition(2).is_none());
    }
}
