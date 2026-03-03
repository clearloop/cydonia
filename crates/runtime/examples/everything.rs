//! Everything example — combines custom tools, memory, skills, and teams.
//!
//! Demonstrates the full runtime feature set:
//! 1. Custom tool registration (current_time)
//! 2. Skill injection (concise style)
//! 3. Memory context (user preferences)
//! 4. Team delegation (leader + analyst worker)
//!
//! Requires DEEPSEEK_API_KEY. Run with:
//! ```sh
//! cargo run -p walrus-runtime --example everything
//! ```

mod common;

use std::sync::Arc;
use walrus_runtime::{Hook, Memory, Skill, SkillRegistry, SkillTier, build_team, prelude::*};

/// Hook that wraps ExampleHook and adds skill enrichment.
struct EverythingHook {
    inner: common::ExampleHook,
    skills: SkillRegistry,
}

impl Hook for EverythingHook {
    type Model = model::ProviderManager;

    fn model(&self) -> &model::ProviderManager {
        self.inner.model()
    }

    fn tools(&self, agent: &str) -> Vec<Tool> {
        self.inner.tools(agent)
    }

    fn dispatch(
        &self,
        agent: &str,
        calls: &[(&str, &str)],
    ) -> impl std::future::Future<Output = Vec<anyhow::Result<String>>> + Send {
        self.inner.dispatch(agent, calls)
    }

    fn enrich_prompt(&self, config: &AgentConfig) -> String {
        let mut prompt = config.system_prompt.clone();
        for skill in self.skills.find_by_tags(&config.skill_tags) {
            if !skill.body.is_empty() {
                prompt.push_str("\n\n");
                prompt.push_str(&skill.body);
            }
        }
        prompt
    }
}

#[tokio::main]
async fn main() {
    common::init_tracing();
    let mut inner = common::build_hook();

    // 1. Register a custom tool.
    let time_tool = Tool {
        name: "current_time".into(),
        description: "Returns the current UTC date and time.".into(),
        parameters: serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {}
        }))
        .unwrap(),
        strict: false,
    };
    inner.register(
        time_tool,
        |_| async move { chrono::Utc::now().to_rfc3339() },
    );

    // 2. Load a skill.
    let mut skills = SkillRegistry::new();
    skills.add(
        Skill {
            name: "concise".into(),
            description: "Encourages concise responses".into(),
            license: None,
            compatibility: None,
            metadata: [("tags".into(), "style".into())].into_iter().collect(),
            allowed_tools: vec![],
            body: "Always respond in 2-3 sentences maximum.".into(),
        },
        SkillTier::Bundled,
    );

    // 3. Store memory context.
    inner
        .memory()
        .set("preference", "User prefers direct answers with examples.");

    let hook = Arc::new(EverythingHook { inner, skills });

    // 4. Build a team: leader delegates to analyst worker.
    let leader = AgentConfig::new("leader")
        .system_prompt("You are a team leader. Delegate research to the analyst.")
        .skill_tag("style")
        .tool("current_time");
    let analyst = AgentConfig::new("analyst")
        .description("Research analyst — answers factual questions.")
        .system_prompt("You are a research analyst. Provide well-reasoned answers.")
        .tool("current_time");

    let (leader, worker_entries) = build_team(leader, vec![analyst], &hook);

    let mut runtime = Runtime::new(Arc::clone(&hook));
    for (worker_config, _tool, _handler) in worker_entries {
        runtime.add_agent(worker_config);
    }
    runtime.add_agent(leader);

    println!("Everything REPL — leader + analyst team, tools, memory, skills");
    println!("(type 'exit' to quit)");
    println!("---");
    common::repl(&runtime, "leader").await;
}
