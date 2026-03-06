//! Memory tool schema constructors.
//!
//! Returns [`Tool`] schema definitions for `remember` and `recall`.
//! No handlers — dispatch is handled statically by the daemon event loop.

use crate::model::Tool;

/// Build the `remember` tool schema.
pub fn remember_schema() -> Tool {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "key": { "type": "string", "description": "Memory key" },
            "value": { "type": "string", "description": "Value to remember" }
        },
        "required": ["key", "value"]
    });
    Tool {
        name: "remember".into(),
        description: "Store a key-value pair in memory.".into(),
        parameters: serde_json::from_value(schema).unwrap(),
        strict: false,
    }
}

/// Build the `recall` tool schema.
pub fn recall_schema() -> Tool {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "query": { "type": "string", "description": "Search query for relevant memories" },
            "limit": { "type": "integer", "description": "Maximum number of results (default: 10)" }
        },
        "required": ["query"]
    });
    Tool {
        name: "recall".into(),
        description: "Search memory for entries relevant to a query.".into(),
        parameters: serde_json::from_value(schema).unwrap(),
        strict: false,
    }
}
