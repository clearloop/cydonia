//! Stateful Hook implementation for the daemon.
//!
//! [`DaemonHook`] composes memory, skill, MCP, and cron sub-hooks.
//! `on_build_agent` delegates to skills and memory; `on_register_tools`
//! delegates to memory, cron, and MCP sub-hooks in sequence.

use mcp::McpHandler;
use memory::InMemory;
use skill::SkillHandler;
use std::future::Future;
use wcore::{AgentConfig, AgentEvent, Hook, ToolRegistry};
use wcron::CronHandler;

/// Stateful Hook implementation for the daemon.
///
/// Composes memory, skill, MCP, and cron sub-hooks. Each sub-hook
/// self-registers its tools via `on_register_tools`.
pub struct DaemonHook {
    pub memory: InMemory,
    pub skills: SkillHandler,
    pub mcp: McpHandler,
    pub cron: CronHandler,
}

impl DaemonHook {
    /// Create a new DaemonHook with the given backends.
    pub fn new(memory: InMemory, skills: SkillHandler, mcp: McpHandler, cron: CronHandler) -> Self {
        Self {
            memory,
            skills,
            mcp,
            cron,
        }
    }
}

impl Hook for DaemonHook {
    fn on_build_agent(&self, config: AgentConfig) -> AgentConfig {
        let config = self.skills.on_build_agent(config);
        self.memory.on_build_agent(config)
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

    fn on_register_tools(&self, tools: &mut ToolRegistry) -> impl Future<Output = ()> + Send {
        // Memory and cron: inserts happen synchronously inside on_register_tools;
        // the returned trivial async{} futures are intentionally dropped.
        drop(self.memory.on_register_tools(tools));
        drop(self.cron.on_register_tools(tools));
        // MCP: captures bridge Arc synchronously, registers tools async.
        self.mcp.on_register_tools(tools)
    }
}
