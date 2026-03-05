//! Unix domain socket transport for the Walrus daemon.
//!
//! Wire message types and API traits live in `walrus-core::protocol`.
//! This crate provides only the UDS transport layer.

pub mod client;
pub mod codec;
pub mod server;

pub use client::{ClientConfig, Connection, WalrusClient};
