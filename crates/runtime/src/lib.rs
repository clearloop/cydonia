//! Walrus runtime: agent registry and hook orchestration.
//!
//! The [`Runtime`] holds agents behind a `RwLock` and provides take/put
//! semantics for callers to drive execution directly via `Agent::run()`
//! or `Agent::run_stream()`.

pub use hook::Hook;
pub use memory::{InMemory, Memory, NoEmbedder};
pub use wcore::AgentConfig;
pub use wcore::model::{Message, Request, Response, Role, StreamChunk, Tool};

use anyhow::Result;
use compact_str::CompactString;
use std::{collections::BTreeMap, future::Future, sync::Arc};
use tokio::sync::RwLock;

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
/// Generic over `H: Hook` for the runtime backend. Stores agents (each
/// holding its own model clone, config, and history). Callers take agents
/// out for execution and put them back when done.
pub struct Runtime<H: Hook> {
    hook: Arc<H>,
    agents: RwLock<BTreeMap<CompactString, wcore::Agent<H::Model>>>,
}

impl<H: Hook + 'static> Runtime<H> {
    /// Create a new runtime with the given hook backend.
    pub fn new(hook: Arc<H>) -> Self {
        Self {
            hook,
            agents: RwLock::new(BTreeMap::new()),
        }
    }

    /// Access the hook backend.
    pub fn hook(&self) -> &H {
        &self.hook
    }

    /// Register an agent from its configuration.
    ///
    /// Clones the Hook's model into the agent so it can drive LLM calls.
    pub async fn add_agent(&self, config: AgentConfig) {
        let name = config.name.clone();
        let agent = wcore::AgentBuilder::new(self.hook.model().clone())
            .config(config)
            .build();
        self.agents.write().await.insert(name, agent);
    }

    /// Get a registered agent's config by name (cloned).
    pub async fn agent(&self, name: &str) -> Option<AgentConfig> {
        self.agents.read().await.get(name).map(|a| a.config.clone())
    }

    /// Get all registered agent configs (cloned, alphabetical order).
    pub async fn agents(&self) -> Vec<AgentConfig> {
        self.agents
            .read()
            .await
            .values()
            .map(|a| a.config.clone())
            .collect()
    }

    /// Take an agent out of the registry for execution.
    ///
    /// The agent is removed from the map. Caller must call [`put_agent`]
    /// to re-insert it after execution completes.
    pub async fn take_agent(&self, name: &str) -> Option<wcore::Agent<H::Model>> {
        self.agents.write().await.remove(name)
    }

    /// Put an agent back into the registry after execution.
    pub async fn put_agent(&self, agent: wcore::Agent<H::Model>) {
        let name = agent.config.name.clone();
        self.agents.write().await.insert(name, agent);
    }

    /// Clear the conversation history for a named agent.
    pub async fn clear_session(&self, agent: &str) {
        if let Some(a) = self.agents.write().await.get_mut(agent) {
            a.clear_history();
        }
    }
}
