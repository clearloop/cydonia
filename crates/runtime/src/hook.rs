//! Hook trait — lifecycle backend for agent building and event observation.
//!
//! Hook abstracts the lifecycle of building agents. Runtime delegates to Hook
//! at specific points: `on_build_agent` before registering an agent and
//! `on_event` after each step completes. Tool registration and dispatch are
//! handled by Runtime's own tool registry.

use wcore::{AgentConfig, AgentEvent};

/// Lifecycle backend for agent building and event observation.
///
/// Implementations provide prompt enrichment (via `on_build_agent`) and event
/// observation. Runtime holds `H` directly and calls these methods at the
/// appropriate lifecycle points.
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

    /// Called by Runtime after each agent step during execution.
    ///
    /// Receives every `AgentEvent` produced during `send_to` and
    /// `stream_to`. Use for logging, metrics, persistence, or forwarding.
    ///
    /// Default: no-op.
    fn on_event(&self, _agent: &str, _event: &AgentEvent) {}
}

impl Hook for () {}
