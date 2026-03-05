//! System agent implementation.

use wcore::AgentConfig;

const SYSTEM_AGENT: &str = include_str!("../../prompts/system.md");

/// Parse the system agent from the system prompt.
pub fn system_agent() -> anyhow::Result<AgentConfig> {
    wcore::parse_agent_md(SYSTEM_AGENT)
}
