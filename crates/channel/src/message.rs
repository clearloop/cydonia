//! Channel message types and platform identifiers.

use compact_str::CompactString;

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
