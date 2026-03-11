//! Tool dispatch handlers for memory tools.

use crate::hook::memory::{
    MemoryHook,
    lance::{Direction, EntityRow, RelationRow},
    tool::{Connections, Distill, Recall, Relate, Remember},
};
use wcore::COMPACT_SENTINEL;

/// User-scoped entity types — prefixed with sender when non-empty.
const USER_SCOPED_TYPES: &[&str] = &["profile", "person", "preference"];

/// Build entity ID with optional sender scoping.
///
/// Agent-scoped: `{agent}:{type}:{key}`
/// User-scoped (non-empty sender): `{agent}:{sender}:{type}:{key}`
fn entity_id(agent: &str, entity_type: &str, key: &str, sender: &str) -> String {
    if !sender.is_empty() && USER_SCOPED_TYPES.contains(&entity_type) {
        format!("{agent}:{sender}:{entity_type}:{key}")
    } else {
        format!("{agent}:{entity_type}:{key}")
    }
}

impl MemoryHook {
    /// Dispatch the `remember` tool call.
    pub(crate) async fn dispatch_remember(&self, args: &str, agent: &str, sender: &str) -> String {
        let input: Remember = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments: {e}"),
        };
        if input.key.is_empty() {
            return "missing required field: key".to_owned();
        }
        if !self.is_valid_entity(&input.entity_type) {
            return format!(
                "unknown entity_type: '{}'. allowed: {}",
                input.entity_type,
                self.allowed_entities.join(", ")
            );
        }

        // Identity guard: only local/owner sessions may write identity entities.
        if input.entity_type == "identity" && !sender.is_empty() {
            return "identity entities can only be written by the owner".to_owned();
        }

        let id = entity_id(agent, &input.entity_type, &input.key, sender);
        let row = EntityRow {
            id: &id,
            entity_type: &input.entity_type,
            key: &input.key,
            value: &input.value,
            agent,
        };
        match self.lance.upsert_entity(&row).await {
            Ok(()) => format!(
                "remembered ({}/{}): {}",
                input.entity_type, agent, input.key
            ),
            Err(e) => format!("failed to store entity: {e}"),
        }
    }

    /// Dispatch the `recall` tool call.
    pub(crate) async fn dispatch_recall(&self, args: &str, agent: &str, sender: &str) -> String {
        let input: Recall = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments: {e}"),
        };
        if input.query.is_empty() {
            return "missing required field: query".to_owned();
        }
        let limit = input.limit.unwrap_or(10) as usize;

        match self
            .lance
            .search_entities(&input.query, agent, input.entity_type.as_deref(), limit)
            .await
        {
            Ok(entities) if entities.is_empty() => "no entities found".to_owned(),
            Ok(entities) => {
                // Filter user-scoped entities to those belonging to this sender.
                let sender_prefix = if sender.is_empty() {
                    String::new()
                } else {
                    format!("{agent}:{sender}:")
                };
                let filtered: Vec<_> = entities
                    .iter()
                    .filter(|e| {
                        if !USER_SCOPED_TYPES.contains(&e.entity_type.as_str()) {
                            return true; // agent-scoped: visible to all
                        }
                        if sender.is_empty() {
                            // Owner sees all user-scoped entries (full visibility)
                            true
                        } else {
                            // Channel user sees only their own user-scoped entries
                            e.id.starts_with(&sender_prefix)
                        }
                    })
                    .collect();
                if filtered.is_empty() {
                    return "no entities found".to_owned();
                }
                filtered
                    .iter()
                    .map(|e| format!("[{}] {}: {}", e.entity_type, e.key, e.value))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            Err(e) => format!("recall failed: {e}"),
        }
    }

    /// Dispatch the `relate` tool call.
    pub(crate) async fn dispatch_relate(&self, args: &str, agent: &str, sender: &str) -> String {
        let input: Relate = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments: {e}"),
        };
        if input.source_key.is_empty() || input.target_key.is_empty() {
            return "missing required field: source_key or target_key".to_owned();
        }
        if input.relation.is_empty() {
            return "missing required field: relation".to_owned();
        }
        if !self.is_valid_relation(&input.relation) {
            return format!(
                "unknown relation: '{}'. allowed: {}",
                input.relation,
                self.allowed_relations.join(", ")
            );
        }

        // Look up source entity (scoped by sender for user-scoped types).
        let source = match self
            .lance
            .find_entity_by_key(&input.source_key, agent, sender)
            .await
        {
            Ok(Some(e)) => e,
            Ok(None) => return format!("source entity not found: '{}'", input.source_key),
            Err(e) => return format!("failed to look up source: {e}"),
        };

        // Look up target entity (scoped by sender for user-scoped types).
        let target = match self
            .lance
            .find_entity_by_key(&input.target_key, agent, sender)
            .await
        {
            Ok(Some(e)) => e,
            Ok(None) => return format!("target entity not found: '{}'", input.target_key),
            Err(e) => return format!("failed to look up target: {e}"),
        };

        let row = RelationRow {
            source: &source.id,
            relation: &input.relation,
            target: &target.id,
            agent,
        };
        match self.lance.upsert_relation(&row).await {
            Ok(()) => format!(
                "related: {} -[{}]-> {}",
                input.source_key, input.relation, input.target_key
            ),
            Err(e) => format!("failed to create relation: {e}"),
        }
    }

    /// Dispatch the `connections` tool call.
    pub(crate) async fn dispatch_connections(
        &self,
        args: &str,
        agent: &str,
        sender: &str,
    ) -> String {
        let input: Connections = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments: {e}"),
        };
        if input.key.is_empty() {
            return "missing required field: key".to_owned();
        }

        // Look up the entity (scoped by sender for user-scoped types).
        let entity = match self
            .lance
            .find_entity_by_key(&input.key, agent, sender)
            .await
        {
            Ok(Some(e)) => e,
            Ok(None) => return format!("entity not found: '{}'", input.key),
            Err(e) => return format!("failed to look up entity: {e}"),
        };

        let direction = match input.direction.as_deref() {
            Some("incoming") => Direction::Incoming,
            Some("both") => Direction::Both,
            _ => Direction::Outgoing,
        };

        let limit = input
            .limit
            .map(|l| (l as usize).min(100))
            .unwrap_or(self.connection_limit);

        let relations = match self
            .lance
            .find_connections(
                &entity.id,
                agent,
                input.relation.as_deref(),
                direction,
                limit,
            )
            .await
        {
            Ok(r) => r,
            Err(e) => return format!("connections query failed: {e}"),
        };

        if relations.is_empty() {
            return "no connections found".to_owned();
        }

        relations
            .iter()
            .map(|r| format!("{} -[{}]-> {}", r.source, r.relation, r.target))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Dispatch the `compact` tool call.
    ///
    /// Returns the compact sentinel followed by recent journal context.
    /// The agent loop detects the sentinel and triggers compaction.
    pub(crate) async fn dispatch_compact(&self, agent: &str) -> String {
        let mut result = COMPACT_SENTINEL.to_owned();

        // Append recent journal entries for continuity context.
        if let Ok(journals) = self.lance.recent_journals(agent, 3).await
            && !journals.is_empty()
        {
            result.push_str("\n\nPrevious journal entries:\n");
            for j in &journals {
                let ts = chrono::DateTime::from_timestamp(j.created_at as i64, 0)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| j.created_at.to_string());
                result.push_str(&format!("- [{ts}] {}\n", j.summary));
            }
        }

        result
    }

    /// Internal dispatch for storing a journal entry.
    ///
    /// Called by the agent loop after compaction — `args` is the raw summary text.
    pub(crate) async fn dispatch_journal(&self, args: &str, agent: &str) -> String {
        if args.is_empty() {
            return "empty journal entry".to_owned();
        }

        let vector = match self.embed(args).await {
            Ok(v) => v,
            Err(e) => return format!("failed to embed journal: {e}"),
        };

        match self.lance.insert_journal(agent, args, vector).await {
            Ok(()) => "journal entry stored".to_owned(),
            Err(e) => format!("failed to store journal: {e}"),
        }
    }

    /// Dispatch the `distill` tool call — semantic search over journal entries.
    pub(crate) async fn dispatch_distill(&self, args: &str, agent: &str) -> String {
        let input: Distill = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments: {e}"),
        };
        if input.query.is_empty() {
            return "missing required field: query".to_owned();
        }
        let limit = input.limit.unwrap_or(5) as usize;

        let vector = match self.embed(&input.query).await {
            Ok(v) => v,
            Err(e) => return format!("failed to embed query: {e}"),
        };

        match self.lance.search_journals(&vector, agent, limit).await {
            Ok(journals) if journals.is_empty() => "no journal entries found".to_owned(),
            Ok(journals) => journals
                .iter()
                .map(|j| {
                    let ts = chrono::DateTime::from_timestamp(j.created_at as i64, 0)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                        .unwrap_or_else(|| j.created_at.to_string());
                    format!("[{ts}] {}", j.summary)
                })
                .collect::<Vec<_>>()
                .join("\n\n"),
            Err(e) => format!("distill failed: {e}"),
        }
    }
}
