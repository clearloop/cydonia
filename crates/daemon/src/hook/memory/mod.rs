//! Graph-based memory hook — owns LanceDB with entities and relations tables.
//!
//! Registers `remember`, `recall`, `relate`, `connections`, and `compact`
//! tool schemas. Entities are typed (identity, profile, fact, etc.) and
//! relations are directed edges between entities.

use lance::LanceStore;
use schemars::JsonSchema;
use serde::Deserialize;
use std::path::Path;
use wcore::{AgentConfig, Hook, ToolRegistry, model::Tool};

pub(crate) mod dispatch;
pub(crate) mod lance;

const MEMORY_PROMPT: &str = include_str!("../../../prompts/memory.md");

/// Default entity types provided by the framework.
const DEFAULT_ENTITY_TYPES: &[&str] = &[
    "fact",
    "preference",
    "person",
    "event",
    "concept",
    "identity",
    "profile",
];

/// Graph-based memory hook owning LanceDB entity and relation storage.
pub struct MemoryHook {
    pub(crate) lance: LanceStore,
    pub(crate) allowed_types: Vec<String>,
}

impl MemoryHook {
    /// Create a new MemoryHook, opening or creating the LanceDB database.
    ///
    /// `extra_types` are additional entity types from daemon config, merged
    /// with the framework defaults.
    pub async fn open(
        memory_dir: impl AsRef<Path>,
        extra_types: Vec<String>,
    ) -> anyhow::Result<Self> {
        let memory_dir = memory_dir.as_ref();
        tokio::fs::create_dir_all(memory_dir).await?;
        let lance_dir = memory_dir.join("lance");
        let lance = LanceStore::open(&lance_dir).await?;

        let mut allowed_types: Vec<String> = DEFAULT_ENTITY_TYPES
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        for t in extra_types {
            if !allowed_types.contains(&t) {
                allowed_types.push(t);
            }
        }

        Ok(Self {
            lance,
            allowed_types,
        })
    }

    /// Check if an entity type is allowed.
    pub(crate) fn is_valid_type(&self, entity_type: &str) -> bool {
        self.allowed_types.iter().any(|t| t == entity_type)
    }
}

impl Hook for MemoryHook {
    fn on_build_agent(&self, mut config: AgentConfig) -> AgentConfig {
        // Entity injection from LanceDB happens synchronously via a blocking
        // read. We use tokio::task::block_in_place to avoid deadlocks since
        // Hook::on_build_agent is not async.
        let agent_name = config.name.to_string();
        let lance = &self.lance;

        let extra = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let mut buf = String::new();

                // Inject identity entities.
                if let Ok(identities) = lance.query_by_type(&agent_name, "identity", 50).await
                    && !identities.is_empty()
                {
                    buf.push_str("\n\n<identity>\n");
                    for e in &identities {
                        buf.push_str(&format!("- **{}**: {}\n", e.key, e.value));
                    }
                    buf.push_str("</identity>");
                }

                // Inject profile entities.
                if let Ok(profiles) = lance.query_by_type(&agent_name, "profile", 50).await
                    && !profiles.is_empty()
                {
                    buf.push_str("\n\n<profile>\n");
                    for e in &profiles {
                        buf.push_str(&format!("- **{}**: {}\n", e.key, e.value));
                    }
                    buf.push_str("</profile>");
                }

                buf
            })
        });

        if !extra.is_empty() {
            config.system_prompt = format!("{}{extra}", config.system_prompt);
        }
        config.system_prompt = format!("{}\n\n{MEMORY_PROMPT}", config.system_prompt);
        config
    }

    fn on_compact(&self, _prompt: &mut String) {
        // Profile/identity entities are already in the system prompt via
        // on_build_agent. The compaction LLM sees them in context, so no
        // additional injection is needed here. Agent-scoped queries require
        // the agent name, which on_compact does not receive.
    }

    async fn on_register_tools(&self, tools: &mut ToolRegistry) {
        tools.insert(Tool {
            name: "remember".into(),
            description: format!(
                "Store a memory entity. Types: {}.",
                self.allowed_types.join(", ")
            ),
            parameters: schemars::schema_for!(RememberInput),
            strict: false,
        });
        tools.insert(Tool {
            name: "recall".into(),
            description: "Search memory entities by query, optionally filtered by type.".into(),
            parameters: schemars::schema_for!(RecallInput),
            strict: false,
        });
        tools.insert(Tool {
            name: "relate".into(),
            description: "Create a directed relation between two entities by key.".into(),
            parameters: schemars::schema_for!(RelateInput),
            strict: false,
        });
        tools.insert(Tool {
            name: "connections".into(),
            description: "Find entities connected to a given entity (1-hop graph traversal)."
                .into(),
            parameters: schemars::schema_for!(ConnectionsInput),
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

/// Input for the `remember` tool.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct RememberInput {
    /// Entity type (e.g. "fact", "preference", "identity", "profile").
    pub entity_type: String,
    /// Human-readable key/name for the entity.
    pub key: String,
    /// Value/content to store.
    pub value: String,
}

/// Input for the `recall` tool.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct RecallInput {
    /// Search query for relevant entities.
    pub query: String,
    /// Optional entity type filter.
    pub entity_type: Option<String>,
    /// Maximum number of results (default: 10).
    pub limit: Option<u32>,
}

/// Input for the `relate` tool.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct RelateInput {
    /// Key of the source entity.
    pub source_key: String,
    /// Relation type (e.g. "knows", "prefers", "related_to", "caused_by").
    pub relation: String,
    /// Key of the target entity.
    pub target_key: String,
}

/// Input for the `connections` tool.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct ConnectionsInput {
    /// Key of the entity to find connections for.
    pub key: String,
    /// Optional relation type filter.
    pub relation: Option<String>,
    /// Direction: "outgoing" (default), "incoming", or "both".
    pub direction: Option<String>,
}

/// Input for the `compact` tool (no parameters).
#[derive(Deserialize, JsonSchema)]
pub(crate) struct CompactInput {}
