//! OS hook — filesystem tools for agents.
//!
//! [`OsHook`] registers `read` and `write` tool schemas and provides
//! async dispatch methods backed by `tokio::fs`. Paths must be absolute.

use wcore::{ToolRegistry, model::Tool};

/// OS hook providing filesystem read/write tools.
pub struct OsHook;

impl OsHook {
    /// Dispatch a `read` tool call — read file at absolute path.
    pub async fn dispatch_read(&self, args: &str) -> String {
        let parsed: serde_json::Value = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments: {e}"),
        };
        let path = match parsed["path"].as_str() {
            Some(p) => p,
            None => return "missing required field: path".to_owned(),
        };
        match tokio::fs::read_to_string(path).await {
            Ok(content) => content,
            Err(e) => format!("read failed: {e}"),
        }
    }

    /// Dispatch a `write` tool call — write content to file at absolute path.
    pub async fn dispatch_write(&self, args: &str) -> String {
        let parsed: serde_json::Value = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments: {e}"),
        };
        let path = match parsed["path"].as_str() {
            Some(p) => p,
            None => return "missing required field: path".to_owned(),
        };
        let content = parsed["content"].as_str().unwrap_or("");
        match tokio::fs::write(path, content).await {
            Ok(()) => format!("written: {path}"),
            Err(e) => format!("write failed: {e}"),
        }
    }
}

impl wcore::Hook for OsHook {
    fn on_register_tools(
        &self,
        registry: &mut ToolRegistry,
    ) -> impl std::future::Future<Output = ()> + Send {
        registry.insert(read_schema());
        registry.insert(write_schema());
        async {}
    }
}

fn read_schema() -> Tool {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "Absolute path to the file to read" }
        },
        "required": ["path"]
    });
    Tool {
        name: "read".into(),
        description: "Read the contents of a file at an absolute path.".into(),
        parameters: serde_json::from_value(schema).expect("valid static schema"),
        strict: false,
    }
}

fn write_schema() -> Tool {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "Absolute path to the file to write" },
            "content": { "type": "string", "description": "Content to write to the file" }
        },
        "required": ["path", "content"]
    });
    Tool {
        name: "write".into(),
        description: "Write content to a file at an absolute path. Creates or overwrites the file."
            .into(),
        parameters: serde_json::from_value(schema).expect("valid static schema"),
        strict: false,
    }
}
