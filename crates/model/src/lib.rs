//! Model crate — LLM provider implementations, enum dispatch, configuration,
//! construction, and runtime management.
//!
//! Merges all provider backends (OpenAI, Claude, Local) with the `Provider`
//! enum, `ProviderManager`, and `ProviderConfig` into a single crate. Config
//! uses flat `ProviderConfig` with model-prefix kind detection. DeepSeek and
//! other OpenAI-compatible providers route through the OpenAI backend.

pub mod config;
pub mod manager;
mod provider;

#[path = "../remote/mod.rs"]
pub mod remote;

#[cfg(feature = "local")]
#[path = "../local/mod.rs"]
pub mod local;

pub use config::{ProviderConfig, ProviderKind};
pub use manager::ProviderManager;
pub use provider::{Provider, build_provider};
pub use reqwest::Client;
