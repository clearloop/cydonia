//! Model configuration.

use compact_str::CompactString;
use model::ProviderConfig;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Model configuration for the daemon.
///
/// `default` is the single active model (local or remote). Remote providers
/// are configured in `providers`. Local models come from the built-in registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    /// Default model ID (e.g. "Qwen/Qwen3-4B-Instruct-2507" or "deepseek-chat")
    pub default: CompactString,
    /// Optional embedding model
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding: Option<CompactString>,
    /// Remote providers (local models come from the built-in registry)
    #[serde(default)]
    pub providers: BTreeMap<CompactString, ProviderConfig>,
}

#[cfg(not(feature = "local"))]
impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            default: "deepseek-chat".into(),
            embedding: None,
            providers: [(
                "deepseek-chat".into(),
                ProviderConfig {
                    model: "deepseek-chat".into(),
                    api_key: None,
                    base_url: Some("https://api.deepseek.com/chat/completions".into()),
                    standard: Default::default(),
                },
            )]
            .into(),
        }
    }
}

#[cfg(feature = "local")]
impl Default for ModelConfig {
    fn default() -> Self {
        let entry = model::local::registry::default_model();
        Self {
            default: CompactString::from(entry.model_id),
            embedding: None,
            providers: BTreeMap::new(),
        }
    }
}
