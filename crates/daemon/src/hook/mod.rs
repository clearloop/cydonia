//! Stateful Hook implementation for the daemon.
//!
//! [`DaemonHook`] composes memory, skill, and MCP sub-hooks.
//! `on_build_agent` delegates to skills and memory; `on_register_tools`
//! delegates to memory and MCP sub-hooks in sequence.

use crate::hook::skill::SkillHandler;
use mcp::McpHandler;
use memory::InMemory;
use wcore::{AgentConfig, AgentEvent, Hook, ToolRegistry};

pub mod mcp;
pub mod skill;

/// Stateful Hook implementation for the daemon.
///
/// Composes memory, skill, and MCP sub-hooks. Each sub-hook
/// self-registers its tools via `on_register_tools`.
pub struct DaemonHook {
    pub memory: InMemory,
    pub skills: SkillHandler,
    pub mcp: McpHandler,
}

impl DaemonHook {
    /// Create a new DaemonHook with the given backends.
    pub fn new(memory: InMemory, skills: SkillHandler, mcp: McpHandler) -> Self {
        Self {
            memory,
            skills,
            mcp,
        }
    }
}

impl Hook for DaemonHook {
    fn on_build_agent(&self, config: AgentConfig) -> AgentConfig {
        let config = self.skills.on_build_agent(config);
        self.memory.on_build_agent(config)
    }

    async fn on_register_tools(&self, tools: &mut ToolRegistry) {
        self.memory.on_register_tools(tools).await;
        self.mcp.on_register_tools(tools).await
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
