//! Gateway — daemon core composing runtime, MCP, skills, and memory.

use crate::MemoryBackend;
use crate::feature::mcp::McpHandler;
use crate::feature::skill::SkillHandler;
use anyhow::Result;
use compact_str::CompactString;
use model::ProviderManager;
use runtime::{Handler, Hook, Runtime, Tool};
use std::collections::BTreeMap;
use std::sync::Arc;
use wcore::{AgentConfig, AgentEvent};

pub mod builder;
pub mod serve;
pub mod uds;

/// Shared state available to all request handlers.
pub struct Gateway {
    /// The walrus runtime.
    pub runtime: Arc<Runtime<GatewayHook>>,
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
/// Owns the model provider, memory backend, skill registry, MCP bridge,
/// and tool registry. Provides all backend services to Runtime.
pub struct GatewayHook {
    provider: ProviderManager,
    memory: Arc<MemoryBackend>,
    skills: SkillHandler,
    mcp: McpHandler,
    tools: BTreeMap<CompactString, (Tool, Handler)>,
}

impl GatewayHook {
    /// Create a new GatewayHook with the given backends.
    pub fn new(
        provider: ProviderManager,
        memory: MemoryBackend,
        skills: SkillHandler,
        mcp: McpHandler,
    ) -> Self {
        Self {
            provider,
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
    pub fn memory(&self) -> &MemoryBackend {
        &self.memory
    }

    /// Get a clone of the memory Arc.
    pub fn memory_arc(&self) -> Arc<MemoryBackend> {
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
    type Model = ProviderManager;

    fn model(&self) -> &ProviderManager {
        &self.provider
    }

    fn tools(&self, _agent: &str) -> Vec<Tool> {
        let mut tools: Vec<Tool> = self.tools.values().map(|(t, _)| t.clone()).collect();
        // Merge MCP tools, skipping duplicates.
        if let Some(bridge) = self.mcp.try_bridge() {
            for tool in bridge.try_tools() {
                if !self.tools.contains_key(&tool.name) {
                    tools.push(tool);
                }
            }
        }
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

    fn enrich_prompt(&self, config: &AgentConfig) -> String {
        if let Ok(skills) = self.skills.registry().try_read() {
            build_system_prompt(config, &skills)
        } else {
            config.system_prompt.clone()
        }
    }

    fn on_event(&self, event: &AgentEvent) {
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

/// Build a system prompt enriched with skills for the given agent config.
fn build_system_prompt(
    agent_config: &AgentConfig,
    skills: &crate::feature::skill::SkillRegistry,
) -> String {
    let mut prompt = agent_config.system_prompt.clone();
    for skill in skills.find_by_tags(&agent_config.skill_tags) {
        if !skill.body.is_empty() {
            prompt.push_str("\n\n");
            prompt.push_str(&skill.body);
        }
    }
    prompt
}
