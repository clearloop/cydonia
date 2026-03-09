//! Tool dispatch and schema registration for MCP tools.

use super::{CallMcpToolInput, SearchMcpInput};
use crate::hook::DaemonHook;
use wcore::{ToolRegistry, model::Tool};

pub(crate) fn register_tools(tools: &mut ToolRegistry) {
    tools.insert(Tool {
        name: "search_mcp".into(),
        description: "Search available MCP tools by keyword.".into(),
        parameters: schemars::schema_for!(SearchMcpInput),
        strict: false,
    });
    tools.insert(Tool {
        name: "call_mcp_tool".into(),
        description: "Call an MCP tool by name with JSON-encoded arguments.".into(),
        parameters: schemars::schema_for!(CallMcpToolInput),
        strict: false,
    });
}

impl DaemonHook {
    pub(crate) async fn dispatch_search_mcp(&self, args: &str) -> String {
        let input: SearchMcpInput = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments: {e}"),
        };
        let query = input.query.to_lowercase();
        let bridge = self.mcp.bridge().await;
        let tools = bridge.tools().await;
        let matches: Vec<String> = tools
            .iter()
            .filter(|t| {
                t.name.to_lowercase().contains(&query)
                    || t.description.to_lowercase().contains(&query)
            })
            .map(|t| format!("{}: {}", t.name, t.description))
            .collect();
        if matches.is_empty() {
            "no tools found".to_owned()
        } else {
            matches.join("\n")
        }
    }

    pub(crate) async fn dispatch_call_mcp_tool(&self, args: &str) -> String {
        let input: CallMcpToolInput = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments: {e}"),
        };
        let tool_args = input.args.unwrap_or_default();
        let bridge = self.mcp.bridge().await;
        bridge.call(&input.name, &tool_args).await
    }
}
