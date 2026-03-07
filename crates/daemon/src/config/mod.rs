//! Daemon configuration loaded from TOML.

pub use ::model::{ProviderConfig, ProviderManager};
use anyhow::Result;
use compact_str::CompactString;
pub use default::{
    AGENTS_DIR, DATA_DIR, GLOBAL_CONFIG_DIR, SKILLS_DIR, SOCKET_PATH, WORK_DIR,
    scaffold_config_dir, scaffold_work_dir,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
pub use {channel::ChannelConfig, mcp::McpServerConfig};
pub use {loader::load_agents_dir, model::ModelConfig};

mod default;
mod loader;
mod mcp;
mod model;

/// Top-level daemon configuration.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct DaemonConfig {
    /// Model configurations.
    #[serde(default)]
    pub model: ModelConfig,
    /// Channel configurations keyed by a user-defined name.
    /// If a channel's `platform` field is empty, it is filled from the key.
    #[serde(default)]
    pub channels: BTreeMap<CompactString, ChannelConfig>,
    /// MCP server configurations.
    #[serde(default)]
    pub mcp_servers: BTreeMap<CompactString, mcp::McpServerConfig>,
    /// Optional symlink path for the workspace sandbox root (`~/.walrus/work/`).
    ///
    /// When set, a symlink is created at this path pointing to `~/.walrus/work/`.
    #[serde(default)]
    pub work_dir: Option<PathBuf>,
}

impl DaemonConfig {
    /// Parse a TOML string into a `DaemonConfig`.
    pub fn from_toml(toml_str: &str) -> Result<Self> {
        let mut config: Self = toml::from_str(toml_str)?;
        config
            .model
            .providers
            .iter_mut()
            .for_each(|(key, provider)| {
                if provider.model.is_empty() {
                    provider.model = key.clone();
                }
            });
        config.channels.iter_mut().for_each(|(key, channel)| {
            if channel.platform.is_empty() {
                channel.platform = key.clone();
            }
        });
        config.mcp_servers.iter_mut().for_each(|(name, server)| {
            if server.name.is_empty() {
                server.name = name.clone();
            }
        });
        Ok(config)
    }

    /// Load configuration from a file path.
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::from_toml(&content)
    }
}
