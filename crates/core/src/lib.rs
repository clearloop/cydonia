//! Walrus agent library.
//!
//! - [`Agent`]: Stateful execution unit with step/run/run_stream.
//! - [`AgentBuilder`]: Fluent construction with a model provider.
//! - [`AgentConfig`]: Serializable agent parameters.
//! - [`Dispatcher`]: Generic async trait for tool dispatch.
//! - [`ToolRegistry`]: Canonical dispatcher — holds `(Tool, Handler)` pairs.
//! - [`Hook`]: Lifecycle backend for agent building, events, and tool registration.
//! - [`model`]: Unified LLM interface types and traits.
//! - Agent event types: [`AgentEvent`], [`AgentStep`], [`AgentResponse`], [`AgentStopReason`].

pub use agent::{Agent, AgentBuilder, AgentConfig, parse_agent_md};
pub use dispatch::{Dispatcher, Handler, ToolRegistry};
pub use event::{AgentEvent, AgentResponse, AgentStep, AgentStopReason};
pub use hook::Hook;

mod agent;
mod dispatch;
mod event;
pub mod hook;
pub mod model;
pub mod utils;
