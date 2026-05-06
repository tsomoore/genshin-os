// User Interface Layer
//
// This layer provides user interaction with the kernel through:
// - CLI Shell: Interactive command-line interface
// - TUI Monitor: Text-based system monitoring dashboard

pub mod shell;

// Re-export shell for convenience
pub use shell::{Shell, ShellConfig};

use crate::messaging::{MessageBus, KernelMsg};
use std::sync::Arc;

/// User interface context
pub struct UIContext {
    /// Message bus for kernel communication
    pub bus: Arc<dyn MessageBus>,
}

impl UIContext {
    pub fn new(bus: Arc<dyn MessageBus>) -> Self {
        Self { bus }
    }

    /// Send a message to the kernel
    pub fn send(&self, msg: KernelMsg) {
        self.bus.send(msg);
    }
}
