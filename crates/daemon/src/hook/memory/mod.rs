//! Graph-based memory hook — owns LanceDB with entities, relations, and
//! journals tables. Registers `remember`, `recall`, `relate`, `connections`,
//! `compact`, and `distill` tool schemas. Journals store compaction summaries
//! with vector embeddings for semantic search via fastembed.

use crate::config::MemoryConfig;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use lance::LanceStore;
use schemars::JsonSchema;
use serde::Deserialize;
use std::path::Path;
use std::sync::Mutex;
use wcore::{AgentConfig, Hook, ToolRegistry, model::Tool};

pub(crate) mod dispatch;
pub(crate) mod lance;

const MEMORY_PROMPT: &str = include_str!("../../../prompts/memory.md");

/// Default entity types provided by the framework.
const DEFAULT_ENTITIES: &[&str] = &[
    "fact",
    "preference",
    "person",
    "event",
    "concept",
    "identity",
    "profile",
];

/// Default relation types provided by the framework.
const DEFAULT_RELATIONS: &[&str] = &[
    "knows",
    "prefers",
    "related_to",
    "caused_by",
    "part_of",
    "depends_on",
    "tagged_with",
];

/// Graph-based memory hook owning LanceDB entity, relation, and journal storage.
pub struct MemoryHook {
    pub(crate) lance: LanceStore,
    pub(crate) embedder: Mutex<TextEmbedding>,
    pub(crate) allowed_entities: Vec<String>,
    pub(crate) allowed_relations: Vec<String>,
    pub(crate) connection_limit: usize,
}

impl MemoryHook {
    /// Create a new MemoryHook, opening or creating the LanceDB database.
    pub async fn open(memory_dir: impl AsRef<Path>, config: &MemoryConfig) -> anyhow::Result<Self> {
        let memory_dir = memory_dir.as_ref();
        tokio::fs::create_dir_all(memory_dir).await?;
        let lance_dir = memory_dir.join("lance");
        let lance = LanceStore::open(&lance_dir).await?;

        let embedder = tokio::task::spawn_blocking(|| {
            TextEmbedding::try_new(InitOptions::new(EmbeddingModel::AllMiniLML6V2))
        })
        .await??;

        let allowed_entities = merge_defaults(DEFAULT_ENTITIES, &config.entities);
        let allowed_relations = merge_defaults(DEFAULT_RELATIONS, &config.relations);
        let connection_limit = config.connection_limit.clamp(1, 100);

        Ok(Self {
            lance,
            embedder: Mutex::new(embedder),
            allowed_entities,
            allowed_relations,
            connection_limit,
        })
    }

    /// Check if an entity type is allowed.
    pub(crate) fn is_valid_entity(&self, entity_type: &str) -> bool {
        self.allowed_entities.iter().any(|t| t == entity_type)
    }

    /// Check if a relation type is allowed.
    pub(crate) fn is_valid_relation(&self, relation: &str) -> bool {
        self.allowed_relations.iter().any(|r| r == relation)
    }

    /// Generate an embedding vector for text. Runs fastembed in a blocking task.
    pub(crate) async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let text = text.to_owned();
        let embedding = tokio::task::block_in_place(|| {
            let mut embedder = self
                .embedder
                .lock()
                .map_err(|e| anyhow::anyhow!("embedder lock poisoned: {e}"))?;
            embedder.embed(vec![text], None)
        })?;
        embedding
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("embedding returned no results"))
    }
}

fn merge_defaults(defaults: &[&str], extras: &[String]) -> Vec<String> {
    let mut merged: Vec<String> = defaults.iter().map(|s| (*s).to_owned()).collect();
    for t in extras {
        if !merged.contains(t) {
            merged.push(t.clone());
        }
    }
    merged
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

                // Inject recent journal entries.
                if let Ok(journals) = lance.recent_journals(&agent_name, 3).await
                    && !journals.is_empty()
                {
                    buf.push_str("\n\n<journal>\n");
                    for j in &journals {
                        let ts = chrono::DateTime::from_timestamp(j.created_at as i64, 0)
                            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                            .unwrap_or_else(|| j.created_at.to_string());
                        // Truncate summary to avoid bloating the system prompt.
                        let summary = if j.summary.len() > 500 {
                            format!("{}...", &j.summary[..500])
                        } else {
                            j.summary.clone()
                        };
                        buf.push_str(&format!("- **{ts}**: {summary}\n"));
                    }
                    buf.push_str("</journal>");
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
                self.allowed_entities.join(", ")
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
            description: format!(
                "Create a directed relation between two entities by key. Relations: {}.",
                self.allowed_relations.join(", ")
            ),
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
            description: "Trigger context compaction. Summarizes the conversation, stores a \
                          journal entry, and replaces history with the summary."
                .into(),
            parameters: schemars::schema_for!(CompactInput),
            strict: false,
        });
        tools.insert(Tool {
            name: "distill".into(),
            description: "Search journal entries by semantic similarity. Returns past \
                          conversation summaries. Use `remember`/`relate` to extract durable facts."
                .into(),
            parameters: schemars::schema_for!(DistillInput),
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
    /// Maximum number of results (default: config value, max: 100).
    pub limit: Option<u32>,
}

/// Input for the `compact` tool (no parameters).
#[derive(Deserialize, JsonSchema)]
pub(crate) struct CompactInput {}

/// Input for the `distill` tool.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct DistillInput {
    /// Semantic search query over journal entries.
    pub query: String,
    /// Maximum number of results (default: 5).
    pub limit: Option<u32>,
}
