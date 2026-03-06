//! Wire protocol message types — enums, payload structs, and conversions.

pub mod client;
pub mod server;

pub use client::{ClientMessage, DownloadRequest, HubAction, SendRequest, StreamRequest};
pub use server::{DownloadEvent, HubEvent, SendResponse, ServerMessage, StreamEvent};
