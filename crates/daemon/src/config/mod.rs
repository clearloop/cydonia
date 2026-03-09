//! Daemon configuration loaded from TOML.

pub use ::model::{ProviderConfig, ProviderManager};
use anyhow::Result;
pub use default::{scaffold_config_dir, scaffold_work_dir};
pub use perm::{PermissionConfig, ToolPermission};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
pub use wcore::paths::{
    AGENTS_DIR, CONFIG_DIR, DATA_DIR, MEMORY_DB, SKILLS_DIR, SOCKET_PATH, WORK_DIR,
};
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
    pub mcp_servers: BTreeMap<String, mcp::McpServerConfig>,
    /// Memory configuration.
    #[serde(default)]
    pub memory: MemoryConfig,
    /// Task executor pool configuration.
    #[serde(default)]
    pub tasks: TasksConfig,
    /// Permission configuration: global defaults + per-agent overrides.
    #[serde(default)]
    pub permissions: PermissionConfig,
    /// Heartbeat timer configuration.
    #[serde(default)]
    pub heartbeat: HeartbeatConfig,
    /// Optional symlink path for the workspace sandbox root (`~/.openwalrus/work/`).
    ///
    /// When set, a symlink is created at this path pointing to `~/.openwalrus/work/`.
    #[serde(default)]
    pub work_dir: Option<PathBuf>,
}

/// Task executor pool configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TasksConfig {
    /// Maximum number of concurrently InProgress tasks (default 4).
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,
    /// Maximum number of tasks returned by queries (default 16).
    #[serde(default = "default_viewable_window")]
    pub viewable_window: usize,
    /// Per-task execution timeout in seconds (default 300).
    #[serde(default = "default_task_timeout")]
    pub task_timeout: u64,
}

impl Default for TasksConfig {
    fn default() -> Self {
        Self {
            max_concurrent: default_max_concurrent(),
            viewable_window: default_viewable_window(),
            task_timeout: default_task_timeout(),
        }
    }
}

fn default_max_concurrent() -> usize {
    4
}

fn default_viewable_window() -> usize {
    16
}

fn default_task_timeout() -> u64 {
    300
}

/// Memory subsystem configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryConfig {
    /// Additional entity types beyond the framework defaults.
    #[serde(default)]
    pub entities: Vec<String>,
    /// Additional relation types beyond the framework defaults.
    #[serde(default)]
    pub relations: Vec<String>,
    /// Default limit for `connections` traversal results (default: 20, max: 100).
    #[serde(default = "default_connection_limit")]
    pub connection_limit: usize,
}

fn default_connection_limit() -> usize {
    20
}

/// Heartbeat timer configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatConfig {
    /// Interval in seconds (default 60, 0 = disabled).
    #[serde(default = "default_heartbeat_interval")]
    pub interval: u64,
    /// System prompt for heartbeat-triggered agent runs.
    #[serde(default)]
    pub prompt: String,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            interval: default_heartbeat_interval(),
            prompt: String::new(),
        }
    }
}

fn default_heartbeat_interval() -> u64 {
    60
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
        config.mcp_servers.iter_mut().for_each(|(name, server)| {
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
