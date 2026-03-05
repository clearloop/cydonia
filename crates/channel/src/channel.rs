//! Channel trait and live connection handle types.

use crate::message::{ChannelMessage, Platform};
use anyhow::Result;
use std::{future::Future, pin::Pin, sync::Arc};
use tokio::sync::mpsc;

/// Type-erased async sender function.
type SenderFn =
    Arc<dyn Fn(ChannelMessage) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> + Send + Sync>;

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
