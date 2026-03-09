//! Agent configuration — shared by `[walrus]` and each `[agents.*]` entry.

use compact_str::CompactString;
use serde::{Deserialize, Serialize};

/// Heartbeat timer configuration. Interval 0 = disabled.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HeartbeatConfig {
    /// Interval in minutes (0 = disabled).
    #[serde(default)]
    pub interval: u64,
    /// System prompt for heartbeat-triggered agent runs.
    #[serde(default)]
    pub prompt: String,
}

/// Agent configuration — shared by `[walrus]` and each `[agents.*]` entry.
///
/// ```toml
/// [walrus]
/// model = "qwen3:4b"
///
/// [agents.researcher]
/// model = "qwen3:4b"
/// heartbeat = { interval = 5, prompt = "Check your research queue." }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Model ID for this agent. For `[walrus]` this is the daemon-wide default.
    #[serde(default)]
    pub model: CompactString,
    /// Heartbeat configuration. Interval 0 (the default) means no heartbeat.
    #[serde(default)]
    pub heartbeat: HeartbeatConfig,
}

#[cfg(not(feature = "local"))]
impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            model: "deepseek-chat".into(),
            heartbeat: HeartbeatConfig::default(),
        }
    }
}

#[cfg(feature = "local")]
impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            model: CompactString::from(::model::local::registry::default_model().model_id),
            heartbeat: HeartbeatConfig::default(),
        }
    }
}
