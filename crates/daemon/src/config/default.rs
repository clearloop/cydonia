//! Default configuration and first-run scaffolding.

use crate::config::{AgentConfig, DaemonConfig};
use anyhow::{Context, Result};
use model::ProviderConfig;
use std::path::{Path, PathBuf};

/// Agents subdirectory (contains *.md files).
pub const AGENTS_DIR: &str = "agents";
/// Skills subdirectory.
pub const SKILLS_DIR: &str = "skills";
/// Cron subdirectory (contains *.md files).
pub const CRON_DIR: &str = "cron";
/// Data subdirectory.
pub const DATA_DIR: &str = "data";

#[allow(dead_code)]
/// SQLite memory database filename.
pub const MEMORY_DB: &str = "memory.db";

/// Resolve the global configuration directory (`~/.walrus/`).
pub fn global_config_dir() -> PathBuf {
    dirs::home_dir().expect("no home directory").join(".walrus")
}

/// Pinned socket path (`~/.walrus/walrus.sock`).
pub fn socket_path() -> PathBuf {
    global_config_dir().join("walrus.sock")
}

#[cfg(not(feature = "local"))]
impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            models: [(
                "deepseek-chat".into(),
                ProviderConfig {
                    model: "deepseek-chat".into(),
                    api_key: None,
                    base_url: None,
                    loader: None,
                    quantization: None,
                    chat_template: None,
                },
            )]
            .into(),
            channels: Default::default(),
            mcp_servers: Default::default(),
            agents: AgentConfig::default(),
        }
    }
}

#[cfg(feature = "local")]
impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            models: [(
                "local".into(),
                ProviderConfig {
                    model: "Qwen/Qwen3-4B".into(),
                    api_key: None,
                    base_url: None,
                    loader: Some(model::Loader::Text),
                    quantization: None,
                    chat_template: None,
                },
            )]
            .into(),
            channels: Default::default(),
            mcp_servers: Default::default(),
            agents: AgentConfig::default(),
        }
    }
}

/// Default agent markdown content for first-run scaffold.
pub const DEFAULT_AGENT_MD: &str = r#"---
name: assistant
description: A helpful assistant
tools:
  - remember
---

You are a helpful assistant. Be concise.
"#;

/// Scaffold the full config directory structure on first run.
///
/// Creates subdirectories (agents, skills, cron, data), writes a default
/// walrus.toml and a default assistant agent markdown file.
pub fn scaffold_config_dir(config_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(config_dir.join(AGENTS_DIR))
        .context("failed to create agents directory")?;
    std::fs::create_dir_all(config_dir.join(SKILLS_DIR))
        .context("failed to create skills directory")?;
    std::fs::create_dir_all(config_dir.join(CRON_DIR))
        .context("failed to create cron directory")?;
    std::fs::create_dir_all(config_dir.join(DATA_DIR))
        .context("failed to create data directory")?;

    let gateway_toml = config_dir.join("walrus.toml");
    let contents = toml::to_string_pretty(&DaemonConfig::default())
        .context("failed to serialize default config")?;
    std::fs::write(&gateway_toml, contents)
        .with_context(|| format!("failed to write {}", gateway_toml.display()))?;

    let agent_path = config_dir.join(AGENTS_DIR).join("assistant.md");
    std::fs::write(&agent_path, DEFAULT_AGENT_MD)
        .with_context(|| format!("failed to write {}", agent_path.display()))?;

    Ok(())
}
