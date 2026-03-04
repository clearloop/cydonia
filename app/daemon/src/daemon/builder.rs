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
use std::sync::Arc;

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
        let mem = hook.memory_arc();
        let cron_jobs = hook.cron().jobs_arc();

        let mut runtime = Runtime::new(manager, hook);
        self.register_tools(&mut runtime, mem, cron_jobs).await?;
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
        let cron_handler = build_cron_handler(&cron_dir);

        DaemonHook::new(memory, skills, mcp_handler, cron_handler)
    }

    /// Register memory, cron, and MCP tools on the runtime.
    async fn register_tools(
        &self,
        runtime: &mut Runtime<ProviderManager, DaemonHook>,
        mem: Arc<memory::InMemory>,
        cron_jobs: Arc<tokio::sync::RwLock<Vec<wcron::CronJob>>>,
    ) -> Result<()> {
        // Memory tools (remember, recall).
        for mt in [
            memory::tools::remember(Arc::clone(&mem)),
            memory::tools::recall(mem),
        ] {
            runtime.register_tool(mt.tool, mt.handler).await;
        }

        // Cron tool (create_cron) — with event notification.
        let event_tx = self.event_tx.clone();
        let (cron_tool, cron_handler_fn) =
            wcron::hook::create_cron_handler_with_notify(cron_jobs, move |job| {
                let _ = event_tx.send(DaemonEvent::CronJobCreated(Box::new(job)));
            });
        runtime.register_tool(cron_tool, cron_handler_fn).await;

        // MCP tools — each MCP server tool becomes a registered handler.
        for (tool, handler) in runtime.hook().mcp().tool_handlers().await {
            runtime.register_tool(tool, handler).await;
        }

        Ok(())
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

/// Load cron entries from disk and build a CronHandler.
fn build_cron_handler(cron_dir: &Path) -> wcron::CronHandler {
    let entries = match crate::config::load_cron_dir(cron_dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("failed to load cron entries: {e}");
            return wcron::CronHandler::new(Vec::new());
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

    wcron::CronHandler::new(jobs)
}
