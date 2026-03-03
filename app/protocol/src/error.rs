//! Protocol error type shared between client and server traits.

use serde::{Deserialize, Serialize};

/// Error returned by protocol operations.
///
/// On the wire this corresponds to `ServerMessage::Error { code, message }`.
/// In trait APIs it appears as the `Err` variant of `Result`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolError {
    /// Error code (e.g. 404, 500).
    pub code: u16,
    /// Human-readable error description.
    pub message: String,
}

impl ProtocolError {
    /// Create a new protocol error.
    pub fn new(code: u16, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "protocol error ({}): {}", self.code, self.message)
    }
}

impl std::error::Error for ProtocolError {}
