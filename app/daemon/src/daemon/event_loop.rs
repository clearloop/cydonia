//! Central event loop — processes all daemon events.
//!
//! Receives [`DaemonEvent`] variants from a single mpsc channel and
//! spawns a task for each event to avoid blocking the loop on LLM calls.

use crate::{
    daemon::{Daemon, event::DaemonEvent},
    hook::DaemonHook,
};
use compact_str::CompactString;
use futures_util::{StreamExt, pin_mut};
use model::ProviderManager;
use protocol::{
    api::Server,
    message::{client::ClientMessage, server::ServerMessage},
};
use runtime::Runtime;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

/// Run the central event loop until a [`DaemonEvent::Shutdown`] is received.
pub(crate) async fn event_loop(
    mut rx: mpsc::UnboundedReceiver<DaemonEvent>,
    daemon: Daemon,
    cron_add_tx: mpsc::UnboundedSender<wcron::CronJob>,
) {
    tracing::info!("event loop started");
    while let Some(event) = rx.recv().await {
        match event {
            DaemonEvent::Channel {
                agent,
                content,
                reply,
            } => {
                let rt = Arc::clone(&daemon.runtime);
                tokio::spawn(handle_channel(rt, agent, content, reply));
            }
            DaemonEvent::Cron {
                agent,
                content,
                job_name,
            } => {
                let rt = Arc::clone(&daemon.runtime);
                tokio::spawn(handle_cron(rt, agent, content, job_name));
            }
            DaemonEvent::CronJobCreated(job) => {
                tracing::info!("routing dynamic cron job '{}' to scheduler", job.name);
                let _ = cron_add_tx.send(*job);
            }
            DaemonEvent::Socket { msg, reply } => {
                let d = daemon.clone();
                tokio::spawn(handle_socket(d, msg, reply));
            }
            DaemonEvent::Shutdown => {
                tracing::info!("event loop shutting down");
                break;
            }
        }
    }
    tracing::info!("event loop stopped");
}

/// Dispatch a channel message to the target agent and reply via oneshot.
async fn handle_channel(
    runtime: Arc<Runtime<ProviderManager, DaemonHook>>,
    agent: CompactString,
    content: String,
    reply: oneshot::Sender<Result<String, String>>,
) {
    tracing::info!(%agent, "event loop: channel dispatch");
    let result = match runtime.send_to(&agent, &content).await {
        Ok(resp) => Ok(resp.final_response.unwrap_or_default()),
        Err(e) => Err(e.to_string()),
    };
    let _ = reply.send(result);
}

/// Dispatch a cron job message to the target agent (fire-and-forget).
async fn handle_cron(
    runtime: Arc<Runtime<ProviderManager, DaemonHook>>,
    agent: CompactString,
    content: String,
    job_name: CompactString,
) {
    match runtime.send_to(&agent, &content).await {
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
}

/// Dispatch a socket message through the Server trait and stream replies.
async fn handle_socket(
    daemon: Daemon,
    msg: ClientMessage,
    reply: mpsc::UnboundedSender<ServerMessage>,
) {
    let stream = daemon.dispatch(msg);
    pin_mut!(stream);
    while let Some(server_msg) = stream.next().await {
        if reply.send(server_msg).is_err() {
            break;
        }
    }
}
