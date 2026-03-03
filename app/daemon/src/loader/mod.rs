//! Markdown-based configuration loading for agents, cron jobs, and skills.
//!
//! All filesystem I/O and YAML frontmatter parsing lives here, keeping the
//! runtime crate free of `std::fs` and `serde_yaml` dependencies.

pub use agent::{load_agents_dir, parse_agent_md};
pub use cron::{CronEntry, load_cron_dir, parse_cron_md};

pub mod agent;
pub mod cron;

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
