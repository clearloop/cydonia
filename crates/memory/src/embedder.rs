//! No-op embedder implementation.

use wcore::Embedder;

/// A no-op embedder that always returns an empty vector.
///
/// Used with memory backends that support optional embeddings
/// (e.g. `SqliteMemory<NoEmbedder>`) when no embedding model is configured.
pub struct NoEmbedder;

impl Embedder for NoEmbedder {
    async fn embed(&self, _text: &str) -> Vec<f32> {
        Vec::new()
    }
}
