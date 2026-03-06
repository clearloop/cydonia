//! Default configuration and first-run scaffolding.

use crate::config::DaemonConfig;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Agents subdirectory (contains *.md files).
pub const AGENTS_DIR: &str = "agents";
/// Skills subdirectory.
pub const SKILLS_DIR: &str = "skills";
/// Data subdirectory.
pub const DATA_DIR: &str = "data";

#[allow(dead_code)]
/// SQLite memory database filename.
pub const MEMORY_DB: &str = "memory.db";

/// Global configuration directory (`~/.walrus/`).
pub static GLOBAL_CONFIG_DIR: LazyLock<PathBuf> =
    LazyLock::new(|| dirs::home_dir().expect("no home directory").join(".walrus"));

/// Pinned socket path (`~/.walrus/walrus.sock`).
pub static SOCKET_PATH: LazyLock<PathBuf> = LazyLock::new(|| GLOBAL_CONFIG_DIR.join("walrus.sock"));

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
