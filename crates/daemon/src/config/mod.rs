//! Daemon configuration loaded from TOML.

use anyhow::Result;
pub use model::{ProviderConfig, ProviderManager};
pub use router::ChannelConfig;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub use default::{DEFAULT_AGENT_MD, scaffold_config_dir};
pub use loader::{load_agents_dir, load_cron_dir};

mod default;
mod loader;

/// Agents subdirectory (contains *.md files).
pub const AGENTS_DIR: &str = "agents";
/// Skills subdirectory.
pub const SKILLS_DIR: &str = "skills";
/// Cron subdirectory (contains *.md files).
pub const CRON_DIR: &str = "cron";
/// Data subdirectory.
pub const DATA_DIR: &str = "data";
/// SQLite memory database filename.
pub const MEMORY_DB: &str = "memory.db";

/// Resolve the global configuration directory (`~/.walrus/`).
pub fn global_config_dir() -> PathBuf {
    dirs::home_dir().expect("no home directory").join(".walrus")
}

/// Pinned socket path (`~/.walrus/walrus.sock`).
pub fn socket_path() -> PathBuf {
    global_config_dir().join("walrus.sock")
}

/// Top-level daemon configuration.
#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonConfig {
    /// LLM provider configurations (`[[models]]` array).
    pub models: Vec<ProviderConfig>,
    /// Channel configurations.
    #[serde(default)]
    pub channels: Vec<ChannelConfig>,
    /// MCP server configurations.
    #[serde(default)]
    pub mcp_servers: Vec<mcp::McpServerConfig>,
}

impl DaemonConfig {
    /// Parse a TOML string into a `DaemonConfig`.
    pub fn from_toml(toml_str: &str) -> Result<Self> {
        let config: Self = toml::from_str(toml_str)?;
        Ok(config)
    }

    /// Load configuration from a file path.
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::from_toml(&content)
    }
}
