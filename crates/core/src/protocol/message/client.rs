//! Messages sent by the client to the gateway.

use compact_str::CompactString;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
    /// Clear the session history for an agent.
    ClearSession {
        /// Target agent identifier.
        agent: CompactString,
    },
    /// List all registered agents.
    ListAgents,
    /// Get detailed info for a specific agent.
    AgentInfo {
        /// Agent name.
        agent: CompactString,
    },
    /// List all memory entries.
    ListMemory,
    /// Get a specific memory entry by key.
    GetMemory {
        /// Memory key.
        key: String,
    },
    /// Request download of a model's files with progress reporting.
    Download {
        /// HuggingFace model ID (e.g. "microsoft/Phi-3.5-mini-instruct").
        model: CompactString,
    },
    /// Reload skills from disk.
    ReloadSkills,
    /// Add an MCP server to config and reload.
    McpAdd {
        /// Server name.
        name: CompactString,
        /// Command to spawn.
        command: String,
        /// Command arguments.
        #[serde(default)]
        args: Vec<String>,
        /// Environment variables.
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
    /// Remove an MCP server from config and reload.
    McpRemove {
        /// Server name to remove.
        name: CompactString,
    },
    /// Reload MCP servers from walrus.toml.
    McpReload,
    /// List connected MCP servers and their tools.
    McpList,
    /// Ping the server (keepalive).
    Ping,
}
