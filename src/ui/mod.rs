// User Interface Layer
//
// This layer provides user interaction with the kernel through:
// - CLI Shell: Interactive command-line interface
// - TUI Monitor: Text-based system monitoring dashboard

pub mod shell;
pub mod monitor;

// Re-export shell for convenience
pub use shell::{Shell, ShellConfig};

use crate::messaging::{MessageBus, KernelMsg, Response, BusError};
use std::sync::Arc;
use crossbeam_channel::Receiver;

/// User interface context
pub struct UIContext {
    /// Message bus for kernel communication
    pub bus: Arc<dyn MessageBus>,
}

impl UIContext {
    pub fn new(bus: Arc<dyn MessageBus>) -> Self {
        Self { bus }
    }

    /// Send a fire-and-forget message to the kernel
    pub fn send(&self, msg: KernelMsg) {
        let _ = self.bus.send(msg);
    }

    /// Send a request and receive a response
    ///
    /// Returns a Receiver that will receive the Response from the handling service.
    /// Call `rx.recv()` to block until the response arrives.
    pub fn send_request(&self, msg: KernelMsg) -> Result<Receiver<Response>, BusError> {
        self.bus.send_request(msg)
    }
}
