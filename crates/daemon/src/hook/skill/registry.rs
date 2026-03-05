//! Walrus skill registry — tag-indexed skill matching and prompt enrichment.

use compact_str::CompactString;
use std::collections::BTreeMap;

/// A registry of loaded skills with tag and trigger indices.
#[derive(Debug, Clone)]
pub struct SkillRegistry {
    skills: Vec<IndexedSkill>,
    tag_index: BTreeMap<CompactString, Vec<usize>>,
    trigger_index: BTreeMap<CompactString, Vec<usize>>,
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            skills: Vec::new(),
            tag_index: BTreeMap::new(),
            trigger_index: BTreeMap::new(),
        }
    }

    /// Add a skill to the registry with the given tier.
    pub fn add(&mut self, skill: Skill, tier: SkillTier) {
        let priority = skill
            .metadata
            .get("priority")
            .and_then(|v| v.parse::<u8>().ok())
            .unwrap_or(0);

        let idx = self.skills.len();

        // Index tags from metadata["tags"] (comma-separated).
        if let Some(tags) = skill.metadata.get("tags") {
            for tag in tags.split(',') {
                let tag = tag.trim();
                if !tag.is_empty() {
                    self.tag_index
                        .entry(CompactString::from(tag))
                        .or_default()
                        .push(idx);
                }
            }
        }

        // Index triggers from metadata["triggers"] (comma-separated).
        if let Some(triggers) = skill.metadata.get("triggers") {
            for trigger in triggers.split(',') {
                let trigger = trigger.trim().to_lowercase();
                if !trigger.is_empty() {
                    self.trigger_index
                        .entry(CompactString::from(trigger))
                        .or_default()
                        .push(idx);
                }
            }
        }

        self.skills.push(IndexedSkill {
            skill,
            tier,
            priority,
        });
    }

    /// Find skills matching any of the given tags, sorted by tier (desc) then priority (desc).
    pub fn find_by_tags(&self, tags: &[CompactString]) -> Vec<&Skill> {
        let mut indices: Vec<usize> = tags
            .iter()
            .filter_map(|tag| self.tag_index.get(tag))
            .flatten()
            .copied()
            .collect();

        indices.sort_unstable();
        indices.dedup();

        indices.sort_by(|&a, &b| {
            let sa = &self.skills[a];
            let sb = &self.skills[b];
            sb.tier
                .cmp(&sa.tier)
                .then_with(|| sb.priority.cmp(&sa.priority))
        });

        indices.iter().map(|&i| &self.skills[i].skill).collect()
    }

    /// Find skills whose trigger keywords match the query (case-insensitive).
    pub fn find_by_trigger(&self, query: &str) -> Vec<&Skill> {
        let query_lower = query.to_lowercase();
        let mut indices: Vec<usize> = self
            .trigger_index
            .iter()
            .filter(|(keyword, _)| query_lower.contains(keyword.as_str()))
            .flat_map(|(_, idxs)| idxs.iter().copied())
            .collect();

        indices.sort_unstable();
        indices.dedup();

        indices.sort_by(|&a, &b| {
            let sa = &self.skills[a];
            let sb = &self.skills[b];
            sb.tier
                .cmp(&sa.tier)
                .then_with(|| sb.priority.cmp(&sa.priority))
        });

        indices.iter().map(|&i| &self.skills[i].skill).collect()
    }

    /// Get all loaded skills.
    pub fn skills(&self) -> Vec<&Skill> {
        self.skills.iter().map(|s| &s.skill).collect()
    }

    /// Number of loaded skills.
    pub fn len(&self) -> usize {
        self.skills.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }
}

/// Priority tier for skill resolution.
///
/// Variant order defines precedence: Workspace overrides Managed, which
/// overrides Bundled. Assigned by the registry at load time based on
/// source directory — not stored in the skill file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SkillTier {
    /// Ships with the binary.
    Bundled,
    /// Installed via package manager.
    Managed,
    /// Defined in the project workspace.
    Workspace,
}

/// A named unit of agent behavior (agentskills.io format).
///
/// Pure data struct — parsing logic lives in the [`loader`] module.
/// Fields mirror the agentskills.io specification. Runtime-only concepts
/// like tier and priority live in the registry, not here.
#[derive(Debug, Clone)]
pub struct Skill {
    /// Skill identifier (lowercase, hyphens, 1-64 chars).
    pub name: CompactString,
    /// Human-readable description (1-1024 chars).
    pub description: String,
    /// SPDX license identifier.
    pub license: Option<CompactString>,
    /// Compatibility constraints (e.g. "walrus>=0.1").
    pub compatibility: Option<CompactString>,
    /// Arbitrary key-value metadata map.
    pub metadata: BTreeMap<CompactString, String>,
    /// Tool names this skill is allowed to use.
    pub allowed_tools: Vec<CompactString>,
    /// Skill body (Markdown instructions).
    pub body: String,
}

/// An indexed skill with its tier and priority (extracted from metadata).
#[derive(Debug, Clone)]
struct IndexedSkill {
    skill: Skill,
    tier: SkillTier,
    priority: u8,
}
