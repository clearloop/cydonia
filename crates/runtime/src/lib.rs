//! Walrus runtime: the top-level orchestrator.
//!
//! The [`Runtime`] holds agent configurations and session state. All backend
//! concerns (model, tools, skills, MCP) are delegated to the [`Hook`] trait.
//! Agents are created as ephemeral execution units per request, driven via
//! `Agent::run()` and `Agent::run_stream()`.

pub use hook::Hook;
pub use memory::{InMemory, Memory, NoEmbedder};
pub use skills::{Skill, SkillRegistry, SkillTier};
pub use wcore::AgentConfig;
pub use wcore::model::{Message, Request, Response, Role, StreamChunk, Tool};

use anyhow::Result;
use compact_str::CompactString;
use futures_core::Stream;
use std::{collections::BTreeMap, future::Future, sync::Arc};
use tokio::sync::RwLock;
use wcore::AgentEvent;

pub mod hook;
pub mod skills;

/// Re-exports of the most commonly used types.
pub mod prelude {
    pub use crate::{
        AgentConfig, Hook, InMemory, Message, Request, Response, Role, Runtime, SkillRegistry,
        StreamChunk, Tool,
    };
}

/// A type-erased async tool handler.
pub type Handler =
    Arc<dyn Fn(String) -> std::pin::Pin<Box<dyn Future<Output = String> + Send>> + Send + Sync>;

/// Private session state, keyed by agent name in the sessions map.
struct Session {
    messages: Vec<Message>,
}

impl Session {
    fn new() -> Self {
        Self {
            messages: Vec::new(),
        }
    }
}

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

/// The walrus runtime — top-level orchestrator.
///
/// Generic over `H: Hook` for the runtime backend. Stores agent configurations
/// and manages sessions. Creates ephemeral [`wcore::Agent`] instances per request.
pub struct Runtime<H: Hook> {
    hook: Arc<H>,
    agents: BTreeMap<CompactString, AgentConfig>,
    sessions: RwLock<BTreeMap<CompactString, Session>>,
}

impl<H: Hook + 'static> Runtime<H> {
    /// Create a new runtime with the given hook backend.
    pub fn new(hook: Arc<H>) -> Self {
        Self {
            hook,
            agents: BTreeMap::new(),
            sessions: RwLock::new(BTreeMap::new()),
        }
    }

    /// Access the hook backend.
    pub fn hook(&self) -> &H {
        &self.hook
    }

    /// Register an agent configuration.
    pub fn add_agent(&mut self, agent: AgentConfig) {
        self.agents.insert(agent.name.clone(), agent);
    }

    /// Get a registered agent config by name.
    pub fn agent(&self, name: &str) -> Option<&AgentConfig> {
        self.agents.get(name)
    }

    /// Iterate over all registered agent configs in alphabetical order.
    pub fn agents(&self) -> impl Iterator<Item = &AgentConfig> {
        self.agents.values()
    }

    /// Clear the session for a named agent, resetting conversation history.
    pub async fn clear_session(&self, agent: &str) {
        self.sessions.write().await.remove(agent);
    }

    /// Create an ephemeral Agent from config with a fresh event channel.
    fn create_agent(
        hook: &H,
        agent_config: &AgentConfig,
        history: Vec<Message>,
    ) -> (wcore::Agent, tokio::sync::mpsc::Receiver<AgentEvent>) {
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let enriched_prompt = hook.enrich_prompt(agent_config);
        let mut config = agent_config.clone();
        config.system_prompt = enriched_prompt;
        let mut agent = wcore::AgentBuilder::new(tx).config(config).build();
        for msg in history {
            agent.push_message(msg);
        }
        (agent, rx)
    }

    /// Send a message to a named agent using Agent.run().
    ///
    /// Creates an ephemeral Agent, runs it with the Hook as dispatcher,
    /// and returns the final response. Events are emitted via Hook::on_event().
    pub async fn send_to(&self, agent: &str, message: Message) -> Result<Response> {
        let agent_config = self
            .agents
            .get(agent)
            .ok_or_else(|| anyhow::anyhow!("agent '{agent}' not registered"))?
            .clone();
        let key = CompactString::from(agent);

        self.hook.on_build_agent(&agent_config);

        let mut session = {
            self.sessions
                .write()
                .await
                .remove(&key)
                .unwrap_or_else(Session::new)
        };
        session.messages.push(message);

        let dispatcher = AgentDispatcher {
            hook: &*self.hook,
            agent,
        };
        let (mut agent_instance, mut rx) =
            Self::create_agent(&self.hook, &agent_config, session.messages);

        // Spawn event drain task.
        let hook = Arc::clone(&self.hook);
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                hook.on_event(&event);
            }
        });

        let agent_response = agent_instance.run(self.hook.model(), &dispatcher).await;
        let messages = agent_instance.messages().to_vec();
        self.sessions
            .write()
            .await
            .insert(key, Session { messages });

        agent_response
            .steps
            .last()
            .map(|s| s.response.clone())
            .ok_or_else(|| anyhow::anyhow!("agent produced no response"))
    }

    /// Stream events from a named agent using Agent.run_stream().
    ///
    /// Creates an ephemeral Agent and yields AgentEvents as a stream.
    pub fn stream_to<'a>(
        &'a self,
        agent: &'a str,
        message: Message,
    ) -> impl Stream<Item = AgentEvent> + 'a {
        async_stream::stream! {
            let agent_config = match self.agents.get(agent) {
                Some(c) => c.clone(),
                None => {
                    yield AgentEvent::Done(wcore::AgentResponse {
                        steps: vec![],
                        final_response: Some(format!("agent '{agent}' not registered")),
                        iterations: 0,
                        stop_reason: wcore::AgentStopReason::Error(
                            format!("agent '{agent}' not registered"),
                        ),
                    });
                    return;
                }
            };
            let key = CompactString::from(agent);

            self.hook.on_build_agent(&agent_config);

            let mut session = {
                self.sessions.write().await.remove(&key).unwrap_or_else(Session::new)
            };
            session.messages.push(message);

            let dispatcher = AgentDispatcher {
                hook: &*self.hook,
                agent,
            };
            let (mut agent_instance, _rx) =
                Self::create_agent(&self.hook, &agent_config, session.messages);

            {
                let stream = agent_instance.run_stream(self.hook.model(), &dispatcher);
                futures_util::pin_mut!(stream);

                while let Some(event) = futures_util::StreamExt::next(&mut stream).await {
                    self.hook.on_event(&event);
                    yield event;
                }
            }

            let messages = agent_instance.messages().to_vec();
            self.sessions.write().await.insert(key, Session { messages });
        }
    }
}
