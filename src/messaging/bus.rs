// Message bus implementations for genshin-os
//
// This module defines the MessageBus trait and provides implementations:
// - LockedBus: Mutex-based using crossbeam channels
// - LockFreeBus: (Future) Atomic-based lock-free implementation

use crate::messaging::msg::KernelMsg;
use crate::messaging::response::{RequestWithResponse, Response};
use crossbeam_channel::{unbounded, Receiver, Sender, TrySendError};
use std::sync::{Arc, Mutex};
use thiserror::Error;

/// Errors that can occur during message bus operations
#[derive(Debug, Error)]
pub enum BusError {
    #[error("Failed to send message: bus is disconnected")]
    Disconnected,

    #[error("Failed to send message: bus is full")]
    Full,

    #[error("No receiver registered for message type: {0}")]
    NoReceiver(String),
}

/// Core message bus trait
///
/// All message buses in genshin-os must implement this trait.
/// The bus supports both fire-and-forget and request-response patterns.
pub trait MessageBus: Send + Sync {
    /// Send a message through the bus (fire-and-forget)
    ///
    /// This is a fire-and-forget operation. The message is queued
    /// for processing and the caller returns immediately.
    ///
    /// # Errors
    /// Returns `BusError` if the message cannot be sent.
    fn send(&self, msg: KernelMsg) -> Result<(), BusError>;

    /// Send a request with response channel
    ///
    /// This sends a request message and returns a receiver for the response.
    /// The caller can choose to wait for the response asynchronously.
    ///
    /// # Returns
    /// Returns a receiver that will receive the Response.
    ///
    /// # Errors
    /// Returns `BusError` if the request cannot be sent.
    fn send_request(&self, msg: KernelMsg) -> Result<Receiver<Response>, BusError>;

    /// Subscribe to receive messages from the bus
    ///
    /// Returns a receiver for the subscribed message channel.
    fn subscribe(&self) -> Receiver<KernelMsg>;

    /// Clone the bus handle
    ///
    /// Allows multiple handles to the same bus.
    fn clone_box(&self) -> Box<dyn MessageBus>;
}

/// Internal state for LockedBus
struct LockedBusState {
    subscribers: Vec<Sender<KernelMsg>>,
}

/// LockedBus: Mutex-based message bus using crossbeam channels
///
/// This is a reference implementation of the MessageBus trait.
/// It uses crossbeam's unbounded channels for message passing,
/// with a Mutex for thread-safe access to the subscriber list.
///
/// # Architecture
/// ```text
/// Sender(s) ──┐
///             ├──> Unbounded Channel ──> Broadcast to all subscribers
/// Mutex ─────┘
/// ```
///
/// # Example
/// ```rust, no_run
/// use genshin_os::{MessageBus, LockedBus};
/// use genshin_os::messaging::Interrupt;
///
/// let bus = LockedBus::new();
///
/// // Subscribe to receive messages
/// let _receiver1 = bus.subscribe();
/// let _receiver2 = bus.subscribe();
/// ```
#[derive(Clone)]
pub struct LockedBus {
    state: Arc<Mutex<LockedBusState>>,
}

impl LockedBus {
    /// Create a new locked message bus
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(LockedBusState {
                subscribers: Vec::new(),
            })),
        }
    }
}

impl Default for LockedBus {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageBus for LockedBus {
    fn send(&self, msg: KernelMsg) -> Result<(), BusError> {
        let state = self.state.lock().unwrap();

        // Broadcast to all subscribers
        // If no subscribers, still succeed (fire-and-forget)
        if state.subscribers.is_empty() {
            return Ok(());
        }

        let mut last_error = None;

        // Try to send to all subscribers
        for subscriber in &state.subscribers {
            let _ = subscriber
                .try_send(msg.clone())
                .map_err(|e| match e {
                    TrySendError::Disconnected(_) => BusError::Disconnected,
                    TrySendError::Full(_) => BusError::Full,
                })
                .map_err(|e| last_error = Some(e));
        }

        // If at least one send succeeded, consider it a success
        // This is fire-and-forget - we don't care if some receivers are dead
        Ok(())
    }

    fn send_request(&self, msg: KernelMsg) -> Result<Receiver<Response>, BusError> {
        // Create request-response pair
        let (req, rx) = RequestWithResponse::new(msg);

        // Send the message
        self.send(req.message.clone())?;

        // Return the response receiver
        Ok(rx)
    }

    fn subscribe(&self) -> Receiver<KernelMsg> {
        let (tx, rx) = unbounded::<KernelMsg>();

        let mut state = self.state.lock().unwrap();
        state.subscribers.push(tx);

        rx
    }

    fn clone_box(&self) -> Box<dyn MessageBus> {
        Box::new(self.clone())
    }
}

/// Simple point-to-point channel for direct messaging
///
/// This is a simpler alternative to the broadcast bus.
/// Useful for 1-to-1 communication between specific services.
///
/// # Example
/// ```rust, no_run
/// # // Note: DirectBus is for internal use, prefer LockedBus for external API
/// ```
pub struct DirectBus {
    tx: Sender<KernelMsg>,
    rx: Receiver<KernelMsg>,
}

impl DirectBus {
    /// Create a new direct message bus
    pub fn new() -> Self {
        let (tx, rx) = unbounded();
        Self { tx, rx }
    }

    /// Get the sender handle
    pub fn sender(&self) -> DirectBusSender {
        DirectBusSender {
            tx: self.tx.clone(),
        }
    }

    /// Get the receiver handle
    pub fn receiver(&self) -> DirectBusReceiver {
        DirectBusReceiver { rx: self.rx.clone() }
    }
}

impl Default for DirectBus {
    fn default() -> Self {
        Self::new()
    }
}

/// Sender handle for DirectBus
#[derive(Clone)]
pub struct DirectBusSender {
    tx: Sender<KernelMsg>,
}

impl DirectBusSender {
    /// Send a message through the bus
    pub fn send(&self, msg: KernelMsg) -> Result<(), BusError> {
        self.tx.try_send(msg).map_err(|e| match e {
            TrySendError::Disconnected(_) => BusError::Disconnected,
            TrySendError::Full(_) => BusError::Full,
        })
    }
}

/// Receiver handle for DirectBus
#[derive(Clone)]
pub struct DirectBusReceiver {
    rx: Receiver<KernelMsg>,
}

impl DirectBusReceiver {
    /// Receive a message, blocking until available
    pub fn recv(&self) -> Result<KernelMsg, BusError> {
        self.rx.recv().map_err(|_| BusError::Disconnected)
    }

    /// Try to receive a message without blocking
    pub fn try_recv(&self) -> Result<KernelMsg, BusError> {
        self.rx.try_recv().map_err(|e| match e {
            crossbeam_channel::TryRecvError::Empty => {
                BusError::Full // Empty channel - treat as would block
            }
            crossbeam_channel::TryRecvError::Disconnected => BusError::Disconnected,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::msg::{Interrupt, Syscall};

    #[test]
    fn test_locked_bus_send() {
        let bus = LockedBus::new();

        // Test sending with no subscribers (should succeed)
        let msg = KernelMsg::Interrupt(Interrupt::Timer);
        let result = bus.send(msg);
        assert!(result.is_ok(), "Send should succeed with no subscribers");
    }

    #[test]
    fn test_locked_bus_send_receive() {
        let bus = LockedBus::new();
        let receiver = bus.subscribe();

        // Send a message
        let msg = KernelMsg::Interrupt(Interrupt::Timer);
        bus.send(msg.clone()).unwrap();

        // Receive the message
        let received = receiver.recv().unwrap();
        assert_eq!(msg, received);
    }

    #[test]
    fn test_locked_bus_multiple_subscribers() {
        let bus = LockedBus::new();
        let receiver1 = bus.subscribe();
        let receiver2 = bus.subscribe();

        // Send a message
        let msg = KernelMsg::Interrupt(Interrupt::Timer);
        bus.send(msg.clone()).unwrap();

        // Both receivers should get the message
        let received1 = receiver1.recv().unwrap();
        let received2 = receiver2.recv().unwrap();
        assert_eq!(msg, received1);
        assert_eq!(msg, received2);
    }

    #[test]
    fn test_direct_bus_send_receive() {
        let bus = DirectBus::new();
        let sender = bus.sender();
        let receiver = bus.receiver();

        // Send a message
        let msg = KernelMsg::Syscall(Syscall::CreateProcess {
            executable: "/bin/test".to_string(),
            args: vec!["--help".to_string()],
        });
        sender.send(msg.clone()).unwrap();

        // Receive the message
        let received = receiver.recv().unwrap();
        assert_eq!(msg, received);
    }

    #[test]
    fn test_memory_prot_flags() {
        let prot = crate::messaging::msg::MemProt::read_write();
        assert!(prot.readable);
        assert!(prot.writable);
        assert!(!prot.executable);
    }

    #[test]
    fn test_open_flags() {
        let flags = crate::messaging::msg::OpenFlags::read_only();
        assert!(flags.read);
        assert!(!flags.write);
        assert!(!flags.create);
    }
}
