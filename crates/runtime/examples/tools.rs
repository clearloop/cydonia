//! Tools example — interactive REPL with a tool LLMs can't do natively.
//!
//! Registers a `current_time` tool that returns the actual UTC time —
//! something LLMs don't have access to.
//!
//! Requires DEEPSEEK_API_KEY. Run with:
//! ```sh
//! cargo run -p walrus-runtime --example tools
//! ```

mod common;

use std::sync::Arc;
use walrus_runtime::prelude::*;

#[tokio::main]
async fn main() {
    common::init_tracing();
    let mut hook = common::build_hook();

    // current_time: LLMs don't know the current time.
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
    hook.register(
        time_tool,
        |_| async move { chrono::Utc::now().to_rfc3339() },
    );

    let runtime = Runtime::new(Arc::new(hook));

    runtime
        .add_agent(
            AgentConfig::new("assistant")
                .system_prompt(
                    "You are a helpful assistant with access to tools. \
                     Use current_time when the user asks about the current time or date.",
                )
                .tool("current_time"),
        )
        .await;

    println!("Tools REPL — try asking:");
    println!("  'What time is it?'");
    println!("  'What day of the week is it today?'");
    println!("(type 'exit' to quit)");
    println!("---");
    common::repl(&runtime, "assistant").await;
}
