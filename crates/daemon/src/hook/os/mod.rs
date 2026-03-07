//! OS hook — sandboxed filesystem and shell tools for agents.
//!
//! [`OsHook`] registers `read`, `write`, and `bash` tool schemas and provides
//! async dispatch methods. All operations are confined to a sandbox root
//! (`~/.openwalrus/work/`). Paths that exist as real absolute paths on the host
//! filesystem are rejected with an "operation out of sandbox" error.

use schemars::JsonSchema;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use wcore::{ToolRegistry, model::Tool};

/// OS hook providing sandboxed filesystem and shell tools.
pub struct OsHook {
    work_dir: PathBuf,
}

impl OsHook {
    /// Create a new `OsHook` with the given sandbox root.
    pub fn new(work_dir: PathBuf) -> Self {
        Self { work_dir }
    }

    /// Dispatch a `read` tool call — read file at a sandbox-relative path.
    pub async fn dispatch_read(&self, args: &str) -> String {
        let input: ReadInput = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments: {e}"),
        };
        let path = match validate_path(&input.path, &self.work_dir) {
            Ok(p) => p,
            Err(e) => return e,
        };
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => content,
            Err(e) => format!("read failed: {e}"),
        }
    }

    /// Dispatch a `write` tool call — write content to a sandbox-relative path.
    pub async fn dispatch_write(&self, args: &str) -> String {
        let input: WriteInput = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments: {e}"),
        };
        let path = match validate_path(&input.path, &self.work_dir) {
            Ok(p) => p,
            Err(e) => return e,
        };
        if let Some(parent) = path.parent()
            && let Err(e) = tokio::fs::create_dir_all(parent).await
        {
            return format!("write failed: {e}");
        }
        match tokio::fs::write(&path, &input.content).await {
            Ok(()) => format!("written: {}", input.path),
            Err(e) => format!("write failed: {e}"),
        }
    }

    /// Dispatch a `bash` tool call — run a command inside the sandbox.
    pub async fn dispatch_bash(&self, args: &str) -> String {
        let input: BashInput = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments: {e}"),
        };
        // Validate command — reject if it's a real absolute path on the host.
        if let Err(e) = validate_path(&input.command, &self.work_dir) {
            return e;
        }
        // Remap args: reject real absolute paths, rewrite non-existent ones under work_dir.
        let remapped_args: Vec<PathBuf> = match input
            .args
            .iter()
            .map(|a| validate_path(a, &self.work_dir))
            .collect()
        {
            Ok(v) => v,
            Err(e) => return e,
        };
        let mut cmd = tokio::process::Command::new(&input.command);
        cmd.args(&remapped_args)
            .envs(&input.env)
            .current_dir(&self.work_dir)
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

impl wcore::Hook for OsHook {
    fn on_register_tools(
        &self,
        registry: &mut ToolRegistry,
    ) -> impl std::future::Future<Output = ()> + Send {
        registry.insert(read_schema());
        registry.insert(write_schema());
        registry.insert(bash_schema());
        async {}
    }
}

/// Validate a path token against the sandbox boundary.
///
/// If the input (treated as an absolute path) exists on the real FS, it is
/// rejected. Otherwise the leading `/` is stripped and the path is joined
/// under `work_dir`.
fn validate_path(input: &str, work_dir: &Path) -> Result<PathBuf, String> {
    if Path::new(input).exists() {
        return Err(format!("operation out of sandbox: {input}"));
    }
    let stripped = input.trim_start_matches('/');
    Ok(work_dir.join(stripped))
}

#[derive(Deserialize, JsonSchema)]
struct ReadInput {
    /// Sandbox-relative path to the file to read (e.g. `/notes.txt` reads `~/.openwalrus/work/notes.txt`)
    path: String,
}

#[derive(Deserialize, JsonSchema)]
struct WriteInput {
    /// Sandbox-relative path to the file to write
    path: String,
    /// Content to write to the file
    content: String,
}

#[derive(Deserialize, JsonSchema)]
struct BashInput {
    /// Executable to run (e.g. `"ls"`, `"python3"`)
    command: String,
    /// Arguments to pass to the executable
    #[serde(default)]
    args: Vec<String>,
    /// Environment variables to set for the process
    #[serde(default)]
    env: BTreeMap<String, String>,
}

fn read_schema() -> Tool {
    Tool {
        name: "read".into(),
        description:
            "Read a file at a sandbox-relative path. Paths resolve under ~/.openwalrus/work/."
                .into(),
        parameters: schemars::schema_for!(ReadInput),
        strict: false,
    }
}

fn write_schema() -> Tool {
    Tool {
        name: "write".into(),
        description: "Write content to a file at a sandbox-relative path under ~/.openwalrus/work/. Creates or overwrites the file.".into(),
        parameters: schemars::schema_for!(WriteInput),
        strict: false,
    }
}

fn bash_schema() -> Tool {
    Tool {
        name: "bash".into(),
        description: "Run a command inside the workspace sandbox (~/.openwalrus/work/). The working directory is always the sandbox root. Arguments that are real absolute paths on the host are rejected.".into(),
        parameters: schemars::schema_for!(BashInput),
        strict: false,
    }
}
