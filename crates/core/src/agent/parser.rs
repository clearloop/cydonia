//! Agent markdown parsing.
//!
//! Parses YAML frontmatter + Markdown body into an [`AgentConfig`].

use crate::{agent::config::AgentConfig, utils::split_yaml_frontmatter};
use serde::Deserialize;

/// YAML frontmatter for agent markdown files.
#[derive(Deserialize)]
struct AgentFrontmatter {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    tools: Vec<String>,
    #[serde(default)]
    skill_tags: Vec<String>,
    #[serde(default)]
    model: Option<String>,
}

/// Parse an agent markdown file (YAML frontmatter + body) into an [`AgentConfig`].
///
/// The frontmatter provides name, description, tools, and skill_tags.
/// The markdown body (trimmed) becomes the agent's system prompt.
pub fn parse_agent_md(content: &str) -> anyhow::Result<AgentConfig> {
    let (frontmatter, body) = split_yaml_frontmatter(content)?;
    let fm: AgentFrontmatter = serde_yaml::from_str(frontmatter)?;

    let config = AgentConfig {
        name: fm.name.into(),
        description: fm.description.into(),
        system_prompt: body.trim().to_owned(),
        model: fm.model.map(Into::into),
        tools: fm.tools.into_iter().map(Into::into).collect(),
        skill_tags: fm.skill_tags.into_iter().map(Into::into).collect(),
        ..AgentConfig::default()
    };

    Ok(config)
}
