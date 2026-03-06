//! Daemon configuration loaded from TOML.

pub use ::model::{ProviderConfig, ProviderManager};
use anyhow::Result;
use compact_str::CompactString;
pub use default::{
    AGENTS_DIR, DATA_DIR, GLOBAL_CONFIG_DIR, SKILLS_DIR, SOCKET_PATH, scaffold_config_dir,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub use {agent::AgentConfig, loader::load_agents_dir};
pub use {channel::ChannelConfig, mcp::McpServerConfig};

mod agent;
mod default;
mod loader;
mod mcp;

/// Top-level daemon configuration.
#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonConfig {
    /// LLM provider configurations keyed by a user-defined name.
    /// If a provider's `model` field is empty, it is filled from the key.
    #[serde(default)]
    pub models: BTreeMap<CompactString, ProviderConfig>,
    /// Channel configurations keyed by a user-defined name.
    /// If a channel's `platform` field is empty, it is filled from the key.
    #[serde(default)]
    pub channels: BTreeMap<CompactString, ChannelConfig>,
    /// MCP server configurations.
    #[serde(default)]
    pub mcp_servers: BTreeMap<CompactString, mcp::McpServerConfig>,
    /// Agent configurations.
    #[serde(default)]
    pub agents: AgentConfig,
}

impl DaemonConfig {
    /// Parse a TOML string into a `DaemonConfig`.
    pub fn from_toml(toml_str: &str) -> Result<Self> {
        let mut config: Self = toml::from_str(toml_str)?;
        config.models.iter_mut().for_each(|(key, provider)| {
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

#[cfg(not(feature = "local"))]
impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            models: [(
                "deepseek-chat".into(),
                ProviderConfig {
                    model: "deepseek-chat".into(),
                    api_key: None,
                    base_url: None,
                    loader: None,
                    quantization: None,
                    chat_template: None,
                },
            )]
            .into(),
            channels: Default::default(),
            mcp_servers: Default::default(),
            agents: AgentConfig::default(),
        }
    }
}

#[cfg(feature = "local")]
impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            models: [(
                "local".into(),
                ProviderConfig {
                    model: "Qwen/Qwen3-4B".into(),
                    api_key: None,
                    base_url: None,
                    loader: Some(model::Loader::Text),
                    quantization: None,
                    chat_template: None,
                },
            )]
            .into(),
            channels: Default::default(),
            mcp_servers: Default::default(),
            agents: AgentConfig::default(),
        }
    }
}
