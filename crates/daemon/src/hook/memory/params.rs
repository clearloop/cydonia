//! Input parameters for the memory tools.

use schemars::JsonSchema;
use serde::Deserialize;

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
