//! Memory tool schemas and handlers for agent tool registration.

use crate::{Memory, RecallOptions};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use wcore::model::Tool;

/// Async handler function type — matches `runtime::Handler` inner signature.
type HandlerFn = Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = String> + Send>> + Send + Sync>;

/// Tool schema + handler pair, ready to register on a hook.
pub struct MemoryTool {
    pub tool: Tool,
    pub handler: HandlerFn,
}

/// Build the `remember` tool + handler for the given memory backend.
pub fn remember<M: Memory + 'static>(mem: Arc<M>) -> MemoryTool {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "key": { "type": "string", "description": "Memory key" },
            "value": { "type": "string", "description": "Value to remember" }
        },
        "required": ["key", "value"]
    });
    let tool = Tool {
        name: "remember".into(),
        description: "Store a key-value pair in memory.".into(),
        parameters: serde_json::from_value(schema).unwrap(),
        strict: false,
    };
    let handler: HandlerFn = Arc::new(move |args| {
        let mem = Arc::clone(&mem);
        Box::pin(async move {
            let parsed: serde_json::Value = match serde_json::from_str(&args) {
                Ok(v) => v,
                Err(e) => return format!("invalid arguments: {e}"),
            };
            let key = parsed["key"].as_str().unwrap_or("");
            let value = parsed["value"].as_str().unwrap_or("");
            match mem.store(key.to_owned(), value.to_owned()).await {
                Ok(()) => format!("remembered: {key}"),
                Err(e) => format!("failed to store: {e}"),
            }
        })
    });
    MemoryTool { tool, handler }
}

/// Build the `recall` tool + handler for the given memory backend.
pub fn recall<M: Memory + 'static>(mem: Arc<M>) -> MemoryTool {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "query": { "type": "string", "description": "Search query for relevant memories" },
            "limit": { "type": "integer", "description": "Maximum number of results (default: 10)" }
        },
        "required": ["query"]
    });
    let tool = Tool {
        name: "recall".into(),
        description: "Search memory for entries relevant to a query.".into(),
        parameters: serde_json::from_value(schema).unwrap(),
        strict: false,
    };
    let handler: HandlerFn = Arc::new(move |args| {
        let mem = Arc::clone(&mem);
        Box::pin(async move {
            let parsed: serde_json::Value = match serde_json::from_str(&args) {
                Ok(v) => v,
                Err(e) => return format!("invalid arguments: {e}"),
            };
            let query = parsed["query"].as_str().unwrap_or("");
            let limit = parsed["limit"].as_u64().unwrap_or(10) as usize;
            let options = RecallOptions {
                limit,
                ..Default::default()
            };
            match mem.recall(query, options).await {
                Ok(entries) if entries.is_empty() => "no memories found".to_owned(),
                Ok(entries) => {
                    let mut out = String::new();
                    for entry in &entries {
                        out.push_str(&format!("{}: {}\n", entry.key, entry.value));
                    }
                    out
                }
                Err(e) => format!("recall failed: {e}"),
            }
        })
    });
    MemoryTool { tool, handler }
}
