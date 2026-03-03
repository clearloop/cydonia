//! Team composition: build worker tools for a leader agent.
//!
//! Each worker agent is exposed as a tool on the leader. When the leader
//! calls a worker tool, the handler creates an ephemeral Agent instance
//! and runs it using the Hook for dispatch and model access.
//!
//! # Example
//!
//! ```rust,ignore
//! use walrus_core::AgentConfig;
//! use walrus_runtime::{build_team, worker_tool};
//!
//! let leader = AgentConfig::new("leader").system_prompt("You coordinate.");
//! let analyst = AgentConfig::new("analyst").description("Market analysis");
//!
//! let (leader, worker_tools) = build_team(leader, vec![analyst], &hook);
//! // Register worker_tools on your Hook implementation, then:
//! runtime.add_agent(leader);
//! ```

use crate::{Handler, Hook};
use compact_str::CompactString;
use std::sync::Arc;
use wcore::AgentConfig;
use wcore::model::{Message, Tool};

/// Build a team: create worker tool definitions and handlers for a leader.
///
/// Returns the leader config (with worker tool names appended) and a list
/// of `(AgentConfig, Tool, Handler)` triples for each worker. The caller
/// is responsible for registering the tools on their Hook implementation
/// and adding the worker AgentConfigs to the Runtime.
pub fn build_team<H: Hook + 'static>(
    mut leader: AgentConfig,
    workers: Vec<AgentConfig>,
    hook: &Arc<H>,
) -> (AgentConfig, Vec<(AgentConfig, Tool, Handler)>) {
    let mut worker_entries = Vec::with_capacity(workers.len());

    for worker in workers {
        let tool_def = worker_tool(worker.name.clone(), worker.description.to_string());

        let hook = Arc::clone(hook);
        let worker_config = worker.clone();
        let handler: Handler = Arc::new(move |args| {
            let hook = Arc::clone(&hook);
            let config = worker_config.clone();
            Box::pin(async move {
                let input = match extract_input(&args) {
                    Ok(input) => input,
                    Err(e) => return format!("invalid arguments: {e}"),
                };
                worker_send(&*hook, &config, input).await
            })
        });

        leader.tools.push(worker.name.clone());
        worker_entries.push((worker, tool_def, handler));
    }

    (leader, worker_entries)
}

/// Run a worker agent using Agent.run().
///
/// Creates an ephemeral Agent with a fresh event channel, enriches the
/// system prompt via Hook, pushes the user input, and runs to completion.
async fn worker_send<H: Hook>(hook: &H, config: &AgentConfig, input: String) -> String {
    let mut config = config.clone();
    config.system_prompt = hook.enrich_prompt(&config);

    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let mut agent = wcore::AgentBuilder::new(tx).config(config).build();
    agent.push_message(Message::user(&input));

    // Drain events (discard for workers).
    tokio::spawn(async move { while rx.recv().await.is_some() {} });

    // Workers use the hook directly as their context for model access.
    let agent_name = agent.config.name.clone();
    let dispatcher = crate::AgentDispatcher {
        hook,
        agent: &agent_name,
    };
    let response = agent.run(hook.model(), &dispatcher).await;
    response.final_response.unwrap_or_default()
}

/// Build a tool definition for a worker agent.
///
/// Uses a standard `{ input: string }` schema so the leader
/// can delegate tasks with a single text field.
pub fn worker_tool(name: impl Into<CompactString>, description: impl Into<String>) -> Tool {
    Tool {
        name: name.into(),
        description: description.into(),
        parameters: default_input_schema(),
        strict: true,
    }
}

/// Extract the `input` field from tool call arguments JSON.
pub fn extract_input(arguments: &str) -> anyhow::Result<String> {
    let parsed: serde_json::Value = serde_json::from_str(arguments)?;
    parsed
        .get("input")
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| anyhow::anyhow!("missing 'input' field in arguments"))
}

/// Default input schema for agent-as-tool calls.
#[derive(schemars::JsonSchema, serde::Deserialize)]
#[allow(dead_code)]
struct DefaultInput {
    /// The task or question to delegate to this agent.
    input: String,
}

fn default_input_schema() -> schemars::Schema {
    schemars::schema_for!(DefaultInput)
}
