//! Cron markdown loading.

use crate::loader::split_yaml_frontmatter;
use compact_str::CompactString;
use serde::Deserialize;
use std::path::Path;

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
