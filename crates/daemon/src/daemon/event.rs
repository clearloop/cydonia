//! Daemon event types and dispatch.
//!
//! All inbound stimuli (socket messages, channel messages, tool calls) are
//! represented as [`DaemonEvent`] variants sent through a single
//! `mpsc::unbounded_channel`. The [`Daemon`] processes them via
//! [`handle_events`](Daemon::handle_events).

use crate::daemon::Daemon;
use compact_str::CompactString;
use futures_util::{StreamExt, pin_mut};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use wcore::{
    Memory, RecallOptions, ToolRequest,
    protocol::{
        api::Server,
        message::{client::ClientMessage, server::ServerMessage},
    },
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
    /// A tool call from an agent, dispatched statically to memory or MCP.
    ToolCall(ToolRequest),
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
                DaemonEvent::ToolCall(req) => self.handle_tool_call(req),
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
            let rt: Arc<_> = runtime.read().await.clone();
            let result = match rt.send_to(&agent, &content).await {
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

    /// Statically dispatch a tool call to memory or MCP.
    ///
    /// System tools (`remember`, `recall`) are called directly on the memory
    /// backend via the runtime's hook. All other tool names are routed to the
    /// MCP bridge.
    fn handle_tool_call(&self, req: ToolRequest) {
        let runtime = self.runtime.clone();
        tokio::spawn(async move {
            tracing::debug!(tool = %req.name, "tool dispatch");
            let rt = runtime.read().await.clone();
            let result = match req.name.as_str() {
                "remember" => Self::dispatch_remember(&rt.hook, &req.args).await,
                "recall" => Self::dispatch_recall(&rt.hook, &req.args).await,
                name => {
                    let bridge = rt.hook.mcp.bridge().await;
                    bridge.call(name, &req.args).await
                }
            };
            let _ = req.reply.send(result);
        });
    }

    /// Parse args and call `memory.store`.
    async fn dispatch_remember(hook: &crate::hook::DaemonHook, args: &str) -> String {
        let parsed: serde_json::Value = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments: {e}"),
        };
        let key = parsed["key"].as_str().unwrap_or("").to_owned();
        let value = parsed["value"].as_str().unwrap_or("").to_owned();
        match hook.memory.store(key.clone(), value).await {
            Ok(()) => format!("remembered: {key}"),
            Err(e) => format!("failed to store: {e}"),
        }
    }

    /// Parse args and call `memory.recall`.
    async fn dispatch_recall(hook: &crate::hook::DaemonHook, args: &str) -> String {
        let parsed: serde_json::Value = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments: {e}"),
        };
        let query = parsed["query"].as_str().unwrap_or("");
        let limit = parsed["limit"].as_u64().unwrap_or(10) as usize;
        let options = RecallOptions {
            limit,
            ..Default::default()
        };
        match hook.memory.recall(query, options).await {
            Ok(entries) if entries.is_empty() => "no memories found".to_owned(),
            Ok(entries) => entries
                .iter()
                .map(|e| format!("{}: {}", e.key, e.value))
                .collect::<Vec<_>>()
                .join("\n"),
            Err(e) => format!("recall failed: {e}"),
        }
    }
}
