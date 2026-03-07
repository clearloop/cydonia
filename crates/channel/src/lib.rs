//! Walrus channel — Telegram messaging integration for OpenWalrus agents.
//!
//! Provides configuration types and a spawn function that connects the
//! Telegram bot to the daemon's agent event loop.

pub mod message;
pub mod spawn;
pub(crate) mod telegram;

pub use message::{Attachment, AttachmentKind, ChannelMessage};
pub use spawn::{ChannelConfig, TelegramConfig, spawn_channels};
