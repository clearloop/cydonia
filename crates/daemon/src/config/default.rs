//! Default configuration and first-run scaffolding.

use crate::config::DaemonConfig;
use anyhow::{Context, Result};
use std::path::Path;
use wcore::paths::{AGENTS_DIR, DATA_DIR, SKILLS_DIR, WORK_DIR};

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

/// Scaffold the workspace sandbox directory and optional symlink.
///
/// Creates `~/.openwalrus/work/` and, if `work_dir` is `Some`, creates a symlink
/// at that path pointing to the sandbox root.
pub fn scaffold_work_dir(config_dir: &Path, work_dir: Option<&Path>) -> Result<()> {
    let sandbox = config_dir.join(WORK_DIR);
    std::fs::create_dir_all(&sandbox).context("failed to create work directory")?;

    if let Some(link_path) = work_dir {
        if link_path.exists() {
            // Check if it's already a correct symlink.
            if link_path.is_symlink() {
                let target =
                    std::fs::read_link(link_path).context("failed to read symlink target")?;
                if target == sandbox {
                    return Ok(());
                }
                // Wrong target — remove and recreate.
                std::fs::remove_file(link_path).context("failed to remove stale symlink")?;
            } else {
                anyhow::bail!(
                    "work_dir path {} exists and is not a symlink",
                    link_path.display()
                );
            }
        }
        if let Some(parent) = link_path.parent() {
            std::fs::create_dir_all(parent).context("failed to create work_dir parent")?;
        }
        std::os::unix::fs::symlink(&sandbox, link_path)
            .with_context(|| format!("failed to create symlink at {}", link_path.display()))?;
    }

    Ok(())
}
