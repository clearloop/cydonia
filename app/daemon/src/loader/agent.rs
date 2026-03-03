//! Agent markdown loading.

use crate::loader::split_yaml_frontmatter;
use serde::Deserialize;
use std::path::Path;
use wcore::AgentConfig;

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

/// Load all agent markdown files from a directory.
///
/// Each `.md` file is parsed with [`parse_agent_md`]. Non-`.md` files are
/// silently skipped. Entries are sorted by filename for deterministic ordering.
/// Returns an empty vec if the directory does not exist.
pub fn load_agents_dir(path: &Path) -> anyhow::Result<Vec<AgentConfig>> {
    if !path.exists() {
        tracing::warn!("agent directory does not exist: {}", path.display());
        return Ok(Vec::new());
    }

    let mut entries: Vec<_> = std::fs::read_dir(path)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
        .collect();
    entries.sort_by_key(|e| e.file_name());

    let mut agents = Vec::with_capacity(entries.len());
    for entry in entries {
        let content = std::fs::read_to_string(entry.path())?;
        agents.push(parse_agent_md(&content)?);
    }

    Ok(agents)
}
