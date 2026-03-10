//! Stateful Hook implementation for the daemon.
//!
//! [`DaemonHook`] composes memory, skill, MCP, and OS sub-hooks.
//! `on_build_agent` delegates to skills and memory; `on_register_tools`
//! delegates to all sub-hooks in sequence. `dispatch_tool` routes every
//! agent tool call by name — the single entry point from `event.rs`.

use crate::hook::{
    mcp::McpHandler, memory::MemoryHook, os::PermissionConfig, skill::SkillHandler,
    task::TaskRegistry,
};
use std::sync::Arc;
use tokio::sync::Mutex;
use wcore::{AgentConfig, AgentEvent, Hook, ToolRegistry};

pub mod mcp;
pub mod memory;
pub mod os;
pub mod skill;
pub mod task;

/// Stateful Hook implementation for the daemon.
///
/// Composes memory, skill, MCP, and OS sub-hooks. Each sub-hook
/// self-registers its tools via `on_register_tools`. All tool dispatch
/// is routed through `dispatch_tool`.
pub struct DaemonHook {
    pub memory: MemoryHook,
    pub skills: SkillHandler,
    pub mcp: McpHandler,
    pub tasks: Arc<Mutex<TaskRegistry>>,
    pub permissions: PermissionConfig,
    /// Whether the daemon is running as the `walrus` OS user (sandbox active).
    pub sandboxed: bool,
}

/// OS tool names — bypass permission check when running in sandbox mode.
const OS_TOOLS: &[&str] = &["read", "write", "bash"];

impl DaemonHook {
    /// Create a new DaemonHook with the given backends.
    pub fn new(
        memory: MemoryHook,
        skills: SkillHandler,
        mcp: McpHandler,
        tasks: Arc<Mutex<TaskRegistry>>,
        permissions: PermissionConfig,
        sandboxed: bool,
    ) -> Self {
        Self {
            memory,
            skills,
            mcp,
            tasks,
            permissions,
            sandboxed,
        }
    }

    /// Check tool permission. Returns `Some(denied_message)` if denied,
    /// `None` if allowed.
    async fn check_perm(
        &self,
        name: &str,
        args: &str,
        agent: &str,
        task_id: Option<u64>,
    ) -> Option<String> {
        // OS tools bypass permission when running in sandbox mode.
        if self.sandboxed && OS_TOOLS.contains(&name) {
            return None;
        }
        use crate::hook::os::ToolPermission;
        match self.permissions.resolve(agent, name) {
            ToolPermission::Deny => Some(format!("permission denied: {name}")),
            ToolPermission::Ask => {
                if let Some(tid) = task_id {
                    let summary = if args.len() > 200 {
                        format!("{}…", &args[..200])
                    } else {
                        args.to_string()
                    };
                    let question = format!("{name}: {summary}");
                    let rx = self.tasks.lock().await.block(tid, question);
                    if let Some(rx) = rx {
                        match rx.await {
                            Ok(resp) if resp == "denied" => {
                                return Some(format!("permission denied: {name}"));
                            }
                            Err(_) => {
                                return Some(format!("permission denied: {name} (inbox dropped)"));
                            }
                            _ => {} // approved → proceed
                        }
                    }
                }
                // No task_id → can't block, treat as Allow.
                None
            }
            ToolPermission::Allow => None,
        }
    }

    /// Route a tool call by name to the appropriate handler.
    ///
    /// This is the single dispatch entry point — `event.rs` calls this
    /// and never matches on tool names itself. Unrecognised names are
    /// forwarded to the MCP bridge after a warn-level log.
    pub async fn dispatch_tool(
        &self,
        name: &str,
        args: &str,
        agent: &str,
        task_id: Option<u64>,
    ) -> String {
        if let Some(denied) = self.check_perm(name, args, agent, task_id).await {
            return denied;
        }
        match name {
            "remember" => self.memory.dispatch_remember(args, agent).await,
            "recall" => self.memory.dispatch_recall(args, agent).await,
            "relate" => self.memory.dispatch_relate(args, agent).await,
            "connections" => self.memory.dispatch_connections(args, agent).await,
            "compact" => self.memory.dispatch_compact(agent).await,
            "__journal__" => self.memory.dispatch_journal(args, agent).await,
            "distill" => self.memory.dispatch_distill(args, agent).await,
            "search_mcp" => self.dispatch_search_mcp(args).await,
            "call_mcp_tool" => self.dispatch_call_mcp_tool(args).await,
            "search_skill" => self.dispatch_search_skill(args).await,
            "load_skill" => self.dispatch_load_skill(args).await,
            "read" => self.dispatch_read(args).await,
            "write" => self.dispatch_write(args).await,
            "bash" => self.dispatch_bash(args).await,
            "spawn_task" => self.dispatch_spawn_task(args, agent, task_id).await,
            "check_tasks" => self.dispatch_check_tasks(args).await,
            "create_task" => self.dispatch_create_task(args, agent).await,
            "ask_user" => self.dispatch_ask_user(args, task_id).await,
            "await_tasks" => self.dispatch_await_tasks(args, task_id).await,
            name => {
                tracing::debug!(tool = name, "forwarding tool to MCP bridge");
                let bridge = self.mcp.bridge().await;
                bridge.call(name, args).await
            }
        }
    }
}

impl Hook for DaemonHook {
    fn on_build_agent(&self, config: AgentConfig) -> AgentConfig {
        self.memory.on_build_agent(config)
    }

    fn on_compact(&self, prompt: &mut String) {
        self.memory.on_compact(prompt);
    }

    async fn on_register_tools(&self, tools: &mut ToolRegistry) {
        self.memory.on_register_tools(tools).await;
        self.mcp.on_register_tools(tools).await;
        tools.insert_all(os::tool::tools());
        tools.insert_all(skill::tool::tools());
        tools.insert_all(task::tool::tools());
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
                // Track token usage on the active task for this agent.
                let (prompt, completion) = response.steps.iter().fold((0u64, 0u64), |(p, c), s| {
                    (
                        p + u64::from(s.response.usage.prompt_tokens),
                        c + u64::from(s.response.usage.completion_tokens),
                    )
                });
                if (prompt > 0 || completion > 0)
                    && let Ok(mut registry) = self.tasks.try_lock()
                {
                    let tid = registry
                        .list(Some(agent), Some(task::TaskStatus::InProgress), None)
                        .first()
                        .map(|t| t.id);
                    if let Some(tid) = tid {
                        registry.add_tokens(tid, prompt, completion);
                    }
                }
            }
        }
    }
}
