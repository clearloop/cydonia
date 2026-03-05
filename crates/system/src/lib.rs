//! Walrus system hooks — skill registry and MCP bridge.
//!
//! Combines the two core hook subsystems into a single crate. Each subsystem
//! lives in its own module and implements [`wcore::Hook`].

pub use mcp::{McpBridge, McpHandler, McpServerConfig};
pub use skill::{Skill, SkillHandler, SkillRegistry, SkillTier};

pub mod hub;
pub mod mcp;
pub mod skill;
