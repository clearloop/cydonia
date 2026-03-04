//! Dispatcher trait, Handler type, and ToolRegistry.
//!
//! [`Dispatcher`] is a generic async trait for tool dispatch, passed to
//! `Agent::step()`. [`ToolRegistry`] is the canonical implementation — it
//! holds `(Tool, Handler)` pairs keyed by name and implements `Dispatcher`
//! directly, removing the need for `ClosureDispatcher` or `DispatchFn`.

use crate::model::Tool;
use anyhow::Result;
use compact_str::CompactString;
use std::{collections::BTreeMap, future::Future, pin::Pin, sync::Arc};

/// Type-erased async tool handler.
///
/// Takes JSON-encoded arguments, returns a result string. Captured state
/// (e.g. `Arc<M>`) must be `Send + Sync + 'static`.
pub type Handler =
    Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = String> + Send>> + Send + Sync>;

/// Generic tool dispatcher.
///
/// Passed as a method param to `Agent::step()`. Uses RPITIT for async
/// without boxing — callers monomorphize over concrete dispatcher types.
pub trait Dispatcher: Send + Sync {
    /// Dispatch a batch of tool calls. Each entry is `(method, params)`.
    ///
    /// Returns one result per call in the same order.
    fn dispatch(&self, calls: &[(&str, &str)]) -> impl Future<Output = Vec<Result<String>>> + Send;

    /// Return the tool schemas this dispatcher can handle.
    ///
    /// Agent uses this to populate `Request.tools` before calling the model.
    fn tools(&self) -> Vec<Tool>;
}

/// Registry of named tools with their async handlers.
///
/// Implements [`Dispatcher`] directly — `tools()` returns schemas and
/// `dispatch()` looks up handlers by name. Used as both the runtime's
/// shared store and the per-agent dispatcher snapshot.
#[derive(Default)]
pub struct ToolRegistry {
    tools: BTreeMap<CompactString, (Tool, Handler)>,
}

impl ToolRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a tool and its handler.
    pub fn insert(&mut self, tool: Tool, handler: Handler) {
        self.tools.insert(tool.name.clone(), (tool, handler));
    }

    /// Remove a tool by name. Returns `true` if it existed.
    pub fn remove(&mut self, name: &str) -> bool {
        self.tools.remove(name).is_some()
    }

    /// Check if a tool is registered.
    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// Iterate over all `(Tool, Handler)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&Tool, &Handler)> {
        self.tools.values().map(|(t, h)| (t, h))
    }

    /// Number of registered tools.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Build a filtered snapshot containing only the named tools.
    ///
    /// If `names` is empty, all tools are included. Used by runtime to
    /// build a per-agent dispatcher.
    pub fn filtered_snapshot(&self, names: &[CompactString]) -> Self {
        let tools = if names.is_empty() {
            self.tools.clone()
        } else {
            self.tools
                .iter()
                .filter(|(k, _)| names.iter().any(|n| n == *k))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        };
        Self { tools }
    }
}

impl Clone for ToolRegistry {
    fn clone(&self) -> Self {
        Self {
            tools: self.tools.clone(),
        }
    }
}

impl Dispatcher for ToolRegistry {
    fn dispatch(&self, calls: &[(&str, &str)]) -> impl Future<Output = Vec<Result<String>>> + Send {
        let owned: Vec<(String, String, Option<Handler>)> = calls
            .iter()
            .map(|(m, p)| {
                let handler = self.tools.get(*m).map(|(_, h)| Arc::clone(h));
                (m.to_string(), p.to_string(), handler)
            })
            .collect();

        async move {
            let mut results = Vec::with_capacity(owned.len());
            for (method, params, handler) in owned {
                let output = if let Some(h) = handler {
                    Ok(h(params).await)
                } else {
                    Ok(format!("function {method} not available"))
                };
                results.push(output);
            }
            results
        }
    }

    fn tools(&self) -> Vec<Tool> {
        self.tools.values().map(|(t, _)| t.clone()).collect()
    }
}
