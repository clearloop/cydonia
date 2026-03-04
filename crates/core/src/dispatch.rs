//! Dispatcher trait and concrete implementations for tool dispatch.
//!
//! The [`Dispatcher`] trait provides batch tool dispatch with RPITIT async.
//! [`ClosureDispatcher`] is a concrete implementation constructed by Runtime
//! with pre-filtered tools and a type-erased dispatch function.

use crate::model::Tool;
use anyhow::Result;
use std::{future::Future, pin::Pin, sync::Arc};

/// Generic tool dispatcher.
///
/// Passed as a method param to `Agent::step()`. Implementations wrap a tool
/// registry, MCP bridge, or any other tool backend. Uses RPITIT for async
/// without boxing — callers monomorphize over concrete dispatcher types.
pub trait Dispatcher: Send + Sync {
    /// Dispatch a batch of tool calls. Each entry is `(method, params)`.
    ///
    /// Returns one result per call in the same order. Implementations may
    /// execute calls concurrently.
    fn dispatch(&self, calls: &[(&str, &str)]) -> impl Future<Output = Vec<Result<String>>> + Send;

    /// Return the tool schemas this dispatcher can handle.
    ///
    /// Agent uses this to populate `Request.tools` before calling the model.
    fn tools(&self) -> Vec<Tool>;
}

/// Type-erased dispatch function.
pub type DispatchFn = Arc<
    dyn Fn(Vec<(String, String)>) -> Pin<Box<dyn Future<Output = Vec<Result<String>>> + Send>>
        + Send
        + Sync,
>;

/// Concrete dispatcher constructed by Runtime with pre-filtered tools and
/// a type-erased dispatch closure.
///
/// Created once per `send_to`/`stream_to` call. The single `dyn` dispatch
/// is negligible compared to the LLM and tool IO on the hot path.
pub struct ClosureDispatcher {
    tools: Vec<Tool>,
    dispatch_fn: DispatchFn,
}

impl ClosureDispatcher {
    /// Create a new dispatcher with the given tools and dispatch function.
    pub fn new(tools: Vec<Tool>, dispatch_fn: DispatchFn) -> Self {
        Self { tools, dispatch_fn }
    }
}

impl Dispatcher for ClosureDispatcher {
    fn dispatch(&self, calls: &[(&str, &str)]) -> impl Future<Output = Vec<Result<String>>> + Send {
        let owned: Vec<(String, String)> = calls
            .iter()
            .map(|(m, p)| (m.to_string(), p.to_string()))
            .collect();
        (self.dispatch_fn)(owned)
    }

    fn tools(&self) -> Vec<Tool> {
        self.tools.clone()
    }
}
