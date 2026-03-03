//! Agent example — minimal streaming REPL.
//!
//! The simplest possible agent: one system prompt, streaming responses.
//!
//! Requires DEEPSEEK_API_KEY. Run with:
//! ```sh
//! cargo run -p walrus-runtime --example agent
//! ```

mod common;

use walrus_runtime::prelude::*;

#[tokio::main]
async fn main() {
    common::init_tracing();
    let (runtime, _hook) = common::build_runtime();

    runtime
        .add_agent(
            AgentConfig::new("assistant").system_prompt("You are a helpful assistant. Be concise."),
        )
        .await;

    println!("Agent REPL (type 'exit' to quit)");
    println!("---");
    common::repl(&runtime, "assistant").await;
}
