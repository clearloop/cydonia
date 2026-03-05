//! Unix domain socket transport for the Walrus daemon.
//!
//! Wire message types and API traits live in `walrus-core::protocol`.
//! This crate provides only the UDS transport layer.

pub use wcore::protocol::api;
pub use wcore::protocol::message;

#[cfg(feature = "socket")]
pub mod socket;
