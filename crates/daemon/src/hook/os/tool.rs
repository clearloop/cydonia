//! Tool schemas and input types for OS tools.

use schemars::JsonSchema;
use serde::Deserialize;
use std::collections::BTreeMap;
use wcore::{
    agent::{AsTool, ToolDescription},
    model::Tool,
};

#[derive(Deserialize, JsonSchema)]
pub(crate) struct Read {
    /// Path to the file to read.
    pub path: String,
}

impl ToolDescription for Read {
    const DESCRIPTION: &'static str = "Read a file at the given path.";
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct Write {
    /// Path to the file to write.
    pub path: String,
    /// Content to write to the file.
    pub content: String,
}

impl ToolDescription for Write {
    const DESCRIPTION: &'static str =
        "Write content to a file. Creates parent directories if needed.";
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct Edit {
    /// Path to the file to edit.
    pub path: String,
    /// Exact text to find (must appear exactly once in the file).
    pub old_string: String,
    /// Replacement text.
    pub new_string: String,
}

impl ToolDescription for Edit {
    const DESCRIPTION: &'static str = "Replace a unique occurrence of old_string with new_string in a file. Fails if old_string is not found or appears more than once.";
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct Bash {
    /// Executable to run (e.g. `"ls"`, `"python3"`).
    pub command: String,
    /// Arguments to pass to the executable.
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables to set for the process.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

impl ToolDescription for Bash {
    const DESCRIPTION: &'static str = "Run a shell command.";
}

pub(crate) fn tools() -> Vec<Tool> {
    vec![
        Read::as_tool(),
        Write::as_tool(),
        Edit::as_tool(),
        Bash::as_tool(),
    ]
}

impl crate::hook::DaemonHook {
    /// Dispatch a `read` tool call — read file at the given path.
    pub(crate) async fn dispatch_read(&self, args: &str) -> String {
        let input: Read = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments: {e}"),
        };
        match tokio::fs::read_to_string(&input.path).await {
            Ok(content) => content,
            Err(e) => format!("read failed: {e}"),
        }
    }

    /// Dispatch a `write` tool call — write content to the given path.
    pub(crate) async fn dispatch_write(&self, args: &str) -> String {
        let input: Write = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments: {e}"),
        };
        let path = std::path::Path::new(&input.path);
        if let Some(parent) = path.parent()
            && let Err(e) = tokio::fs::create_dir_all(parent).await
        {
            return format!("write failed: {e}");
        }
        match tokio::fs::write(path, &input.content).await {
            Ok(()) => format!("written: {}", input.path),
            Err(e) => format!("write failed: {e}"),
        }
    }

    /// Dispatch an `edit` tool call — replace a unique string occurrence in a file.
    pub(crate) async fn dispatch_edit(&self, args: &str) -> String {
        let input: Edit = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments: {e}"),
        };
        let content = match tokio::fs::read_to_string(&input.path).await {
            Ok(c) => c,
            Err(e) => return format!("edit failed: {e}"),
        };
        let count = content.matches(&input.old_string).count();
        if count == 0 {
            return "edit failed: old_string not found in file".to_owned();
        }
        if count > 1 {
            return format!("edit failed: old_string found {count} times (must be unique)");
        }
        let new_content = content.replacen(&input.old_string, &input.new_string, 1);
        match tokio::fs::write(&input.path, &new_content).await {
            Ok(()) => format!("edited: {}", input.path),
            Err(e) => format!("edit failed: {e}"),
        }
    }

    /// Dispatch a `bash` tool call — run a command directly.
    pub(crate) async fn dispatch_bash(&self, args: &str) -> String {
        let input: Bash = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments: {e}"),
        };
        let mut cmd = tokio::process::Command::new(&input.command);
        cmd.args(&input.args)
            .envs(&input.env)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return format!("bash failed: {e}"),
        };

        match tokio::time::timeout(std::time::Duration::from_secs(30), child.wait_with_output())
            .await
        {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                if stderr.is_empty() {
                    stdout.into_owned()
                } else if stdout.is_empty() {
                    stderr.into_owned()
                } else {
                    format!("{stdout}\n{stderr}")
                }
            }
            Ok(Err(e)) => format!("bash failed: {e}"),
            Err(_) => "bash timed out after 30 seconds".to_owned(),
        }
    }
}
