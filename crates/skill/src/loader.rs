//! Skill markdown loading.
//!
//! Parses `SKILL.md` files (YAML frontmatter + Markdown body) from skill
//! directories and builds a [`SkillRegistry`].

use crate::{Skill, SkillRegistry, SkillTier};
use compact_str::CompactString;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

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

/// Split YAML frontmatter from the body. Frontmatter is delimited by `---`.
fn split_yaml_frontmatter(content: &str) -> anyhow::Result<(&str, &str)> {
    let content = content.trim_start();
    if !content.starts_with("---") {
        anyhow::bail!("missing YAML frontmatter delimiter (---)");
    }

    let after_first = content[3..].trim_start_matches(['\n', '\r']);

    let mut pos = 0;
    for line in after_first.lines() {
        if line.trim() == "---" {
            let frontmatter = &after_first[..pos].trim_end();
            let body_start = pos + line.len();
            let body = after_first[body_start..].trim_start_matches(['\n', '\r']);
            return Ok((frontmatter, body));
        }
        pos += line.len() + 1;
    }

    anyhow::bail!("missing closing YAML frontmatter delimiter (---)")
}
