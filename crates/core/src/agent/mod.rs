//! Stateful agent execution unit.
//!
//! [`Agent`] owns its configuration, model, and message history. It drives
//! LLM execution through [`Agent::step`], [`Agent::run`], and
//! [`Agent::run_stream`]. `run_stream()` is the canonical step loop —
//! `run()` collects its events and returns the final response.

use crate::{
    dispatch::Dispatcher,
    event::{AgentEvent, AgentResponse, AgentStep, AgentStopReason},
    model::{Message, Model, Request},
};
use anyhow::Result;
use async_stream::stream;
use futures_core::Stream;
use tokio::sync::mpsc;

pub use builder::AgentBuilder;
pub use config::AgentConfig;
pub use parser::parse_agent_md;

mod builder;
pub mod config;
mod parser;

/// A stateful agent execution unit.
///
/// Generic over `M: Model` — stores the model provider alongside config
/// and conversation history. Callers drive execution via `step()` (single
/// LLM round), `run()` (loop to completion), or `run_stream()` (yields
/// events as a stream).
pub struct Agent<M: Model> {
    /// Agent configuration (name, prompt, model, limits, tool_choice).
    pub config: AgentConfig,
    /// The model provider for LLM calls.
    model: M,
    /// Conversation history (user/assistant/tool messages).
    pub(crate) history: Vec<Message>,
}

impl<M: Model> Agent<M> {
    /// Push a message into the conversation history.
    pub fn push_message(&mut self, message: Message) {
        self.history.push(message);
    }

    /// Return a reference to the conversation history.
    pub fn messages(&self) -> &[Message] {
        &self.history
    }

    /// Clear the conversation history, keeping configuration intact.
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// Perform a single LLM round: send request, dispatch tools, return step.
    ///
    /// Composes a [`Request`] from config state (system prompt + history +
    /// dispatcher tools), calls the stored model, dispatches any tool calls
    /// via `dispatcher.dispatch()`, and appends results to history.
    pub async fn step<D: Dispatcher>(&mut self, dispatcher: &D) -> Result<AgentStep> {
        let model_name = self
            .config
            .model
            .clone()
            .unwrap_or_else(|| self.model.active_model());

        let mut messages = Vec::with_capacity(1 + self.history.len());
        if !self.config.system_prompt.is_empty() {
            messages.push(Message::system(&self.config.system_prompt));
        }
        messages.extend(self.history.iter().cloned());

        let tools = dispatcher.tools();
        let mut request = Request::new(model_name)
            .with_messages(messages)
            .with_tool_choice(self.config.tool_choice.clone());
        if !tools.is_empty() {
            request = request.with_tools(tools);
        }

        let response = self.model.send(&request).await?;
        let tool_calls = response.tool_calls().unwrap_or_default().to_vec();

        // Append the assistant message to history.
        if let Some(msg) = response.message() {
            self.history.push(msg);
        }

        // Dispatch tool calls if any.
        let mut tool_results = Vec::new();
        if !tool_calls.is_empty() {
            let calls: Vec<(&str, &str)> = tool_calls
                .iter()
                .map(|tc| (tc.function.name.as_str(), tc.function.arguments.as_str()))
                .collect();

            let results = dispatcher.dispatch(&calls).await;

            for (tc, result) in tool_calls.iter().zip(results) {
                let output = match result {
                    Ok(s) => s,
                    Err(e) => format!("error: {e}"),
                };

                let msg = Message::tool(&output, tc.id.clone());
                self.history.push(msg.clone());
                tool_results.push(msg);
            }
        }

        Ok(AgentStep {
            response,
            tool_calls,
            tool_results,
        })
    }

    /// Determine the stop reason for a step with no tool calls.
    fn stop_reason(step: &AgentStep) -> AgentStopReason {
        if step.response.content().is_some() {
            AgentStopReason::TextResponse
        } else {
            AgentStopReason::NoAction
        }
    }

    /// Run the agent loop to completion, returning the final response.
    ///
    /// Wraps [`Agent::run_stream`] — collects all events, sends each through
    /// `events`, and extracts the `Done` response. The event sender allows
    /// callers (like Runtime) to observe execution without reimplementing
    /// the step loop.
    pub async fn run<D: Dispatcher>(
        &mut self,
        dispatcher: &D,
        events: mpsc::UnboundedSender<AgentEvent>,
    ) -> AgentResponse {
        use futures_util::StreamExt;

        let mut stream = std::pin::pin!(self.run_stream(dispatcher));
        let mut response = None;
        while let Some(event) = stream.next().await {
            if let AgentEvent::Done(ref resp) = event {
                response = Some(resp.clone());
            }
            let _ = events.send(event);
        }

        response.unwrap_or_else(|| AgentResponse {
            final_response: None,
            iterations: 0,
            stop_reason: AgentStopReason::Error("stream ended without Done".into()),
            steps: vec![],
        })
    }

    /// Run the agent loop as a stream of [`AgentEvent`]s.
    ///
    /// The canonical step loop. Calls [`Agent::step`] up to `max_iterations`
    /// times, yielding events as they are produced. Always finishes with a
    /// `Done` event containing the [`AgentResponse`].
    pub fn run_stream<'a, D: Dispatcher + 'a>(
        &'a mut self,
        dispatcher: &'a D,
    ) -> impl Stream<Item = AgentEvent> + 'a {
        stream! {
            let mut steps = Vec::new();
            let max = self.config.max_iterations;

            for _ in 0..max {
                match self.step(dispatcher).await {
                    Ok(step) => {
                        let has_tool_calls = !step.tool_calls.is_empty();
                        let text = step.response.content().cloned();

                        if let Some(ref t) = text {
                            yield AgentEvent::TextDelta(t.clone());
                        }

                        if has_tool_calls {
                            yield AgentEvent::ToolCallsStart(step.tool_calls.clone());
                            for (tc, result) in step.tool_calls.iter().zip(&step.tool_results) {
                                yield AgentEvent::ToolResult {
                                    call_id: tc.id.clone(),
                                    output: result.content.clone(),
                                };
                            }
                            yield AgentEvent::ToolCallsComplete;
                        }

                        if !has_tool_calls {
                            let stop_reason = Self::stop_reason(&step);
                            steps.push(step);
                            yield AgentEvent::Done(AgentResponse {
                                final_response: text,
                                iterations: steps.len(),
                                stop_reason,
                                steps,
                            });
                            return;
                        }

                        steps.push(step);
                    }
                    Err(e) => {
                        yield AgentEvent::Done(AgentResponse {
                            final_response: None,
                            iterations: steps.len(),
                            stop_reason: AgentStopReason::Error(e.to_string()),
                            steps,
                        });
                        return;
                    }
                }
            }

            let final_response = steps.last().and_then(|s| s.response.content().cloned());
            yield AgentEvent::Done(AgentResponse {
                final_response,
                iterations: steps.len(),
                stop_reason: AgentStopReason::MaxIterations,
                steps,
            });
        }
    }
}
