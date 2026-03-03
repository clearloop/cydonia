//! Skill hot-reload feature.
//!
//! Skills are loaded from the configured skills directory at daemon startup
//! and injected into agent system prompts per-request. This module owns the
//! [`SkillRegistry`] behind a [`RwLock`] and supports hot-reloading via the
//! `ReloadSkills` protocol message.
//!
//! Because system prompts are rebuilt for every request from `AgentConfig +
//! matched skills`, a registry swap is all that's needed — the next request
//! to any agent automatically picks up the new skills.

use crate::loader::load_skills_dir;
use anyhow::Result;
use runtime::{SkillRegistry, SkillTier};
use std::path::PathBuf;
use tokio::sync::RwLock;

/// Daemon-side skill registry owner with hot-reload support.
pub struct SkillHandler {
    skills_dir: PathBuf,
    registry: RwLock<SkillRegistry>,
}

impl SkillHandler {
    /// Load skills from the given directory. Tolerates a missing directory
    /// by creating an empty registry.
    pub fn load(skills_dir: PathBuf) -> Result<Self> {
        let registry = if skills_dir.exists() {
            match load_skills_dir(&skills_dir, SkillTier::Workspace) {
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
            load_skills_dir(&self.skills_dir, SkillTier::Workspace)?
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
