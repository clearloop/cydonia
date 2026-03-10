//! Configuration loading and first-run scaffolding.
//!
//! Handles filesystem I/O: reads agent directories, sorts entries, delegates
//! parsing to [`wcore::parse_agent_md`]. Also scaffolds the config directory
//! structure on first run.

use crate::config::DaemonConfig;
use anyhow::{Context, Result};
use std::path::Path;
use wcore::paths::{AGENTS_DIR, DATA_DIR, SKILLS_DIR};

/// Load all agent markdown files from a directory.
///
/// Each `.md` file is parsed with [`wcore::parse_agent_md`]. Non-`.md` files
/// are silently skipped. Entries are sorted by filename for deterministic
/// ordering. Returns an empty vec if the directory does not exist.
pub fn load_agents_dir(path: &Path) -> Result<Vec<wcore::AgentConfig>> {
    if !path.exists() {
        tracing::warn!("agent directory does not exist: {}", path.display());
        return Ok(Vec::new());
    }

    let mut entries: Vec<_> = std::fs::read_dir(path)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
        .collect();
    entries.sort_by_key(|e| e.file_name());

    let mut agents = Vec::with_capacity(entries.len());
    for entry in entries {
        let content = std::fs::read_to_string(entry.path())?;
        agents.push(wcore::parse_agent_md(&content)?);
    }

    Ok(agents)
}

/// Scaffold the full config directory structure on first run.
///
/// Creates subdirectories (agents, skills, data) and writes a default walrus.toml.
pub fn scaffold_config_dir(config_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(config_dir.join(AGENTS_DIR))
        .context("failed to create agents directory")?;
    std::fs::create_dir_all(config_dir.join(SKILLS_DIR))
        .context("failed to create skills directory")?;
    std::fs::create_dir_all(config_dir.join(DATA_DIR))
        .context("failed to create data directory")?;

    let gateway_toml = config_dir.join("walrus.toml");
    let contents = toml::to_string_pretty(&DaemonConfig::default())
        .context("failed to serialize default config")?;
    std::fs::write(&gateway_toml, contents)
        .with_context(|| format!("failed to write {}", gateway_toml.display()))?;

    Ok(())
}
