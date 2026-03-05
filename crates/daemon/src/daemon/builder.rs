//! Runtime builder — constructs a fully-configured Runtime from DaemonConfig.

use crate::{DaemonConfig, config, hook, hook::DaemonHook};
use anyhow::Result;
use model::ProviderManager;
use std::path::{Path, PathBuf};
use wcore::Runtime;

const SYSTEM_AGENT: &str = include_str!("../../prompts/system.md");

/// Step-by-step builder for the daemon's [`Runtime`].
///
/// Each logical phase (providers, hook, agents) is a separate method.
/// Call [`Builder::build`] to execute them all in order.
pub(crate) struct Builder<'a> {
    config: &'a DaemonConfig,
    config_dir: &'a Path,
}

impl<'a> Builder<'a> {
    /// Create a new builder.
    pub fn new(config: &'a DaemonConfig, config_dir: &'a Path) -> Self {
        Self { config, config_dir }
    }

    /// Build the fully-configured runtime.
    pub async fn build(self) -> Result<Runtime<ProviderManager, DaemonHook>> {
        let manager = self.build_providers().await?;
        let hook = self.build_hook().await;
        let mut runtime = Runtime::new(manager, hook).await;
        self.load_agents(&mut runtime)?;
        Ok(runtime)
    }

    /// Construct the provider manager from config.
    async fn build_providers(&self) -> Result<ProviderManager> {
        let models = self.config.models.values().cloned().collect::<Vec<_>>();
        let manager = ProviderManager::from_configs(&models).await?;
        tracing::info!(
            "provider manager initialized — active model: {}",
            manager.active_model()
        );
        Ok(manager)
    }

    /// Build the daemon hook with all backends (memory, skills, MCP).
    async fn build_hook(&self) -> DaemonHook {
        let memory = memory::InMemory::new();
        tracing::info!("using in-memory backend");

        let skills_dir = self.config_dir.join(config::SKILLS_DIR);
        let skills = hook::skill::SkillHandler::load(skills_dir).unwrap_or_else(|e| {
            tracing::warn!("failed to load skills: {e}");
            hook::skill::SkillHandler::load(PathBuf::new()).expect("empty skill handler")
        });

        let mcp_servers = self
            .config
            .mcp_servers
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mcp_handler =
            hook::mcp::McpHandler::load(self.config_dir.to_path_buf(), &mcp_servers).await;

        DaemonHook::new(memory, skills, mcp_handler)
    }

    /// Load agents from markdown files and add them to the runtime.
    fn load_agents(&self, runtime: &mut Runtime<ProviderManager, DaemonHook>) -> Result<()> {
        let agents = crate::config::load_agents_dir(&self.config_dir.join(config::AGENTS_DIR))?;
        runtime.add_agent(wcore::parse_agent_md(SYSTEM_AGENT)?);
        for agent in agents {
            tracing::info!("registered agent '{}'", agent.name);
            runtime.add_agent(agent);
        }
        Ok(())
    }
}
