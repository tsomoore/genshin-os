// Response mechanism for genshin-os
//
// This module adds request-response capability while maintaining
// the fire-and-forget pattern for performance-critical paths.

use std::fmt;
use crossbeam_channel::{Sender, Receiver, unbounded};
use super::msg::KernelMsg;

/// Unique request identifier
///
/// Used to match requests with responses.
pub type RequestId = u64;

/// Atomic request ID counter
static NEXT_REQUEST_ID: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

/// Generate a unique request ID
pub fn generate_request_id() -> RequestId {
    NEXT_REQUEST_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
}

/// Response data types
///
/// Different services return different types of data.
#[derive(Debug, Clone, PartialEq)]
pub enum ResponseData {
    /// No data (operation succeeded but returns nothing)
    Void,

    /// Process ID
    Pid(u64),

    /// Thread ID
    Tid(u64),

    /// File descriptor
    Fd(u32),

    /// Number of bytes read/written
    BytesProcessed(usize),

    /// Physical address
    PhysicalAddr(u64),

    /// Disk statistics
    DiskStats {
        total_sectors: u32,
        used_sectors: usize,
        total_bytes: u64,
    },
    /// Raw bytes
    Bytes(Vec<u8>),

    /// Integer value
    Integer(u64),

    /// String value
    String(String),

    /// Boolean value
    Bool(bool),

    /// List of strings
    StringList(Vec<String>),
}

impl fmt::Display for ResponseData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Void => write!(f, "Void"),
            Self::Pid(pid) => write!(f, "Pid({})", pid),
            Self::Tid(tid) => write!(f, "Tid({})", tid),
            Self::Fd(fd) => write!(f, "Fd({})", fd),
            Self::BytesProcessed(n) => write!(f, "{} bytes", n),
            Self::PhysicalAddr(addr) => write!(f, "PhysAddr({:#x})", addr),
            Self::Bytes(data) => write!(f, "Bytes({} bytes)", data.len()),
            Self::Integer(n) => write!(f, "{}", n),
            Self::String(s) => write!(f, "\"{}\"", s),
            Self::Bool(b) => write!(f, "{}", b),
            Self::StringList(list) => write!(f, "[{}]", list.join(", ")),
            Self::DiskStats { total_sectors, used_sectors, .. } => {
                write!(f, "Disk({}/{} sectors)", used_sectors, total_sectors)
            }
        }
    }
}

/// Service error types
///
/// Errors that can occur during service operations.
#[derive(Debug, Clone, PartialEq)]
pub enum ServiceError {
    /// Invalid arguments
    InvalidArguments { msg: String },

    /// Resource not found
    NotFound { resource: String, id: String },

    /// Permission denied
    PermissionDenied { operation: String },

    /// Resource exhausted
    ResourceExhausted { resource: String },

    /// I/O error
    Io { details: String },

    /// Operation timeout
    Timeout { operation: String, duration_ms: u64 },

    /// Not implemented
    NotImplemented { feature: String },

    /// Generic error
    Other { code: u32, msg: String },
}

impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArguments { msg } => {
                write!(f, "Invalid arguments: {}", msg)
            }
            Self::NotFound { resource, id } => {
                write!(f, "{} not found: {}", resource, id)
            }
            Self::PermissionDenied { operation } => {
                write!(f, "Permission denied: {}", operation)
            }
            Self::ResourceExhausted { resource } => {
                write!(f, "Resource exhausted: {}", resource)
            }
            Self::Io { details } => {
                write!(f, "I/O error: {}", details)
            }
            Self::Timeout { operation, duration_ms } => {
                write!(f, "Operation '{}' timed out after {}ms", operation, duration_ms)
            }
            Self::NotImplemented { feature } => {
                write!(f, "Not implemented: {}", feature)
            }
            Self::Other { code, msg } => {
                write!(f, "Error (code {}): {}", code, msg)
            }
        }
    }
}

impl std::error::Error for ServiceError {}

/// Response from a service
///
/// Contains either success data or an error.
#[derive(Debug, Clone, PartialEq)]
pub enum Response {
    /// Operation succeeded
    Success {
        request_id: RequestId,
        data: ResponseData,
    },

    /// Operation failed
    Error {
        request_id: RequestId,
        error: ServiceError,
    },
}

impl Response {
    /// Create a success response
    pub fn success(request_id: RequestId, data: ResponseData) -> Self {
        Self::Success { request_id, data }
    }

    /// Create an error response
    pub fn error(request_id: RequestId, error: ServiceError) -> Self {
        Self::Error { request_id, error }
    }

    /// Get the request ID
    pub fn request_id(&self) -> RequestId {
        match self {
            Self::Success { request_id, .. } => *request_id,
            Self::Error { request_id, .. } => *request_id,
        }
    }

    /// Check if response is successful
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }

    /// Check if response is an error
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error { .. })
    }

    /// Get response data if successful
    pub fn data(&self) -> Option<&ResponseData> {
        match self {
            Self::Success { data, .. } => Some(data),
            Self::Error { .. } => None,
        }
    }

    /// Get error if failed
    pub fn service_error(&self) -> Option<&ServiceError> {
        match self {
            Self::Success { .. } => None,
            Self::Error { error, .. } => Some(error),
        }
    }

    /// Unwrap response data, panic if error
    pub fn unwrap_data(&self) -> &ResponseData {
        self.data().expect("Called unwrap_data on error response")
    }

    /// Unwrap error, panic if success
    pub fn unwrap_error(&self) -> &ServiceError {
        self.service_error().expect("Called unwrap_error on success response")
    }
}

impl fmt::Display for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Success { request_id, data } => {
                write!(f, "Request {}: Success {}", request_id, data)
            }
            Self::Error { request_id, error } => {
                write!(f, "Request {}: Error {}", request_id, error)
            }
        }
    }
}

/// Request with response channel
///
/// Wraps a KernelMsg with a channel to receive the response.
pub struct RequestWithResponse {
    /// Unique request ID
    pub request_id: RequestId,

    /// The actual message
    pub message: KernelMsg,

    /// Channel to send response back to requester
    pub response_channel: Sender<Response>,

    /// Timeout in milliseconds (None = no timeout)
    pub timeout_ms: Option<u64>,
}

impl RequestWithResponse {
    /// Create a new request with response
    pub fn new(message: KernelMsg) -> (Self, Receiver<Response>) {
        let request_id = generate_request_id();
        let (tx, rx) = unbounded();

        let req = Self {
            request_id,
            message,
            response_channel: tx,
            timeout_ms: None,
        };

        (req, rx)
    }

    /// Create a new request with timeout
    pub fn with_timeout(message: KernelMsg, timeout_ms: u64) -> (Self, Receiver<Response>) {
        let request_id = generate_request_id();
        let (tx, rx) = unbounded();

        let req = Self {
            request_id,
            message,
            response_channel: tx,
            timeout_ms: Some(timeout_ms),
        };

        (req, rx)
    }

    /// Send a success response through the response channel
    pub fn respond_success(&self, data: ResponseData) -> Result<(), crossbeam_channel::SendError<Response>> {
        let resp = Response::success(self.request_id, data);
        self.response_channel.send(resp)
    }

    /// Send an error response through the response channel
    pub fn respond_error(&self, error: ServiceError) -> Result<(), crossbeam_channel::SendError<Response>> {
        let resp = Response::error(self.request_id, error);
        self.response_channel.send(resp)
    }

    /// Get the request ID
    pub fn request_id(&self) -> RequestId {
        self.request_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::msg::ProcessRequest;

    #[test]
    fn test_request_id_generation() {
        let id1 = generate_request_id();
        let id2 = generate_request_id();
        assert!(id2 > id1);
    }

    #[test]
    fn test_response_success() {
        let resp = Response::success(1, ResponseData::Pid(100));
        assert!(resp.is_success());
        assert!(!resp.is_error());
        assert_eq!(resp.request_id(), 1);
        assert!(resp.data().is_some());
        assert!(resp.service_error().is_none());
    }

    #[test]
    fn test_response_error() {
        let err = ServiceError::NotFound {
            resource: "Process".to_string(),
            id: "100".to_string(),
        };
        let resp = Response::error(1, err);
        assert!(!resp.is_success());
        assert!(resp.is_error());
        assert_eq!(resp.request_id(), 1);
        assert!(resp.data().is_none());
        assert!(resp.service_error().is_some());
    }

    #[test]
    fn test_request_with_response() {
        let msg = KernelMsg::Process(ProcessRequest::Schedule { pid: 1, tid: 1 });
        let (req, _rx) = RequestWithResponse::new(msg);

        // Just check that it's non-zero (actual value depends on global counter)
        assert!(req.request_id() > 0);
        assert!(req.timeout_ms.is_none());
    }

    #[test]
    fn test_request_with_timeout() {
        let msg = KernelMsg::Process(ProcessRequest::Schedule { pid: 1, tid: 1 });
        let (req, _rx) = RequestWithResponse::with_timeout(msg, 5000);

        // Check that timeout is set correctly
        assert_eq!(req.timeout_ms, Some(5000));
    }

    #[test]
    fn test_response_channel() {
        let msg = KernelMsg::Process(ProcessRequest::Schedule { pid: 1, tid: 1 });
        let (req, rx) = RequestWithResponse::new(msg);

        // Send success response
        req.respond_success(ResponseData::Pid(100)).unwrap();

        // Receive response
        let resp = rx.recv().unwrap();
        assert!(resp.is_success());
        assert_eq!(resp.request_id(), req.request_id());
    }

    #[test]
    fn test_service_error_display() {
        let err = ServiceError::InvalidArguments {
            msg: "Invalid PID".to_string(),
        };
        assert_eq!(format!("{}", err), "Invalid arguments: Invalid PID");
    }

    #[test]
    fn test_response_data_types() {
        let pid_data = ResponseData::Pid(100);
        let bool_data = ResponseData::Bool(true);
        let bytes_data = ResponseData::Bytes(vec![1, 2, 3]);

        assert!(matches!(pid_data, ResponseData::Pid(100)));
        assert!(matches!(bool_data, ResponseData::Bool(true)));
        assert!(matches!(bytes_data, ResponseData::Bytes(_)));
    }

    #[test]
    fn test_response_display() {
        let resp = Response::success(1, ResponseData::Pid(100));
        assert_eq!(format!("{}", resp), "Request 1: Success Pid(100)");
    }
}

/// Wrapper for KernelMsg to support both fire-and-forget and request-response
///
/// This enum allows the message bus to handle both patterns seamlessly.
#[derive(Debug, Clone, PartialEq)]
pub enum KernelMessage {
    /// Fire-and-forget message (original behavior)
    Message(KernelMsg),

    /// Request that expects a response
    Request {
        request_id: RequestId,
        message: KernelMsg,
    },
}

impl From<KernelMsg> for KernelMessage {
    fn from(msg: KernelMsg) -> Self {
        Self::Message(msg)
    }
}

impl From<RequestWithResponse> for KernelMessage {
    fn from(req: RequestWithResponse) -> Self {
        Self::Request {
            request_id: req.request_id,
            message: req.message,
        }
    }
}
