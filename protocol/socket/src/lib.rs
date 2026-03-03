//! Unix domain socket transport for the walrus protocol.
//!
//! Provides both client ([`Connection`], [`WalrusClient`]) and server
//! ([`accept_loop`]) sides of the UDS transport, plus the length-prefixed
//! framing [`codec`].

pub mod client;
pub mod codec;
pub mod server;

pub use client::{ClientConfig, Connection, WalrusClient};
