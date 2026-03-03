//! Protocol impls for the gateway.

use crate::MemoryBackend;
use model::ProviderManager;
use runtime::Hook;
use runtime::Runtime;
use std::sync::Arc;
use wcore::AgentEvent;

pub mod builder;
pub mod serve;
pub mod uds;

/// Shared state available to all request handlers.
pub struct Gateway<H: Hook + 'static> {
    /// The walrus runtime (immutable after init).
    pub runtime: Arc<Runtime<H>>,
    /// HuggingFace endpoint selected at startup (fastest of official/mirror).
    pub hf_endpoint: Arc<str>,
}

impl<H: Hook + 'static> Clone for Gateway<H> {
    fn clone(&self) -> Self {
        Self {
            runtime: Arc::clone(&self.runtime),
            hf_endpoint: Arc::clone(&self.hf_endpoint),
        }
    }
}

/// Type-level hook wiring `MemoryBackend` as the memory implementation.
pub struct GatewayHook;

impl Hook for GatewayHook {
    type Model = ProviderManager;
    type Memory = MemoryBackend;

    fn on_event(event: &AgentEvent) {
        match event {
            AgentEvent::TextDelta(text) => {
                tracing::trace!(text_len = text.len(), "agent text delta");
            }
            AgentEvent::ToolCallsStart(calls) => {
                tracing::debug!(count = calls.len(), "agent tool calls started");
            }
            AgentEvent::ToolResult { call_id, .. } => {
                tracing::debug!(%call_id, "agent tool result");
            }
            AgentEvent::ToolCallsComplete => {
                tracing::debug!("agent tool calls complete");
            }
            AgentEvent::Done(response) => {
                tracing::info!(
                    iterations = response.iterations,
                    stop_reason = ?response.stop_reason,
                    "agent run complete"
                );
            }
        }
    }
}
