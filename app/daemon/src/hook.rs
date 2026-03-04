//! Stateful Hook implementation for the daemon.
//!
//! [`DaemonHook`] handles prompt enrichment (via skills) and event
//! observation (logging). Tool registration and dispatch are handled
//! by Runtime's tool registry.

use mcp::McpHandler;
use memory::InMemory;
use runtime::Hook;
use skill::SkillHandler;
use std::sync::Arc;
use wcore::{AgentConfig, AgentEvent};
use wcron::CronHandler;

/// Stateful Hook implementation for the daemon.
///
/// Handles prompt enrichment (via skills) and event observation (logging).
/// Tool registration and dispatch are handled by Runtime's tool registry.
pub struct DaemonHook {
    memory: Arc<InMemory>,
    skills: SkillHandler,
    mcp: McpHandler,
    cron: CronHandler,
}

impl DaemonHook {
    /// Create a new DaemonHook with the given backends.
    pub fn new(memory: InMemory, skills: SkillHandler, mcp: McpHandler, cron: CronHandler) -> Self {
        Self {
            memory: Arc::new(memory),
            skills,
            mcp,
            cron,
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

impl Hook for DaemonHook {
    fn on_build_agent(&self, config: AgentConfig) -> AgentConfig {
        // Skills enrich the system prompt based on agent tags.
        self.skills.on_build_agent(config)
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
