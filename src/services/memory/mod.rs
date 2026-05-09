// Memory Service
//
// This service handles all storage-related operations:
// - Memory allocation (physical frames)
// - Paging and page table management
// - Swap space management
// - Page fault handling

pub mod alloc;
pub mod paging;
pub mod swap;
pub mod service;

// Re-export key types
pub use alloc::{FrameAllocator, Frame, PhysicalMemoryManager};
pub use paging::{PageTable, PageTableEntry, PageFlags};
pub use swap::{SwapManager, SwapSlot};
pub use service::MemoryService;
