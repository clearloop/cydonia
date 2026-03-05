//! Daemon event types and dispatch.
//!
//! All inbound stimuli (socket messages, channel messages, tool side-effects)
//! are represented as [`DaemonEvent`] variants sent through a single
//! `mpsc::unbounded_channel`. The [`Daemon`] processes them via
//! [`handle_events`](Daemon::handle_events).

use crate::daemon::Daemon;
use compact_str::CompactString;
use futures_util::{StreamExt, pin_mut};
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
    pub(crate) async fn handle_events(&self, mut rx: mpsc::UnboundedReceiver<DaemonEvent>) {
        tracing::info!("event loop started");
        while let Some(event) = rx.recv().await {
            match event {
                DaemonEvent::Channel {
                    agent,
                    content,
                    reply,
                } => self.handle_channel(agent, content, reply),
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
