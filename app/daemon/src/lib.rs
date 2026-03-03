//! Walrus daemon — message central composing runtime, channels, and cron
//! scheduling. Personal agent, local-first.

pub mod config;
pub mod gateway;
pub mod loader;

pub use config::DaemonConfig;
pub use gateway::{
    Gateway, GatewayHook,
    builder::build_runtime,
    serve::{ServeHandle, serve, serve_with_config},
};
