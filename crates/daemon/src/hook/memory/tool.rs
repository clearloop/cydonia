//! Input parameters for the memory tools.

use schemars::JsonSchema;
use serde::Deserialize;
use wcore::agent::ToolDescription;

#[derive(Deserialize, JsonSchema)]
pub(crate) struct Remember {
    /// Entity type (e.g. "fact", "preference", "identity", "profile").
    pub entity_type: String,
    /// Human-readable key/name for the entity.
    pub key: String,
    /// Value/content to store.
    pub value: String,
}

impl ToolDescription for Remember {
    const DESCRIPTION: &'static str = "Store a memory entity.";
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct Recall {
    /// Search query for relevant entities.
    pub query: String,
    /// Optional entity type filter.
    pub entity_type: Option<String>,
    /// Maximum number of results (default: 10).
    pub limit: Option<u32>,
}

impl ToolDescription for Recall {
    const DESCRIPTION: &'static str =
        "Search memory entities by query, optionally filtered by type.";
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct Relate {
    /// Key of the source entity.
    pub source_key: String,
    /// Relation type (e.g. "knows", "prefers", "related_to", "caused_by").
    pub relation: String,
    /// Key of the target entity.
    pub target_key: String,
}

impl ToolDescription for Relate {
    const DESCRIPTION: &'static str = "Create a directed relation between two entities by key.";
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct Connections {
    /// Key of the entity to find connections for.
    pub key: String,
    /// Optional relation type filter.
    pub relation: Option<String>,
    /// Direction: "outgoing" (default), "incoming", or "both".
    pub direction: Option<String>,
    /// Maximum number of results (default: config value, max: 100).
    pub limit: Option<u32>,
}

impl ToolDescription for Connections {
    const DESCRIPTION: &'static str =
        "Find entities connected to a given entity (1-hop graph traversal).";
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct Compact {}

impl ToolDescription for Compact {
    const DESCRIPTION: &'static str = "Trigger context compaction. Summarizes the conversation, stores a journal entry, and replaces history with the summary.";
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct Distill {
    /// Semantic search query over journal entries.
    pub query: String,
    /// Maximum number of results (default: 5).
    pub limit: Option<u32>,
}

impl ToolDescription for Distill {
    const DESCRIPTION: &'static str = "Search journal entries by semantic similarity. Returns past conversation summaries. Use `remember`/`relate` to extract durable facts.";
}
