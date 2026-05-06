// File Service
//
// This service handles all file-related operations:
// - File system management (VFS)
// - File operations (open, close, read, write, create, delete)
// - Directory management
// - File descriptor management
// - File permissions and locking

pub mod vfs;
pub mod file;
pub mod descriptor;
pub mod service;

// Re-export key types
pub use vfs::{VirtualFileSystem, VFSNode, NodeType};
pub use file::{File, Directory, FileMetadata, FilePermissions, FileType, OpenMode};
pub use descriptor::{FileDescriptorManager, FileTable, OpenFile, Fd};
pub use service::{FileService, VfsStats};
