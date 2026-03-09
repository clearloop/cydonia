//! Daemon configuration loaded from TOML.

pub use ::model::{ProviderConfig, ProviderManager};
use anyhow::Result;
pub use default::scaffold_config_dir;
pub use perm::{PermissionConfig, ToolPermission};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub use wcore::paths::{AGENTS_DIR, CONFIG_DIR, DATA_DIR, MEMORY_DB, SKILLS_DIR, SOCKET_PATH};
pub use {channel::ChannelConfig, mcp::McpServerConfig};
pub use {loader::load_agents_dir, model::ModelConfig};

mod default;
mod loader;
mod mcp;
mod model;
mod perm;

/// Top-level daemon configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DaemonConfig {
    /// Model configurations.
    #[serde(default)]
    pub model: ModelConfig,
    /// Channel configuration (Telegram bot).
    #[serde(default)]
    pub channel: ChannelConfig,
    /// MCP server configurations.
    #[serde(default)]
    pub mcps: BTreeMap<String, mcp::McpServerConfig>,
    /// Memory configuration.
    #[serde(default)]
    pub memory: MemoryConfig,
    /// Task executor pool configuration.
    #[serde(default)]
    pub tasks: TasksConfig,
    /// Per-agent heartbeat configuration.
    #[serde(default)]
    pub agents: AgentsConfig,
    /// Permission configuration: global defaults + per-agent overrides.
    #[serde(default)]
    pub permissions: PermissionConfig,
}

/// Task executor pool configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TasksConfig {
    /// Maximum number of concurrently InProgress tasks (default 4).
    pub max_concurrent: usize,
    /// Maximum number of tasks returned by queries (default 16).
    pub viewable_window: usize,
    /// Per-task execution timeout in seconds (default 300).
    pub task_timeout: u64,
}

impl Default for TasksConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 4,
            viewable_window: 16,
            task_timeout: 300,
        }
    }
}

/// Memory subsystem configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryConfig {
    /// Additional entity types beyond the framework defaults.
    pub entities: Vec<String>,
    /// Additional relation types beyond the framework defaults.
    pub relations: Vec<String>,
    /// Default limit for `connections` traversal results (default: 20, max: 100).
    pub connections: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            entities: Vec::new(),
            relations: Vec::new(),
            connections: 20,
        }
    }
}

/// Heartbeat timer configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HeartbeatConfig {
    /// Interval in minutes (default 1, 0 = disabled).
    pub interval: u64,
    /// System prompt for heartbeat-triggered agent runs.
    pub prompt: String,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            interval: 1,
            prompt: String::new(),
        }
    }
}

/// Per-agent override configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentOverride {
    /// Per-agent heartbeat override.
    #[serde(default)]
    pub heartbeat: Option<HeartbeatConfig>,
}

/// Per-agent heartbeat configuration: global defaults + per-agent overrides.
///
/// TOML layout:
/// ```toml
/// [agents.heartbeat]
/// interval = 1
/// prompt = ""
///
/// [agents.researcher.heartbeat]
/// interval = 5
/// prompt = "Check your research queue."
/// ```
///
/// The `heartbeat` key is the global default. Other table keys are per-agent
/// overrides containing a `heartbeat` sub-table.
#[derive(Debug, Clone, Serialize, Default)]
pub struct AgentsConfig {
    /// Global heartbeat defaults.
    pub heartbeat: HeartbeatConfig,
    /// Per-agent overrides (agent_name → config).
    pub overrides: BTreeMap<String, AgentOverride>,
}

impl<'de> Deserialize<'de> for AgentsConfig {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw: BTreeMap<String, toml::Value> = BTreeMap::deserialize(deserializer)?;
        let mut config = AgentsConfig::default();
        for (key, value) in raw {
            match key.as_str() {
                "heartbeat" => {
                    config.heartbeat =
                        serde::Deserialize::deserialize(value).map_err(serde::de::Error::custom)?;
                }
                _ => {
                    if let toml::Value::Table(_) = &value {
                        let agent_override: AgentOverride = serde::Deserialize::deserialize(value)
                            .map_err(serde::de::Error::custom)?;
                        config.overrides.insert(key, agent_override);
                    }
                }
            }
        }
        Ok(config)
    }
}

impl AgentsConfig {
    /// Resolve the effective heartbeat config for a given agent.
    ///
    /// Priority: agent override > global default.
    pub fn resolve_heartbeat(&self, agent: &str) -> (u64, &str) {
        if let Some(over) = self.overrides.get(agent)
            && let Some(hb) = &over.heartbeat
        {
            return (hb.interval, &hb.prompt);
        }
        (self.heartbeat.interval, &self.heartbeat.prompt)
    }
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
        config.mcps.iter_mut().for_each(|(name, server)| {
            if server.name.is_empty() {
                server.name = name.clone().into();
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
