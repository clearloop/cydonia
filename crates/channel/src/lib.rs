//! Walrus channel — platform-agnostic messaging abstraction.
//!
//! Provides the [`Channel`] trait, message types, platform routing,
//! configuration helpers, and the Telegram adapter.

pub mod channel;
pub mod message;
pub mod router;
pub mod spawn;
pub(crate) mod telegram;

pub use channel::{Channel, ChannelHandle, ChannelSender};
pub use message::{Attachment, AttachmentKind, ChannelMessage, Platform};
pub use router::{ChannelRouter, RoutingRule, parse_platform};
pub use spawn::{ChannelConfig, build_router, spawn_channels};
