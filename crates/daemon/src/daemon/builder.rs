//! Runtime builder — constructs a fully-configured Runtime from DaemonConfig.

use crate::{
    DaemonConfig, config,
    daemon::event::{DaemonEvent, DaemonEventSender},
    hook::DaemonHook,
};
use anyhow::Result;
use model::ProviderManager;
use runtime::Runtime;
use std::path::{Path, PathBuf};

/// Step-by-step builder for the daemon's [`Runtime`].
///
/// Each logical phase (providers, hook, tools, agents) is a separate method.
/// Call [`Builder::build`] to execute them all in order.
pub(crate) struct Builder<'a> {
    config: &'a DaemonConfig,
    config_dir: &'a Path,
    event_tx: DaemonEventSender,
}

impl<'a> Builder<'a> {
    /// Create a new builder.
    pub fn new(
        config: &'a DaemonConfig,
        config_dir: &'a Path,
        event_tx: DaemonEventSender,
    ) -> Self {
        Self {
            config,
            config_dir,
            event_tx,
        }
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
        let manager = ProviderManager::from_configs(&self.config.models).await?;
        tracing::info!(
            "provider manager initialized — active model: {}",
            manager.active_model()
        );
        Ok(manager)
    }

    /// Build the daemon hook with all backends (memory, skills, MCP, cron).
    async fn build_hook(&self) -> DaemonHook {
        let memory = memory::InMemory::new();
        tracing::info!("using in-memory backend");

        let skills_dir = self.config_dir.join(config::SKILLS_DIR);
        let skills = skill::SkillHandler::load(skills_dir).unwrap_or_else(|e| {
            tracing::warn!("failed to load skills: {e}");
            skill::SkillHandler::load(PathBuf::new()).expect("empty skill handler")
        });

        let mcp_handler =
            mcp::McpHandler::load(self.config_dir.to_path_buf(), &self.config.mcp_servers).await;

        let cron_dir = self.config_dir.join(config::CRON_DIR);
        let event_tx = self.event_tx.clone();
        let cron_handler = build_cron_handler(&cron_dir, move |job| {
            let _ = event_tx.send(DaemonEvent::CronJobCreated(Box::new(job)));
        });

        DaemonHook::new(memory, skills, mcp_handler, cron_handler)
    }

    /// Load agents from markdown files and add them to the runtime.
    fn load_agents(&self, runtime: &mut Runtime<ProviderManager, DaemonHook>) -> Result<()> {
        let agents = crate::config::load_agents_dir(&self.config_dir.join(config::AGENTS_DIR))?;
        for agent in agents {
            tracing::info!("registered agent '{}'", agent.name);
            runtime.add_agent(agent);
        }
        Ok(())
    }
}

/// Load cron entries from disk and build a CronHandler with the given creation callback.
fn build_cron_handler<F: Fn(wcron::CronJob) + Send + Sync + 'static>(
    cron_dir: &Path,
    on_create: F,
) -> wcron::CronHandler {
    let entries = match crate::config::load_cron_dir(cron_dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("failed to load cron entries: {e}");
            return wcron::CronHandler::new(Vec::new(), on_create);
        }
    };

    let mut jobs = Vec::new();
    for entry in entries {
        match wcron::CronJob::new(entry.name, &entry.schedule, entry.agent, entry.message) {
            Ok(job) => {
                tracing::info!("registered cron job '{}' → agent '{}'", job.name, job.agent);
                jobs.push(job);
            }
            Err(e) => {
                tracing::warn!("skipping cron entry: {e}");
            }
        }
    }

    wcron::CronHandler::new(jobs, on_create)
}
