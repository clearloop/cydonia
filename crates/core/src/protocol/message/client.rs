//! Messages sent by the client to the gateway.

use compact_str::CompactString;
use serde::{Deserialize, Serialize};

/// Hub package action.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HubAction {
    /// Install a hub package.
    Install,
    /// Uninstall a hub package.
    Uninstall,
}

/// Send a message to an agent and receive a complete response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendRequest {
    /// Target agent identifier.
    pub agent: CompactString,
    /// Message content.
    pub content: String,
}

/// Send a message to an agent and receive a streamed response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamRequest {
    /// Target agent identifier.
    pub agent: CompactString,
    /// Message content.
    pub content: String,
}

/// Request download of a model's files with progress reporting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadRequest {
    /// HuggingFace model ID.
    pub model: CompactString,
}

/// Install or uninstall a hub package.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubRequest {
    /// Package identifier in `scope/name` format.
    pub package: CompactString,
    /// Action to perform.
    pub action: HubAction,
}

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
    /// Install or uninstall a hub package.
    Hub {
        /// Package identifier in `scope/name` format.
        package: CompactString,
        /// Action to perform.
        action: HubAction,
    },
}

impl From<SendRequest> for ClientMessage {
    fn from(r: SendRequest) -> Self {
        Self::Send {
            agent: r.agent,
            content: r.content,
        }
    }
}

impl From<StreamRequest> for ClientMessage {
    fn from(r: StreamRequest) -> Self {
        Self::Stream {
            agent: r.agent,
            content: r.content,
        }
    }
}

impl From<DownloadRequest> for ClientMessage {
    fn from(r: DownloadRequest) -> Self {
        Self::Download { model: r.model }
    }
}

impl From<HubRequest> for ClientMessage {
    fn from(r: HubRequest) -> Self {
        Self::Hub {
            package: r.package,
            action: r.action,
        }
    }
}
