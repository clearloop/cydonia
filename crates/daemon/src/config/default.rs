//! Default configuration and first-run scaffolding.

use crate::config::DaemonConfig;
use anyhow::{Context, Result};
use std::path::Path;
use wcore::paths::{AGENTS_DIR, DATA_DIR, SKILLS_DIR};

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
