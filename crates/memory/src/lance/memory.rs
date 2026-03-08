//! Memory trait implementation for LanceMemory.

use crate::lance::{EntryRow, LanceMemory, now_unix};
use anyhow::Result;
use arrow_array::{Array, RecordBatch, RecordBatchIterator, cast::AsArray};
use compact_str::CompactString;
use futures::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase};
use std::future::Future;
use wcore::{Embedder, Memory, MemoryEntry, RecallOptions};

impl<E: Embedder> Memory for LanceMemory<E> {
    fn get(&self, key: &str) -> Option<String> {
        let handle = tokio::runtime::Handle::current();
        handle.block_on(self.get_async(key))
    }

    fn entries(&self) -> Vec<(String, String)> {
        let handle = tokio::runtime::Handle::current();
        handle.block_on(self.entries_async())
    }

    fn set(&self, key: impl Into<String>, value: impl Into<String>) -> Option<String> {
        let key = key.into();
        let value = value.into();
        let handle = tokio::runtime::Handle::current();
        handle.block_on(self.set_async(&key, &value))
    }

    fn remove(&self, key: &str) -> Option<String> {
        let handle = tokio::runtime::Handle::current();
        handle.block_on(self.remove_async(key))
    }

    fn store(
        &self,
        key: impl Into<String> + Send,
        value: impl Into<String> + Send,
    ) -> impl Future<Output = Result<()>> + Send {
        let key = key.into();
        let value = value.into();

        async move {
            let embedding = if let Some(embedder) = &self.embedder {
                let emb = embedder.embed(&value).await;
                if emb.is_empty() { None } else { Some(emb) }
            } else {
                None
            };

            self.upsert(&key, &value, None, embedding.as_deref()).await
        }
    }

    fn recall(
        &self,
        query: &str,
        options: RecallOptions,
    ) -> impl Future<Output = Result<Vec<MemoryEntry>>> + Send {
        let query = query.to_owned();

        async move {
            let limit = if options.limit == 0 {
                10
            } else {
                options.limit
            };

            let query_embedding = if let Some(embedder) = &self.embedder {
                let emb = embedder.embed(&query).await;
                if emb.is_empty() { None } else { Some(emb) }
            } else {
                None
            };

            let mut entries = if let Some(ref emb) = query_embedding {
                self.vector_search(emb, limit).await?
            } else {
                self.text_search(&query, limit).await?
            };

            // Apply filters.
            if let Some((start, end)) = options.time_range {
                entries.retain(|e| e.created_at >= start && e.created_at <= end);
            }

            entries.truncate(limit);
            Ok(entries)
        }
    }

    fn compile_relevant(&self, query: &str) -> impl Future<Output = String> + Send {
        let query = query.to_owned();

        async move {
            let opts = RecallOptions {
                limit: 5,
                ..Default::default()
            };

            let entries = self.recall(&query, opts).await.unwrap_or_default();
            if entries.is_empty() {
                return String::new();
            }

            let mut out = String::from("<memory>\n");
            for entry in &entries {
                out.push_str(&format!("<{}>\n", entry.key));
                out.push_str(&entry.value);
                if !entry.value.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str(&format!("</{}>\n", entry.key));
            }
            out.push_str("</memory>");
            out
        }
    }
}

impl<E: Embedder> LanceMemory<E> {
    /// Async get a value by key.
    async fn get_async(&self, key: &str) -> Option<String> {
        let filter = format!("key = '{}'", escape_sql(key));
        let batches: Vec<RecordBatch> = self
            .table
            .query()
            .only_if(filter)
            .limit(1)
            .execute()
            .await
            .ok()?
            .try_collect()
            .await
            .ok()?;

        let batch = batches.first()?;
        if batch.num_rows() == 0 {
            return None;
        }
        let values = batch.column_by_name("value")?.as_string::<i32>();
        Some(values.value(0).to_string())
    }

    /// Async get all key-value pairs.
    async fn entries_async(&self) -> Vec<(String, String)> {
        let batches: Vec<RecordBatch> = match self.table.query().execute().await {
            Ok(stream) => match stream.try_collect().await {
                Ok(b) => b,
                Err(_) => return Vec::new(),
            },
            Err(_) => return Vec::new(),
        };

        let mut entries = Vec::new();
        for batch in &batches {
            let keys = batch.column_by_name("key").unwrap().as_string::<i32>();
            let values = batch.column_by_name("value").unwrap().as_string::<i32>();
            for i in 0..batch.num_rows() {
                entries.push((keys.value(i).to_string(), values.value(i).to_string()));
            }
        }
        entries
    }

    /// Async set a key-value pair, returning the old value.
    async fn set_async(&self, key: &str, value: &str) -> Option<String> {
        let old = self.get_async(key).await;
        let _ = self.upsert(key, value, None, None).await;
        old
    }

    /// Async remove a key, returning the old value.
    async fn remove_async(&self, key: &str) -> Option<String> {
        let old = self.get_async(key).await;
        if old.is_some() {
            let filter = format!("key = '{}'", escape_sql(key));
            let _ = self.table.delete(&filter).await;
        }
        old
    }

    /// Upsert a memory entry (delete + insert).
    async fn upsert(
        &self,
        key: &str,
        value: &str,
        metadata: Option<&str>,
        embedding: Option<&[f32]>,
    ) -> Result<()> {
        // Delete existing row if present.
        let filter = format!("key = '{}'", escape_sql(key));
        let _ = self.table.delete(&filter).await;

        let now = now_unix();
        let row = EntryRow {
            key,
            value,
            metadata,
            created_at: now,
            accessed_at: now,
            access_count: 0,
            embedding,
        };
        let batch = self.make_batch(&row)?;
        let schema = batch.schema();
        let batches = RecordBatchIterator::new(std::iter::once(Ok(batch)), schema);

        self.table.add(Box::new(batches)).execute().await?;
        Ok(())
    }

    /// Vector similarity search via LanceDB's nearest_to.
    async fn vector_search(&self, embedding: &[f32], limit: usize) -> Result<Vec<MemoryEntry>> {
        let batches: Vec<RecordBatch> = self
            .table
            .vector_search(embedding)
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .limit(limit)
            .execute()
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .try_collect()
            .await?;

        Ok(batches_to_entries(&batches))
    }

    /// Full-text search fallback when no embedder is present.
    async fn text_search(&self, query: &str, limit: usize) -> Result<Vec<MemoryEntry>> {
        // Scan all entries and filter by substring match.
        let batches: Vec<RecordBatch> = self
            .table
            .query()
            .limit(limit * 3)
            .execute()
            .await?
            .try_collect()
            .await?;

        let all = batches_to_entries(&batches);
        let query_lower = query.to_lowercase();
        let mut matched: Vec<MemoryEntry> = all
            .into_iter()
            .filter(|e| {
                e.key.to_lowercase().contains(&query_lower)
                    || e.value.to_lowercase().contains(&query_lower)
            })
            .collect();
        matched.truncate(limit);
        Ok(matched)
    }
}

/// Convert Arrow RecordBatches to MemoryEntry vec.
fn batches_to_entries(batches: &[RecordBatch]) -> Vec<MemoryEntry> {
    let mut entries = Vec::new();
    for batch in batches {
        let keys = batch.column_by_name("key").unwrap().as_string::<i32>();
        let values = batch.column_by_name("value").unwrap().as_string::<i32>();
        let metadata_col = batch.column_by_name("metadata").unwrap().as_string::<i32>();
        let created = batch
            .column_by_name("created_at")
            .unwrap()
            .as_primitive::<arrow_array::types::UInt64Type>();
        let accessed = batch
            .column_by_name("accessed_at")
            .unwrap()
            .as_primitive::<arrow_array::types::UInt64Type>();
        let counts = batch
            .column_by_name("access_count")
            .unwrap()
            .as_primitive::<arrow_array::types::UInt32Type>();

        for i in 0..batch.num_rows() {
            entries.push(MemoryEntry {
                key: CompactString::new(keys.value(i)),
                value: values.value(i).to_string(),
                metadata: if metadata_col.is_null(i) {
                    None
                } else {
                    serde_json::from_str(metadata_col.value(i)).ok()
                },
                created_at: created.value(i),
                accessed_at: accessed.value(i),
                access_count: counts.value(i),
                embedding: None,
            });
        }
    }
    entries
}

/// Escape single quotes in SQL filter strings.
fn escape_sql(s: &str) -> String {
    s.replace('\'', "''")
}
