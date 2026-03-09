//! Tool input parameter types for task-related tools.

use crate::hook::task::TaskStatus;
use serde::Deserialize;

/// Input for the `spawn_task` tool.
#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct SpawnTaskInput {
    /// Target agent name to delegate the task to.
    pub agent: String,
    /// Message/instruction for the target agent.
    pub message: String,
}

/// Input for the `check_tasks` tool.
#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct CheckTasksInput {
    /// Filter by agent name.
    #[serde(default)]
    pub agent: Option<String>,
    /// Filter by status (queued, in_progress, blocked, finished, failed).
    #[serde(default)]
    pub status: Option<String>,
    /// Filter by parent task ID.
    #[serde(default)]
    pub parent_id: Option<u64>,
}

/// Input for the `create_task` tool.
#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct CreateTaskInput {
    /// Target agent name.
    pub agent: String,
    /// Human-readable task description.
    pub description: String,
}

/// Input for the `ask_user` tool.
#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct AskUserInput {
    /// Question to ask the user.
    pub question: String,
}

/// Input for the `await_tasks` tool.
#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct AwaitTasksInput {
    /// Task IDs to wait for.
    pub task_ids: Vec<u64>,
}

/// Parse a status string into a `TaskStatus`.
pub(crate) fn parse_task_status(s: &str) -> Option<TaskStatus> {
    match s {
        "queued" => Some(TaskStatus::Queued),
        "in_progress" => Some(TaskStatus::InProgress),
        "blocked" => Some(TaskStatus::Blocked),
        "finished" => Some(TaskStatus::Finished),
        "failed" => Some(TaskStatus::Failed),
        _ => None,
    }
}
