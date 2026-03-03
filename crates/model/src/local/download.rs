//! Model download with progress reporting via hf-hub.
//!
//! Pre-downloads model files from HuggingFace into the walrus cache
//! directory so mistralrs finds them without re-downloading. Progress
//! events are sent through an mpsc channel for streaming to clients.

use crate::local::cache_dir;
use hf_hub::api::tokio::{ApiBuilder, Progress};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::time::Instant;

/// Events emitted during model download.
#[derive(Debug, Clone)]
pub enum DownloadEvent {
    /// A file download has started.
    FileStart {
        /// Filename within the repo.
        filename: String,
        /// Total size in bytes.
        size: u64,
    },
    /// Incremental download progress (delta, not cumulative).
    Progress {
        /// Bytes downloaded in this chunk (delta).
        bytes: u64,
    },
    /// A file download has completed.
    FileEnd {
        /// Filename within the repo.
        filename: String,
    },
}

/// Progress reporter that sends events through an mpsc channel.
///
/// Throttles `update()` calls to at most once per 100ms across all
/// clones (shared `Instant` via `Arc<Mutex<_>>`). hf-hub clones the
/// progress per parallel download chunk, so the shared clock prevents
/// event flooding.
#[derive(Clone)]
struct ChannelProgress {
    tx: mpsc::UnboundedSender<DownloadEvent>,
    filename: String,
    last_update: Arc<Mutex<Instant>>,
}

impl ChannelProgress {
    fn new(tx: mpsc::UnboundedSender<DownloadEvent>) -> Self {
        Self {
            tx,
            filename: String::new(),
            last_update: Arc::new(Mutex::new(Instant::now())),
        }
    }
}

impl Progress for ChannelProgress {
    async fn init(&mut self, size: usize, filename: &str) {
        self.filename = filename.to_owned();
        *self.last_update.lock().unwrap() = Instant::now();
        let _ = self.tx.send(DownloadEvent::FileStart {
            filename: filename.to_owned(),
            size: size as u64,
        });
    }

    async fn update(&mut self, size: usize) {
        let mut last = self.last_update.lock().unwrap();
        let now = Instant::now();
        if now.duration_since(*last).as_millis() >= 100 {
            *last = now;
            drop(last);
            let _ = self.tx.send(DownloadEvent::Progress { bytes: size as u64 });
        }
    }

    async fn finish(&mut self) {
        let _ = self.tx.send(DownloadEvent::FileEnd {
            filename: self.filename.clone(),
        });
    }
}

/// Download all files for a model repo, sending progress events to `tx`.
///
/// Uses hf-hub's async API with `download_with_progress()`. Files are
/// cached in `cache_dir()` (`~/.walrus/hf`), matching the path
/// mistralrs reads from.
pub async fn download_model(
    model_id: &str,
    tx: mpsc::UnboundedSender<DownloadEvent>,
) -> anyhow::Result<()> {
    let api = ApiBuilder::new()
        .with_progress(false)
        .with_cache_dir(cache_dir())
        .build()?;
    let repo = api.model(model_id.to_owned());
    let info = repo.info().await?;

    let progress = ChannelProgress::new(tx);
    for sibling in &info.siblings {
        repo.download_with_progress(&sibling.rfilename, progress.clone())
            .await?;
    }

    Ok(())
}
