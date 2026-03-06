//! Walrus skill registry — tag-indexed skill matching and prompt enrichment.
//!
//! Skills are named units of agent behavior loaded from Markdown files with
//! YAML frontmatter. The [`SkillRegistry`] indexes skills by tags and triggers,
//! and implements [`Hook`] to enrich agent system prompts based on skill tags.

use wcore::Hook;
pub use {
    handler::SkillHandler,
    registry::{Skill, SkillRegistry, SkillTier},
};

mod handler;
pub mod loader;
pub mod registry;

impl Hook for SkillHandler {
    fn on_build_agent(&self, mut config: wcore::AgentConfig) -> wcore::AgentConfig {
        for skill in self.registry.find_by_tags(&config.skill_tags) {
            if !skill.body.is_empty() {
                config.system_prompt.push_str("\n\n");
                config.system_prompt.push_str(&skill.body);
            }
        }
        config
    }
}
