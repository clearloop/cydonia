//! LanceDB graph storage for the memory hook.
//!
//! Two tables: `entities` (typed nodes with FTS) and `relations` (directed
//! edges between entities). Mutations use lancedb directly; graph traversal
//! uses lance-graph Cypher queries via `DirNamespace`. All operations scoped
//! by agent name.

use anyhow::Result;
use arrow_array::{
    Array, RecordBatch, RecordBatchIterator, StringArray, UInt64Array, cast::AsArray,
};
use arrow_schema::{DataType, Field, Schema};
use futures::TryStreamExt;
use lance_graph::{CypherQuery, DirNamespace, GraphConfig};
use lancedb::{
    Connection, Table as LanceTable, connect,
    index::{Index, scalar::FullTextSearchQuery},
    query::{ExecutableQuery, QueryBase},
};
use std::{path::Path, sync::Arc};

const ENTITIES_TABLE: &str = "entities";
const RELATIONS_TABLE: &str = "relations";
const CONNECTIONS_MAX: usize = 100;

/// Row data for an entity.
pub(crate) struct EntityRow<'a> {
    pub id: &'a str,
    pub entity_type: &'a str,
    pub key: &'a str,
    pub value: &'a str,
    pub agent: &'a str,
}

/// Row data for a relation.
pub(crate) struct RelationRow<'a> {
    pub source: &'a str,
    pub relation: &'a str,
    pub target: &'a str,
    pub agent: &'a str,
}

/// An entity returned from queries.
pub(crate) struct EntityResult {
    pub id: String,
    pub entity_type: String,
    pub key: String,
    pub value: String,
}

/// A relation returned from queries.
pub(crate) struct RelationResult {
    pub source: String,
    pub relation: String,
    pub target: String,
}

/// LanceDB graph store with entities and relations tables.
///
/// Mutations use lancedb's merge_insert directly. Graph traversal
/// (`find_connections`) uses lance-graph Cypher queries.
pub(crate) struct LanceStore {
    _db: Connection,
    entities: LanceTable,
    relations: LanceTable,
    namespace: Arc<DirNamespace>,
    graph_config: GraphConfig,
}

impl LanceStore {
    /// Open or create the LanceDB database with entities and relations tables.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let db = connect(path.to_str().unwrap_or(".")).execute().await?;

        let entities = open_or_create(&db, ENTITIES_TABLE, entity_schema()).await?;
        let relations = open_or_create(&db, RELATIONS_TABLE, relation_schema()).await?;

        let namespace = Arc::new(DirNamespace::new(path.to_str().unwrap_or(".")));
        let graph_config = GraphConfig::builder()
            .with_node_label(ENTITIES_TABLE, "id")
            .with_relationship(RELATIONS_TABLE, "source", "target")
            .build()?;

        let store = Self {
            _db: db,
            entities,
            relations,
            namespace,
            graph_config,
        };
        store.ensure_entity_indices().await;
        store.ensure_relation_indices().await;
        Ok(store)
    }

    /// Upsert an entity by its id.
    ///
    /// Note: `when_matched_update_all` resets `created_at` on update.
    /// LanceDB merge_insert does not support column exclusion, and a
    /// read-before-write adds a round-trip per upsert. `updated_at`
    /// tracks the last modification time; `created_at` is best-effort.
    pub async fn upsert_entity(&self, row: &EntityRow<'_>) -> Result<()> {
        let batch = make_entity_batch(row)?;
        let schema = batch.schema();
        let batches = RecordBatchIterator::new(std::iter::once(Ok(batch)), schema);

        let mut merge = self.entities.merge_insert(&["id"]);
        merge
            .when_matched_update_all(None)
            .when_not_matched_insert_all();
        merge.execute(Box::new(batches)).await?;
        Ok(())
    }

    /// Full-text search on entities, scoped by agent and optional type filter.
    pub async fn search_entities(
        &self,
        query: &str,
        agent: &str,
        entity_type: Option<&str>,
        limit: usize,
    ) -> Result<Vec<EntityResult>> {
        let mut filter = format!("agent = '{}'", escape_sql(agent));
        if let Some(et) = entity_type {
            filter.push_str(&format!(" AND entity_type = '{}'", escape_sql(et)));
        }
        let batches: Vec<RecordBatch> = self
            .entities
            .query()
            .full_text_search(FullTextSearchQuery::new(query.to_owned()))
            .only_if(filter)
            .limit(limit)
            .execute()
            .await?
            .try_collect()
            .await?;

        Ok(batches_to_entities(&batches))
    }

    /// Query entities by type and agent (no FTS, returns all matching).
    pub async fn query_by_type(
        &self,
        agent: &str,
        entity_type: &str,
        limit: usize,
    ) -> Result<Vec<EntityResult>> {
        let filter = format!(
            "agent = '{}' AND entity_type = '{}'",
            escape_sql(agent),
            escape_sql(entity_type)
        );
        let batches: Vec<RecordBatch> = self
            .entities
            .query()
            .only_if(filter)
            .limit(limit)
            .execute()
            .await?
            .try_collect()
            .await?;

        Ok(batches_to_entities(&batches))
    }

    /// Look up an entity by key within an agent's scope.
    pub async fn find_entity_by_key(&self, key: &str, agent: &str) -> Result<Option<EntityResult>> {
        let filter = format!(
            "agent = '{}' AND key = '{}'",
            escape_sql(agent),
            escape_sql(key)
        );
        let batches: Vec<RecordBatch> = self
            .entities
            .query()
            .only_if(filter)
            .limit(1)
            .execute()
            .await?
            .try_collect()
            .await?;

        Ok(batches_to_entities(&batches).into_iter().next())
    }

    /// Upsert a relation (deduplicated by source+relation+target+agent).
    pub async fn upsert_relation(&self, row: &RelationRow<'_>) -> Result<()> {
        let batch = make_relation_batch(row)?;
        let schema = batch.schema();
        let batches = RecordBatchIterator::new(std::iter::once(Ok(batch)), schema);

        let mut merge = self
            .relations
            .merge_insert(&["source", "relation", "target", "agent"]);
        merge
            .when_matched_update_all(None)
            .when_not_matched_insert_all();
        merge.execute(Box::new(batches)).await?;
        Ok(())
    }

    /// Find 1-hop connections from/to an entity using lance-graph Cypher.
    pub async fn find_connections(
        &self,
        entity_id: &str,
        agent: &str,
        relation: Option<&str>,
        direction: Direction,
        limit: usize,
    ) -> Result<Vec<RelationResult>> {
        let limit = limit.min(CONNECTIONS_MAX);
        let cypher = build_connections_cypher(entity_id, agent, relation, direction, limit);
        let query = CypherQuery::new(&cypher)?.with_config(self.graph_config.clone());
        let batch = query
            .execute_with_namespace_arc(Arc::clone(&self.namespace), None)
            .await?;

        Ok(batch_to_relations(&batch))
    }

    /// Create indices on the entities table. Errors are non-fatal.
    async fn ensure_entity_indices(&self) {
        let idx = [
            (
                vec!["key", "value"],
                Index::FTS(Default::default()),
                "entities FTS",
            ),
            (vec!["id"], Index::BTree(Default::default()), "entities id"),
            (
                vec!["key"],
                Index::BTree(Default::default()),
                "entities key",
            ),
            (
                vec!["entity_type"],
                Index::Bitmap(Default::default()),
                "entities entity_type",
            ),
            (
                vec!["agent"],
                Index::Bitmap(Default::default()),
                "entities agent",
            ),
        ];
        for (cols, index, name) in idx {
            if let Err(e) = self.entities.create_index(&cols, index).execute().await {
                tracing::warn!("{name} index creation skipped: {e}");
            }
        }
    }

    /// Create indices on the relations table. Errors are non-fatal.
    async fn ensure_relation_indices(&self) {
        let idx = [
            (
                vec!["source"],
                Index::BTree(Default::default()),
                "relations source",
            ),
            (
                vec!["target"],
                Index::BTree(Default::default()),
                "relations target",
            ),
            (
                vec!["relation"],
                Index::Bitmap(Default::default()),
                "relations relation",
            ),
            (
                vec!["agent"],
                Index::Bitmap(Default::default()),
                "relations agent",
            ),
        ];
        for (cols, index, name) in idx {
            if let Err(e) = self.relations.create_index(&cols, index).execute().await {
                tracing::warn!("{name} index creation skipped: {e}");
            }
        }
    }
}

/// Direction for connection queries.
pub(crate) enum Direction {
    Outgoing,
    Incoming,
    Both,
}

// ── Helpers ─────────────────────────────────────────────────────────────

async fn open_or_create(db: &Connection, name: &str, schema: Arc<Schema>) -> Result<LanceTable> {
    match db.open_table(name).execute().await {
        Ok(t) => Ok(t),
        Err(_) => {
            let batches = RecordBatchIterator::new(std::iter::empty(), Arc::clone(&schema));
            Ok(db.create_table(name, Box::new(batches)).execute().await?)
        }
    }
}

fn entity_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("entity_type", DataType::Utf8, false),
        Field::new("key", DataType::Utf8, false),
        Field::new("value", DataType::Utf8, false),
        Field::new("agent", DataType::Utf8, false),
        Field::new("created_at", DataType::UInt64, false),
        Field::new("updated_at", DataType::UInt64, false),
    ]))
}

fn relation_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("source", DataType::Utf8, false),
        Field::new("relation", DataType::Utf8, false),
        Field::new("target", DataType::Utf8, false),
        Field::new("agent", DataType::Utf8, false),
        Field::new("created_at", DataType::UInt64, false),
    ]))
}

fn make_entity_batch(row: &EntityRow<'_>) -> Result<RecordBatch> {
    let schema = entity_schema();
    let now = now_unix();
    Ok(RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(vec![row.id])) as Arc<dyn Array>,
            Arc::new(StringArray::from(vec![row.entity_type])) as Arc<dyn Array>,
            Arc::new(StringArray::from(vec![row.key])) as Arc<dyn Array>,
            Arc::new(StringArray::from(vec![row.value])) as Arc<dyn Array>,
            Arc::new(StringArray::from(vec![row.agent])) as Arc<dyn Array>,
            Arc::new(UInt64Array::from(vec![now])) as Arc<dyn Array>,
            Arc::new(UInt64Array::from(vec![now])) as Arc<dyn Array>,
        ],
    )?)
}

fn make_relation_batch(row: &RelationRow<'_>) -> Result<RecordBatch> {
    let schema = relation_schema();
    let now = now_unix();
    Ok(RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(vec![row.source])) as Arc<dyn Array>,
            Arc::new(StringArray::from(vec![row.relation])) as Arc<dyn Array>,
            Arc::new(StringArray::from(vec![row.target])) as Arc<dyn Array>,
            Arc::new(StringArray::from(vec![row.agent])) as Arc<dyn Array>,
            Arc::new(UInt64Array::from(vec![now])) as Arc<dyn Array>,
        ],
    )?)
}

fn batches_to_entities(batches: &[RecordBatch]) -> Vec<EntityResult> {
    let mut results = Vec::new();
    for batch in batches {
        let ids = batch.column_by_name("id").unwrap().as_string::<i32>();
        let types = batch
            .column_by_name("entity_type")
            .unwrap()
            .as_string::<i32>();
        let keys = batch.column_by_name("key").unwrap().as_string::<i32>();
        let values = batch.column_by_name("value").unwrap().as_string::<i32>();
        for i in 0..batch.num_rows() {
            results.push(EntityResult {
                id: ids.value(i).to_string(),
                entity_type: types.value(i).to_string(),
                key: keys.value(i).to_string(),
                value: values.value(i).to_string(),
            });
        }
    }
    results
}

fn batch_to_relations(batch: &RecordBatch) -> Vec<RelationResult> {
    if batch.num_rows() == 0 {
        return Vec::new();
    }
    // lance-graph qualifies columns as {variable}__{field} (lowercase).
    // The Cypher query binds the relationship to variable `r`.
    let sources = batch
        .column_by_name("r__source")
        .unwrap()
        .as_string::<i32>();
    let relations = batch
        .column_by_name("r__relation")
        .unwrap()
        .as_string::<i32>();
    let targets = batch
        .column_by_name("r__target")
        .unwrap()
        .as_string::<i32>();
    (0..batch.num_rows())
        .map(|i| RelationResult {
            source: sources.value(i).to_string(),
            relation: relations.value(i).to_string(),
            target: targets.value(i).to_string(),
        })
        .collect()
}

/// Build a Cypher query for 1-hop connection traversal.
fn build_connections_cypher(
    entity_id: &str,
    agent: &str,
    relation: Option<&str>,
    direction: Direction,
    limit: usize,
) -> String {
    let eid = escape_cypher(entity_id);
    let ag = escape_cypher(agent);

    let rel_type = relation
        .map(|r| format!(":`{}`", escape_cypher_ident(r)))
        .unwrap_or_default();

    let (pattern, agent_filter) = match direction {
        Direction::Outgoing => (
            format!("(e:entities {{id: '{eid}'}})-[r:relations{rel_type}]->(t:entities)"),
            format!("r.agent = '{ag}'"),
        ),
        Direction::Incoming => (
            format!("(e:entities)<-[r:relations{rel_type}]-(s:entities {{id: '{eid}'}})"),
            format!("r.agent = '{ag}'"),
        ),
        Direction::Both => (
            format!("(e:entities)-[r:relations{rel_type}]-(o:entities {{id: '{eid}'}})"),
            format!("r.agent = '{ag}'"),
        ),
    };

    format!(
        "MATCH {pattern} WHERE {agent_filter} RETURN r.source, r.relation, r.target LIMIT {limit}"
    )
}

fn escape_sql(s: &str) -> String {
    s.replace('\'', "''")
}

fn escape_cypher(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

/// Escape a Cypher identifier for backtick quoting.
fn escape_cypher_ident(s: &str) -> String {
    s.replace('`', "``")
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_secs()
}
