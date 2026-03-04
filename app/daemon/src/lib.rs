//! Walrus daemon — message central composing runtime, channels, and cron
//! scheduling. Personal agent, local-first.

pub mod config;
pub mod daemon;
pub(crate) mod hook;

pub use config::DaemonConfig;
pub use daemon::{Daemon, DaemonHandle};
pub use hook::DaemonHook;
