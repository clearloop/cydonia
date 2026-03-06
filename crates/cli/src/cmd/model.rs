//! Model management command.

use crate::repl::runner::Runner;
use anyhow::Result;
use clap::{Args, Subcommand};
use futures_util::StreamExt;
use std::io::Write;
use wcore::protocol::message::DownloadEvent;

/// Manage local models.
#[derive(Args, Debug)]
pub struct Model {
    /// Model subcommand.
    #[command(subcommand)]
    pub command: ModelCommand,
}

/// Model subcommands.
#[derive(Subcommand, Debug)]
pub enum ModelCommand {
    /// Download a model from HuggingFace.
    Download(ModelId),
}

/// Model identifier argument.
#[derive(Args, Debug)]
pub struct ModelId {
    /// HuggingFace model ID (e.g. `microsoft/Phi-3.5-mini-instruct`).
    pub model: String,
}

impl Model {
    /// Run the model command.
    pub async fn run(self, runner: &mut Runner) -> Result<()> {
        let ModelCommand::Download(m) = self.command;
        let stream = runner.download_stream(&m.model);
        futures_util::pin_mut!(stream);
        let mut current_size: u64 = 0;
        let mut downloaded: u64 = 0;
        let mut current_file = String::new();
        while let Some(result) = stream.next().await {
            match result? {
                DownloadEvent::Start { model } => println!("Downloading {model}..."),
                DownloadEvent::FileStart { filename, size, .. } => {
                    current_file = filename;
                    current_size = size;
                    downloaded = 0;
                }
                DownloadEvent::Progress { model, bytes } => {
                    downloaded += bytes;
                    let pct = if current_size > 0 {
                        downloaded * 100 / current_size
                    } else {
                        0
                    };
                    eprint!(
                        "\r  [{model}] {} {}% ({} / {})",
                        current_file,
                        pct,
                        format_bytes(downloaded),
                        format_bytes(current_size),
                    );
                    std::io::stderr().flush().ok();
                }
                DownloadEvent::FileEnd { filename, .. } => {
                    eprintln!("\r  {filename} done{:30}", "");
                }
                DownloadEvent::End { model } => println!("Download complete: {model}"),
            }
        }
        Ok(())
    }
}

/// Format a byte count as a human-readable string.
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}
