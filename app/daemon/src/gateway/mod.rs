//! Gateway — daemon core composing runtime, MCP, skills, and memory.

use anyhow::Result;
use compact_str::CompactString;
use mcp::McpHandler;
use memory::InMemory;
use model::ProviderManager;
use runtime::{Handler, Hook, Runtime, Tool};
use skill::SkillHandler;
use std::collections::BTreeMap;
use std::sync::Arc;
use wcore::{AgentConfig, AgentEvent};

pub mod builder;
pub mod channel;
pub mod dispatch;
pub mod serve;
pub mod server;

/// Shared state available to all request handlers.
pub struct Gateway {
    /// The walrus runtime.
    pub runtime: Arc<Runtime<ProviderManager, GatewayHook>>,
    /// Per-agent execution locks shared across all message sources.
    pub locks: Arc<dispatch::AgentLock>,
    /// HuggingFace endpoint selected at startup (fastest of official/mirror).
    pub hf_endpoint: Arc<str>,
}

impl Clone for Gateway {
    fn clone(&self) -> Self {
        Self {
            runtime: Arc::clone(&self.runtime),
            locks: Arc::clone(&self.locks),
            hf_endpoint: Arc::clone(&self.hf_endpoint),
        }
    }
}

/// Stateful Hook implementation for the daemon.
///
/// Composes MCP and Skills as sub-hooks, plus daemon-registered tools
/// (memory, etc). Delegates lifecycle methods to each sub-hook.
pub struct GatewayHook {
    memory: Arc<InMemory>,
    skills: SkillHandler,
    mcp: McpHandler,
    tools: BTreeMap<CompactString, (Tool, Handler)>,
}

impl GatewayHook {
    /// Create a new GatewayHook with the given backends.
    pub fn new(memory: InMemory, skills: SkillHandler, mcp: McpHandler) -> Self {
        Self {
            memory: Arc::new(memory),
            skills,
            mcp,
            tools: BTreeMap::new(),
        }
    }

    /// Register a tool with its handler.
    pub fn register<F, Fut>(&mut self, tool: Tool, handler: F)
    where
        F: Fn(String) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = String> + Send + 'static,
    {
        let name = tool.name.clone();
        let handler: Handler = Arc::new(move |args| Box::pin(handler(args)));
        self.tools.insert(name, (tool, handler));
    }

    /// Access the memory backend.
    pub fn memory(&self) -> &InMemory {
        &self.memory
    }

    /// Get a clone of the memory Arc.
    pub fn memory_arc(&self) -> Arc<InMemory> {
        Arc::clone(&self.memory)
    }

    /// Access the skill handler (for hot-reload operations).
    pub fn skills(&self) -> &SkillHandler {
        &self.skills
    }

    /// Access the MCP handler (for hot-reload operations).
    pub fn mcp(&self) -> &McpHandler {
        &self.mcp
    }
}

impl Hook for GatewayHook {
    fn on_build_agent(&self, config: AgentConfig) -> AgentConfig {
        // Skills enrich the system prompt based on agent tags.
        let config = self.skills.on_build_agent(config);
        // MCP could enrich in the future (currently a no-op).
        self.mcp.on_build_agent(config)
    }

    fn tools(&self, agent: &str) -> Vec<Tool> {
        // Daemon-registered tools (memory, etc).
        let mut tools: Vec<Tool> = self.tools.values().map(|(t, _)| t.clone()).collect();
        // MCP tools.
        tools.extend(self.mcp.tools(agent));
        // Skill tools (currently empty).
        tools.extend(self.skills.tools(agent));
        tools
    }

    fn dispatch(
        &self,
        _agent: &str,
        calls: &[(&str, &str)],
    ) -> impl std::future::Future<Output = Vec<Result<String>>> + Send {
        let calls: Vec<(String, String)> = calls
            .iter()
            .map(|(m, p)| (m.to_string(), p.to_string()))
            .collect();
        let handlers: Vec<_> = calls
            .iter()
            .map(|(method, _)| self.tools.get(method.as_str()).map(|(_, h)| Arc::clone(h)))
            .collect();
        let mcp = self.mcp.try_bridge();

        async move {
            let mut results = Vec::with_capacity(calls.len());
            for (i, (method, params)) in calls.iter().enumerate() {
                let output = if let Some(ref handler) = handlers[i] {
                    Ok(handler(params.clone()).await)
                } else if let Some(ref bridge) = mcp {
                    Ok(bridge.call(method, params).await)
                } else {
                    Ok(format!("function {method} not available"))
                };
                results.push(output);
            }
            results
        }
    }

    fn on_event(&self, agent: &str, event: &AgentEvent) {
        match event {
            AgentEvent::TextDelta(text) => {
                tracing::trace!(%agent, text_len = text.len(), "agent text delta");
            }
            AgentEvent::ToolCallsStart(calls) => {
                tracing::debug!(%agent, count = calls.len(), "agent tool calls started");
            }
            AgentEvent::ToolResult { call_id, .. } => {
                tracing::debug!(%agent, %call_id, "agent tool result");
            }
            AgentEvent::ToolCallsComplete => {
                tracing::debug!(%agent, "agent tool calls complete");
            }
            AgentEvent::Done(response) => {
                tracing::info!(
                    %agent,
                    iterations = response.iterations,
                    stop_reason = ?response.stop_reason,
                    "agent run complete"
                );
            }
        }
    }
}
