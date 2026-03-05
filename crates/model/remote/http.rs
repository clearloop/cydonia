//! Shared HTTP transport for OpenAI-compatible and Anthropic LLM providers.
//!
//! `HttpProvider` wraps a `reqwest::Client` with pre-configured headers and
//! endpoint URL. Provides `send()` / `send_raw()` for non-streaming, `stream_sse()`
//! for OpenAI-format SSE, and `stream_anthropic()` for Anthropic block-buffer SSE.

use crate::remote::claude::stream::parse_sse_block;
use anyhow::Result;
use async_stream::try_stream;
use futures_core::Stream;
use futures_util::StreamExt;
use reqwest::{
    Client, Method,
    header::{self, HeaderMap, HeaderName, HeaderValue},
};
use serde::Serialize;
use wcore::model::{Response, StreamChunk};

/// Anthropic API version header value.
const API_VERSION: &str = "2023-06-01";

/// Shared HTTP transport for OpenAI-compatible providers.
///
/// Holds a `reqwest::Client`, pre-built headers (auth + content-type),
/// and the target endpoint URL.
#[derive(Clone)]
pub struct HttpProvider {
    client: Client,
    headers: HeaderMap,
    endpoint: String,
}

impl HttpProvider {
    /// Create a provider with Bearer token authentication.
    pub fn bearer(client: Client, key: &str, endpoint: &str) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(header::AUTHORIZATION, format!("Bearer {key}").parse()?);
        Ok(Self {
            client,
            headers,
            endpoint: endpoint.to_owned(),
        })
    }

    /// Create a provider without authentication (e.g. Ollama).
    pub fn no_auth(client: Client, endpoint: &str) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
        Self {
            client,
            headers,
            endpoint: endpoint.to_owned(),
        }
    }

    /// Create a provider with a custom header for authentication.
    ///
    /// Used by providers that don't use Bearer tokens (e.g. Anthropic
    /// uses `x-api-key`).
    pub fn custom_header(
        client: Client,
        header_name: &str,
        header_value: &str,
        endpoint: &str,
    ) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(
            header_name.parse::<HeaderName>()?,
            header_value.parse::<HeaderValue>()?,
        );
        Ok(Self {
            client,
            headers,
            endpoint: endpoint.to_owned(),
        })
    }

    /// Create a provider with Anthropic authentication headers.
    ///
    /// Inserts `x-api-key` and `anthropic-version` in addition to the
    /// standard content-type and accept headers.
    pub fn anthropic(client: Client, key: &str, endpoint: &str) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(
            "x-api-key".parse::<HeaderName>()?,
            key.parse::<HeaderValue>()?,
        );
        headers.insert(
            "anthropic-version".parse::<HeaderName>()?,
            API_VERSION.parse::<HeaderValue>()?,
        );
        Ok(Self {
            client,
            headers,
            endpoint: endpoint.to_owned(),
        })
    }

    /// Send a non-streaming request and deserialize the response as JSON.
    pub async fn send(&self, body: &impl Serialize) -> Result<Response> {
        tracing::trace!("request: {}", serde_json::to_string(body)?);
        let response = self
            .client
            .request(Method::POST, &self.endpoint)
            .headers(self.headers.clone())
            .json(body)
            .send()
            .await?;

        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() {
            anyhow::bail!("API error ({status}): {text}");
        }

        serde_json::from_str(&text).map_err(Into::into)
    }

    /// Stream an SSE response (OpenAI-compatible format).
    ///
    /// Parses `data: ` prefixed lines, skips `[DONE]` sentinel, and
    /// deserializes each chunk as [`StreamChunk`].
    pub fn stream_sse(
        &self,
        body: &impl Serialize,
    ) -> impl Stream<Item = Result<StreamChunk>> + Send {
        if let Ok(body) = serde_json::to_string(body) {
            tracing::trace!("request: {}", body);
        }
        let request = self
            .client
            .request(Method::POST, &self.endpoint)
            .headers(self.headers.clone())
            .json(body);

        try_stream! {
            let response = request.send().await?;
            let mut stream = response.bytes_stream();
            while let Some(next) = stream.next().await {
                let bytes = next?;
                let text = String::from_utf8_lossy(&bytes);
                tracing::trace!("chunk: {}", text);
                for data in text.split("data: ").skip(1).filter(|s| !s.starts_with("[DONE]")) {
                    let trimmed = data.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<StreamChunk>(trimmed) {
                        Ok(chunk) => yield chunk,
                        Err(e) => tracing::warn!("failed to parse chunk: {e}, data: {trimmed}"),
                    }
                }
            }
        }
    }

    /// Send a non-streaming request and return the raw response body text.
    ///
    /// Unlike `send()`, the caller is responsible for deserialization.
    /// Used by providers whose response schema differs from the OpenAI format (e.g. Anthropic).
    pub async fn send_raw(&self, body: &impl Serialize) -> Result<String> {
        tracing::trace!("request: {}", serde_json::to_string(body)?);
        let response = self
            .client
            .request(Method::POST, &self.endpoint)
            .headers(self.headers.clone())
            .json(body)
            .send()
            .await?;
        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() {
            anyhow::bail!("API error ({status}): {text}");
        }
        Ok(text)
    }

    /// Stream an SSE response in Anthropic block-buffer format.
    ///
    /// Anthropic uses `\n\n`-delimited blocks each containing `event:` and
    /// `data:` lines, unlike OpenAI's line-by-line `data: ` prefix format.
    /// Takes the body as an owned `serde_json::Value` so the stream can be
    /// `'static` without capturing a borrow.
    pub fn stream_anthropic(
        &self,
        body: serde_json::Value,
    ) -> impl Stream<Item = Result<StreamChunk>> + Send {
        tracing::trace!("request: {}", body);
        let request = self
            .client
            .request(Method::POST, &self.endpoint)
            .headers(self.headers.clone())
            .json(&body);

        try_stream! {
            let response = request.send().await?;
            let mut stream = response.bytes_stream();
            let mut buf = String::new();
            while let Some(Ok(bytes)) = stream.next().await {
                buf.push_str(&String::from_utf8_lossy(&bytes));
                while let Some(pos) = buf.find("\n\n") {
                    let block = buf[..pos].to_owned();
                    buf = buf[pos + 2..].to_owned();
                    if let Some(chunk) = parse_sse_block(&block) {
                        yield chunk;
                    }
                }
            }
            if !buf.trim().is_empty()
                && let Some(chunk) = parse_sse_block(&buf)
            {
                yield chunk;
            }
        }
    }

    /// Get the endpoint URL.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Get a reference to the headers.
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }
}
