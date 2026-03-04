//! Agent configuration.

use compact_str::CompactString;
use serde::{Deserialize, Serialize};

/// Agent configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Model used for the main process
    pub default: CompactString,

    /// Model used for vision tasks
    pub vision: Option<CompactString>,

    /// Model used for embedding tasks
    pub embedding: Option<CompactString>,
}

#[cfg(not(feature = "local"))]
impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            default: "deepseek-chat".into(),
            vision: None,
            embedding: None,
        }
    }
}

#[cfg(feature = "local")]
impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            default: "Qwen/Qwen3-4B".into(),
            vision: None,
            embedding: None,
        }
    }
}
