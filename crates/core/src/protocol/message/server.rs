//! Messages sent by the gateway to the client.

use compact_str::CompactString;
use serde::{Deserialize, Serialize};

/// Complete response from an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendResponse {
    /// Source agent identifier.
    pub agent: CompactString,
    /// Response content.
    pub content: String,
}

/// Events emitted during a streamed agent response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StreamEvent {
    /// Stream has started.
    Start {
        /// Source agent identifier.
        agent: CompactString,
    },
    /// A chunk of streamed content.
    Chunk {
        /// Chunk content.
        content: String,
    },
    /// Stream has ended.
    End {
        /// Source agent identifier.
        agent: CompactString,
    },
}

/// Events emitted during a model download.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DownloadEvent {
    /// Download has started.
    Start {
        /// Model being downloaded.
        model: CompactString,
    },
    /// A file download has started.
    FileStart {
        /// Model being downloaded.
        model: CompactString,
        /// Filename within the repo.
        filename: String,
        /// Total size in bytes.
        size: u64,
    },
    /// Download progress for current file (delta, not cumulative).
    Progress {
        /// Model being downloaded.
        model: CompactString,
        /// Bytes downloaded in this chunk.
        bytes: u64,
    },
    /// A file download has completed.
    FileEnd {
        /// Model being downloaded.
        model: CompactString,
        /// Filename within the repo.
        filename: String,
    },
    /// All downloads complete.
    End {
        /// Model that was downloaded.
        model: CompactString,
    },
}

/// Events emitted during a hub install or uninstall operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HubEvent {
    /// Operation has started.
    Start {
        /// Package being operated on.
        package: CompactString,
    },
    /// A progress step message.
    Step {
        /// Human-readable step description.
        message: String,
    },
    /// Operation has completed.
    End {
        /// Package that was operated on.
        package: CompactString,
    },
}

/// Messages sent by the gateway to the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Complete response from an agent.
    Response(SendResponse),
    /// A streamed response event.
    Stream(StreamEvent),
    /// A model download event.
    Download(DownloadEvent),
    /// Error response.
    Error {
        /// Error code.
        code: u16,
        /// Error message.
        message: String,
    },
    /// Pong response to client ping.
    Pong,
    /// A hub install/uninstall event.
    Hub(HubEvent),
}

impl From<SendResponse> for ServerMessage {
    fn from(r: SendResponse) -> Self {
        Self::Response(r)
    }
}

impl From<StreamEvent> for ServerMessage {
    fn from(e: StreamEvent) -> Self {
        Self::Stream(e)
    }
}

impl From<DownloadEvent> for ServerMessage {
    fn from(e: DownloadEvent) -> Self {
        Self::Download(e)
    }
}

impl From<HubEvent> for ServerMessage {
    fn from(e: HubEvent) -> Self {
        Self::Hub(e)
    }
}

fn error_or_unexpected(msg: ServerMessage) -> anyhow::Error {
    match msg {
        ServerMessage::Error { code, message } => {
            anyhow::anyhow!("server error ({code}): {message}")
        }
        other => anyhow::anyhow!("unexpected response: {other:?}"),
    }
}

impl TryFrom<ServerMessage> for SendResponse {
    type Error = anyhow::Error;
    fn try_from(msg: ServerMessage) -> anyhow::Result<Self> {
        match msg {
            ServerMessage::Response(r) => Ok(r),
            other => Err(error_or_unexpected(other)),
        }
    }
}

impl TryFrom<ServerMessage> for StreamEvent {
    type Error = anyhow::Error;
    fn try_from(msg: ServerMessage) -> anyhow::Result<Self> {
        match msg {
            ServerMessage::Stream(e) => Ok(e),
            other => Err(error_or_unexpected(other)),
        }
    }
}

impl TryFrom<ServerMessage> for DownloadEvent {
    type Error = anyhow::Error;
    fn try_from(msg: ServerMessage) -> anyhow::Result<Self> {
        match msg {
            ServerMessage::Download(e) => Ok(e),
            other => Err(error_or_unexpected(other)),
        }
    }
}

impl TryFrom<ServerMessage> for HubEvent {
    type Error = anyhow::Error;
    fn try_from(msg: ServerMessage) -> anyhow::Result<Self> {
        match msg {
            ServerMessage::Hub(e) => Ok(e),
            other => Err(error_or_unexpected(other)),
        }
    }
}
