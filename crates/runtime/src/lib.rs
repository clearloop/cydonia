//! Walrus runtime: agent registry, tool registry, and hook orchestration.
//!
//! The [`Runtime`] holds agents in a plain `BTreeMap` with per-agent
//! `Mutex` for concurrent execution. Tools are stored in a shared registry
//! (`Arc<RwLock>`) supporting post-startup registration (e.g. MCP hot-reload).

pub use hook::Hook;
pub use memory::{InMemory, Memory, NoEmbedder};
pub use wcore::AgentConfig;
pub use wcore::model::{Message, Request, Response, Role, StreamChunk, Tool};

use anyhow::Result;
use async_stream::stream;
use compact_str::CompactString;
use futures_core::Stream;
use futures_util::StreamExt;
use std::{collections::BTreeMap, future::Future, pin::Pin, sync::Arc};
use tokio::sync::{Mutex, RwLock, mpsc};
use wcore::{AgentEvent, ClosureDispatcher, DispatchFn};

pub mod hook;

/// Re-exports of the most commonly used types.
pub mod prelude {
    pub use crate::{
        AgentConfig, Hook, InMemory, Message, Request, Response, Role, Runtime, StreamChunk, Tool,
    };
}

/// A type-erased async tool handler.
pub type Handler =
    Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = String> + Send>> + Send + Sync>;

/// The walrus runtime — agent registry, tool registry, and hook orchestration.
///
/// Each agent is wrapped in a per-agent `Mutex` for concurrent execution.
/// Tools are stored behind `Arc<RwLock>` — registered before or after
/// wrapping in `Arc` (for MCP hot-reload).
pub struct Runtime<M: wcore::model::Model, H: Hook> {
    model: M,
    hook: H,
    agents: BTreeMap<CompactString, Arc<Mutex<wcore::Agent<M>>>>,
    tools: Arc<RwLock<BTreeMap<CompactString, (Tool, Handler)>>>,
}

impl<M: wcore::model::Model + Send + Sync + Clone + 'static, H: Hook + 'static> Runtime<M, H> {
    /// Create a new runtime with the given model and hook backend.
    pub fn new(model: M, hook: H) -> Self {
        Self {
            model,
            hook,
            agents: BTreeMap::new(),
            tools: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    /// Access the model backend.
    pub fn model(&self) -> &M {
        &self.model
    }

    /// Access the hook backend.
    pub fn hook(&self) -> &H {
        &self.hook
    }

    // --- Tool registry ---

    /// Register a tool with its handler.
    ///
    /// Works both before and after wrapping in `Arc` (the registry is
    /// behind `RwLock`).
    pub async fn register_tool(&self, tool: Tool, handler: Handler) {
        let name = tool.name.clone();
        self.tools.write().await.insert(name, (tool, handler));
    }

    /// Remove a tool by name. Returns `true` if it existed.
    pub async fn unregister_tool(&self, name: &str) -> bool {
        self.tools.write().await.remove(name).is_some()
    }

    /// Atomically replace a set of tools. Removes `old_names` and inserts
    /// `new_tools` under a single write lock — no window where agents see
    /// partial state.
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
            let name = tool.name.clone();
            registry.insert(name, (tool, handler));
        }
    }

    /// Build a [`ClosureDispatcher`] for the named agent.
    ///
    /// Reads the agent's `config.tools` list and filters the registry.
    /// If the list is empty, all registered tools are included.
    async fn dispatcher_for(&self, agent: &str) -> ClosureDispatcher {
        let registry = self.tools.read().await;

        // Get the agent's tool filter list.
        let filter: Option<Vec<CompactString>> = self
            .agents
            .get(agent)
            .and_then(|m| m.try_lock().ok())
            .map(|g| g.config.tools.to_vec())
            .filter(|t| !t.is_empty());

        let tools: Vec<Tool> = match &filter {
            Some(names) => registry
                .values()
                .filter(|(t, _)| names.iter().any(|n| n == &*t.name))
                .map(|(t, _)| t.clone())
                .collect(),
            None => registry.values().map(|(t, _)| t.clone()).collect(),
        };

        let registry_arc = Arc::clone(&self.tools);
        let dispatch_fn: DispatchFn = Arc::new(move |calls: Vec<(String, String)>| {
            let registry = Arc::clone(&registry_arc);
            Box::pin(async move {
                let reg = registry.read().await;
                let mut results = Vec::with_capacity(calls.len());
                for (method, params) in &calls {
                    let output = if let Some((_, handler)) = reg.get(method.as_str()) {
                        Ok(handler(params.clone()).await)
                    } else {
                        Ok(format!("function {method} not available"))
                    };
                    results.push(output);
                }
                results
            })
        });

        ClosureDispatcher::new(tools, dispatch_fn)
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
    /// Builds a dispatcher from the tool registry, locks the per-agent mutex,
    /// pushes the user message, delegates to `agent.run()`, and forwards all
    /// events to `hook.on_event()`.
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

        // Drain buffered events and forward to hook.
        while let Ok(event) = rx.try_recv() {
            self.hook.on_event(agent, &event);
        }

        Ok(response)
    }

    /// Send a message to an agent and stream response events.
    ///
    /// Builds a dispatcher from the tool registry, locks the per-agent mutex,
    /// delegates to `agent.run_stream()`, and forwards each event to
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
