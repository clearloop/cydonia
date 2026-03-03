//! Walrus runtime: agent registry and hook orchestration.
//!
//! The [`Runtime`] holds agents behind a `RwLock` and drives execution
//! through the [`Hook`] lifecycle. `send_to` and `stream_to` are the
//! primary execution entry points — they take an agent out, run the step
//! loop, emit events through the hook, and put the agent back.

pub use hook::Hook;
pub use memory::{InMemory, Memory, NoEmbedder};
pub use wcore::AgentConfig;
pub use wcore::model::{Message, Request, Response, Role, StreamChunk, Tool};

use anyhow::Result;
use async_stream::stream;
use compact_str::CompactString;
use futures_core::Stream;
use std::{collections::BTreeMap, future::Future, sync::Arc};
use tokio::sync::RwLock;
use wcore::{AgentEvent, AgentResponse, AgentStopReason};

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
/// Generic over `M: Model` (the LLM provider) and `H: Hook` (the lifecycle
/// backend). The model is owned by Runtime and cloned into each agent.
/// The hook provides tool schemas, tool dispatch, prompt enrichment, and
/// event observation.
pub struct Runtime<M: wcore::model::Model, H: Hook> {
    model: M,
    hook: Arc<H>,
    agents: RwLock<BTreeMap<CompactString, wcore::Agent<M>>>,
}

impl<M: wcore::model::Model + Send + Sync + Clone + 'static, H: Hook + 'static> Runtime<M, H> {
    /// Create a new runtime with the given model and hook backend.
    pub fn new(model: M, hook: Arc<H>) -> Self {
        Self {
            model,
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
    /// Calls `hook.on_build_agent(config)` to enrich the config before
    /// building the agent. Clones the runtime's model into the agent.
    pub async fn add_agent(&self, config: AgentConfig) {
        let config = self.hook.on_build_agent(config);
        let name = config.name.clone();
        let agent = wcore::AgentBuilder::new(self.model.clone())
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
    pub async fn take_agent(&self, name: &str) -> Option<wcore::Agent<M>> {
        self.agents.write().await.remove(name)
    }

    /// Put an agent back into the registry after execution.
    pub async fn put_agent(&self, agent: wcore::Agent<M>) {
        let name = agent.config.name.clone();
        self.agents.write().await.insert(name, agent);
    }

    /// Clear the conversation history for a named agent.
    pub async fn clear_session(&self, agent: &str) {
        if let Some(a) = self.agents.write().await.get_mut(agent) {
            a.clear_history();
        }
    }

    /// Send a message to an agent and run to completion.
    ///
    /// Takes the agent from the registry, pushes the user message, runs
    /// the step loop, emits events through `hook.on_event()`, and puts
    /// the agent back. Returns the final response.
    pub async fn send_to(&self, agent: &str, content: &str) -> Result<AgentResponse> {
        let mut agent_instance = self
            .agents
            .write()
            .await
            .remove(agent)
            .ok_or_else(|| anyhow::anyhow!("agent '{agent}' not registered"))?;

        agent_instance.push_message(Message::user(content));
        let dispatcher = AgentDispatcher {
            hook: &*self.hook,
            agent,
        };

        let mut steps = Vec::new();
        let max = agent_instance.config.max_iterations;

        let response = 'outer: {
            for _ in 0..max {
                match agent_instance.step(&dispatcher).await {
                    Ok(step) => {
                        let has_tool_calls = !step.tool_calls.is_empty();
                        let text = step.response.content().cloned();

                        if let Some(ref t) = text {
                            self.hook.on_event(agent, &AgentEvent::TextDelta(t.clone()));
                        }
                        if has_tool_calls {
                            self.hook.on_event(
                                agent,
                                &AgentEvent::ToolCallsStart(step.tool_calls.clone()),
                            );
                            for (tc, result) in step.tool_calls.iter().zip(&step.tool_results) {
                                self.hook.on_event(
                                    agent,
                                    &AgentEvent::ToolResult {
                                        call_id: tc.id.clone(),
                                        output: result.content.clone(),
                                    },
                                );
                            }
                            self.hook.on_event(agent, &AgentEvent::ToolCallsComplete);
                        }

                        if !has_tool_calls {
                            let stop_reason = if text.is_some() {
                                AgentStopReason::TextResponse
                            } else {
                                AgentStopReason::NoAction
                            };
                            steps.push(step);
                            let resp = AgentResponse {
                                final_response: text,
                                iterations: steps.len(),
                                stop_reason,
                                steps,
                            };
                            self.hook.on_event(agent, &AgentEvent::Done(resp.clone()));
                            break 'outer resp;
                        }

                        steps.push(step);
                    }
                    Err(e) => {
                        let resp = AgentResponse {
                            final_response: None,
                            iterations: steps.len(),
                            stop_reason: AgentStopReason::Error(e.to_string()),
                            steps,
                        };
                        self.hook.on_event(agent, &AgentEvent::Done(resp.clone()));
                        break 'outer resp;
                    }
                }
            }

            let final_response = steps.last().and_then(|s| s.response.content().cloned());
            let resp = AgentResponse {
                final_response,
                iterations: steps.len(),
                stop_reason: AgentStopReason::MaxIterations,
                steps,
            };
            self.hook.on_event(agent, &AgentEvent::Done(resp.clone()));
            resp
        };

        self.agents
            .write()
            .await
            .insert(CompactString::from(agent), agent_instance);
        Ok(response)
    }

    /// Send a message to an agent and stream response events.
    ///
    /// Takes the agent, runs the step loop, yields `AgentEvent`s to the
    /// caller AND emits them through `hook.on_event()`. Puts the agent
    /// back when done.
    pub fn stream_to<'a>(
        &'a self,
        agent: &'a str,
        content: &'a str,
    ) -> impl Stream<Item = AgentEvent> + 'a {
        stream! {
            let mut agent_instance = match self.agents.write().await.remove(agent) {
                Some(a) => a,
                None => {
                    let resp = AgentResponse {
                        final_response: None,
                        iterations: 0,
                        stop_reason: AgentStopReason::Error(
                            format!("agent '{agent}' not registered"),
                        ),
                        steps: vec![],
                    };
                    yield AgentEvent::Done(resp);
                    return;
                }
            };

            agent_instance.push_message(Message::user(content));
            let dispatcher = AgentDispatcher {
                hook: &*self.hook,
                agent,
            };

            let mut steps = Vec::new();
            let max = agent_instance.config.max_iterations;

            for _ in 0..max {
                match agent_instance.step(&dispatcher).await {
                    Ok(step) => {
                        let has_tool_calls = !step.tool_calls.is_empty();
                        let text = step.response.content().cloned();

                        if let Some(ref t) = text {
                            let event = AgentEvent::TextDelta(t.clone());
                            self.hook.on_event(agent, &event);
                            yield event;
                        }

                        if has_tool_calls {
                            let event = AgentEvent::ToolCallsStart(step.tool_calls.clone());
                            self.hook.on_event(agent, &event);
                            yield event;

                            for (tc, result) in step.tool_calls.iter().zip(&step.tool_results) {
                                let event = AgentEvent::ToolResult {
                                    call_id: tc.id.clone(),
                                    output: result.content.clone(),
                                };
                                self.hook.on_event(agent, &event);
                                yield event;
                            }

                            let event = AgentEvent::ToolCallsComplete;
                            self.hook.on_event(agent, &event);
                            yield event;
                        }

                        if !has_tool_calls {
                            let stop_reason = if text.is_some() {
                                AgentStopReason::TextResponse
                            } else {
                                AgentStopReason::NoAction
                            };
                            steps.push(step);
                            let resp = AgentResponse {
                                final_response: text,
                                iterations: steps.len(),
                                stop_reason,
                                steps,
                            };
                            self.hook.on_event(agent, &AgentEvent::Done(resp.clone()));
                            yield AgentEvent::Done(resp);
                            self.agents.write().await.insert(
                                CompactString::from(agent),
                                agent_instance,
                            );
                            return;
                        }

                        steps.push(step);
                    }
                    Err(e) => {
                        let resp = AgentResponse {
                            final_response: None,
                            iterations: steps.len(),
                            stop_reason: AgentStopReason::Error(e.to_string()),
                            steps,
                        };
                        self.hook.on_event(agent, &AgentEvent::Done(resp.clone()));
                        yield AgentEvent::Done(resp);
                        self.agents.write().await.insert(
                            CompactString::from(agent),
                            agent_instance,
                        );
                        return;
                    }
                }
            }

            let final_response = steps.last().and_then(|s| s.response.content().cloned());
            let resp = AgentResponse {
                final_response,
                iterations: steps.len(),
                stop_reason: AgentStopReason::MaxIterations,
                steps,
            };
            self.hook.on_event(agent, &AgentEvent::Done(resp.clone()));
            yield AgentEvent::Done(resp);
            self.agents.write().await.insert(
                CompactString::from(agent),
                agent_instance,
            );
        }
    }
}
