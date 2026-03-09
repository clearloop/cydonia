//! Tool dispatch and schema registration for skill tools.

use crate::hook::{DaemonHook, skill::loader};
use serde::Deserialize;
use wcore::{
    agent::{AsTool, ToolDescription},
    model::Tool,
};

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct SearchSkill {
    /// Keyword to match skill names and descriptions. Leave empty to list all.
    pub query: String,
}

impl ToolDescription for SearchSkill {
    const DESCRIPTION: &'static str =
        "Search available skills by keyword. Returns name and description only.";
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct LoadSkill {
    /// Skill name
    pub name: String,
}

impl ToolDescription for LoadSkill {
    const DESCRIPTION: &'static str = "Load a skill by name. Returns its instructions and the skill directory path for resolving relative file references.";
}

pub(crate) fn tools() -> Vec<Tool> {
    vec![SearchSkill::as_tool(), LoadSkill::as_tool()]
}

impl DaemonHook {
    pub(crate) async fn dispatch_search_skill(&self, args: &str) -> String {
        let input: SearchSkill = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments: {e}"),
        };
        let query = input.query.to_lowercase();
        let registry = self.skills.registry.lock().await;
        let matches: Vec<String> = registry
            .skills()
            .into_iter()
            .filter(|s| {
                s.name.to_lowercase().contains(&query)
                    || s.description.to_lowercase().contains(&query)
            })
            .map(|s| format!("{}: {}", s.name, s.description))
            .collect();
        if matches.is_empty() {
            "no skills found".to_owned()
        } else {
            matches.join("\n")
        }
    }

    pub(crate) async fn dispatch_load_skill(&self, args: &str) -> String {
        let input: LoadSkill = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments: {e}"),
        };
        let name = &input.name;
        // Guard against path traversal in the skill name.
        if name.contains("..") || name.contains('/') || name.contains('\\') {
            return format!("invalid skill name: {name}");
        }
        let skill_dir = self.skills.skills_dir.join(name);
        let skill_file = skill_dir.join("SKILL.md");
        let content = match tokio::fs::read_to_string(&skill_file).await {
            Ok(c) => c,
            Err(_) => return format!("skill not found: {name}"),
        };
        let skill = match loader::parse_skill_md(&content) {
            Ok(s) => s,
            Err(e) => return format!("failed to parse skill: {e}"),
        };
        let body = skill.body.clone();
        self.skills.registry.lock().await.add(skill);
        let dir_path = skill_dir.display();
        format!("{body}\n\nSkill directory: {dir_path}")
    }
}
