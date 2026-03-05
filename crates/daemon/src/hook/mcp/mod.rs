//! Walrus MCP bridge — connects to MCP servers and dispatches tool calls.
//!
//! The [`McpBridge`] manages connections to MCP servers via the rmcp SDK,
//! converts tool definitions to walrus-core format, and routes tool calls.
//! [`McpHandler`] wraps the bridge with hot-reload and config persistence.
//! Produces `(Tool, Handler)` pairs for registration on Runtime.

use std::{pin::Pin, sync::Arc};
use wcore::Handler;
pub use {bridge::McpBridge, handler::McpHandler};

mod bridge;
mod handler;

// ── Handler (hot-reload + config persistence) ──────────────────────────

impl wcore::Hook for McpHandler {
    fn on_register_tools(
        &self,
        registry: &mut wcore::ToolRegistry,
    ) -> impl std::future::Future<Output = ()> + Send {
        // Capture the bridge Arc synchronously — bridge is initialized at construction
        // so try_bridge() succeeds unless an in-progress reload holds the write lock.
        let bridge = self.try_bridge();
        async move {
            let Some(bridge) = bridge else { return };
            let tools = bridge.tools().await;
            for tool in tools {
                let b = Arc::clone(&bridge);
                let name = tool.name.clone();
                let handler: Handler = Arc::new(move |args: String| {
                    let b = Arc::clone(&b);
                    let name = name.clone();
                    Box::pin(async move { b.call(&name, &args).await })
                        as Pin<Box<dyn std::future::Future<Output = String> + Send>>
                });
                registry.insert(tool, handler);
            }
        }
    }
}
