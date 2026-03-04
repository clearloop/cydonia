//! Walrus runtime: agent registry, tool registry, and hook orchestration.
//!
//! The [`Runtime`] holds agents in a plain `BTreeMap` with per-agent
//! `Mutex` for concurrent execution. Tools are stored in a shared
//! [`ToolRegistry`] behind `Arc<RwLock>` supporting post-startup
//! registration (e.g. MCP hot-reload).

pub use memory::{InMemory, Memory, NoEmbedder};
pub use wcore::{
    AgentConfig, Handler, Hook, ToolRegistry,
    model::{Message, Request, Response, Role, StreamChunk, Tool},
};

use anyhow::Result;
use async_stream::stream;
use compact_str::CompactString;
use futures_core::Stream;
use futures_util::StreamExt;
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::{Mutex, RwLock, mpsc};
use wcore::AgentEvent;

/// Re-exports of the most commonly used types.
pub mod prelude {
    pub use crate::{
        AgentConfig, Handler, Hook, InMemory, Message, Request, Response, Role, Runtime,
        StreamChunk, Tool, ToolRegistry,
    };
}

/// The walrus runtime — agent registry, tool registry, and hook orchestration.
///
/// Each agent is wrapped in a per-agent `Mutex` for concurrent execution.
/// Tools are stored in a shared `ToolRegistry` behind `Arc<RwLock>`.
/// `Runtime::new()` is async — it calls `hook.on_register_tools()` during
/// construction so hooks self-register their tools.
pub struct Runtime<M: wcore::model::Model, H: Hook> {
    pub model: M,
    pub hook: H,
    agents: BTreeMap<CompactString, Arc<Mutex<wcore::Agent<M>>>>,
    tools: Arc<RwLock<ToolRegistry>>,
}

impl<M: wcore::model::Model + Send + Sync + Clone + 'static, H: Hook + 'static> Runtime<M, H> {
    /// Create a new runtime with the given model and hook backend.
    ///
    /// Calls `hook.on_register_tools()` to populate the tool registry before
    /// returning. All hook crates self-register their tools here.
    pub async fn new(model: M, hook: H) -> Self {
        let mut registry = ToolRegistry::new();
        hook.on_register_tools(&mut registry).await;
        Self {
            model,
            hook,
            agents: BTreeMap::new(),
            tools: Arc::new(RwLock::new(registry)),
        }
    }

    // --- Tool registry ---

    /// Register a tool with its handler.
    ///
    /// Works both before and after wrapping in `Arc` (the registry is
    /// behind `RwLock`). Used for hot-reload (MCP add/remove/reload).
    pub async fn register_tool(&self, tool: Tool, handler: Handler) {
        self.tools.write().await.insert(tool, handler);
    }

    /// Remove a tool by name. Returns `true` if it existed.
    pub async fn unregister_tool(&self, name: &str) -> bool {
        self.tools.write().await.remove(name)
    }

    /// Atomically replace a set of tools.
    ///
    /// Removes `old_names` and inserts `new_tools` under a single write lock
    /// — no window where agents see partial state.
    pub async fn replace_tools(
        &self,
        old_names: &[CompactString],
        new_tools: Vec<(Tool, Handler)>,
    ) {
        let mut registry = self.tools.write().await;
        for name in old_names {
            registry.remove(name);
        }
        for (tool, handler) in new_tools {
            registry.insert(tool, handler);
        }
    }

    /// Build a filtered [`ToolRegistry`] snapshot for the named agent.
    ///
    /// Reads the agent's `config.tools` list and filters the shared registry.
    /// If the list is empty, all registered tools are included.
    async fn dispatcher_for(&self, agent: &str) -> ToolRegistry {
        let registry = self.tools.read().await;

        let filter: Vec<CompactString> = self
            .agents
            .get(agent)
            .and_then(|m| m.try_lock().ok())
            .map(|g| g.config.tools.to_vec())
            .unwrap_or_default();

        registry.filtered_snapshot(&filter)
    }

    // --- Agent registry ---

    /// Register an agent from its configuration.
    ///
    /// Must be called before wrapping the runtime in `Arc`. Calls
    /// `hook.on_build_agent(config)` to enrich the config before building.
    pub fn add_agent(&mut self, config: AgentConfig) {
        let config = self.hook.on_build_agent(config);
        let name = config.name.clone();
        let agent = wcore::AgentBuilder::new(self.model.clone())
            .config(config)
            .build();
        self.agents.insert(name, Arc::new(Mutex::new(agent)));
    }

    /// Get a registered agent's config by name (cloned).
    pub async fn agent(&self, name: &str) -> Option<AgentConfig> {
        let mutex = self.agents.get(name)?;
        Some(mutex.lock().await.config.clone())
    }

    /// Get all registered agent configs (cloned, alphabetical order).
    pub async fn agents(&self) -> Vec<AgentConfig> {
        let mut configs = Vec::with_capacity(self.agents.len());
        for mutex in self.agents.values() {
            configs.push(mutex.lock().await.config.clone());
        }
        configs
    }

    /// Get the per-agent mutex by name.
    pub fn agent_mutex(&self, name: &str) -> Option<Arc<Mutex<wcore::Agent<M>>>> {
        self.agents.get(name).cloned()
    }

    /// Clear the conversation history for a named agent.
    pub async fn clear_session(&self, agent: &str) {
        if let Some(mutex) = self.agents.get(agent) {
            mutex.lock().await.clear_history();
        }
    }

    // --- Execution ---

    /// Send a message to an agent and run to completion.
    ///
    /// Builds a dispatcher snapshot from the tool registry, locks the per-agent
    /// mutex, pushes the user message, delegates to `agent.run()`, and forwards
    /// all events to `hook.on_event()`.
    pub async fn send_to(&self, agent: &str, content: &str) -> Result<wcore::AgentResponse> {
        let mutex = self
            .agents
            .get(agent)
            .ok_or_else(|| anyhow::anyhow!("agent '{agent}' not registered"))?;

        let dispatcher = self.dispatcher_for(agent).await;
        let mut guard = mutex.lock().await;
        guard.push_message(Message::user(content));

        let (tx, mut rx) = mpsc::unbounded_channel();
        let response = guard.run(&dispatcher, tx).await;

        while let Ok(event) = rx.try_recv() {
            self.hook.on_event(agent, &event);
        }

        Ok(response)
    }

    /// Send a message to an agent and stream response events.
    ///
    /// Builds a dispatcher snapshot from the tool registry, locks the per-agent
    /// mutex, delegates to `agent.run_stream()`, and forwards each event to
    /// `hook.on_event()`.
    pub fn stream_to<'a>(
        &'a self,
        agent: &'a str,
        content: &'a str,
    ) -> impl Stream<Item = AgentEvent> + 'a {
        stream! {
            let mutex = match self.agents.get(agent) {
                Some(m) => m,
                None => {
                    let resp = wcore::AgentResponse {
                        final_response: None,
                        iterations: 0,
                        stop_reason: wcore::AgentStopReason::Error(
                            format!("agent '{agent}' not registered"),
                        ),
                        steps: vec![],
                    };
                    yield AgentEvent::Done(resp);
                    return;
                }
            };

            let dispatcher = self.dispatcher_for(agent).await;
            let mut guard = mutex.lock().await;
            guard.push_message(Message::user(content));

            let mut event_stream = std::pin::pin!(guard.run_stream(&dispatcher));
            while let Some(event) = event_stream.next().await {
                self.hook.on_event(agent, &event);
                yield event;
            }
        }
    }
}
