//! Memory hook module — owns LanceDB and flat files (SOUL.md, User.toml).
//!
//! Replaces the standalone `walrus-memory` crate. Registers `remember`,
//! `recall`, and `compact` tool schemas. Dispatches via target routing:
//! Soul → SOUL.md, User → User.toml, Store → LanceDB.

use lance::LanceStore;
use schemars::JsonSchema;
use serde::Deserialize;
use std::path::PathBuf;
use wcore::{AgentConfig, Hook, ToolRegistry, model::Tool};

mod dispatch;
pub(crate) mod lance;

const MEMORY_PROMPT: &str = include_str!("../../../prompts/memory.md");

/// Memory hook owning LanceDB and file-based profile storage.
pub struct MemoryHook {
    pub(crate) lance: LanceStore,
    pub(crate) memory_dir: PathBuf,
}

impl MemoryHook {
    /// Create a new MemoryHook, opening or creating the LanceDB database.
    pub async fn open(memory_dir: PathBuf) -> anyhow::Result<Self> {
        tokio::fs::create_dir_all(&memory_dir).await?;
        let lance_dir = memory_dir.join("lance");
        let lance = LanceStore::open(&lance_dir).await?;
        Ok(Self { lance, memory_dir })
    }
}

impl Hook for MemoryHook {
    fn on_build_agent(&self, mut config: AgentConfig) -> AgentConfig {
        let mut extra = String::new();

        // Inject SOUL.md if it exists for this agent.
        let soul_path = self.memory_dir.join(&*config.name).join("SOUL.md");
        if let Ok(content) = std::fs::read_to_string(&soul_path)
            && !content.is_empty()
        {
            extra.push_str("\n\n<soul>\n");
            extra.push_str(&content);
            extra.push_str("</soul>");
        }

        // Inject User.toml if it exists.
        let user_path = self.memory_dir.join("User.toml");
        if let Ok(content) = std::fs::read_to_string(&user_path)
            && !content.is_empty()
        {
            extra.push_str("\n\n<user-profile>\n");
            extra.push_str(&content);
            extra.push_str("</user-profile>");
        }

        if !extra.is_empty() {
            config.system_prompt = format!("{}{extra}", config.system_prompt);
        }

        // Append memory usage instructions.
        config.system_prompt = format!("{}\n\n{MEMORY_PROMPT}", config.system_prompt);
        config
    }

    fn on_compact(&self, prompt: &mut String) {
        // Append profile context so the LLM preserves it during compaction.
        let user_path = self.memory_dir.join("User.toml");
        if let Ok(content) = std::fs::read_to_string(&user_path)
            && !content.is_empty()
        {
            prompt.push_str("\n\n## User Profile\n");
            prompt.push_str(&content);
        }
    }

    async fn on_register_tools(&self, tools: &mut ToolRegistry) {
        tools.insert(Tool {
            name: "remember".into(),
            description: "Store a memory entry. Target: Soul (identity), User (profile), or Store (searchable facts).".into(),
            parameters: schemars::schema_for!(RememberInput),
            strict: false,
        });
        tools.insert(Tool {
            name: "recall".into(),
            description: "Search memory for entries relevant to a query.".into(),
            parameters: schemars::schema_for!(RecallInput),
            strict: false,
        });
        tools.insert(Tool {
            name: "compact".into(),
            description: "Trigger context compaction of the current conversation.".into(),
            parameters: schemars::schema_for!(CompactInput),
            strict: false,
        });
    }
}

/// Target for the remember tool.
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum MemoryTarget {
    /// Write to SOUL.md — agent identity and values.
    Soul,
    /// Write to User.toml — user profile and preferences.
    User,
    /// Write to LanceDB — searchable fact storage.
    Store,
}

/// Input for the `remember` tool.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct RememberInput {
    /// Where to store the memory.
    pub target: MemoryTarget,
    /// Memory key.
    pub key: String,
    /// Value to remember.
    pub value: String,
}

/// Input for the `recall` tool.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct RecallInput {
    /// Search query for relevant memories.
    pub query: String,
    /// Maximum number of results (default: 10).
    pub limit: Option<u32>,
}

/// Input for the `compact` tool (no parameters).
#[derive(Deserialize, JsonSchema)]
pub(crate) struct CompactInput {}
