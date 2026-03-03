//! Walrus runtime: the top-level orchestrator.
//!
//! The [`Runtime`] holds agents (config + model + history) behind a `RwLock`.
//! All backend concerns (tools, skills, MCP) are delegated to the [`Hook`]
//! trait. Events are emitted via `Hook::on_event()`.

pub use hook::Hook;
pub use memory::{InMemory, Memory, NoEmbedder};
pub use wcore::AgentConfig;
pub use wcore::model::{Message, Request, Response, Role, StreamChunk, Tool};

use anyhow::Result;
use compact_str::CompactString;
use futures_core::Stream;
use std::{collections::BTreeMap, future::Future, sync::Arc};
use tokio::sync::RwLock;
use wcore::{AgentEvent, AgentStopReason};

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

/// The walrus runtime — top-level orchestrator.
///
/// Generic over `H: Hook` for the runtime backend. Stores agents (each
/// holding its own model clone, config, and history) and delegates tool
/// dispatch and events to the Hook.
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

    /// Clear the conversation history for a named agent.
    pub async fn clear_session(&self, agent: &str) {
        if let Some(a) = self.agents.write().await.get_mut(agent) {
            a.clear_history();
        }
    }

    /// Send a message to a named agent, returning the final response.
    ///
    /// Extracts the agent, enriches its prompt, runs a step loop with
    /// events emitted via `Hook::on_event()`, then re-inserts the agent.
    pub async fn send_to(&self, agent: &str, message: Message) -> Result<Response> {
        let key = CompactString::from(agent);
        let mut agent_instance = self
            .agents
            .write()
            .await
            .remove(&key)
            .ok_or_else(|| anyhow::anyhow!("agent '{agent}' not registered"))?;

        self.hook.on_build_agent(&agent_instance.config);

        // Enrich prompt for this execution.
        let base_prompt = agent_instance.config.system_prompt.clone();
        agent_instance.config.system_prompt = self.hook.enrich_prompt(&agent_instance.config);
        agent_instance.push_message(message);

        let dispatcher = AgentDispatcher {
            hook: &*self.hook,
            agent,
        };

        // Step loop with event emission via Hook.
        let mut steps = Vec::new();
        let max = agent_instance.config.max_iterations;

        let result = loop {
            if steps.len() >= max {
                break steps
                    .last()
                    .map(|s: &wcore::AgentStep| s.response.clone())
                    .ok_or_else(|| anyhow::anyhow!("agent produced no response"));
            }

            match agent_instance.step(&dispatcher).await {
                Ok(step) => {
                    let has_tool_calls = !step.tool_calls.is_empty();

                    // Emit events via Hook.
                    if let Some(text) = step.response.content() {
                        self.hook.on_event(&AgentEvent::TextDelta(text.clone()));
                    }
                    if !step.tool_calls.is_empty() {
                        self.hook
                            .on_event(&AgentEvent::ToolCallsStart(step.tool_calls.clone()));
                        for (tc, result) in step.tool_calls.iter().zip(&step.tool_results) {
                            self.hook.on_event(&AgentEvent::ToolResult {
                                call_id: tc.id.clone(),
                                output: result.content.clone(),
                            });
                        }
                        self.hook.on_event(&AgentEvent::ToolCallsComplete);
                    }

                    if !has_tool_calls {
                        let response = step.response.clone();
                        steps.push(step);
                        let done = wcore::AgentResponse {
                            final_response: response.content().cloned(),
                            iterations: steps.len(),
                            stop_reason: AgentStopReason::TextResponse,
                            steps,
                        };
                        self.hook.on_event(&AgentEvent::Done(done));
                        break Ok(response);
                    }

                    steps.push(step);
                }
                Err(e) => {
                    let done = wcore::AgentResponse {
                        final_response: None,
                        iterations: steps.len(),
                        stop_reason: AgentStopReason::Error(e.to_string()),
                        steps,
                    };
                    self.hook.on_event(&AgentEvent::Done(done));
                    break Err(e);
                }
            }
        };

        // Restore base prompt and re-insert agent.
        agent_instance.config.system_prompt = base_prompt;
        self.agents.write().await.insert(key, agent_instance);

        result
    }

    /// Stream events from a named agent.
    ///
    /// Extracts the agent, enriches its prompt, runs a step loop yielding
    /// events, then re-inserts the agent.
    pub fn stream_to<'a>(
        &'a self,
        agent: &'a str,
        message: Message,
    ) -> impl Stream<Item = AgentEvent> + 'a {
        async_stream::stream! {
            let key = CompactString::from(agent);
            let mut agent_instance = match self.agents.write().await.remove(&key) {
                Some(a) => a,
                None => {
                    yield AgentEvent::Done(wcore::AgentResponse {
                        steps: vec![],
                        final_response: Some(format!("agent '{agent}' not registered")),
                        iterations: 0,
                        stop_reason: AgentStopReason::Error(
                            format!("agent '{agent}' not registered"),
                        ),
                    });
                    return;
                }
            };

            self.hook.on_build_agent(&agent_instance.config);

            // Enrich prompt for this execution.
            let base_prompt = agent_instance.config.system_prompt.clone();
            agent_instance.config.system_prompt =
                self.hook.enrich_prompt(&agent_instance.config);
            agent_instance.push_message(message);

            let dispatcher = AgentDispatcher {
                hook: &*self.hook,
                agent,
            };

            // Step loop yielding events.
            let mut steps = Vec::new();
            let max = agent_instance.config.max_iterations;

            for _ in 0..max {
                match agent_instance.step(&dispatcher).await {
                    Ok(step) => {
                        let has_tool_calls = !step.tool_calls.is_empty();
                        let text = step.response.content().cloned();

                        if let Some(ref t) = text {
                            let event = AgentEvent::TextDelta(t.clone());
                            self.hook.on_event(&event);
                            yield event;
                        }

                        if has_tool_calls {
                            let event = AgentEvent::ToolCallsStart(step.tool_calls.clone());
                            self.hook.on_event(&event);
                            yield event;

                            for (tc, result) in step.tool_calls.iter().zip(&step.tool_results) {
                                let event = AgentEvent::ToolResult {
                                    call_id: tc.id.clone(),
                                    output: result.content.clone(),
                                };
                                self.hook.on_event(&event);
                                yield event;
                            }

                            let event = AgentEvent::ToolCallsComplete;
                            self.hook.on_event(&event);
                            yield event;
                        }

                        if !has_tool_calls {
                            let stop_reason = if text.is_some() {
                                AgentStopReason::TextResponse
                            } else {
                                AgentStopReason::NoAction
                            };
                            steps.push(step);
                            let response = wcore::AgentResponse {
                                final_response: text,
                                iterations: steps.len(),
                                stop_reason,
                                steps,
                            };
                            let event = AgentEvent::Done(response);
                            self.hook.on_event(&event);
                            yield event;

                            // Restore base prompt and re-insert.
                            agent_instance.config.system_prompt = base_prompt;
                            self.agents.write().await.insert(key, agent_instance);
                            return;
                        }

                        steps.push(step);
                    }
                    Err(e) => {
                        let response = wcore::AgentResponse {
                            final_response: None,
                            iterations: steps.len(),
                            stop_reason: AgentStopReason::Error(e.to_string()),
                            steps,
                        };
                        let event = AgentEvent::Done(response);
                        self.hook.on_event(&event);
                        yield event;

                        // Restore base prompt and re-insert.
                        agent_instance.config.system_prompt = base_prompt;
                        self.agents.write().await.insert(key, agent_instance);
                        return;
                    }
                }
            }

            // Max iterations reached.
            let final_response = steps.last().and_then(|s| s.response.content().cloned());
            let response = wcore::AgentResponse {
                final_response,
                iterations: steps.len(),
                stop_reason: AgentStopReason::MaxIterations,
                steps,
            };
            let event = AgentEvent::Done(response);
            self.hook.on_event(&event);
            yield event;

            // Restore base prompt and re-insert.
            agent_instance.config.system_prompt = base_prompt;
            self.agents.write().await.insert(key, agent_instance);
        }
    }
}
