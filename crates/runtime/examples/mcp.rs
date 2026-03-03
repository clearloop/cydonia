//! MCP example — connect to a Playwright MCP server and browse the web via REPL.
//!
//! Connects to `@playwright/mcp` for headless browser automation. The LLM
//! can navigate pages, click elements, fill forms, and read page content.
//!
//! Requires DEEPSEEK_API_KEY and `npx` (Node.js 18+). Run with:
//! ```sh
//! cargo run -p walrus-runtime --example mcp
//! ```

mod common;

use std::sync::Arc;
use walrus_runtime::{McpBridge, prelude::*};

#[tokio::main]
async fn main() {
    common::init_tracing();
    let mut runtime = common::build_runtime();
    let skills = SkillRegistry::new();
    let bridge = McpBridge::new();

    // Connect to Playwright MCP server (headless browser automation).
    let mut cmd = tokio::process::Command::new("npx");
    cmd.args(["@playwright/mcp@latest"]);

    println!("Connecting to Playwright MCP server...");
    bridge
        .connect_stdio(cmd)
        .await
        .expect("failed to connect to Playwright MCP server (is npx/node installed?)");

    // List discovered tools.
    let tools = bridge.tools().await;
    println!("Discovered {} MCP tools:", tools.len());
    for tool in &tools {
        println!("  - {}: {}", tool.name, tool.description);
    }

    // Wire MCP bridge into the runtime.
    runtime.set_mcp_bridge(Arc::new(bridge)).await;

    runtime.add_agent(
        AgentConfig::new("assistant")
            .system_prompt(
                "You are a helpful web browsing assistant. Use Playwright tools \
                 to navigate pages, interact with elements, and read page content.",
            )
            .tool("*"),
    );

    println!("\nMCP REPL — try asking:");
    println!("  'Go to https://example.com and tell me what you see'");
    println!("  'Search for Rust programming on Wikipedia'");
    println!("(type 'exit' to quit)");
    println!("---");
    common::repl(&runtime, "assistant", &skills).await;
}
