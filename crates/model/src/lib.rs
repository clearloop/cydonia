//! Model crate — LLM provider implementations, enum dispatch, configuration,
//! construction, and runtime management.
//!
//! Merges all provider backends (OpenAI, Claude, Local) with the `Provider`
//! enum, `ProviderManager`, and `ProviderConfig` into a single crate. Config
//! uses `ApiStandard` (OpenAI or Anthropic) to select the API protocol.
//! All OpenAI-compatible providers route through the OpenAI backend.

pub mod config;
pub mod manager;
mod provider;

#[path = "../remote/mod.rs"]
pub mod remote;

#[cfg(feature = "local")]
#[path = "../local/mod.rs"]
pub mod local;

/// Default model name when none is configured.
///
/// When the `local` feature is enabled, uses the platform-optimal model
/// from the built-in registry. Otherwise falls back to `"deepseek-chat"`.
pub fn default_model() -> &'static str {
    #[cfg(feature = "local")]
    {
        local::registry::default_model().model_id
    }
    #[cfg(not(feature = "local"))]
    {
        "deepseek-chat"
    }
}

pub use config::{ApiStandard, HfModelConfig, ModelConfig, ProviderConfig};
pub use manager::ProviderManager;
pub use provider::{Provider, build_provider};
pub use reqwest::Client;
