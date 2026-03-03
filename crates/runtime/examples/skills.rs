//! Skills example — side-by-side comparison showing skill behavioral effects.
//!
//! Creates two agents with identical prompts: one with a "concise" skill
//! tag, one without. Sends the same questions to both and prints responses
//! side by side so you can see the skill body modifying LLM behavior.
//!
//! Requires DEEPSEEK_API_KEY. Run with:
//! ```sh
//! cargo run -p walrus-runtime --example skills
//! ```

mod common;

use std::sync::Arc;
use walrus_runtime::{Hook, Skill, SkillRegistry, SkillTier, prelude::*};

/// Hook that wraps ExampleHook but enriches prompts with a SkillRegistry.
struct SkillHook {
    inner: common::ExampleHook,
    skills: SkillRegistry,
}

impl Hook for SkillHook {
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
    let inner = common::build_hook();

    // Register a "concise" skill that constrains response length.
    let mut skills = SkillRegistry::new();
    skills.add(
        Skill {
            name: "concise".into(),
            description: "Constrains responses to exactly 2 sentences.".into(),
            license: None,
            compatibility: None,
            metadata: [("tags".into(), "style".into())].into_iter().collect(),
            allowed_tools: vec![],
            body: "Always respond in exactly 2 sentences. No exceptions.".into(),
        },
        SkillTier::Bundled,
    );

    let hook = Arc::new(SkillHook { inner, skills });
    let mut runtime = Runtime::new(Arc::clone(&hook));

    // Two agents: same base prompt, different skill tags.
    runtime.add_agent(
        AgentConfig::new("default").system_prompt("You are a helpful programming assistant."),
    );
    runtime.add_agent(
        AgentConfig::new("concise")
            .system_prompt("You are a helpful programming assistant.")
            .skill_tag("style"),
    );

    let prompts = [
        "Explain what Rust's ownership system is.",
        "How do I create and use a HashMap in Rust?",
        "What are async/await and why are they useful?",
    ];

    for &prompt in &prompts {
        println!("======================================");
        println!("Question: {prompt}");
        println!("--------------------------------------");

        // Send to default agent (no skill).
        let default_response = runtime
            .send_to("default", Message::user(prompt))
            .await
            .expect("default agent failed");
        println!(
            "\n[default agent]:\n{}",
            default_response.content().cloned().unwrap_or_default()
        );

        // Send to concise agent (with skill).
        let concise_response = runtime
            .send_to("concise", Message::user(prompt))
            .await
            .expect("concise agent failed");
        println!(
            "\n[concise agent (skill: 2 sentences)]:\n{}",
            concise_response.content().cloned().unwrap_or_default()
        );

        // Clear sessions so each question is independent.
        runtime.clear_session("default").await;
        runtime.clear_session("concise").await;

        println!();
    }
}
