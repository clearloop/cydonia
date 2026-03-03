//! Walrus wire protocol — message types, API traits, and optional UDS client.

pub mod api;
#[cfg(feature = "client")]
pub mod client;
pub mod codec;
pub mod error;
pub mod message;

/// Current protocol version.
pub const PROTOCOL_VERSION: &str = "0.1";
