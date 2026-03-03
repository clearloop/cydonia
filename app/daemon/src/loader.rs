//! Markdown-based configuration loading for agents, cron jobs, and skills.
//!
//! All filesystem I/O and YAML frontmatter parsing lives here, keeping the
//! runtime crate free of `std::fs` and `serde_yaml` dependencies.

use crate::feature::skill::{Skill, SkillRegistry, SkillTier};
use compact_str::CompactString;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use wcore::AgentConfig;

// ── YAML frontmatter helpers ───────────────────────────────────────────

/// Split YAML frontmatter from the body. Frontmatter is delimited by `---`.
///
/// Handles CRLF line endings and trailing whitespace on delimiter lines.
pub fn split_yaml_frontmatter(content: &str) -> anyhow::Result<(&str, &str)> {
    let content = content.trim_start();
    if !content.starts_with("---") {
        anyhow::bail!("missing YAML frontmatter delimiter (---)");
    }

    // Skip opening delimiter and its trailing newline.
    let after_first = content[3..].trim_start_matches(['\n', '\r']);

    // Scan line-by-line for the closing `---` delimiter.
    let mut pos = 0;
    for line in after_first.lines() {
        if line.trim() == "---" {
            let frontmatter = &after_first[..pos].trim_end();
            let body_start = pos + line.len();
            // Skip the newline after `---` if present.
            let body = after_first[body_start..].trim_start_matches(['\n', '\r']);
            return Ok((frontmatter, body));
        }
        pos += line.len() + 1; // +1 for the newline consumed by lines()
    }

    anyhow::bail!("missing closing YAML frontmatter delimiter (---)")
}

// ── Skill loading ──────────────────────────────────────────────────────

/// YAML frontmatter deserialization target for SKILL.md files.
#[derive(Debug, Deserialize)]
struct SkillFrontmatter {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    license: Option<String>,
    #[serde(default)]
    compatibility: Option<String>,
    #[serde(default)]
    metadata: BTreeMap<String, String>,
    #[serde(default, rename = "allowed-tools")]
    allowed_tools: Option<String>,
}

/// Parse a SKILL.md file (YAML frontmatter + Markdown body) into a [`Skill`].
pub fn parse_skill_md(content: &str) -> anyhow::Result<Skill> {
    let (frontmatter, body) = split_yaml_frontmatter(content)?;
    let fm: SkillFrontmatter = serde_yaml::from_str(frontmatter)?;

    let allowed_tools = fm
        .allowed_tools
        .map(|s| {
            s.split_whitespace()
                .map(CompactString::from)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let metadata = fm
        .metadata
        .into_iter()
        .map(|(k, v)| (CompactString::from(k), v))
        .collect();

    Ok(Skill {
        name: CompactString::from(fm.name),
        description: fm.description,
        license: fm.license.map(CompactString::from),
        compatibility: fm.compatibility.map(CompactString::from),
        metadata,
        allowed_tools,
        body: body.to_owned(),
    })
}

/// Load skills from a directory. Each subdirectory should contain a `SKILL.md`.
/// The given tier is assigned to all loaded skills.
pub fn load_skills_dir(path: impl AsRef<Path>, tier: SkillTier) -> anyhow::Result<SkillRegistry> {
    let path = path.as_ref();
    let mut registry = SkillRegistry::new();

    let entries = std::fs::read_dir(path)
        .map_err(|e| anyhow::anyhow!("failed to read skill directory {}: {e}", path.display()))?;

    for entry in entries {
        let entry = entry?;
        let entry_path = entry.path();
        if !entry_path.is_dir() {
            continue;
        }

        let skill_file = entry_path.join("SKILL.md");
        if !skill_file.exists() {
            continue;
        }

        let content = std::fs::read_to_string(&skill_file)
            .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", skill_file.display()))?;

        let skill = parse_skill_md(&content)?;
        registry.add(skill, tier);
    }

    Ok(registry)
}

// ── Agent loading ──────────────────────────────────────────────────────

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

// ── Cron loading ───────────────────────────────────────────────────────

/// A cron job entry parsed from a markdown file.
#[derive(Debug, Clone)]
pub struct CronEntry {
    /// Cron job name.
    pub name: CompactString,
    /// Cron schedule expression (e.g. "0 0 9 * * *").
    pub schedule: String,
    /// Name of the agent to invoke.
    pub agent: CompactString,
    /// Message template (from the markdown body).
    pub message: String,
}

/// YAML frontmatter for cron markdown files.
#[derive(Deserialize)]
struct CronFrontmatter {
    name: String,
    schedule: String,
    agent: String,
}

/// Parse a cron markdown file (YAML frontmatter + body) into a [`CronEntry`].
///
/// The frontmatter provides name, schedule, and agent. The markdown body
/// (trimmed) becomes the cron entry's message template.
pub fn parse_cron_md(content: &str) -> anyhow::Result<CronEntry> {
    let (frontmatter, body) = split_yaml_frontmatter(content)?;
    let fm: CronFrontmatter = serde_yaml::from_str(frontmatter)?;

    Ok(CronEntry {
        name: CompactString::from(fm.name),
        schedule: fm.schedule,
        agent: CompactString::from(fm.agent),
        message: body.trim().to_owned(),
    })
}

/// Load all cron markdown files from a directory.
///
/// Each `.md` file is parsed with [`parse_cron_md`]. Non-`.md` files are
/// silently skipped. Entries are sorted by filename for deterministic ordering.
/// Returns an empty vec if the directory does not exist.
pub fn load_cron_dir(path: &Path) -> anyhow::Result<Vec<CronEntry>> {
    if !path.exists() {
        tracing::warn!("cron directory does not exist: {}", path.display());
        return Ok(Vec::new());
    }

    let mut entries: Vec<_> = std::fs::read_dir(path)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
        .collect();
    entries.sort_by_key(|e| e.file_name());

    let mut crons = Vec::with_capacity(entries.len());
    for entry in entries {
        let content = std::fs::read_to_string(entry.path())?;
        crons.push(parse_cron_md(&content)?);
    }

    Ok(crons)
}
