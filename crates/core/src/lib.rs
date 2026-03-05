//! Walrus agent library.
//!
//! - [`Agent`]: Stateful execution unit with step/run/run_stream.
//! - [`AgentBuilder`]: Fluent construction with a model provider.
//! - [`AgentConfig`]: Serializable agent parameters.
//! - [`Dispatcher`]: Generic async trait for tool dispatch.
//! - [`ToolRegistry`]: Canonical dispatcher — holds `(Tool, Handler)` pairs.
//! - [`Hook`]: Lifecycle backend for agent building, events, and tool registration.
//! - [`Runtime`]: Agent registry, tool registry, and hook orchestration.
//! - [`model`]: Unified LLM interface types and traits.
//! - Agent event types: [`AgentEvent`], [`AgentStep`], [`AgentResponse`], [`AgentStopReason`].

pub use agent::{
    Agent, AgentBuilder, AgentConfig,
    event::{AgentEvent, AgentResponse, AgentStep, AgentStopReason},
    parse_agent_md,
    tool::{Dispatcher, Handler, ToolRegistry},
};
pub use memory::{Embedder, Memory, MemoryEntry, RecallOptions};
pub use runtime::{Runtime, hook::Hook};

mod agent;
pub mod memory;
pub mod model;
pub mod protocol;
mod runtime;
pub mod utils;
