//! Walrus gateway — application shell composing runtime, channels, and cron
//! scheduling. Personal agent, local-first.

pub mod config;
pub mod feature;
pub mod gateway;
pub mod loader;

pub use channel::{ChannelRouter, RoutingRule};
pub use config::DaemonConfig;
pub use feature::cron::{CronJob, CronScheduler};
pub use gateway::{
    Gateway, GatewayHook,
    builder::build_runtime,
    serve::{ServeHandle, serve, serve_with_config},
};
