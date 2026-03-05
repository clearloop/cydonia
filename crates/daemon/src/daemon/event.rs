//! Daemon event types and dispatch.
//!
//! All inbound stimuli (socket messages, channel messages, cron fires,
//! tool side-effects) are represented as [`DaemonEvent`] variants sent
//! through a single `mpsc::unbounded_channel`. The [`Daemon`] processes
//! them via [`handle_events`](Daemon::handle_events).

use crate::daemon::Daemon;
use compact_str::CompactString;
use futures_util::{StreamExt, pin_mut};
use system::cron::CronJob;
use tokio::sync::{mpsc, oneshot};
use wcore::protocol::{
    api::Server,
    message::{client::ClientMessage, server::ServerMessage},
};

/// Inbound event from any source, processed by the central event loop.
pub(crate) enum DaemonEvent {
    /// A client message from a socket connection.
    Socket {
        /// The parsed client message.
        msg: ClientMessage,
        /// Per-connection reply channel for streaming `ServerMessage`s back.
        reply: mpsc::UnboundedSender<ServerMessage>,
    },
    /// A message from an external channel (Telegram, etc.) with a oneshot
    /// reply channel so the channel loop can await the response.
    Channel {
        /// Target agent name (resolved by the router).
        agent: CompactString,
        /// Message content.
        content: String,
        /// Oneshot channel to send the response back to the channel loop.
        reply: oneshot::Sender<Result<String, String>>,
    },
    /// A cron job fired. Fire-and-forget: no reply channel. Cron jobs
    /// log their outcome but do not return a value to the scheduler.
    Cron {
        /// Target agent name.
        agent: CompactString,
        /// Message to send.
        content: String,
        /// Job name (for logging).
        job_name: CompactString,
    },
    /// A tool dynamically created a cron job.
    CronJobCreated(Box<CronJob>),
    /// Graceful shutdown request.
    Shutdown,
}

/// Shorthand for the event sender half of the daemon event channel.
pub(crate) type DaemonEventSender = mpsc::UnboundedSender<DaemonEvent>;

// ── Event dispatch ───────────────────────────────────────────────────

impl Daemon {
    /// Process events until [`DaemonEvent::Shutdown`] is received.
    ///
    /// Spawns a task for each event to avoid blocking on LLM calls.
    pub(crate) async fn handle_events(
        &self,
        mut rx: mpsc::UnboundedReceiver<DaemonEvent>,
        cron_add_tx: mpsc::UnboundedSender<CronJob>,
    ) {
        tracing::info!("event loop started");
        while let Some(event) = rx.recv().await {
            match event {
                DaemonEvent::Channel {
                    agent,
                    content,
                    reply,
                } => self.handle_channel(agent, content, reply),
                DaemonEvent::Cron {
                    agent,
                    content,
                    job_name,
                } => self.handle_cron(agent, content, job_name),
                DaemonEvent::CronJobCreated(job) => {
                    tracing::info!("routing dynamic cron job '{}' to scheduler", job.name);
                    let _ = cron_add_tx.send(*job);
                }
                DaemonEvent::Socket { msg, reply } => self.handle_socket(msg, reply),
                DaemonEvent::Shutdown => {
                    tracing::info!("event loop shutting down");
                    break;
                }
            }
        }
        tracing::info!("event loop stopped");
    }

    /// Dispatch a channel message to the target agent and reply via oneshot.
    fn handle_channel(
        &self,
        agent: CompactString,
        content: String,
        reply: oneshot::Sender<Result<String, String>>,
    ) {
        let runtime = self.runtime.clone();
        tokio::spawn(async move {
            tracing::info!(%agent, "channel dispatch");
            let result = match runtime.send_to(&agent, &content).await {
                Ok(resp) => Ok(resp.final_response.unwrap_or_default()),
                Err(e) => Err(e.to_string()),
            };
            let _ = reply.send(result);
        });
    }

    /// Dispatch a cron job message to the target agent (fire-and-forget).
    fn handle_cron(&self, agent: CompactString, content: String, job_name: CompactString) {
        let runtime = self.runtime.clone();
        tokio::spawn(async move {
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
        });
    }

    /// Dispatch a socket message through the Server trait and stream replies.
    fn handle_socket(&self, msg: ClientMessage, reply: mpsc::UnboundedSender<ServerMessage>) {
        let daemon = self.clone();
        tokio::spawn(async move {
            let stream = daemon.dispatch(msg);
            pin_mut!(stream);
            while let Some(server_msg) = stream.next().await {
                if reply.send(server_msg).is_err() {
                    break;
                }
            }
        });
    }
}
