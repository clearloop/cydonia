//! Walrus system hooks — skill registry, MCP bridge, and cron scheduler.
//!
//! Combines the three core hook subsystems into a single crate. Each subsystem
//! lives in its own module and implements [`wcore::Hook`].

pub use cron::{CronHandler, CronJob};
pub use mcp::{McpBridge, McpHandler, McpServerConfig};
pub use skill::{Skill, SkillHandler, SkillRegistry, SkillTier};

pub mod agent;
pub mod cron;
pub mod hub;
pub mod mcp;
pub mod skill;
