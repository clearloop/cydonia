//! Tool dispatch and schema registration for skill tools.

use super::{LoadSkillInput, SearchSkillInput, loader};
use crate::hook::DaemonHook;
use wcore::{ToolRegistry, model::Tool};

pub(crate) fn register_tools(tools: &mut ToolRegistry) {
    tools.insert(Tool {
        name: "search_skill".into(),
        description: "Search available skills by keyword. Returns name and description only."
            .into(),
        parameters: schemars::schema_for!(SearchSkillInput),
        strict: false,
    });
    tools.insert(Tool {
        name: "load_skill".into(),
        description: "Load a skill by name. Returns its instructions and the skill directory path for resolving relative file references.".into(),
        parameters: schemars::schema_for!(LoadSkillInput),
        strict: false,
    });
}

impl DaemonHook {
    pub(crate) async fn dispatch_search_skill(&self, args: &str) -> String {
        let input: SearchSkillInput = match serde_json::from_str(args) {
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
        let input: LoadSkillInput = match serde_json::from_str(args) {
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
