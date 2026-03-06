//! Hook trait — lifecycle backend for agent building, event observation,
//! and tool registration.
//!
//! All hook crates implement this trait. [`Runtime`](crate) calls these
//! methods at the appropriate lifecycle points. `DaemonHook` composes
//! multiple Hook implementations by delegating to each.

use crate::{AgentConfig, AgentEvent, Memory, agent::tool::ToolRegistry, memory::tools};
use std::{future::Future, sync::Arc};

/// Lifecycle backend for agent building, event observation, and tool registration.
///
/// Default implementations are no-ops so implementors only override what they need.
pub trait Hook: Send + Sync {
    /// Called by `Runtime::add_agent()` before building the `Agent`.
    ///
    /// Enriches the agent config: appends skill instructions, injects memory
    /// into the system prompt, etc. The returned config is passed to `AgentBuilder`.
    ///
    /// Default: returns config unchanged.
    fn on_build_agent(&self, config: AgentConfig) -> AgentConfig {
        config
    }

    /// Called by Runtime after each agent step during execution.
    ///
    /// Receives every `AgentEvent` produced during `send_to` and `stream_to`.
    /// Use for logging, metrics, persistence, or forwarding.
    ///
    /// Default: no-op.
    fn on_event(&self, _agent: &str, _event: &AgentEvent) {}

    /// Called by `Runtime::new()` to register tools into the shared registry.
    ///
    /// Implementations insert `(Tool, Handler)` pairs via `tools.insert()`.
    /// `DaemonHook` delegates to each sub-hook's `on_register_tools`.
    ///
    /// Default: no-op async.
    fn on_register_tools(&self, _tools: &mut ToolRegistry) -> impl Future<Output = ()> + Send {
        async {}
    }
}

impl Hook for () {}

/// Blanket Hook impl for all Memory types that are Clone + 'static.
///
/// Injects compiled memory into the system prompt via `on_build_agent`
/// and registers `remember`/`recall` tools via `on_register_tools`.
impl<M: Memory + Clone + 'static> Hook for M {
    fn on_build_agent(&self, mut config: AgentConfig) -> AgentConfig {
        let has_memory_tool = config
            .tools
            .iter()
            .any(|t| t == "recall" || t == "remember");
        if has_memory_tool {
            let compiled = self.compile();
            config.system_prompt = format!("{}\n\n{compiled}", config.system_prompt);
        }
        config
    }

    fn on_register_tools(&self, registry: &mut ToolRegistry) -> impl Future<Output = ()> + Send {
        let mem = Arc::new(self.clone());
        let (tool, handler) = tools::remember(Arc::clone(&mem));
        registry.insert(tool, handler);
        let (tool, handler) = tools::recall(mem);
        registry.insert(tool, handler);
        async {}
    }
}
