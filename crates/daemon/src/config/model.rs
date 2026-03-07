//! Model configuration.

use compact_str::CompactString;
use model::ProviderConfig;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    /// Default models
    pub default: DefaultModels,
    /// Remote providers (local models come from the built-in registry)
    #[serde(default)]
    pub providers: BTreeMap<CompactString, ProviderConfig>,
}

#[cfg(not(feature = "local"))]
impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            default: DefaultModels::default(),
            providers: [(
                "deepseek-chat".into(),
                ProviderConfig {
                    model: "deepseek-chat".into(),
                    api_key: None,
                    base_url: None,
                },
            )]
            .into(),
        }
    }
}

#[cfg(feature = "local")]
#[allow(clippy::derivable_impls)] // conditional: non-local Default is not derivable
impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            default: DefaultModels::default(),
            providers: BTreeMap::new(),
        }
    }
}

/// Agent configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefaultModels {
    /// Model used for the general text process
    pub text: CompactString,

    /// Model used for vision tasks
    pub vision: Option<CompactString>,

    /// Model used for embedding tasks
    pub embedding: Option<CompactString>,
}

#[cfg(not(feature = "local"))]
impl Default for DefaultModels {
    fn default() -> Self {
        Self {
            text: "deepseek-chat".into(),
            vision: None,
            embedding: None,
        }
    }
}

#[cfg(feature = "local")]
impl Default for DefaultModels {
    fn default() -> Self {
        let text_entry = model::local::registry::default_text();
        let vision =
            model::local::registry::default_vision().map(|e| CompactString::from(e.model_id));
        Self {
            text: CompactString::from(text_entry.model_id),
            vision,
            embedding: None,
        }
    }
}
