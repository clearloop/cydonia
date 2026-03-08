//! Tool dispatch handlers for remember, recall, relate, connections, and compact.

use crate::hook::memory::{
    ConnectionsInput, MemoryHook, RecallInput, RelateInput, RememberInput,
    lance::{Direction, EntityRow, RelationRow},
};

impl MemoryHook {
    /// Dispatch the `remember` tool call.
    pub(crate) async fn dispatch_remember(&self, args: &str, agent: &str) -> String {
        let input: RememberInput = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments: {e}"),
        };
        if input.key.is_empty() {
            return "missing required field: key".to_owned();
        }
        if !self.is_valid_type(&input.entity_type) {
            return format!(
                "unknown entity_type: '{}'. allowed: {}",
                input.entity_type,
                self.allowed_types.join(", ")
            );
        }

        let id = format!("{}:{}:{}", agent, input.entity_type, input.key);
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
    pub(crate) async fn dispatch_recall(&self, args: &str, agent: &str) -> String {
        let input: RecallInput = match serde_json::from_str(args) {
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
            Ok(entities) => entities
                .iter()
                .map(|e| format!("[{}] {}: {}", e.entity_type, e.key, e.value))
                .collect::<Vec<_>>()
                .join("\n"),
            Err(e) => format!("recall failed: {e}"),
        }
    }

    /// Dispatch the `relate` tool call.
    pub(crate) async fn dispatch_relate(&self, args: &str, agent: &str) -> String {
        let input: RelateInput = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments: {e}"),
        };
        if input.source_key.is_empty() || input.target_key.is_empty() {
            return "missing required field: source_key or target_key".to_owned();
        }
        if input.relation.is_empty() {
            return "missing required field: relation".to_owned();
        }

        // Look up source entity.
        let source = match self
            .lance
            .find_entity_by_key(&input.source_key, agent)
            .await
        {
            Ok(Some(e)) => e,
            Ok(None) => return format!("source entity not found: '{}'", input.source_key),
            Err(e) => return format!("failed to look up source: {e}"),
        };

        // Look up target entity.
        let target = match self
            .lance
            .find_entity_by_key(&input.target_key, agent)
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
    pub(crate) async fn dispatch_connections(&self, args: &str, agent: &str) -> String {
        let input: ConnectionsInput = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments: {e}"),
        };
        if input.key.is_empty() {
            return "missing required field: key".to_owned();
        }

        // Look up the entity to get its ID.
        let entity = match self.lance.find_entity_by_key(&input.key, agent).await {
            Ok(Some(e)) => e,
            Ok(None) => return format!("entity not found: '{}'", input.key),
            Err(e) => return format!("failed to look up entity: {e}"),
        };

        let direction = match input.direction.as_deref() {
            Some("incoming") => Direction::Incoming,
            Some("both") => Direction::Both,
            _ => Direction::Outgoing,
        };

        let relations = match self
            .lance
            .find_connections(&entity.id, agent, input.relation.as_deref(), direction)
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
    pub(crate) fn dispatch_compact(&self) -> String {
        "compact acknowledged — context compaction will be triggered by the runtime".to_owned()
    }
}
