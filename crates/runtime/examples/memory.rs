//! Memory example — interactive REPL showing memory context in action.
//!
//! Pre-seeds user context into memory, then starts a REPL. Memory state
//! is printed after each exchange.
//!
//! Requires DEEPSEEK_API_KEY. Run with:
//! ```sh
//! cargo run -p walrus-runtime --example memory
//! ```

mod common;

use std::sync::Arc;
use walrus_runtime::{Memory, prelude::*};

#[tokio::main]
async fn main() {
    common::init_tracing();
    let hook = common::build_hook();

    // Pre-seed memory with user context.
    hook.memory().set("user_name", "Alex");
    hook.memory()
        .set("preference", "Prefers concise answers with code examples.");
    hook.memory()
        .set("learning", "Currently learning Rust, focus on async.");

    let hook = Arc::new(hook);
    let runtime = Runtime::new(Arc::clone(&hook));

    runtime
        .add_agent(AgentConfig::new("assistant").system_prompt(
            "You are a helpful assistant. Use any stored memory about the user \
                     to personalize your responses.",
        ))
        .await;

    println!("Memory REPL — the assistant knows your stored context.");
    println!("Try: 'What do you know about me?' or tell it something new.");
    println!("(type 'exit' to quit)");
    println!("---");

    // Show initial memory state.
    let entries = hook.memory().entries();
    println!("[Memory: {} entries]", entries.len());
    for (key, value) in &entries {
        println!("  {key} = {value}");
    }
    println!();

    common::repl_with_memory(&runtime, &hook, "assistant").await;
}
