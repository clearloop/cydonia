//! Directory-based configuration loading for agents.
//!
//! Handles filesystem I/O: reads directories, sorts entries, delegates
//! parsing to [`wcore::parse_agent_md`].

use std::path::Path;
use wcore::AgentConfig;

/// Load all agent markdown files from a directory.
///
/// Each `.md` file is parsed with [`wcore::parse_agent_md`]. Non-`.md` files
/// are silently skipped. Entries are sorted by filename for deterministic
/// ordering. Returns an empty vec if the directory does not exist.
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
        agents.push(wcore::parse_agent_md(&content)?);
    }

    Ok(agents)
}
