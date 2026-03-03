//! Hook builder — constructs a fully-configured GatewayHook from DaemonConfig.

use crate::MemoryBackend;
use crate::config;
use crate::feature::mcp::McpHandler;
use crate::feature::skill::SkillHandler;
use crate::gateway::GatewayHook;
use anyhow::Result;
use memory::Memory;
use model::ProviderManager;
use runtime::{Runtime, Tool};
use std::path::Path;
use std::sync::Arc;

/// Build a fully-configured `Runtime<GatewayHook>` from config and directory.
///
/// Constructs GatewayHook with all backends (model, memory, skills, MCP),
/// then wraps it in a Runtime with loaded agents.
pub async fn build_runtime(
    config: &crate::DaemonConfig,
    config_dir: &Path,
) -> Result<Runtime<GatewayHook>> {
    // Construct in-memory backend.
    let memory = MemoryBackend::in_memory();
    tracing::info!("using in-memory backend");

    // Construct provider manager from config list.
    let manager = ProviderManager::from_configs(&config.models).await?;
    tracing::info!(
        "provider manager initialized — active model: {}",
        manager.active_model()
    );

    // Load skills.
    let skills_dir = config_dir.join(config::SKILLS_DIR);
    let skills = SkillHandler::load(skills_dir)?;

    // Load MCP servers.
    let mcp = McpHandler::load(config_dir.to_path_buf(), &config.mcp_servers).await;

    // Build GatewayHook.
    let mut hook = GatewayHook::new(manager, memory, skills, mcp);

    // Register memory tools on the hook.
    register_memory_tools(&mut hook);

    // Wrap in Runtime.
    let runtime = Runtime::new(Arc::new(hook));

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
    // remember tool
    {
        let mem = hook.memory_arc();
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "key": { "type": "string", "description": "Memory key" },
                "value": { "type": "string", "description": "Value to remember" }
            },
            "required": ["key", "value"]
        });
        let tool = Tool {
            name: "remember".into(),
            description: "Store a key-value pair in memory.".into(),
            parameters: serde_json::from_value(schema).unwrap(),
            strict: false,
        };
        hook.register(tool, move |args| {
            let mem = Arc::clone(&mem);
            async move {
                let parsed: serde_json::Value = match serde_json::from_str(&args) {
                    Ok(v) => v,
                    Err(e) => return format!("invalid arguments: {e}"),
                };
                let key = parsed["key"].as_str().unwrap_or("");
                let value = parsed["value"].as_str().unwrap_or("");
                match mem.store(key.to_owned(), value.to_owned()).await {
                    Ok(()) => format!("remembered: {key}"),
                    Err(e) => format!("failed to store: {e}"),
                }
            }
        });
    }

    // recall tool
    {
        let mem = hook.memory_arc();
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search query for relevant memories" },
                "limit": { "type": "integer", "description": "Maximum number of results (default: 10)" }
            },
            "required": ["query"]
        });
        let tool = Tool {
            name: "recall".into(),
            description: "Search memory for entries relevant to a query.".into(),
            parameters: serde_json::from_value(schema).unwrap(),
            strict: false,
        };
        hook.register(tool, move |args| {
            let mem = Arc::clone(&mem);
            async move {
                let parsed: serde_json::Value = match serde_json::from_str(&args) {
                    Ok(v) => v,
                    Err(e) => return format!("invalid arguments: {e}"),
                };
                let query = parsed["query"].as_str().unwrap_or("");
                let limit = parsed["limit"].as_u64().unwrap_or(10) as usize;
                let options = memory::RecallOptions {
                    limit,
                    ..Default::default()
                };
                match mem.recall(query, options).await {
                    Ok(entries) if entries.is_empty() => "no memories found".to_owned(),
                    Ok(entries) => {
                        let mut out = String::new();
                        for entry in &entries {
                            out.push_str(&format!("{}: {}\n", entry.key, entry.value));
                        }
                        out
                    }
                    Err(e) => format!("recall failed: {e}"),
                }
            }
        });
    }
}
