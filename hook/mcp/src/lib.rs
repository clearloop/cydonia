//! Walrus MCP bridge — connects to MCP servers and dispatches tool calls.
//!
//! The [`McpBridge`] manages connections to MCP servers via the rmcp SDK,
//! converts tool definitions to walrus-core format, and routes tool calls.
//! [`McpHandler`] wraps the bridge with hot-reload and config persistence.
//! Produces `(Tool, Handler)` pairs for registration on Runtime.

use anyhow::Result;
use compact_str::CompactString;
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, RawContent},
    service::{RoleClient, RunningService},
    transport::TokioChildProcess,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf, pin::Pin, sync::Arc};
use tokio::sync::{Mutex, RwLock};
use wcore::{Handler, model::Tool};

// ── Config ─────────────────────────────────────────────────────────────

/// MCP server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Server name.
    pub name: CompactString,
    /// Command to spawn.
    pub command: String,
    /// Command arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Auto-restart on failure.
    #[serde(default = "default_true")]
    pub auto_restart: bool,
}

fn default_true() -> bool {
    true
}

// ── Bridge ─────────────────────────────────────────────────────────────

/// A connected MCP server peer with its tool names.
struct ConnectedPeer {
    name: CompactString,
    peer: RunningService<RoleClient, ()>,
    tools: Vec<CompactString>,
}

/// Bridge to one or more MCP servers via the rmcp SDK.
///
/// Converts MCP tool definitions to walrus-core [`Tool`] schemas and
/// dispatches tool calls through the protocol.
pub struct McpBridge {
    peers: Mutex<Vec<ConnectedPeer>>,
    /// Cache of converted tools keyed by name.
    tool_cache: Mutex<BTreeMap<CompactString, Tool>>,
}

impl Default for McpBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl McpBridge {
    /// Create a new empty bridge with no connected peers.
    pub fn new() -> Self {
        Self {
            peers: Mutex::new(Vec::new()),
            tool_cache: Mutex::new(BTreeMap::new()),
        }
    }

    /// Connect to an MCP server by spawning a child process.
    pub async fn connect_stdio(&self, command: tokio::process::Command) -> Result<()> {
        let name = command
            .as_std()
            .get_program()
            .to_string_lossy()
            .into_owned();
        self.connect_stdio_named(CompactString::from(name), command)
            .await?;
        Ok(())
    }

    /// Connect to a named MCP server by spawning a child process.
    ///
    /// Returns the list of tool names registered by this server.
    pub async fn connect_stdio_named(
        &self,
        name: CompactString,
        command: tokio::process::Command,
    ) -> Result<Vec<CompactString>> {
        let transport = TokioChildProcess::new(command)?;
        let peer: RunningService<RoleClient, ()> = ().serve(transport).await?;

        let mcp_tools = peer.list_all_tools().await?;
        let mut tool_names = Vec::with_capacity(mcp_tools.len());

        {
            let mut cache = self.tool_cache.lock().await;
            for mcp_tool in &mcp_tools {
                let walrus_tool = convert_tool(mcp_tool);
                tool_names.push(walrus_tool.name.clone());
                cache.insert(walrus_tool.name.clone(), walrus_tool);
            }
        }

        self.peers.lock().await.push(ConnectedPeer {
            name,
            peer,
            tools: tool_names.clone(),
        });

        Ok(tool_names)
    }

    /// Disconnect all peers and clear the tool cache.
    pub async fn clear(&self) {
        self.peers.lock().await.clear();
        self.tool_cache.lock().await.clear();
    }

    /// Remove a server by name, returning the tool names that were removed.
    pub async fn remove_server(&self, name: &str) -> Vec<CompactString> {
        let mut peers = self.peers.lock().await;
        let mut removed_tools = Vec::new();

        peers.retain(|p| {
            if p.name.as_str() == name {
                removed_tools.extend(p.tools.iter().cloned());
                false
            } else {
                true
            }
        });

        let mut cache = self.tool_cache.lock().await;
        for tool_name in &removed_tools {
            cache.remove(tool_name);
        }

        removed_tools
    }

    /// List all connected servers with their tool names.
    pub async fn list_servers(&self) -> Vec<(CompactString, Vec<CompactString>)> {
        self.peers
            .lock()
            .await
            .iter()
            .map(|p| (p.name.clone(), p.tools.clone()))
            .collect()
    }

    /// List all tools available across all connected peers.
    pub async fn tools(&self) -> Vec<Tool> {
        self.tool_cache.lock().await.values().cloned().collect()
    }

    /// Try to list tools without blocking. Returns empty if the lock is held.
    pub fn try_tools(&self) -> Vec<Tool> {
        self.tool_cache
            .try_lock()
            .map(|cache| cache.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Call a tool by name, routing to the correct peer.
    pub async fn call(&self, name: &str, arguments: &str) -> String {
        let peers = self.peers.lock().await;
        let connected = peers
            .iter()
            .find(|p| p.tools.iter().any(|t| t.as_str() == name));

        let Some(connected) = connected else {
            return format!("mcp tool '{name}' not available");
        };

        let args: Option<serde_json::Map<String, serde_json::Value>> = if arguments.is_empty() {
            None
        } else {
            match serde_json::from_str(arguments) {
                Ok(v) => Some(v),
                Err(e) => return format!("invalid tool arguments: {e}"),
            }
        };

        let params = CallToolRequestParams {
            meta: None,
            name: name.to_string().into(),
            arguments: args,
            task: None,
        };

        match connected.peer.call_tool(params).await {
            Ok(result) => {
                if result.is_error == Some(true) {
                    format!("mcp tool error: {}", extract_text(&result.content))
                } else {
                    extract_text(&result.content)
                }
            }
            Err(e) => format!("mcp call failed: {e}"),
        }
    }
}

/// Convert an rmcp Tool to a walrus-core Tool.
fn convert_tool(mcp_tool: &rmcp::model::Tool) -> Tool {
    let schema_value =
        serde_json::to_value(mcp_tool.input_schema.as_ref()).unwrap_or(serde_json::json!({}));
    let parameters: schemars::Schema =
        serde_json::from_value(schema_value).unwrap_or_else(|_| schemars::schema_for!(String));

    Tool {
        name: CompactString::from(mcp_tool.name.as_ref()),
        description: mcp_tool
            .description
            .as_ref()
            .map(|d| d.to_string())
            .unwrap_or_default(),
        parameters,
        strict: false,
    }
}

/// Extract text content from MCP Content items.
fn extract_text(content: &[rmcp::model::Content]) -> String {
    content
        .iter()
        .filter_map(|c| match &c.raw {
            RawContent::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ── Handler (hot-reload + config persistence) ──────────────────────────

/// MCP bridge owner with hot-reload and config persistence.
///
/// Implements [`Hook`] — `tools` returns MCP server tools, `dispatch`
/// routes tool calls to MCP peers. `on_build_agent` is a no-op (MCP
/// does not modify agent configs).
pub struct McpHandler {
    config_dir: PathBuf,
    bridge: RwLock<Arc<McpBridge>>,
    /// Serializes mutating operations (add/remove/reload) to prevent
    /// concurrent disk read-modify-write races.
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
    pub async fn load(config_dir: PathBuf, configs: &[McpServerConfig]) -> Self {
        let bridge = Self::build_bridge(configs).await;
        Self {
            config_dir,
            bridge: RwLock::new(Arc::new(bridge)),
            op_lock: Mutex::new(()),
        }
    }

    /// Reload MCP servers from a config file. Builds a fresh bridge and
    /// swaps atomically. Returns the list of `(server_name, tool_names)`.
    pub async fn reload(
        &self,
        load_configs: impl FnOnce(&std::path::Path) -> Result<Vec<McpServerConfig>>,
    ) -> Result<Vec<(CompactString, Vec<CompactString>)>> {
        let _guard = self.op_lock.lock().await;
        let config_path = self.config_dir.join("walrus.toml");
        let configs = load_configs(&config_path)?;
        let bridge = Self::build_bridge(&configs).await;
        let servers = bridge.list_servers().await;
        *self.bridge.write().await = Arc::new(bridge);
        Ok(servers)
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
