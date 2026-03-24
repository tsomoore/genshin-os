// Physical Memory (RAM) Simulation
//
// 曾国藩曰：
// "每日清晨，念虑杂生，当如治军，严整纪律。"
// 内存乃系统之根基，每一字节之读写，皆当如临深渊，如履薄冰。

use std::sync::{Arc, Mutex};
use std::fmt;

/// Physical memory simulation
///
/// Represents the physical RAM of the system. All memory accesses
/// must go through this interface to ensure thread safety and bounds checking.
///
/// 曾国藩曰：
/// "治军之道，以严明为本；治内存之道，以界限为要。"
/// 内存越界乃是大错，必须严加防范。
#[derive(Clone)]
pub struct PhysicalMemory {
    /// Actual memory backing store
    data: Arc<Mutex<Vec<u8>>>,
    /// Total memory size in bytes
    size: usize,
}

impl PhysicalMemory {
    /// Create new physical memory with specified size (in bytes)
    ///
    /// # Panics
    /// Panics if size is zero
    pub fn new(size: usize) -> Self {
        if size == 0 {
            panic!("Physical memory size must be non-zero");
        }

        Self {
            data: Arc::new(Mutex::new(vec![0; size])),
            size,
        }
    }

    /// Get memory size in bytes
    pub fn size(&self) -> usize {
        self.size
    }

    /// Read a single byte from physical memory
    ///
    /// 曾国藩曰：
    /// "读一字当思其来之不易，用一物当念其来处不易。"
    /// 每次读取都需检查边界，此乃基本功。
    pub fn read_u8(&self, addr: usize) -> Result<u8, crate::error::MemoryError> {
        self.check_bounds(addr, 1)?;

        let data = self.data.lock()
            .map_err(|_| crate::error::MemoryError::Locked)?;

        Ok(data[addr])
    }

    /// Read a 16-bit word from physical memory (little-endian)
    pub fn read_u16(&self, addr: usize) -> Result<u16, crate::error::MemoryError> {
        self.check_alignment(addr, 2)?;
        self.check_bounds(addr, 2)?;

        let data = self.data.lock()
            .map_err(|_| crate::error::MemoryError::Locked)?;

        let value = u16::from_le_bytes([
            data[addr],
            data[addr + 1],
        ]);

        Ok(value)
    }

    /// Read a 32-bit word from physical memory (little-endian)
    pub fn read_u32(&self, addr: usize) -> Result<u32, crate::error::MemoryError> {
        self.check_alignment(addr, 4)?;
        self.check_bounds(addr, 4)?;

        let data = self.data.lock()
            .map_err(|_| crate::error::MemoryError::Locked)?;

        let value = u32::from_le_bytes([
            data[addr],
            data[addr + 1],
            data[addr + 2],
            data[addr + 3],
        ]);

        Ok(value)
    }

    /// Read a 64-bit word from physical memory (little-endian)
    pub fn read_u64(&self, addr: usize) -> Result<u64, crate::error::MemoryError> {
        self.check_alignment(addr, 8)?;
        self.check_bounds(addr, 8)?;

        let data = self.data.lock()
            .map_err(|_| crate::error::MemoryError::Locked)?;

        let value = u64::from_le_bytes([
            data[addr],
            data[addr + 1],
            data[addr + 2],
            data[addr + 3],
            data[addr + 4],
            data[addr + 5],
            data[addr + 6],
            data[addr + 7],
        ]);

        Ok(value)
    }

    /// Write a single byte to physical memory
    ///
    /// 曾国藩曰：
    /// "下笔之时，当思此事关系甚大，不可草率。"
    /// 写入内存亦当如此，必须慎之又慎。
    pub fn write_u8(&self, addr: usize, value: u8) -> Result<(), crate::error::MemoryError> {
        self.check_bounds(addr, 1)?;

        let mut data = self.data.lock()
            .map_err(|_| crate::error::MemoryError::Locked)?;

        data[addr] = value;
        Ok(())
    }

    /// Write a 16-bit word to physical memory (little-endian)
    pub fn write_u16(&self, addr: usize, value: u16) -> Result<(), crate::error::MemoryError> {
        self.check_alignment(addr, 2)?;
        self.check_bounds(addr, 2)?;

        let mut data = self.data.lock()
            .map_err(|_| crate::error::MemoryError::Locked)?;

        let bytes = value.to_le_bytes();
        data[addr] = bytes[0];
        data[addr + 1] = bytes[1];

        Ok(())
    }

    /// Write a 32-bit word to physical memory (little-endian)
    pub fn write_u32(&self, addr: usize, value: u32) -> Result<(), crate::error::MemoryError> {
        self.check_alignment(addr, 4)?;
        self.check_bounds(addr, 4)?;

        let mut data = self.data.lock()
            .map_err(|_| crate::error::MemoryError::Locked)?;

        let bytes = value.to_le_bytes();
        data[addr] = bytes[0];
        data[addr + 1] = bytes[1];
        data[addr + 2] = bytes[2];
        data[addr + 3] = bytes[3];

        Ok(())
    }

    /// Write a 64-bit word to physical memory (little-endian)
    pub fn write_u64(&self, addr: usize, value: u64) -> Result<(), crate::error::MemoryError> {
        self.check_alignment(addr, 8)?;
        self.check_bounds(addr, 8)?;

        let mut data = self.data.lock()
            .map_err(|_| crate::error::MemoryError::Locked)?;

        let bytes = value.to_le_bytes();
        for (i, &byte) in bytes.iter().enumerate() {
            data[addr + i] = byte;
        }

        Ok(())
    }

    /// Read a slice of bytes from physical memory
    pub fn read_slice(&self, addr: usize, buf: &mut [u8]) -> Result<(), crate::error::MemoryError> {
        self.check_bounds(addr, buf.len())?;

        let data = self.data.lock()
            .map_err(|_| crate::error::MemoryError::Locked)?;

        buf.copy_from_slice(&data[addr..addr + buf.len()]);
        Ok(())
    }

    /// Write a slice of bytes to physical memory
    pub fn write_slice(&self, addr: usize, buf: &[u8]) -> Result<(), crate::error::MemoryError> {
        self.check_bounds(addr, buf.len())?;

        let mut data = self.data.lock()
            .map_err(|_| crate::error::MemoryError::Locked)?;

        data[addr..addr + buf.len()].copy_from_slice(buf);
        Ok(())
    }

    /// Clear all memory to zero
    pub fn clear(&self) -> Result<(), crate::error::MemoryError> {
        let mut data = self.data.lock()
            .map_err(|_| crate::error::MemoryError::Locked)?;

        for byte in data.iter_mut() {
            *byte = 0;
        }

        Ok(())
    }

    /// Dump memory state for debugging/TUI display
    ///
    /// 曾国藩曰：
    /// "每日三省吾身：为人谋而不忠乎？与朋友交而不信乎？传不习乎？"
    /// 内存状态亦需常省察，方能知其全貌。
    pub fn dump_state(&self) -> MemoryState {
        let data = self.data.lock()
            .map_err(|_| crate::error::MemoryError::Locked)
            .unwrap();

        // Collect first 256 bytes for preview
        let preview: Vec<u8> = data.iter().take(256).cloned().collect();

        MemoryState {
            size: self.size,
            preview,
        }
    }

    /// Check if address access is within bounds
    fn check_bounds(&self, addr: usize, size: usize) -> Result<(), crate::error::MemoryError> {
        if addr.checked_add(size).map_or(true, |end| end > self.size) {
            return Err(crate::error::MemoryError::OutOfBounds {
                addr,
                size,
                max_size: self.size,
            });
        }
        Ok(())
    }

    /// Check if address is properly aligned for access size
    fn check_alignment(&self, addr: usize, size: usize) -> Result<(), crate::error::MemoryError> {
        if addr % size != 0 {
            return Err(crate::error::MemoryError::Misaligned {
                addr,
                required_alignment: size,
            });
        }
        Ok(())
    }
}

impl fmt::Debug for PhysicalMemory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PhysicalMemory")
            .field("size", &self.size)
            .finish()
    }
}

/// Memory state snapshot for TUI display
#[derive(Debug, Clone)]
pub struct MemoryState {
    pub size: usize,
    pub preview: Vec<u8>,
}

impl MemoryState {
    /// Format memory preview as hex dump
    pub fn format_hexdump(&self, start_addr: usize, bytes_per_line: usize) -> String {
        let mut output = String::new();

        for (i, chunk) in self.preview.chunks(bytes_per_line).enumerate() {
            let addr = start_addr + i * bytes_per_line;
            output.push_str(&format!("{:#08x}: ", addr));

            // Hex values
            for (j, &byte) in chunk.iter().enumerate() {
                if j % 8 == 0 && j != 0 {
                    output.push(' ');
                }
                output.push_str(&format!("{:02x} ", byte));
            }

            // Padding
            if chunk.len() < bytes_per_line {
                for _ in 0..(bytes_per_line - chunk.len()) {
                    output.push_str("   ");
                }
            }

            // ASCII representation
            output.push_str(" |");
            for &byte in chunk.iter() {
                if byte.is_ascii_graphic() || byte == b' ' {
                    output.push(byte as char);
                } else {
                    output.push('.');
                }
            }
            output.push_str("|\n");
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_creation() {
        let mem = PhysicalMemory::new(4096);
        assert_eq!(mem.size(), 4096);
    }

    #[test]
    fn test_read_write_u8() {
        let mem = PhysicalMemory::new(4096);
        mem.write_u8(0x100, 0xAB).unwrap();
        assert_eq!(mem.read_u8(0x100).unwrap(), 0xAB);
    }

    #[test]
    fn test_read_write_u32() {
        let mem = PhysicalMemory::new(4096);
        mem.write_u32(0x100, 0xDEADBEEF).unwrap();
        assert_eq!(mem.read_u32(0x100).unwrap(), 0xDEADBEEF);
    }

    #[test]
    fn test_out_of_bounds() {
        let mem = PhysicalMemory::new(4096);
        assert!(matches!(
            mem.read_u8(4096),
            Err(crate::error::MemoryError::OutOfBounds { .. })
        ));
    }

    #[test]
    fn test_misaligned_access() {
        let mem = PhysicalMemory::new(4096);
        assert!(matches!(
            mem.read_u32(0x101),  // Not 4-byte aligned
            Err(crate::error::MemoryError::Misaligned { .. })
        ));
    }

    #[test]
    fn test_hexdump_format() {
        let mem = PhysicalMemory::new(256);
        for i in 0..256usize {
            mem.write_u8(i, i as u8).unwrap();
        }

        let state = mem.dump_state();
        let dump = state.format_hexdump(0, 16);
        // Just verify it generates output
        assert!(dump.len() > 0);
        assert!(dump.contains(":"));
    }
}
