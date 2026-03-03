//! Shared gateway serve entrypoint — used by the binary and CLI.
//!
//! Spawns all message transports (socket, channels, cron) and wires them
//! through the shared dispatch path. A broadcast channel coordinates
//! graceful shutdown across all subsystems.

use crate::config::ChannelConfig;
use crate::cron::{CronJob, CronScheduler};
use crate::gateway::dispatch::AgentLock;
use crate::gateway::{Gateway, GatewayHook};
use crate::{DaemonConfig, loader};
use anyhow::Result;
use channel::{ChannelRouter, RoutingRule, parse_platform};
use model::ProviderManager;
use runtime::Runtime;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::broadcast;

/// Handle returned by [`serve`] — holds the socket path and shutdown trigger.
pub struct ServeHandle {
    /// The Unix domain socket path the gateway is listening on.
    pub socket_path: PathBuf,
    /// Send a value to trigger graceful shutdown of all subsystems.
    shutdown_tx: Option<broadcast::Sender<()>>,
    /// Join handle for the socket accept loop.
    socket_join: Option<tokio::task::JoinHandle<()>>,
}

impl ServeHandle {
    /// Trigger graceful shutdown and wait for the server to stop.
    pub async fn shutdown(mut self) -> Result<()> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(join) = self.socket_join.take() {
            join.await?;
        }
        // Clean up the socket file.
        let _ = std::fs::remove_file(&self.socket_path);
        Ok(())
    }
}

/// Load config, build runtime, bind the Unix domain socket, and start serving.
///
/// Returns a [`ServeHandle`] with the socket path and a shutdown trigger.
pub async fn serve(config_dir: &Path) -> Result<ServeHandle> {
    let config_path = config_dir.join("walrus.toml");
    let config = DaemonConfig::load(&config_path)?;
    tracing::info!("loaded configuration from {}", config_path.display());
    serve_with_config(&config, config_dir).await
}

/// Serve with an already-loaded config. Useful when the caller resolves
/// config separately (e.g. CLI with scaffold logic).
pub async fn serve_with_config(config: &DaemonConfig, config_dir: &Path) -> Result<ServeHandle> {
    let runtime = crate::build_runtime(config, config_dir).await?;

    let hf_endpoint = model::local::download::probe_endpoint().await;
    tracing::info!("using hf endpoint: {hf_endpoint}");

    let locks = Arc::new(AgentLock::new());
    let runtime = Arc::new(runtime);
    let state = Gateway {
        runtime: Arc::clone(&runtime),
        locks: Arc::clone(&locks),
        hf_endpoint: Arc::from(hf_endpoint),
    };

    // Broadcast shutdown — all subsystems subscribe.
    let (shutdown_tx, _) = broadcast::channel::<()>(1);

    // --- Socket transport ---
    let resolved_path = crate::config::socket_path();
    if let Some(parent) = resolved_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if resolved_path.exists() {
        std::fs::remove_file(&resolved_path)?;
    }

    let listener = tokio::net::UnixListener::bind(&resolved_path)?;
    tracing::info!("gateway listening on {}", resolved_path.display());

    // Bridge broadcast → oneshot for the socket accept loop.
    let socket_shutdown = bridge_shutdown(shutdown_tx.subscribe());
    let socket_join = tokio::spawn(socket::server::accept_loop(
        listener,
        state,
        socket_shutdown,
    ));

    // --- Channel transports ---
    let router = build_router(&config.channels);
    let router = Arc::new(router);
    spawn_channels(&config.channels, &runtime, &locks, &router).await;

    // --- Cron scheduler ---
    let cron_dir = config_dir.join(crate::config::CRON_DIR);
    spawn_cron(&cron_dir, &runtime, &locks, shutdown_tx.subscribe());

    Ok(ServeHandle {
        socket_path: resolved_path,
        shutdown_tx: Some(shutdown_tx),
        socket_join: Some(socket_join),
    })
}

/// Bridge a broadcast receiver into a oneshot receiver.
fn bridge_shutdown(mut rx: broadcast::Receiver<()>) -> tokio::sync::oneshot::Receiver<()> {
    let (otx, orx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let _ = rx.recv().await;
        let _ = otx.send(());
    });
    orx
}

/// Build a [`ChannelRouter`] from the channel config entries.
fn build_router(channels: &[ChannelConfig]) -> ChannelRouter {
    let mut rules = Vec::new();
    let mut default_agent = None;

    for ch in channels {
        let Ok(platform) = parse_platform(&ch.platform) else {
            tracing::warn!("unknown platform '{}', skipping", ch.platform);
            continue;
        };
        rules.push(RoutingRule {
            platform,
            channel_id: ch.channel_id.clone(),
            agent: ch.agent.clone(),
        });
        if default_agent.is_none() {
            default_agent = Some(ch.agent.clone());
        }
    }

    ChannelRouter::new(rules, default_agent)
}

/// Connect and spawn channel loops for all configured channels.
async fn spawn_channels(
    channels: &[ChannelConfig],
    runtime: &Arc<Runtime<ProviderManager, GatewayHook>>,
    locks: &Arc<AgentLock>,
    router: &Arc<ChannelRouter>,
) {
    for ch in channels {
        let Ok(platform) = parse_platform(&ch.platform) else {
            continue;
        };

        match platform {
            channel::Platform::Telegram => {
                let token = expand_env(&ch.bot_token);
                let tg = telegram::TelegramChannel::new(token);
                match channel::Channel::connect(tg).await {
                    Ok(mut handle) => {
                        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                        let sender = handle.sender();
                        let rt = Arc::clone(runtime);
                        let lk = Arc::clone(locks);
                        let rr = Arc::clone(router);

                        // Forward messages from ChannelHandle to the mpsc channel.
                        tokio::spawn(async move {
                            while let Some(msg) = handle.recv().await {
                                if tx.send(msg).is_err() {
                                    break;
                                }
                            }
                        });

                        tokio::spawn(crate::gateway::channel::channel_loop(
                            rx, sender, rt, lk, rr,
                        ));

                        tracing::info!(platform = "telegram", "channel transport started");
                    }
                    Err(e) => {
                        tracing::error!(platform = "telegram", "failed to connect channel: {e}");
                    }
                }
            }
            _ => {
                tracing::warn!(platform = %ch.platform, "unsupported channel platform");
            }
        }
    }
}

/// Load cron entries and start the scheduler.
fn spawn_cron(
    cron_dir: &Path,
    runtime: &Arc<Runtime<ProviderManager, GatewayHook>>,
    locks: &Arc<AgentLock>,
    shutdown: broadcast::Receiver<()>,
) {
    let entries = match loader::load_cron_dir(cron_dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("failed to load cron entries: {e}");
            return;
        }
    };

    let mut jobs = Vec::new();
    for entry in &entries {
        match CronJob::from_entry(entry) {
            Ok(job) => {
                tracing::info!("registered cron job '{}' → agent '{}'", job.name, job.agent);
                jobs.push(job);
            }
            Err(e) => {
                tracing::warn!("skipping cron entry '{}': {e}", entry.name);
            }
        }
    }

    let scheduler = CronScheduler::new(jobs);
    let rt = Arc::clone(runtime);
    let lk = Arc::clone(locks);

    scheduler.start(
        move |job| {
            let rt = Arc::clone(&rt);
            let lk = Arc::clone(&lk);
            async move {
                match crate::gateway::dispatch::dispatch_send(&rt, &lk, &job.agent, &job.message)
                    .await
                {
                    Ok(response) => {
                        tracing::info!(
                            job = %job.name,
                            agent = %job.agent,
                            response_len = response.len(),
                            "cron job completed"
                        );
                    }
                    Err(e) => {
                        tracing::error!(job = %job.name, "cron dispatch failed: {e}");
                    }
                }
            }
        },
        shutdown,
    );
}

/// Expand `${ENV_VAR}` patterns in a string. Returns the original if not found.
fn expand_env(s: &str) -> String {
    if s.starts_with("${") && s.ends_with('}') {
        let var = &s[2..s.len() - 1];
        std::env::var(var).unwrap_or_else(|_| s.to_owned())
    } else {
        s.to_owned()
    }
}
