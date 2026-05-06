// Message bus implementations for genshin-os
//
// This module defines the MessageBus trait and provides implementations:
// - LockedBus: Mutex-based using crossbeam channels
// - LockFreeBus: (Future) Atomic-based lock-free implementation

use crate::messaging::msg::KernelMsg;
use crate::messaging::response::{RequestWithResponse, Response, RequestId, generate_request_id};
use crossbeam_channel::{unbounded, Receiver, Sender, TrySendError};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
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

/// Message envelope that wraps KernelMsg with optional response channel
///
/// This allows the message bus to carry both fire-and-forget messages
/// and request-response messages.
pub struct Envelope {
    /// The actual message
    pub message: KernelMsg,

    /// Request ID (if this is a request)
    pub request_id: Option<RequestId>,

    /// Response channel (if this is a request)
    pub response_channel: Option<Sender<Response>>,
}

impl Envelope {
    /// Create a fire-and-forget envelope
    pub fn fire_and_forget(message: KernelMsg) -> Self {
        Self {
            message,
            request_id: None,
            response_channel: None,
        }
    }

    /// Create a request envelope with response channel
    pub fn with_response(message: KernelMsg) -> (Self, Receiver<Response>) {
        let request_id = generate_request_id();
        let (tx, rx) = unbounded();

        let envelope = Self {
            message,
            request_id: Some(request_id),
            response_channel: Some(tx),
        };

        (envelope, rx)
    }

    /// Check if this envelope expects a response
    pub fn expects_response(&self) -> bool {
        self.response_channel.is_some()
    }

    /// Send a success response
    pub fn respond_success(&self, data: crate::messaging::response::ResponseData) -> Result<(), crossbeam_channel::SendError<Response>> {
        if let (Some(request_id), Some(channel)) = (self.request_id, &self.response_channel) {
            let resp = Response::success(request_id, data);
            channel.send(resp)
        } else {
            Ok(()) // No response expected, silently succeed
        }
    }

    /// Send an error response
    pub fn respond_error(&self, error: crate::messaging::response::ServiceError) -> Result<(), crossbeam_channel::SendError<Response>> {
        if let (Some(request_id), Some(channel)) = (self.request_id, &self.response_channel) {
            let resp = Response::error(request_id, error);
            channel.send(resp)
        } else {
            Ok(()) // No response expected, silently succeed
        }
    }
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

    /// Subscribe to receive envelopes from the bus
    ///
    /// Returns a receiver for the subscribed envelope channel.
    fn subscribe(&self) -> Receiver<Envelope>;

    /// Clone the bus handle
    ///
    /// Allows multiple handles to the same bus.
    fn clone_box(&self) -> Box<dyn MessageBus>;
}

/// Internal state for LockedBus
struct LockedBusState {
    subscribers: Vec<Sender<Envelope>>,
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

        let envelope = Envelope::fire_and_forget(msg);

        // Try to send to all subscribers
        // Each gets its own copy of the message
        for subscriber in &state.subscribers {
            let _ = subscriber.try_send(Envelope::fire_and_forget(envelope.message.clone()));
        }

        // Fire-and-forget - don't care about errors
        Ok(())
    }

    fn send_request(&self, msg: KernelMsg) -> Result<Receiver<Response>, BusError> {
        let state = self.state.lock().unwrap();

        // Create envelope with response channel
        let (envelope, rx) = Envelope::with_response(msg);

        // Extract the parts we need
        let message = envelope.message;
        let request_id = envelope.request_id;
        let response_channel = envelope.response_channel;

        // Send to the first available subscriber
        // For request-response, only one handler should process it
        for subscriber in &state.subscribers {
            let env = Envelope {
                message: message.clone(),
                request_id,
                response_channel: response_channel.clone(),
            };

            if subscriber.try_send(env).is_ok() {
                return Ok(rx);
            }
        }

        Err(BusError::Disconnected)
    }

    fn subscribe(&self) -> Receiver<Envelope> {
        let (tx, rx) = unbounded::<Envelope>();

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
    tx: Sender<Envelope>,
    rx: Receiver<Envelope>,
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
    tx: Sender<Envelope>,
}

impl DirectBusSender {
    /// Send a message through the bus
    pub fn send(&self, msg: KernelMsg) -> Result<(), BusError> {
        let envelope = Envelope::fire_and_forget(msg);
        self.tx.try_send(envelope).map_err(|e| match e {
            TrySendError::Disconnected(_) => BusError::Disconnected,
            TrySendError::Full(_) => BusError::Full,
        })
    }

    /// Send a request and wait for response
    pub fn send_request(&self, msg: KernelMsg) -> Result<Receiver<Response>, BusError> {
        let (envelope, rx) = Envelope::with_response(msg);
        self.tx.try_send(envelope).map_err(|e| match e {
            TrySendError::Disconnected(_) => BusError::Disconnected,
            TrySendError::Full(_) => BusError::Full,
        })?;
        Ok(rx)
    }
}

/// Receiver handle for DirectBus
#[derive(Clone)]
pub struct DirectBusReceiver {
    rx: Receiver<Envelope>,
}

impl DirectBusReceiver {
    /// Receive a message, blocking until available
    pub fn recv(&self) -> Result<Envelope, BusError> {
        self.rx.recv().map_err(|_| BusError::Disconnected)
    }

    /// Try to receive a message without blocking
    pub fn try_recv(&self) -> Result<Envelope, BusError> {
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

        // Receive the envelope
        let envelope = receiver.recv().unwrap();
        assert!(!envelope.expects_response());
        assert_eq!(msg, envelope.message);
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
        let env1 = receiver1.recv().unwrap();
        let env2 = receiver2.recv().unwrap();
        assert_eq!(msg, env1.message);
        assert_eq!(msg, env2.message);
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

        // Receive the envelope
        let envelope = receiver.recv().unwrap();
        assert_eq!(msg, envelope.message);
        assert!(!envelope.expects_response());
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
