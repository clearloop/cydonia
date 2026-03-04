//! Shared gateway serve entrypoint — used by the binary and CLI.
//!
//! Spawns all message transports (socket, channels, cron) and wires them
//! through a central event loop (DD#7). All three sources send
//! [`GatewayEvent`] variants into a single mpsc channel.

use crate::{
    DaemonConfig,
    gateway::{
        Gateway,
        event::{EventReceiver, GatewayEvent},
    },
};
use anyhow::Result;
use compact_str::CompactString;
use futures_util::{StreamExt, pin_mut};
use protocol::api::Server;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, oneshot};

/// Handle returned by [`serve`] — holds the socket path and shutdown trigger.
pub struct ServeHandle {
    /// The Unix domain socket path the gateway is listening on.
    pub socket_path: PathBuf,
    /// Send a value to trigger graceful shutdown of all subsystems.
    shutdown_tx: Option<broadcast::Sender<()>>,
    /// Join handle for the socket accept loop.
    socket_join: Option<tokio::task::JoinHandle<()>>,
    /// Join handle for the event loop.
    event_loop_join: Option<tokio::task::JoinHandle<()>>,
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
        if let Some(join) = self.event_loop_join.take() {
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
    // --- Event channel (DD#7) — created early so build_runtime can capture it ---
    let (event_tx, event_rx) = mpsc::unbounded_channel::<GatewayEvent>();

    let runtime =
        crate::gateway::builder::build_runtime(config, config_dir, event_tx.clone()).await?;

    let hf_endpoint = model::local::download::probe_endpoint().await;
    tracing::info!("using hf endpoint: {hf_endpoint}");

    let runtime = Arc::new(runtime);
    let state = Gateway {
        runtime: Arc::clone(&runtime),
        hf_endpoint: Arc::from(hf_endpoint),
    };

    // Broadcast shutdown — all subsystems subscribe.
    let (shutdown_tx, _) = broadcast::channel::<()>(1);

    // Bridge broadcast shutdown into the event loop.
    let shutdown_event_tx = event_tx.clone();
    let mut shutdown_rx = shutdown_tx.subscribe();
    tokio::spawn(async move {
        let _ = shutdown_rx.recv().await;
        let _ = shutdown_event_tx.send(GatewayEvent::Shutdown);
    });

    // --- Socket transport (migrated to event loop via DD#11) ---
    let resolved_path = crate::config::socket_path();
    if let Some(parent) = resolved_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if resolved_path.exists() {
        std::fs::remove_file(&resolved_path)?;
    }

    let listener = tokio::net::UnixListener::bind(&resolved_path)?;
    tracing::info!("gateway listening on {}", resolved_path.display());

    let socket_shutdown = bridge_shutdown(shutdown_tx.subscribe());
    let socket_tx = event_tx.clone();
    let socket_join = tokio::spawn(socket::server::accept_loop(
        listener,
        move |msg, reply| {
            let _ = socket_tx.send(GatewayEvent::Socket { msg, reply });
        },
        socket_shutdown,
    ));

    // --- Channel transports (migrated to event loop via DD#8 adapter) ---
    let router = channel_router::build_router(&config.channels);
    let router = Arc::new(router);
    let channel_tx = event_tx.clone();
    let on_message = Arc::new(move |agent: CompactString, content: String| {
        let tx = channel_tx.clone();
        async move {
            let (reply_tx, reply_rx) = oneshot::channel();
            let event = GatewayEvent::Channel {
                agent,
                content,
                reply: reply_tx,
            };
            if tx.send(event).is_err() {
                return Err("event loop closed".to_owned());
            }
            reply_rx
                .await
                .unwrap_or(Err("event loop dropped".to_owned()))
        }
    });
    channel_router::spawn_channels(&config.channels, router, on_message).await;

    // --- Cron scheduler (migrated to event loop via DD#8 adapter) ---
    let cron_jobs = runtime.hook().cron().jobs().await;
    let cron_tx = event_tx.clone();
    let cron_add_tx = wcron::spawn_with_callback(
        cron_jobs,
        move |job| {
            let tx = cron_tx.clone();
            async move {
                let _ = tx.send(GatewayEvent::Cron {
                    agent: job.agent.clone(),
                    content: job.message.clone(),
                    job_name: job.name.clone(),
                });
            }
        },
        shutdown_tx.subscribe(),
    );

    // --- Central event loop ---
    let event_loop_join = tokio::spawn(event_loop(
        event_rx,
        state,
        Arc::clone(&runtime),
        cron_add_tx,
    ));

    Ok(ServeHandle {
        socket_path: resolved_path,
        shutdown_tx: Some(shutdown_tx),
        socket_join: Some(socket_join),
        event_loop_join: Some(event_loop_join),
    })
}

/// Central event loop — processes all gateway events (DD#7).
///
/// Spawns tasks for each event to avoid blocking the loop on LLM calls.
async fn event_loop(
    mut rx: EventReceiver,
    state: Gateway,
    runtime: Arc<runtime::Runtime<model::ProviderManager, crate::hook::GatewayHook>>,
    cron_add_tx: tokio::sync::mpsc::UnboundedSender<wcron::CronJob>,
) {
    tracing::info!("event loop started");
    while let Some(event) = rx.recv().await {
        match event {
            GatewayEvent::Channel {
                agent,
                content,
                reply,
            } => {
                let rt = Arc::clone(&runtime);
                tokio::spawn(async move {
                    tracing::info!(%agent, "event loop: channel dispatch");
                    let result = match rt.send_to(&agent, &content).await {
                        Ok(resp) => Ok(resp.final_response.unwrap_or_default()),
                        Err(e) => Err(e.to_string()),
                    };
                    let _ = reply.send(result);
                });
            }
            GatewayEvent::Cron {
                agent,
                content,
                job_name,
            } => {
                let rt = Arc::clone(&runtime);
                tokio::spawn(async move {
                    match rt.send_to(&agent, &content).await {
                        Ok(resp) => {
                            tracing::info!(
                                job = %job_name,
                                agent = %agent,
                                response_len = resp.final_response.as_ref().map_or(0, |s| s.len()),
                                "cron job completed"
                            );
                        }
                        Err(e) => {
                            tracing::error!(job = %job_name, "cron dispatch failed: {e}");
                        }
                    }
                });
            }
            GatewayEvent::CronJobCreated(job) => {
                tracing::info!("routing dynamic cron job '{}' to scheduler", job.name);
                let _ = cron_add_tx.send(*job);
            }
            GatewayEvent::Socket { msg, reply } => {
                let gw = state.clone();
                tokio::spawn(async move {
                    let stream = gw.dispatch(msg);
                    pin_mut!(stream);
                    while let Some(server_msg) = stream.next().await {
                        if reply.send(server_msg).is_err() {
                            break;
                        }
                    }
                });
            }
            GatewayEvent::Shutdown => {
                tracing::info!("event loop shutting down");
                break;
            }
        }
    }
    tracing::info!("event loop stopped");
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
