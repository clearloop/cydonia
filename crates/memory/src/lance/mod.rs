//! LanceDB-backed memory store with hybrid search (vector + FTS).
//!
//! Replaces [`crate::SqliteMemory`] with LanceDB's embedded vector database.
//! Supports full-text search, vector similarity search, and hybrid queries
//! with RRF reranking. Uses Apache Arrow for data interchange.

use anyhow::Result;
use arrow_array::{
    Array, FixedSizeListArray, Float32Array, RecordBatch, RecordBatchIterator, StringArray,
    UInt32Array, UInt64Array,
};
use arrow_schema::{DataType, Field, Schema};
use lancedb::{Connection, Table as LanceTable, connect};
use std::{path::Path, sync::Arc};
use wcore::Embedder;

mod memory;

const TABLE_NAME: &str = "memories";

/// Row data for building an Arrow RecordBatch.
struct EntryRow<'a> {
    key: &'a str,
    value: &'a str,
    metadata: Option<&'a str>,
    created_at: u64,
    accessed_at: u64,
    access_count: u32,
    embedding: Option<&'a [f32]>,
}

/// LanceDB-backed memory store.
///
/// Stores key-value memory entries with optional embeddings in a LanceDB
/// table. Supports hybrid search (vector + full-text) via LanceDB's
/// built-in indexing.
pub struct LanceMemory<E: Embedder> {
    _db: Connection,
    table: LanceTable,
    embedder: Option<E>,
    embedding_dim: i32,
}

impl<E: Embedder> LanceMemory<E> {
    /// Open or create a LanceDB database at the given path.
    ///
    /// Creates the `memories` table if it doesn't exist. The `embedding_dim`
    /// parameter sets the fixed size of embedding vectors (e.g. 384, 1536).
    pub async fn open(path: impl AsRef<Path>, embedding_dim: i32) -> Result<Self> {
        let db = connect(path.as_ref().to_str().unwrap_or("."))
            .execute()
            .await?;

        let table = match db.open_table(TABLE_NAME).execute().await {
            Ok(t) => t,
            Err(_) => {
                let schema = memory_schema(embedding_dim);
                let batches = RecordBatchIterator::new(std::iter::empty(), Arc::clone(&schema));
                db.create_table(TABLE_NAME, Box::new(batches))
                    .execute()
                    .await?
            }
        };

        Ok(Self {
            _db: db,
            table,
            embedder: None,
            embedding_dim,
        })
    }

    /// Attach an embedder for vector search.
    pub fn with_embedder(mut self, embedder: E) -> Self {
        self.embedder = Some(embedder);
        self
    }

    /// Build an Arrow `RecordBatch` from a single memory entry.
    fn make_batch(&self, entry: &EntryRow<'_>) -> Result<RecordBatch> {
        let schema = memory_schema(self.embedding_dim);
        let key_arr = Arc::new(StringArray::from(vec![entry.key])) as Arc<dyn Array>;
        let value_arr = Arc::new(StringArray::from(vec![entry.value])) as Arc<dyn Array>;
        let meta_arr = Arc::new(StringArray::from(vec![entry.metadata])) as Arc<dyn Array>;
        let created_arr = Arc::new(UInt64Array::from(vec![entry.created_at])) as Arc<dyn Array>;
        let accessed_arr = Arc::new(UInt64Array::from(vec![entry.accessed_at])) as Arc<dyn Array>;
        let count_arr = Arc::new(UInt32Array::from(vec![entry.access_count])) as Arc<dyn Array>;
        let emb_arr = self.make_embedding_array(entry.embedding)?;

        Ok(RecordBatch::try_new(
            schema,
            vec![
                key_arr,
                value_arr,
                meta_arr,
                created_arr,
                accessed_arr,
                count_arr,
                emb_arr,
            ],
        )?)
    }

    /// Build a `FixedSizeListArray` for the embedding column.
    fn make_embedding_array(&self, embedding: Option<&[f32]>) -> Result<Arc<dyn Array>> {
        let dim = self.embedding_dim as usize;
        let values: Vec<f32> = match embedding {
            Some(e) if e.len() == dim => e.to_vec(),
            _ => vec![0.0; dim],
        };
        let values_arr = Arc::new(Float32Array::from(values));
        let field = Arc::new(Field::new("item", DataType::Float32, true));
        let arr = FixedSizeListArray::try_new(field, self.embedding_dim, values_arr, None)?;
        Ok(Arc::new(arr) as Arc<dyn Array>)
    }
}

/// Build the Arrow schema for the memories table.
fn memory_schema(embedding_dim: i32) -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("key", DataType::Utf8, false),
        Field::new("value", DataType::Utf8, false),
        Field::new("metadata", DataType::Utf8, true),
        Field::new("created_at", DataType::UInt64, false),
        Field::new("accessed_at", DataType::UInt64, false),
        Field::new("access_count", DataType::UInt32, false),
        Field::new(
            "embedding",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                embedding_dim,
            ),
            true,
        ),
    ]))
}

/// Return the current unix timestamp in seconds.
fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_secs()
}
