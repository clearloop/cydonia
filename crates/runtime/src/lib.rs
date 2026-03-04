//! Walrus runtime: agent registry and hook orchestration.
//!
//! The [`Runtime`] holds agents in a plain `BTreeMap` with per-agent
//! `Mutex` for concurrent execution. Agents are registered at startup
//! via `add_agent(&mut self)` before the runtime is wrapped in `Arc`.

pub use hook::Hook;
pub use memory::{InMemory, Memory, NoEmbedder};
pub use wcore::AgentConfig;
pub use wcore::model::{Message, Request, Response, Role, StreamChunk, Tool};

use anyhow::Result;
use async_stream::stream;
use compact_str::CompactString;
use futures_core::Stream;
use futures_util::StreamExt;
use std::{collections::BTreeMap, future::Future, sync::Arc};
use tokio::sync::{Mutex, mpsc};
use wcore::AgentEvent;

pub mod hook;

/// Re-exports of the most commonly used types.
pub mod prelude {
    pub use crate::{
        AgentConfig, Hook, InMemory, Message, Request, Response, Role, Runtime, StreamChunk, Tool,
    };
}

/// A type-erased async tool handler.
pub type Handler =
    Arc<dyn Fn(String) -> std::pin::Pin<Box<dyn Future<Output = String> + Send>> + Send + Sync>;

/// Thin wrapper that implements wcore's `Dispatcher` by forwarding to Hook.
pub struct AgentDispatcher<'a, H: Hook> {
    /// The hook backend.
    pub hook: &'a H,
    /// The agent name for scoped dispatch.
    pub agent: &'a str,
}

impl<H: Hook> wcore::Dispatcher for AgentDispatcher<'_, H> {
    fn dispatch(&self, calls: &[(&str, &str)]) -> impl Future<Output = Vec<Result<String>>> + Send {
        self.hook.dispatch(self.agent, calls)
    }

    fn tools(&self) -> Vec<Tool> {
        self.hook.tools(self.agent)
    }
}

/// The walrus runtime — agent registry and hook orchestration.
///
/// Each agent is wrapped in a per-agent `Mutex` for concurrent execution.
/// The map itself is a plain `BTreeMap` — agents are registered at startup
/// via `add_agent(&mut self)` before wrapping in `Arc`.
pub struct Runtime<M: wcore::model::Model, H: Hook> {
    model: M,
    hook: Arc<H>,
    agents: BTreeMap<CompactString, Arc<Mutex<wcore::Agent<M>>>>,
}

impl<M: wcore::model::Model + Send + Sync + Clone + 'static, H: Hook + 'static> Runtime<M, H> {
    /// Create a new runtime with the given model and hook backend.
    pub fn new(model: M, hook: Arc<H>) -> Self {
        Self {
            model,
            hook,
            agents: BTreeMap::new(),
        }
    }

    /// Access the hook backend.
    pub fn hook(&self) -> &H {
        &self.hook
    }

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

    /// Send a message to an agent and run to completion.
    ///
    /// Locks the per-agent mutex, pushes the user message, delegates to
    /// `agent.run()`, and forwards all events to `hook.on_event()`.
    pub async fn send_to(&self, agent: &str, content: &str) -> Result<wcore::AgentResponse> {
        let mutex = self
            .agents
            .get(agent)
            .ok_or_else(|| anyhow::anyhow!("agent '{agent}' not registered"))?;

        let mut guard = mutex.lock().await;
        guard.push_message(Message::user(content));
        let dispatcher = AgentDispatcher {
            hook: &*self.hook,
            agent,
        };

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
    /// Locks the per-agent mutex, delegates to `agent.run_stream()`, and
    /// forwards each event to `hook.on_event()`.
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

            let mut guard = mutex.lock().await;
            guard.push_message(Message::user(content));

            let dispatcher = AgentDispatcher {
                hook: &*self.hook,
                agent,
            };
            let mut event_stream = std::pin::pin!(guard.run_stream(&dispatcher));
            while let Some(event) = event_stream.next().await {
                self.hook.on_event(agent, &event);
                yield event;
            }
        }
    }
}
