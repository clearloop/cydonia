//! Messages sent by the gateway to the client.

use crate::message::{AgentSummary, McpServerSummary};
use compact_str::CompactString;
use serde::{Deserialize, Serialize};

/// Messages sent by the gateway to the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Complete response from an agent.
    Response {
        /// Source agent identifier.
        agent: CompactString,
        /// Response content.
        content: String,
    },
    /// Start of a streamed response.
    StreamStart {
        /// Source agent identifier.
        agent: CompactString,
    },
    /// A chunk of streamed content.
    StreamChunk {
        /// Chunk content.
        content: String,
    },
    /// End of a streamed response.
    StreamEnd {
        /// Source agent identifier.
        agent: CompactString,
    },
    /// Session cleared for an agent.
    SessionCleared {
        /// Agent whose session was cleared.
        agent: CompactString,
    },
    /// List of registered agents.
    AgentList {
        /// Agent summaries.
        agents: Vec<AgentSummary>,
    },
    /// Detailed agent information.
    AgentDetail {
        /// Agent name.
        name: CompactString,
        /// Agent description.
        description: CompactString,
        /// Registered tool names.
        tools: Vec<CompactString>,
        /// Skill tags.
        skill_tags: Vec<CompactString>,
        /// System prompt.
        system_prompt: String,
    },
    /// List of memory entries.
    MemoryList {
        /// Key-value pairs.
        entries: Vec<(String, String)>,
    },
    /// A single memory entry.
    MemoryEntry {
        /// Memory key.
        key: String,
        /// Memory value (None if not found).
        value: Option<String>,
    },
    /// Download has started for a model.
    DownloadStart {
        /// Model being downloaded.
        model: CompactString,
    },
    /// A file download has started.
    DownloadFileStart {
        /// Filename within the repo.
        filename: String,
        /// Total size in bytes.
        size: u64,
    },
    /// Download progress for current file (delta, not cumulative).
    DownloadProgress {
        /// Bytes downloaded in this chunk (delta).
        bytes: u64,
    },
    /// A file download has completed.
    DownloadFileEnd {
        /// Filename within the repo.
        filename: String,
    },
    /// All downloads complete for a model.
    DownloadEnd {
        /// Model that was downloaded.
        model: CompactString,
    },
    /// Error response.
    Error {
        /// Error code.
        code: u16,
        /// Error message.
        message: String,
    },
    /// Skills were reloaded successfully.
    SkillsReloaded {
        /// Number of skills loaded.
        count: usize,
    },
    /// MCP server added successfully.
    McpAdded {
        /// Server name.
        name: CompactString,
        /// Tools provided by this server.
        tools: Vec<CompactString>,
    },
    /// MCP server removed successfully.
    McpRemoved {
        /// Server name.
        name: CompactString,
        /// Tools that were removed.
        tools: Vec<CompactString>,
    },
    /// MCP servers reloaded from config.
    McpReloaded {
        /// Connected servers after reload.
        servers: Vec<McpServerSummary>,
    },
    /// List of connected MCP servers.
    McpServerList {
        /// Connected servers.
        servers: Vec<McpServerSummary>,
    },
    /// Pong response to client ping.
    Pong,
}
