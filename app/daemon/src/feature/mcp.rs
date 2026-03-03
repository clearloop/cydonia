//! MCP hot-reload feature.
//!
//! MCP servers are connected at daemon startup from `walrus.toml` config and
//! can be dynamically added, removed, or reloaded via protocol messages. This
//! module owns the [`McpBridge`] behind a [`RwLock`] and supports hot-reloading.
//!
//! Because `RuntimeDispatcher` clones the `Arc<McpBridge>` at agent creation,
//! a bridge swap only affects new requests — agents mid-execution continue
//! using the old bridge until they finish.
//!
//! All mutating operations (`add`, `remove`, `reload`) are serialized via an
//! operation lock to prevent concurrent disk read-modify-write races.

use anyhow::{Context, Result};
use compact_str::CompactString;
use runtime::McpBridge;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use crate::config::{DaemonConfig, McpServerConfig};

/// Daemon-side MCP bridge owner with hot-reload support.
pub struct McpHandler {
    config_dir: PathBuf,
    bridge: RwLock<Arc<McpBridge>>,
    /// Serializes mutating operations (add/remove/reload) to prevent
    /// concurrent disk read-modify-write races on walrus.toml.
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

    /// Persist a `DaemonConfig` to `walrus.toml`.
    fn write_config(&self, config: &DaemonConfig) -> Result<()> {
        let config_path = self.config_dir.join("walrus.toml");
        let contents =
            toml::to_string_pretty(config).context("failed to serialize daemon config")?;
        std::fs::write(&config_path, &contents)
            .with_context(|| format!("failed to write {}", config_path.display()))?;
        Ok(())
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

    /// Reload MCP servers from `walrus.toml`. Builds a fresh bridge and swaps
    /// atomically. Returns the list of `(server_name, tool_names)` pairs.
    pub async fn reload(&self) -> Result<Vec<(CompactString, Vec<CompactString>)>> {
        let _guard = self.op_lock.lock().await;
        let config_path = self.config_dir.join("walrus.toml");
        let config = DaemonConfig::load(&config_path)
            .context("failed to load walrus.toml for MCP reload")?;
        let bridge = Self::build_bridge(&config.mcp_servers).await;
        let servers = bridge.list_servers().await;
        *self.bridge.write().await = Arc::new(bridge);
        Ok(servers)
    }

    /// Add an MCP server to `walrus.toml` and connect it incrementally.
    pub async fn add(&self, server: McpServerConfig) -> Result<Vec<CompactString>> {
        let _guard = self.op_lock.lock().await;
        let config_path = self.config_dir.join("walrus.toml");
        let mut config = DaemonConfig::load(&config_path)?;
        let name = server.name.clone();

        // Build command before moving server into config.
        let mut cmd = tokio::process::Command::new(&server.command);
        cmd.args(&server.args);
        for (k, v) in &server.env {
            cmd.env(k, v);
        }

        // Persist first — if writing fails, no in-memory changes occur.
        config.mcp_servers.push(server);
        self.write_config(&config)?;

        // Connect the new server incrementally (no full rebuild).
        let bridge = self.bridge.read().await.clone();
        let tools = bridge.connect_stdio_named(name, cmd).await?;
        Ok(tools)
    }

    /// Remove an MCP server from `walrus.toml` and reload.
    /// Returns the tool names that were removed.
    pub async fn remove(&self, name: &str) -> Result<Vec<CompactString>> {
        let _guard = self.op_lock.lock().await;
        let config_path = self.config_dir.join("walrus.toml");
        let mut config = DaemonConfig::load(&config_path)?;

        // Capture tools before removal from the current bridge state.
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

        // Persist first — if writing fails, no in-memory changes occur.
        config.mcp_servers.retain(|s| s.name.as_str() != name);
        self.write_config(&config)?;

        // Build a fresh bridge without the removed server.
        let bridge = Self::build_bridge(&config.mcp_servers).await;
        *self.bridge.write().await = Arc::new(bridge);
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
}
