//! `walrus daemon` — start the walrus daemon in the foreground.

use anyhow::Result;
use clap::Args;
use daemon::{Daemon as WalrusDaemon, config};

/// Start the walrus daemon in the foreground.
#[derive(Args, Debug)]
pub struct Daemon;

impl Daemon {
    /// Run the daemon, blocking until Ctrl-C.
    pub async fn run(self) -> Result<()> {
        let config_dir = config::global_config_dir();
        if !config_dir.exists() {
            config::scaffold_config_dir(&config_dir)?;
            tracing::info!("created config directory at {}", config_dir.display());
        }
        let handle = WalrusDaemon::start(&config_dir).await?;
        tracing::info!("walrusd listening on {}", handle.socket_path.display());
        tokio::signal::ctrl_c().await?;
        tracing::info!("received ctrl-c, shutting down");
        handle.shutdown().await?;
        tracing::info!("walrusd shut down");
        Ok(())
    }
}
