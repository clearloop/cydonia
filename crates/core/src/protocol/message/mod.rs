//! Wire protocol message types — enums, payload structs, and conversions.

use crate::protocol::message::{client::ClientMessage, server::ServerMessage};
use compact_str::CompactString;
use serde::{Deserialize, Serialize};

pub mod client;
pub mod server;

// ---------------------------------------------------------------------------
// Request structs
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Response structs
// ---------------------------------------------------------------------------

/// Complete response from an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendResponse {
    /// Source agent identifier.
    pub agent: CompactString,
    /// Response content.
    pub content: String,
}

// ---------------------------------------------------------------------------
// Streaming event enums
// ---------------------------------------------------------------------------

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
        /// Filename within the repo.
        filename: String,
        /// Total size in bytes.
        size: u64,
    },
    /// Download progress for current file (delta, not cumulative).
    Progress {
        /// Bytes downloaded in this chunk.
        bytes: u64,
    },
    /// A file download has completed.
    FileEnd {
        /// Filename within the repo.
        filename: String,
    },
    /// All downloads complete.
    End {
        /// Model that was downloaded.
        model: CompactString,
    },
}

// ---------------------------------------------------------------------------
// From<Request> for ClientMessage
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// From<Response> for ServerMessage
// ---------------------------------------------------------------------------

impl From<SendResponse> for ServerMessage {
    fn from(r: SendResponse) -> Self {
        Self::Response {
            agent: r.agent,
            content: r.content,
        }
    }
}

// ---------------------------------------------------------------------------
// From<StreamEvent> for ServerMessage
// ---------------------------------------------------------------------------

impl From<StreamEvent> for ServerMessage {
    fn from(e: StreamEvent) -> Self {
        match e {
            StreamEvent::Start { agent } => Self::StreamStart { agent },
            StreamEvent::Chunk { content } => Self::StreamChunk { content },
            StreamEvent::End { agent } => Self::StreamEnd { agent },
        }
    }
}

// ---------------------------------------------------------------------------
// From<DownloadEvent> for ServerMessage
// ---------------------------------------------------------------------------

impl From<DownloadEvent> for ServerMessage {
    fn from(e: DownloadEvent) -> Self {
        match e {
            DownloadEvent::Start { model } => Self::DownloadStart { model },
            DownloadEvent::FileStart { filename, size } => {
                Self::DownloadFileStart { filename, size }
            }
            DownloadEvent::Progress { bytes } => Self::DownloadProgress { bytes },
            DownloadEvent::FileEnd { filename } => Self::DownloadFileEnd { filename },
            DownloadEvent::End { model } => Self::DownloadEnd { model },
        }
    }
}

// ---------------------------------------------------------------------------
// TryFrom<ServerMessage> for response structs
// ---------------------------------------------------------------------------

fn unexpected(msg: &str) -> anyhow::Error {
    anyhow::anyhow!("unexpected response: {msg}")
}

fn error_or_unexpected(msg: ServerMessage) -> anyhow::Error {
    match msg {
        ServerMessage::Error { code, message } => {
            anyhow::anyhow!("server error ({code}): {message}")
        }
        other => unexpected(&format!("{other:?}")),
    }
}

impl TryFrom<ServerMessage> for SendResponse {
    type Error = anyhow::Error;
    fn try_from(msg: ServerMessage) -> anyhow::Result<Self> {
        match msg {
            ServerMessage::Response { agent, content } => Ok(Self { agent, content }),
            other => Err(error_or_unexpected(other)),
        }
    }
}

// ---------------------------------------------------------------------------
// TryFrom<ServerMessage> for streaming events
// ---------------------------------------------------------------------------

impl TryFrom<ServerMessage> for StreamEvent {
    type Error = anyhow::Error;
    fn try_from(msg: ServerMessage) -> anyhow::Result<Self> {
        match msg {
            ServerMessage::StreamStart { agent } => Ok(Self::Start { agent }),
            ServerMessage::StreamChunk { content } => Ok(Self::Chunk { content }),
            ServerMessage::StreamEnd { agent } => Ok(Self::End { agent }),
            other => Err(error_or_unexpected(other)),
        }
    }
}

impl TryFrom<ServerMessage> for DownloadEvent {
    type Error = anyhow::Error;
    fn try_from(msg: ServerMessage) -> anyhow::Result<Self> {
        match msg {
            ServerMessage::DownloadStart { model } => Ok(Self::Start { model }),
            ServerMessage::DownloadFileStart { filename, size } => {
                Ok(Self::FileStart { filename, size })
            }
            ServerMessage::DownloadProgress { bytes } => Ok(Self::Progress { bytes }),
            ServerMessage::DownloadFileEnd { filename } => Ok(Self::FileEnd { filename }),
            ServerMessage::DownloadEnd { model } => Ok(Self::End { model }),
            other => Err(error_or_unexpected(other)),
        }
    }
}
