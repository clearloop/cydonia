//! Walrus daemon — message central composing runtime, channels, and cron
//! scheduling. Personal agent, local-first.

pub mod config;
pub mod gateway;
pub(crate) mod hook;

pub use config::DaemonConfig;
pub use gateway::{
    Gateway,
    builder::build_runtime,
    handle::{ServeHandle, serve, serve_with_config},
};
pub use hook::GatewayHook;
