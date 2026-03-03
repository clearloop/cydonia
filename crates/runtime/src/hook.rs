//! Hook trait — lifecycle backend for agent building and execution.
//!
//! Hook abstracts the lifecycle of building and calling agents. Runtime
//! delegates to Hook at specific points: `on_build_agent` before registering
//! an agent, `tools`/`dispatch` during each step, and `on_event` after each
//! step completes.

use crate::Handler;
use anyhow::Result;
use std::future::Future;
use wcore::model::Tool;
use wcore::{AgentConfig, AgentEvent};

/// Lifecycle backend for agent building and execution.
///
/// Implementations provide tool schemas, tool dispatch, prompt enrichment
/// (via `on_build_agent`), and event observation. Runtime holds `Arc<H>`
/// and calls these methods at the appropriate lifecycle points.
///
/// Model ownership is separate — Runtime owns the model directly.
pub trait Hook: Send + Sync {
    /// Called by `Runtime::add_agent()` before building the `Agent`.
    ///
    /// Enriches the agent config: appends skill instructions to the system
    /// prompt, validates tool names, adjusts settings, etc. The returned
    /// config is passed to `AgentBuilder`.
    ///
    /// Default: returns config unchanged.
    fn on_build_agent(&self, config: AgentConfig) -> AgentConfig {
        config
    }

    /// Return tool schemas available to the named agent.
    ///
    /// Called once per step to populate the LLM request.
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

    /// Register a tool with its handler.
    ///
    /// Called during the builder phase (before the hook is wrapped in `Arc`).
    /// Hooks that support dynamic tool registration override this method.
    ///
    /// Default: no-op — the tool is silently dropped.
    fn register(&mut self, _tool: Tool, _handler: Handler) {}

    /// Called by Runtime after each agent step during execution.
    ///
    /// Receives every `AgentEvent` produced during `send_to` and
    /// `stream_to`. Use for logging, metrics, persistence, or forwarding.
    ///
    /// Default: no-op.
    fn on_event(&self, _agent: &str, _event: &AgentEvent) {}
}

impl Hook for () {
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
