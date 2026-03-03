//! Wire protocol message types — enums, payload structs, and conversions.

use client::ClientMessage;
use compact_str::CompactString;
use serde::{Deserialize, Serialize};
use server::ServerMessage;
use std::collections::BTreeMap;

pub mod client;
pub mod server;

// ---------------------------------------------------------------------------
// Shared summary types
// ---------------------------------------------------------------------------

/// Summary of a registered agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSummary {
    /// Agent name.
    pub name: CompactString,
    /// Agent description.
    pub description: CompactString,
}

/// Summary of a connected MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerSummary {
    /// Server name.
    pub name: CompactString,
    /// Tool names provided by this server.
    pub tools: Vec<CompactString>,
}

// ---------------------------------------------------------------------------
// Request structs (from ClientMessage variants with fields)
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

/// Clear the session history for an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClearSessionRequest {
    /// Target agent identifier.
    pub agent: CompactString,
}

/// Get detailed info for a specific agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfoRequest {
    /// Agent name.
    pub agent: CompactString,
}

/// Get a specific memory entry by key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetMemoryRequest {
    /// Memory key.
    pub key: String,
}

/// Request download of a model's files with progress reporting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadRequest {
    /// HuggingFace model ID.
    pub model: CompactString,
}

/// Add an MCP server to config and reload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpAddRequest {
    /// Server name.
    pub name: CompactString,
    /// Command to spawn.
    pub command: String,
    /// Command arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// Remove an MCP server from config and reload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRemoveRequest {
    /// Server name to remove.
    pub name: CompactString,
}

// ---------------------------------------------------------------------------
// Response structs (from ServerMessage variants)
// ---------------------------------------------------------------------------

/// Complete response from an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendResponse {
    /// Source agent identifier.
    pub agent: CompactString,
    /// Response content.
    pub content: String,
}

/// Session cleared confirmation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCleared {
    /// Agent whose session was cleared.
    pub agent: CompactString,
}

/// List of registered agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentList {
    /// Agent summaries.
    pub agents: Vec<AgentSummary>,
}

/// Detailed agent information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDetail {
    /// Agent name.
    pub name: CompactString,
    /// Agent description.
    pub description: CompactString,
    /// Registered tool names.
    pub tools: Vec<CompactString>,
    /// Skill tags.
    pub skill_tags: Vec<CompactString>,
    /// System prompt.
    pub system_prompt: String,
}

/// List of memory entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryList {
    /// Key-value pairs.
    pub entries: Vec<(String, String)>,
}

/// A single memory entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// Memory key.
    pub key: String,
    /// Memory value (None if not found).
    pub value: Option<String>,
}

/// Skills reloaded confirmation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsReloaded {
    /// Number of skills loaded.
    pub count: usize,
}

/// MCP server added confirmation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpAdded {
    /// Server name.
    pub name: CompactString,
    /// Tools provided by this server.
    pub tools: Vec<CompactString>,
}

/// MCP server removed confirmation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRemoved {
    /// Server name.
    pub name: CompactString,
    /// Tools that were removed.
    pub tools: Vec<CompactString>,
}

/// MCP servers reloaded confirmation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpReloaded {
    /// Connected servers after reload.
    pub servers: Vec<McpServerSummary>,
}

/// List of connected MCP servers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerList {
    /// Connected servers.
    pub servers: Vec<McpServerSummary>,
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

impl From<ClearSessionRequest> for ClientMessage {
    fn from(r: ClearSessionRequest) -> Self {
        Self::ClearSession { agent: r.agent }
    }
}

impl From<AgentInfoRequest> for ClientMessage {
    fn from(r: AgentInfoRequest) -> Self {
        Self::AgentInfo { agent: r.agent }
    }
}

impl From<GetMemoryRequest> for ClientMessage {
    fn from(r: GetMemoryRequest) -> Self {
        Self::GetMemory { key: r.key }
    }
}

impl From<DownloadRequest> for ClientMessage {
    fn from(r: DownloadRequest) -> Self {
        Self::Download { model: r.model }
    }
}

impl From<McpAddRequest> for ClientMessage {
    fn from(r: McpAddRequest) -> Self {
        Self::McpAdd {
            name: r.name,
            command: r.command,
            args: r.args,
            env: r.env,
        }
    }
}

impl From<McpRemoveRequest> for ClientMessage {
    fn from(r: McpRemoveRequest) -> Self {
        Self::McpRemove { name: r.name }
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

impl From<SessionCleared> for ServerMessage {
    fn from(r: SessionCleared) -> Self {
        Self::SessionCleared { agent: r.agent }
    }
}

impl From<AgentList> for ServerMessage {
    fn from(r: AgentList) -> Self {
        Self::AgentList { agents: r.agents }
    }
}

impl From<AgentDetail> for ServerMessage {
    fn from(r: AgentDetail) -> Self {
        Self::AgentDetail {
            name: r.name,
            description: r.description,
            tools: r.tools,
            skill_tags: r.skill_tags,
            system_prompt: r.system_prompt,
        }
    }
}

impl From<MemoryList> for ServerMessage {
    fn from(r: MemoryList) -> Self {
        Self::MemoryList { entries: r.entries }
    }
}

impl From<MemoryEntry> for ServerMessage {
    fn from(r: MemoryEntry) -> Self {
        Self::MemoryEntry {
            key: r.key,
            value: r.value,
        }
    }
}

impl From<SkillsReloaded> for ServerMessage {
    fn from(r: SkillsReloaded) -> Self {
        Self::SkillsReloaded { count: r.count }
    }
}

impl From<McpAdded> for ServerMessage {
    fn from(r: McpAdded) -> Self {
        Self::McpAdded {
            name: r.name,
            tools: r.tools,
        }
    }
}

impl From<McpRemoved> for ServerMessage {
    fn from(r: McpRemoved) -> Self {
        Self::McpRemoved {
            name: r.name,
            tools: r.tools,
        }
    }
}

impl From<McpReloaded> for ServerMessage {
    fn from(r: McpReloaded) -> Self {
        Self::McpReloaded { servers: r.servers }
    }
}

impl From<McpServerList> for ServerMessage {
    fn from(r: McpServerList) -> Self {
        Self::McpServerList { servers: r.servers }
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

impl TryFrom<ServerMessage> for SessionCleared {
    type Error = anyhow::Error;
    fn try_from(msg: ServerMessage) -> anyhow::Result<Self> {
        match msg {
            ServerMessage::SessionCleared { agent } => Ok(Self { agent }),
            other => Err(error_or_unexpected(other)),
        }
    }
}

impl TryFrom<ServerMessage> for AgentList {
    type Error = anyhow::Error;
    fn try_from(msg: ServerMessage) -> anyhow::Result<Self> {
        match msg {
            ServerMessage::AgentList { agents } => Ok(Self { agents }),
            other => Err(error_or_unexpected(other)),
        }
    }
}

impl TryFrom<ServerMessage> for AgentDetail {
    type Error = anyhow::Error;
    fn try_from(msg: ServerMessage) -> anyhow::Result<Self> {
        match msg {
            ServerMessage::AgentDetail {
                name,
                description,
                tools,
                skill_tags,
                system_prompt,
            } => Ok(Self {
                name,
                description,
                tools,
                skill_tags,
                system_prompt,
            }),
            other => Err(error_or_unexpected(other)),
        }
    }
}

impl TryFrom<ServerMessage> for MemoryList {
    type Error = anyhow::Error;
    fn try_from(msg: ServerMessage) -> anyhow::Result<Self> {
        match msg {
            ServerMessage::MemoryList { entries } => Ok(Self { entries }),
            other => Err(error_or_unexpected(other)),
        }
    }
}

impl TryFrom<ServerMessage> for MemoryEntry {
    type Error = anyhow::Error;
    fn try_from(msg: ServerMessage) -> anyhow::Result<Self> {
        match msg {
            ServerMessage::MemoryEntry { key, value } => Ok(Self { key, value }),
            other => Err(error_or_unexpected(other)),
        }
    }
}

impl TryFrom<ServerMessage> for SkillsReloaded {
    type Error = anyhow::Error;
    fn try_from(msg: ServerMessage) -> anyhow::Result<Self> {
        match msg {
            ServerMessage::SkillsReloaded { count } => Ok(Self { count }),
            other => Err(error_or_unexpected(other)),
        }
    }
}

impl TryFrom<ServerMessage> for McpAdded {
    type Error = anyhow::Error;
    fn try_from(msg: ServerMessage) -> anyhow::Result<Self> {
        match msg {
            ServerMessage::McpAdded { name, tools } => Ok(Self { name, tools }),
            other => Err(error_or_unexpected(other)),
        }
    }
}

impl TryFrom<ServerMessage> for McpRemoved {
    type Error = anyhow::Error;
    fn try_from(msg: ServerMessage) -> anyhow::Result<Self> {
        match msg {
            ServerMessage::McpRemoved { name, tools } => Ok(Self { name, tools }),
            other => Err(error_or_unexpected(other)),
        }
    }
}

impl TryFrom<ServerMessage> for McpReloaded {
    type Error = anyhow::Error;
    fn try_from(msg: ServerMessage) -> anyhow::Result<Self> {
        match msg {
            ServerMessage::McpReloaded { servers } => Ok(Self { servers }),
            other => Err(error_or_unexpected(other)),
        }
    }
}

impl TryFrom<ServerMessage> for McpServerList {
    type Error = anyhow::Error;
    fn try_from(msg: ServerMessage) -> anyhow::Result<Self> {
        match msg {
            ServerMessage::McpServerList { servers } => Ok(Self { servers }),
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
