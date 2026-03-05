//! Messages sent by the client to the gateway.

use compact_str::CompactString;
use serde::{Deserialize, Serialize};

/// Messages sent by the client to the gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Send a message to an agent and receive a complete response.
    Send {
        /// Target agent identifier.
        agent: CompactString,
        /// Message content.
        content: String,
    },
    /// Send a message to an agent and receive a streamed response.
    Stream {
        /// Target agent identifier.
        agent: CompactString,
        /// Message content.
        content: String,
    },
    /// Request download of a model's files with progress reporting.
    Download {
        /// HuggingFace model ID (e.g. "microsoft/Phi-3.5-mini-instruct").
        model: CompactString,
    },
    /// Ping the server (keepalive).
    Ping,
}
