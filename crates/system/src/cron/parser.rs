//! Cron markdown parsing.
//!
//! Parses YAML frontmatter + Markdown body into a [`CronEntry`].

use compact_str::CompactString;
use serde::Deserialize;
use wcore::utils::split_yaml_frontmatter;

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
