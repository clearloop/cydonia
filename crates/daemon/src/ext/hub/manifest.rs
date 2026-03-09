//! walrus hub manifest

use crate::hook::mcp::McpServerConfig;
use compact_str::CompactString;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Walrus resource manifest
#[derive(Serialize, Deserialize)]
pub struct Manifest {
    /// the package manifest
    pub package: Package,

    /// MCP server configs
    pub mcp_servers: BTreeMap<CompactString, McpServerConfig>,

    /// Skill resources
    pub skills: BTreeMap<CompactString, SkillResource>,
}

/// The package manifest
#[derive(Serialize, Deserialize)]
pub struct Package {
    pub name: CompactString,
}

/// A skill resource
#[derive(Serialize, Deserialize)]
pub struct SkillResource {
    /// Skill name (defaults to map key if empty)
    #[serde(default)]
    pub name: CompactString,
    /// Skill description
    pub description: CompactString,
    /// Skill repository URL
    pub repo: CompactString,
    /// Path within the repo to the skill directory
    pub path: CompactString,
}
