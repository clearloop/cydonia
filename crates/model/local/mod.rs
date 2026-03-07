//! Local LLM provider via mistralrs.
//!
//! Wraps `mistralrs::Model` for native on-device inference.
//! No HTTP transport — inference runs in-process.
//! Provides per-builder constructors: `from_text()`, `from_gguf()`,
//! `from_vision()`. Supports lazy loading via `lazy()` — returns
//! immediately and builds the model in a background task. Cache
//! directory is controlled by env vars (`HF_HOME`, `HF_ENDPOINT`).

use compact_str::CompactString;
use std::sync::Arc;
use tokio::sync::watch;

pub mod download;
mod provider;

/// Internal state of a lazy-loaded local model.
#[derive(Clone)]
enum LocalState {
    /// Model is being downloaded/loaded in a background task.
    Loading,
    /// Model is ready for inference.
    Ready(Arc<mistralrs::Model>),
    /// Model loading failed.
    Failed(String),
}

/// Local LLM provider wrapping a mistralrs `Model`.
///
/// Supports both eager construction (via `from_text`, `from_gguf`,
/// `from_vision`) and lazy construction (via `lazy`). Lazy construction
/// returns immediately; the model loads in a background task.
#[derive(Clone)]
pub struct Local {
    state: watch::Receiver<LocalState>,
    model_id: CompactString,
}

impl Local {
    /// Construct from a pre-built mistralrs `Model`.
    pub fn from_model(model: mistralrs::Model) -> Self {
        let (tx, rx) = watch::channel(LocalState::Ready(Arc::new(model)));
        drop(tx);
        Self {
            state: rx,
            model_id: CompactString::from("local"),
        }
    }

    /// Construct a lazy-loading Local provider.
    ///
    /// Returns immediately with `Loading` state. Spawns a background
    /// tokio task that probes the HF endpoint, builds the model, and
    /// transitions state to `Ready` or `Failed`.
    pub fn lazy(
        model_id: &str,
        loader: crate::config::Loader,
        isq: Option<mistralrs::IsqType>,
        chat_template: Option<String>,
    ) -> Self {
        let (tx, rx) = watch::channel(LocalState::Loading);
        let mid = CompactString::from(model_id);
        let id = mid.clone();

        let handle = tokio::runtime::Handle::current();
        tokio::task::spawn_blocking(move || {
            handle.block_on(async move {
                let endpoint = crate::local::download::probe_endpoint().await;
                tracing::info!("lazy load: using hf endpoint: {endpoint}");
                unsafe { std::env::set_var("HF_ENDPOINT", &endpoint) };

                let result = match loader {
                    crate::config::Loader::Text => {
                        Self::build_text(&id, isq, chat_template.as_deref()).await
                    }
                    crate::config::Loader::Gguf => {
                        Self::build_gguf(&id, chat_template.as_deref()).await
                    }
                    crate::config::Loader::Vision => {
                        Self::build_vision(&id, isq, chat_template.as_deref()).await
                    }
                    other => Err(anyhow::anyhow!(
                        "loader {other:?} requires adapter configuration (not yet supported)"
                    )),
                };

                match result {
                    Ok(model) => {
                        tracing::info!("local model '{id}' loaded successfully");
                        let _ = tx.send(LocalState::Ready(Arc::new(model)));
                    }
                    Err(e) => {
                        tracing::error!("local model '{id}' failed to load: {e}");
                        let _ = tx.send(LocalState::Failed(e.to_string()));
                    }
                }
            });
        });

        Self {
            state: rx,
            model_id: mid,
        }
    }

    /// Build using `TextModelBuilder`.
    ///
    /// Standard text models from HuggingFace.
    pub async fn from_text(
        model_id: &str,
        isq: Option<mistralrs::IsqType>,
        chat_template: Option<&str>,
    ) -> anyhow::Result<Self> {
        let model = Self::build_text(model_id, isq, chat_template).await?;
        Ok(Self::from_model(model))
    }

    /// Build using `GgufModelBuilder`.
    ///
    /// GGUF quantized models from HuggingFace. The `model_id` is the HF repo
    /// ID; mistralrs auto-discovers GGUF files in the repo.
    pub async fn from_gguf(model_id: &str, chat_template: Option<&str>) -> anyhow::Result<Self> {
        let model = Self::build_gguf(model_id, chat_template).await?;
        Ok(Self::from_model(model))
    }

    /// Build using `VisionModelBuilder`.
    ///
    /// Vision-language models from HuggingFace.
    pub async fn from_vision(
        model_id: &str,
        isq: Option<mistralrs::IsqType>,
        chat_template: Option<&str>,
    ) -> anyhow::Result<Self> {
        let model = Self::build_vision(model_id, isq, chat_template).await?;
        Ok(Self::from_model(model))
    }

    /// Try to get the ready model. Returns an error describing current state
    /// if not ready.
    pub(crate) fn ready_model(&self) -> anyhow::Result<Arc<mistralrs::Model>> {
        let state = self.state.borrow();
        match &*state {
            LocalState::Ready(model) => Ok(model.clone()),
            LocalState::Loading => Err(anyhow::anyhow!(
                "local model '{}' is still loading, please wait...",
                self.model_id
            )),
            LocalState::Failed(e) => Err(anyhow::anyhow!(
                "local model '{}' failed to load: {e}",
                self.model_id
            )),
        }
    }

    /// Query the context length for a given model ID.
    ///
    /// Returns None if the model isn't ready or doesn't report a sequence length.
    pub fn context_length(&self, model: &str) -> Option<usize> {
        let m = self.ready_model().ok()?;
        m.max_sequence_length_with_model(Some(model)).ok().flatten()
    }

    // ── Private builder methods ──────────────────────────────────────

    async fn build_text(
        model_id: &str,
        isq: Option<mistralrs::IsqType>,
        chat_template: Option<&str>,
    ) -> anyhow::Result<mistralrs::Model> {
        let mut builder = mistralrs::TextModelBuilder::new(model_id).with_logging();
        if let Some(isq) = isq {
            builder = builder.with_isq(isq);
        }
        if let Some(template) = chat_template {
            builder = builder.with_chat_template(template);
        }
        builder.build().await
    }

    async fn build_gguf(
        model_id: &str,
        chat_template: Option<&str>,
    ) -> anyhow::Result<mistralrs::Model> {
        let mut builder =
            mistralrs::GgufModelBuilder::new(model_id, Vec::<String>::new()).with_logging();
        if let Some(template) = chat_template {
            builder = builder.with_chat_template(template);
        }
        builder.build().await
    }

    async fn build_vision(
        model_id: &str,
        isq: Option<mistralrs::IsqType>,
        chat_template: Option<&str>,
    ) -> anyhow::Result<mistralrs::Model> {
        let mut builder = mistralrs::VisionModelBuilder::new(model_id).with_logging();
        if let Some(isq) = isq {
            builder = builder.with_isq(isq);
        }
        if let Some(template) = chat_template {
            builder = builder.with_chat_template(template);
        }
        builder.build().await
    }
}
