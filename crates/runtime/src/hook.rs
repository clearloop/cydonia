//! Hook trait — stateful runtime backend.
//!
//! Hook is the single abstraction that Runtime delegates all backend concerns
//! to: model access, tool dispatch, prompt enrichment, and event observation.
//! Implementations own the concrete backends (model provider, memory, MCP,
//! tool registry, skill registry).

use anyhow::Result;
use std::future::Future;
use wcore::AgentConfig;
use wcore::AgentEvent;
use wcore::model::{Model, Tool};

/// Stateful runtime backend.
///
/// Owns the model provider, tool registry, skill registry, MCP bridge, and
/// any other backend state. Runtime holds `Arc<H>` and delegates through
/// these methods.
pub trait Hook: Send + Sync {
    /// The model provider for this hook.
    type Model: Model + Send + Sync;

    /// Access the model provider.
    fn model(&self) -> &Self::Model;

    /// Return tool schemas available to the named agent.
    fn tools(&self, agent: &str) -> Vec<Tool>;

    /// Dispatch tool calls for the named agent.
    ///
    /// Each entry in `calls` is `(method_name, params_json)`. Returns one
    /// result per call in the same order.
    fn dispatch(
        &self,
        agent: &str,
        calls: &[(&str, &str)],
    ) -> impl Future<Output = Vec<Result<String>>> + Send;

    /// Build an enriched system prompt for the given agent config.
    ///
    /// Default implementation returns the config's system prompt unchanged.
    /// Override to inject skills, MCP context, or other prompt augmentation.
    fn enrich_prompt(&self, config: &AgentConfig) -> String {
        config.system_prompt.clone()
    }

    /// Called when an agent emits an event during execution.
    ///
    /// Default is a no-op.
    fn on_event(&self, _event: &AgentEvent) {}
}

impl Hook for () {
    type Model = ();

    fn model(&self) -> &() {
        &()
    }

    fn tools(&self, _agent: &str) -> Vec<Tool> {
        vec![]
    }

    fn dispatch(
        &self,
        _agent: &str,
        calls: &[(&str, &str)],
    ) -> impl Future<Output = Vec<Result<String>>> + Send {
        let len = calls.len();
        async move { (0..len).map(|_| Ok(String::new())).collect() }
    }
}
