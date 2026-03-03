//! Gateway — daemon core composing runtime, MCP, skills, cron, and memory.

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
use wcron::CronHandler;

pub mod builder;
pub mod serve;
pub mod server;

/// Shared state available to all request handlers.
pub struct Gateway {
    /// The walrus runtime.
    pub runtime: Arc<Runtime<ProviderManager, GatewayHook>>,
    /// HuggingFace endpoint selected at startup (fastest of official/mirror).
    pub hf_endpoint: Arc<str>,
}

impl Clone for Gateway {
    fn clone(&self) -> Self {
        Self {
            runtime: Arc::clone(&self.runtime),
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
    cron: CronHandler,
    tools: BTreeMap<CompactString, (Tool, Handler)>,
}

impl GatewayHook {
    /// Create a new GatewayHook with the given backends.
    pub fn new(memory: InMemory, skills: SkillHandler, mcp: McpHandler, cron: CronHandler) -> Self {
        Self {
            memory: Arc::new(memory),
            skills,
            mcp,
            cron,
            tools: BTreeMap::new(),
        }
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

    /// Access the cron handler.
    pub fn cron(&self) -> &CronHandler {
        &self.cron
    }
}

impl Hook for GatewayHook {
    fn register(&mut self, tool: Tool, handler: Handler) {
        let name = tool.name.clone();
        self.tools.insert(name, (tool, handler));
    }

    fn on_build_agent(&self, config: AgentConfig) -> AgentConfig {
        // Skills enrich the system prompt based on agent tags.
        let config = self.skills.on_build_agent(config);
        // MCP could enrich in the future (currently a no-op).
        self.mcp.on_build_agent(config)
    }

    fn tools(&self, agent: &str) -> Vec<Tool> {
        // Daemon-registered tools (memory, etc).
        let mut tools: Vec<Tool> = self.tools.values().map(|(t, _)| t.clone()).collect();
        // Cron tools.
        tools.extend(Hook::tools(&self.cron, agent));
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
        let cron_tool_names: Vec<CompactString> = Hook::tools(&self.cron, _agent)
            .into_iter()
            .map(|t| t.name)
            .collect();
        let calls: Vec<(String, String)> = calls
            .iter()
            .map(|(m, p)| (m.to_string(), p.to_string()))
            .collect();
        let handlers: Vec<_> = calls
            .iter()
            .map(|(method, _)| self.tools.get(method.as_str()).map(|(_, h)| Arc::clone(h)))
            .collect();
        let mcp = self.mcp.try_bridge();
        let cron_jobs = self.cron.jobs_arc();

        async move {
            let mut results = Vec::with_capacity(calls.len());
            for (i, (method, params)) in calls.iter().enumerate() {
                let output = if cron_tool_names.iter().any(|n| n == method) {
                    wcron::hook::dispatch_call(&cron_jobs, method, params).await
                } else if let Some(ref handler) = handlers[i] {
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
