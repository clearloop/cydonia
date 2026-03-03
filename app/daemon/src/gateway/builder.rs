//! Hook builder — constructs a fully-configured GatewayHook from DaemonConfig.

use crate::config;
use crate::gateway::GatewayHook;
use anyhow::Result;
use model::ProviderManager;
use runtime::{Hook, Runtime};
use std::path::Path;
use std::sync::Arc;

/// Build a fully-configured `Runtime` from config and directory.
///
/// Constructs GatewayHook with all backends (memory, skills, MCP),
/// creates the model provider separately, then wraps both in a Runtime
/// with loaded agents.
pub async fn build_runtime(
    config: &crate::DaemonConfig,
    config_dir: &Path,
) -> Result<Runtime<ProviderManager, GatewayHook>> {
    // Construct in-memory backend.
    let memory = memory::InMemory::new();
    tracing::info!("using in-memory backend");

    // Construct provider manager from config list.
    let manager = ProviderManager::from_configs(&config.models).await?;
    tracing::info!(
        "provider manager initialized — active model: {}",
        manager.active_model()
    );

    // Load skills.
    let skills_dir = config_dir.join(config::SKILLS_DIR);
    let skills = skill::SkillHandler::load(skills_dir)?;

    // Load MCP servers.
    let mcp_handler = mcp::McpHandler::load(config_dir.to_path_buf(), &config.mcp_servers).await;

    // Load cron jobs.
    let cron_dir = config_dir.join(config::CRON_DIR);
    let cron_handler = build_cron_handler(&cron_dir);

    // Build GatewayHook.
    let mut hook = GatewayHook::new(memory, skills, mcp_handler, cron_handler);

    // Register memory tools on the hook.
    register_memory_tools(&mut hook);

    // Wrap in Runtime — model and hook are separate.
    let runtime = Runtime::new(manager, Arc::new(hook));

    // Load agents from markdown files.
    let agents = crate::loader::load_agents_dir(&config_dir.join(config::AGENTS_DIR))?;
    for agent in agents {
        tracing::info!("registered agent '{}'", agent.name);
        runtime.add_agent(agent).await;
    }

    Ok(runtime)
}

/// Register memory-backed tools (remember, recall) on the GatewayHook.
fn register_memory_tools(hook: &mut GatewayHook) {
    let mem = hook.memory_arc();
    for mt in [
        memory::tools::remember(Arc::clone(&mem)),
        memory::tools::recall(mem),
    ] {
        hook.register(mt.tool, mt.handler);
    }
}

/// Load cron entries from disk and build a CronHandler.
fn build_cron_handler(cron_dir: &Path) -> wcron::CronHandler {
    let entries = match crate::loader::load_cron_dir(cron_dir) {
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
