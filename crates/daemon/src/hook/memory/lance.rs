//! LanceDB operations for the memory hook.
//!
//! Manages table creation, schema, indexing, upsert via `merge_insert`,
//! and full-text search via BM25. Adapted from `crates/memory/src/lance/`.

use anyhow::Result;
use arrow_array::{
    Array, RecordBatch, RecordBatchIterator, StringArray, UInt32Array, UInt64Array, cast::AsArray,
};
use arrow_schema::{DataType, Field, Schema};
use futures::TryStreamExt;
use lancedb::{
    Connection, Table as LanceTable, connect,
    index::{Index, scalar::FullTextSearchQuery},
    query::{ExecutableQuery, QueryBase},
};
use std::{path::Path, sync::Arc};

const TABLE_NAME: &str = "memories";

/// Row data for building an Arrow RecordBatch.
pub(crate) struct EntryRow<'a> {
    pub key: &'a str,
    pub value: &'a str,
    pub agent: &'a str,
    pub entry_type: &'a str,
}

/// A recalled memory entry from LanceDB.
pub(crate) struct RecalledEntry {
    pub key: String,
    pub value: String,
}

/// LanceDB handle for the memory table.
pub(crate) struct LanceStore {
    _db: Connection,
    table: LanceTable,
}

impl LanceStore {
    /// Open or create the LanceDB database and memories table.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let db = connect(path.as_ref().to_str().unwrap_or("."))
            .execute()
            .await?;

        let table = match db.open_table(TABLE_NAME).execute().await {
            Ok(t) => t,
            Err(_) => {
                let schema = memory_schema();
                let batches = RecordBatchIterator::new(std::iter::empty(), Arc::clone(&schema));
                db.create_table(TABLE_NAME, Box::new(batches))
                    .execute()
                    .await?
            }
        };

        let store = Self { _db: db, table };
        store.ensure_indices().await;
        Ok(store)
    }

    /// Create indices if they don't already exist. Errors are logged but not fatal.
    async fn ensure_indices(&self) {
        // FTS on key+value for BM25 search.
        if let Err(e) = self
            .table
            .create_index(&["key", "value"], Index::FTS(Default::default()))
            .execute()
            .await
        {
            tracing::warn!("FTS index creation skipped (may already exist): {e}");
        }

        // BTree on key for exact lookups.
        if let Err(e) = self
            .table
            .create_index(&["key"], Index::BTree(Default::default()))
            .execute()
            .await
        {
            tracing::warn!("BTree index creation skipped: {e}");
        }

        // Bitmap on agent for low-cardinality filtering.
        if let Err(e) = self
            .table
            .create_index(&["agent"], Index::Bitmap(Default::default()))
            .execute()
            .await
        {
            tracing::warn!("Bitmap agent index creation skipped: {e}");
        }

        // Bitmap on entry_type for low-cardinality filtering.
        if let Err(e) = self
            .table
            .create_index(&["entry_type"], Index::Bitmap(Default::default()))
            .execute()
            .await
        {
            tracing::warn!("Bitmap entry_type index creation skipped: {e}");
        }
    }

    /// Upsert a row via `merge_insert` on the `key` column.
    pub async fn upsert(&self, row: &EntryRow<'_>) -> Result<()> {
        let batch = make_batch(row)?;
        let schema = batch.schema();
        let batches = RecordBatchIterator::new(std::iter::once(Ok(batch)), schema);

        let mut merge = self.table.merge_insert(&["key", "agent"]);
        merge
            .when_matched_update_all(None)
            .when_not_matched_insert_all();
        merge.execute(Box::new(batches)).await?;
        Ok(())
    }

    /// Full-text search scoped by agent name.
    pub async fn search_by_agent(
        &self,
        query: &str,
        agent: &str,
        limit: usize,
    ) -> Result<Vec<RecalledEntry>> {
        let filter = format!("agent = '{}'", escape_sql(agent));
        let batches: Vec<RecordBatch> = self
            .table
            .query()
            .full_text_search(FullTextSearchQuery::new(query.to_owned()))
            .only_if(filter)
            .limit(limit)
            .execute()
            .await?
            .try_collect()
            .await?;

        Ok(batches_to_entries(&batches))
    }
}

/// Build the Arrow schema for the memories table.
fn memory_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("key", DataType::Utf8, false),
        Field::new("value", DataType::Utf8, false),
        Field::new("agent", DataType::Utf8, false),
        Field::new("entry_type", DataType::Utf8, false),
        Field::new("created_at", DataType::UInt64, false),
        Field::new("access_count", DataType::UInt32, false),
    ]))
}

/// Build an Arrow RecordBatch from a single row.
fn make_batch(row: &EntryRow<'_>) -> Result<RecordBatch> {
    let schema = memory_schema();
    let now = now_unix();
    Ok(RecordBatch::try_new(
        schema,
        vec![
            Arc::new(StringArray::from(vec![row.key])) as Arc<dyn Array>,
            Arc::new(StringArray::from(vec![row.value])) as Arc<dyn Array>,
            Arc::new(StringArray::from(vec![row.agent])) as Arc<dyn Array>,
            Arc::new(StringArray::from(vec![row.entry_type])) as Arc<dyn Array>,
            Arc::new(UInt64Array::from(vec![now])) as Arc<dyn Array>,
            Arc::new(UInt32Array::from(vec![0u32])) as Arc<dyn Array>,
        ],
    )?)
}

/// Convert Arrow RecordBatches to recalled entries.
fn batches_to_entries(batches: &[RecordBatch]) -> Vec<RecalledEntry> {
    let mut entries = Vec::new();
    for batch in batches {
        let keys = batch.column_by_name("key").unwrap().as_string::<i32>();
        let values = batch.column_by_name("value").unwrap().as_string::<i32>();
        for i in 0..batch.num_rows() {
            entries.push(RecalledEntry {
                key: keys.value(i).to_string(),
                value: values.value(i).to_string(),
            });
        }
    }
    entries
}

/// Escape single quotes in SQL filter strings.
fn escape_sql(s: &str) -> String {
    s.replace('\'', "''")
}

/// Current unix timestamp in seconds.
fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_secs()
}
