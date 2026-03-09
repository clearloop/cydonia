//! Permission configuration types for tool access control.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Per-tool permission level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ToolPermission {
    /// Tool proceeds without confirmation.
    #[default]
    Allow,
    /// Tool blocks and waits for user approval.
    Ask,
    /// Tool is rejected immediately.
    Deny,
}

/// Permission configuration: global defaults + per-agent overrides.
///
/// TOML layout:
/// ```toml
/// [permissions]
/// bash = "ask"
/// write = "deny"
///
/// [permissions.researcher]
/// bash = "deny"
/// ```
///
/// String values are global defaults; table values are per-agent overrides.
/// Uses a custom deserializer to split them.
#[derive(Debug, Clone, Serialize, Default)]
pub struct PermissionConfig {
    /// Global tool permission defaults (tool_name → permission).
    pub defaults: BTreeMap<String, ToolPermission>,
    /// Per-agent overrides (agent_name → tool_name → permission).
    pub agents: BTreeMap<String, BTreeMap<String, ToolPermission>>,
}

impl<'de> Deserialize<'de> for PermissionConfig {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw: BTreeMap<String, toml::Value> = BTreeMap::deserialize(deserializer)?;
        let mut defaults = BTreeMap::new();
        let mut agents = BTreeMap::new();
        for (key, value) in raw {
            match value {
                toml::Value::String(s) => {
                    let perm: ToolPermission =
                        serde::Deserialize::deserialize(toml::Value::String(s))
                            .map_err(serde::de::Error::custom)?;
                    defaults.insert(key, perm);
                }
                toml::Value::Table(table) => {
                    let agent_perms: BTreeMap<String, ToolPermission> = table
                        .into_iter()
                        .map(|(k, v)| {
                            let perm: ToolPermission = serde::Deserialize::deserialize(v)
                                .map_err(|e| format!("permissions.{key}.{k}: {e}"))
                                .unwrap_or_default();
                            (k, perm)
                        })
                        .collect();
                    agents.insert(key, agent_perms);
                }
                _ => {}
            }
        }
        Ok(PermissionConfig { defaults, agents })
    }
}

impl PermissionConfig {
    /// Resolve the effective permission for a given agent + tool.
    ///
    /// Priority: agent override > global default > Allow.
    pub fn resolve(&self, agent: &str, tool: &str) -> ToolPermission {
        if let Some(agent_perms) = self.agents.get(agent)
            && let Some(&perm) = agent_perms.get(tool)
        {
            return perm;
        }
        self.defaults.get(tool).copied().unwrap_or_default()
    }
}
