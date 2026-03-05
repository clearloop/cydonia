//! Walrus MCP handler — MCP server management.

use crate::{config::McpServerConfig, hook::mcp::McpBridge};
use anyhow::Result;
use compact_str::CompactString;
use std::{pin::Pin, sync::Arc};
use tokio::sync::{Mutex, RwLock};
use wcore::{Handler, model::Tool};

/// MCP bridge owner.
///
/// Implements [`Hook`] — `tools` returns MCP server tools, `dispatch`
/// routes tool calls to MCP peers. `on_build_agent` is a no-op (MCP
/// does not modify agent configs).
pub struct McpHandler {
    bridge: RwLock<Arc<McpBridge>>,
    /// Serializes mutating operations (add/remove) to prevent races.
    op_lock: Mutex<()>,
}

impl McpHandler {
    /// Build a bridge from the given MCP server configs.
    async fn build_bridge(configs: &[McpServerConfig]) -> McpBridge {
        let bridge = McpBridge::new();
        for server_config in configs {
            let mut cmd = tokio::process::Command::new(&server_config.command);
            cmd.args(&server_config.args);
            for (k, v) in &server_config.env {
                cmd.env(k, v);
            }
            match bridge
                .connect_stdio_named(server_config.name.clone(), cmd)
                .await
            {
                Ok(tools) => {
                    tracing::info!(
                        "connected MCP server '{}' — {} tool(s)",
                        server_config.name,
                        tools.len()
                    );
                }
                Err(e) => {
                    tracing::warn!("failed to connect MCP server '{}': {e}", server_config.name);
                }
            }
        }
        bridge
    }

    /// Load MCP servers from the given configs at startup.
    pub async fn load(configs: &[McpServerConfig]) -> Self {
        let bridge = Self::build_bridge(configs).await;
        Self {
            bridge: RwLock::new(Arc::new(bridge)),
            op_lock: Mutex::new(()),
        }
    }

    /// Add an MCP server and connect it incrementally.
    pub async fn add(&self, server: McpServerConfig) -> Result<Vec<CompactString>> {
        let _guard = self.op_lock.lock().await;
        let name = server.name.clone();

        let mut cmd = tokio::process::Command::new(&server.command);
        cmd.args(&server.args);
        for (k, v) in &server.env {
            cmd.env(k, v);
        }

        let bridge = self.bridge.read().await.clone();
        let tools = bridge.connect_stdio_named(name, cmd).await?;
        Ok(tools)
    }

    /// Remove an MCP server. Returns the tool names that were removed.
    pub async fn remove(&self, name: &str) -> Result<Vec<CompactString>> {
        let _guard = self.op_lock.lock().await;

        let removed_tools: Vec<CompactString> = self
            .bridge
            .read()
            .await
            .list_servers()
            .await
            .into_iter()
            .filter(|(n, _)| n.as_str() == name)
            .flat_map(|(_, tools)| tools)
            .collect();

        let bridge = self.bridge.read().await.clone();
        bridge.remove_server(name).await;
        Ok(removed_tools)
    }

    /// List all connected servers with their tool names.
    pub async fn list(&self) -> Vec<(CompactString, Vec<CompactString>)> {
        self.bridge.read().await.list_servers().await
    }

    /// Get a clone of the current bridge Arc.
    pub async fn bridge(&self) -> Arc<McpBridge> {
        Arc::clone(&*self.bridge.read().await)
    }

    /// Try to get a clone of the current bridge Arc without blocking.
    pub fn try_bridge(&self) -> Option<Arc<McpBridge>> {
        self.bridge.try_read().ok().map(|g| Arc::clone(&*g))
    }

    /// Produce `(Tool, Handler)` pairs for all currently connected MCP tools.
    ///
    /// Each handler captures an `Arc<McpBridge>` and the tool name, routing
    /// calls through `bridge.call()`. Register the returned pairs on Runtime.
    pub async fn tool_handlers(&self) -> Vec<(Tool, Handler)> {
        let bridge = self.bridge().await;
        let tools = bridge.tools().await;
        tools
            .into_iter()
            .map(|tool| {
                let bridge = Arc::clone(&bridge);
                let name = tool.name.clone();
                let handler: Handler = Arc::new(move |args: String| {
                    let bridge = Arc::clone(&bridge);
                    let name = name.clone();
                    Box::pin(async move { bridge.call(&name, &args).await })
                        as Pin<Box<dyn std::future::Future<Output = String> + Send>>
                });
                (tool, handler)
            })
            .collect()
    }
}
