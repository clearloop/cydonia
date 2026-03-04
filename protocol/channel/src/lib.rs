//! Channel abstraction for the walrus protocol.
//!
//! Defines the [`Channel`] trait, platform types, message types, routing,
//! and menu abstractions. Channel implementations (e.g. Telegram) live in
//! separate crates and implement [`Channel`] to produce a [`ChannelHandle`].

use anyhow::Result;
use compact_str::CompactString;
use std::{future::Future, pin::Pin, sync::Arc};
use tokio::sync::mpsc;

/// Type-erased async sender function.
type SenderFn =
    Arc<dyn Fn(ChannelMessage) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> + Send + Sync>;

pub mod menu;
pub mod router;

pub use router::{ChannelRouter, RoutingRule, parse_platform};

/// Messaging platform identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Platform {
    /// Telegram messaging platform.
    Telegram,
}

/// A message received from or sent to a channel.
#[derive(Debug, Clone)]
pub struct ChannelMessage {
    /// Platform this message belongs to.
    pub platform: Platform,
    /// Channel/chat identifier on the platform.
    pub channel_id: CompactString,
    /// Sender identifier on the platform.
    pub sender_id: CompactString,
    /// Message text content.
    pub content: String,
    /// Attached files or media.
    pub attachments: Vec<Attachment>,
    /// ID of the message being replied to, if any.
    pub reply_to: Option<CompactString>,
    /// Unix timestamp when the message was created.
    pub timestamp: u64,
}

/// A file or media attachment.
#[derive(Debug, Clone)]
pub struct Attachment {
    /// Type of attachment.
    pub kind: AttachmentKind,
    /// URL or path to the attachment.
    pub url: String,
    /// Optional human-readable name.
    pub name: Option<String>,
}

/// Type of attachment content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentKind {
    /// Image file (PNG, JPG, etc.).
    Image,
    /// Generic file.
    File,
    /// Audio file.
    Audio,
    /// Video file.
    Video,
}

impl From<ChannelMessage> for wcore::model::Message {
    fn from(msg: ChannelMessage) -> Self {
        wcore::model::Message::user(msg.content)
    }
}

/// A live channel connection that can receive and send messages.
///
/// Created by [`Channel::connect`]. Owns all connection state internally
/// via `Arc`, so both recv and send work concurrently without lifetime issues.
pub struct ChannelHandle {
    /// Platform for this handle.
    platform: Platform,
    /// Incoming message receiver.
    rx: mpsc::UnboundedReceiver<ChannelMessage>,
    /// Send function (boxed for type erasure across channel implementations).
    sender: SenderFn,
}

impl ChannelHandle {
    /// Create a new channel handle.
    pub fn new<F, Fut>(
        platform: Platform,
        rx: mpsc::UnboundedReceiver<ChannelMessage>,
        sender: F,
    ) -> Self
    where
        F: Fn(ChannelMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        Self {
            platform,
            rx,
            sender: Arc::new(move |msg| Box::pin(sender(msg))),
        }
    }

    /// The platform this handle connects to.
    pub fn platform(&self) -> Platform {
        self.platform
    }

    /// Receive the next incoming message.
    pub async fn recv(&mut self) -> Option<ChannelMessage> {
        self.rx.recv().await
    }

    /// Send a message through the channel.
    pub async fn send(&self, message: ChannelMessage) -> Result<()> {
        (self.sender)(message).await
    }

    /// Get a cloneable sender for use from other tasks.
    pub fn sender(&self) -> ChannelSender {
        ChannelSender {
            platform: self.platform,
            sender: Arc::clone(&self.sender),
        }
    }
}

/// A cloneable sender extracted from a [`ChannelHandle`].
///
/// Use [`ChannelHandle::sender`] to get a sender that can be moved
/// into spawned tasks for concurrent send operations.
#[derive(Clone)]
pub struct ChannelSender {
    /// Platform for this sender.
    platform: Platform,
    /// Send function.
    sender: SenderFn,
}

impl ChannelSender {
    /// The platform this sender connects to.
    pub fn platform(&self) -> Platform {
        self.platform
    }

    /// Send a message through the channel.
    pub async fn send(&self, message: ChannelMessage) -> Result<()> {
        (self.sender)(message).await
    }
}

/// A connection to a messaging platform.
///
/// Implementations provide platform-specific connection logic. Call
/// [`connect`](Channel::connect) to establish a live connection and
/// get a [`ChannelHandle`] for receiving and sending messages.
pub trait Channel: Send {
    /// Open a connection and return a handle for bidirectional messaging.
    ///
    /// Consumes self — the handle owns all connection state.
    fn connect(self) -> impl Future<Output = Result<ChannelHandle>> + Send;
}
