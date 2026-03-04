//! Unified gateway event types for the central event loop.
//!
//! All inbound stimuli (socket messages, channel messages, cron fires,
//! tool side-effects) are represented as [`GatewayEvent`] variants sent
//! through a single `mpsc::unbounded_channel`. See DD#7.

use compact_str::CompactString;
use protocol::message::client::ClientMessage;
use protocol::message::server::ServerMessage;
use tokio::sync::{mpsc, oneshot};
use wcron::CronJob;

/// Inbound event from any source, processed by the central event loop.
pub(crate) enum GatewayEvent {
    /// A client message from a socket connection (DD#11).
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
    /// A tool dynamically created a cron job (DD#9).
    CronJobCreated(Box<CronJob>),
    /// Graceful shutdown request.
    Shutdown,
}

/// Shorthand for the event sender half of the gateway event channel.
pub(crate) type EventSender = mpsc::UnboundedSender<GatewayEvent>;

/// Shorthand for the event receiver half of the gateway event channel.
pub(crate) type EventReceiver = mpsc::UnboundedReceiver<GatewayEvent>;
