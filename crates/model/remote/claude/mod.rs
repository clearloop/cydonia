//! Claude (Anthropic) LLM provider.
//!
//! Implements the Anthropic Messages API, which differs from the OpenAI
//! chat completions format in message structure and streaming events.
//! Uses `HttpProvider` for transport, with Anthropic-specific headers and
//! block-buffer SSE parsing.

use crate::remote::HttpProvider;
use compact_str::CompactString;
pub use request::Request;
use reqwest::Client;

mod provider;
mod request;
pub(crate) mod stream;

/// The Anthropic Messages API endpoint.
pub const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";

/// The Claude LLM provider.
#[derive(Clone)]
pub struct Claude {
    /// Shared HTTP transport with Anthropic authentication headers.
    pub(crate) http: HttpProvider,
    /// The configured model name (used by `active_model()`).
    model: CompactString,
}

impl Claude {
    /// Create a provider targeting the Anthropic API.
    pub fn anthropic(client: Client, key: &str, model: &str) -> anyhow::Result<Self> {
        Self::custom(client, key, ENDPOINT, model)
    }

    /// Create a provider targeting a custom Anthropic-compatible endpoint.
    pub fn custom(client: Client, key: &str, endpoint: &str, model: &str) -> anyhow::Result<Self> {
        let http = HttpProvider::anthropic(client, key, endpoint)?;
        Ok(Self {
            http,
            model: CompactString::from(model),
        })
    }
}
