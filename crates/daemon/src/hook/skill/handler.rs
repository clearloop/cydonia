//! Walrus skill handler — hot-reload and config persistence.

use crate::hook::skill::{SkillRegistry, SkillTier, loader};
use anyhow::Result;
use std::path::PathBuf;
use tokio::sync::RwLock;

/// Skill registry owner with hot-reload support.
///
/// Implements [`Hook`] — `on_build_agent` enriches the system prompt with
/// matching skills based on agent tags. Tools and dispatch are no-ops
/// (skills inject behavior via prompt, not via tools).
pub struct SkillHandler {
    skills_dir: PathBuf,

    /// The skill registry.
    pub registry: RwLock<SkillRegistry>,
}

impl SkillHandler {
    /// Load skills from the given directory. Tolerates a missing directory
    /// by creating an empty registry.
    pub fn load(skills_dir: PathBuf) -> Result<Self> {
        let registry = if skills_dir.exists() {
            match loader::load_skills_dir(&skills_dir, SkillTier::Workspace) {
                Ok(r) => {
                    tracing::info!("loaded {} skill(s)", r.len());
                    r
                }
                Err(e) => {
                    tracing::warn!("could not load skills from {}: {e}", skills_dir.display());
                    SkillRegistry::new()
                }
            }
        } else {
            SkillRegistry::new()
        };
        Ok(Self {
            skills_dir,
            registry: RwLock::new(registry),
        })
    }

    /// Reload skills from disk, replacing the entire registry.
    /// Returns the number of skills loaded.
    pub async fn reload(&self) -> Result<usize> {
        let registry = if self.skills_dir.exists() {
            loader::load_skills_dir(&self.skills_dir, SkillTier::Workspace)?
        } else {
            SkillRegistry::new()
        };
        let count = registry.len();
        *self.registry.write().await = registry;
        Ok(count)
    }

    /// Access the skill registry lock for read.
    pub fn registry(&self) -> &RwLock<SkillRegistry> {
        &self.registry
    }
}
