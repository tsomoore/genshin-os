// Process Service
//
// This service handles all process-related operations:
// - Process lifecycle (create, terminate, fork, exec, wait)
// - Thread management
// - IPC (message passing, shared memory, synchronization)
// - Process scheduling (ready queues, state management)

pub mod pcb;
pub mod ipc;
pub mod sync;
pub mod scheduler;
pub mod service;

// Re-export key types
pub use pcb::{PCB, TCB, ProcessState, ThreadState};
pub use ipc::{MessageQueue, SharedMemoryRegion};
pub use sync::{Semaphore, MutexLock};
pub use scheduler::{Scheduler, SchedulingPolicy, SchedulingDecision};
pub use service::ProcessService;
