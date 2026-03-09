//! OS hook — filesystem and shell tools for agents.
//!
//! Registers `read`, `write`, and `bash` tool schemas. Dispatch methods live
//! on [`DaemonHook`](crate::hook::DaemonHook). Access control is handled by
//! the permission layer in `dispatch_tool`.

pub use config::{PermissionConfig, ToolPermission};

pub mod config;
pub(crate) mod tool;
