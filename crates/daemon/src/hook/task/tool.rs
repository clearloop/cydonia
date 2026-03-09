//! Tool dispatch and schema registration for task tools.

use super::{
    TaskStatus,
    params::{
        AskUserInput, AwaitTasksInput, CheckTasksInput, CreateTaskInput, SpawnTaskInput,
        parse_task_status,
    },
};
use crate::hook::DaemonHook;
use wcore::{ToolRegistry, model::Tool};

pub(crate) fn register_tools(tools: &mut ToolRegistry) {
    tools.insert(Tool {
        name: "spawn_task".into(),
        description: "Delegate an async task to another agent. Returns task_id and status (in_progress or queued). Use check_tasks to monitor progress.".into(),
        parameters: schemars::schema_for!(SpawnTaskInput),
        strict: false,
    });
    tools.insert(Tool {
        name: "check_tasks".into(),
        description: "Query the task registry. Filterable by agent, status, parent_id. Returns up to 16 most recent tasks.".into(),
        parameters: schemars::schema_for!(CheckTasksInput),
        strict: false,
    });
    tools.insert(Tool {
        name: "create_task".into(),
        description:
            "Queue a task for later pickup (heartbeat or manual). Always starts as queued.".into(),
        parameters: schemars::schema_for!(CreateTaskInput),
        strict: false,
    });
    tools.insert(Tool {
        name: "ask_user".into(),
        description: "Ask the user a question. Blocks the current task until the user responds. Only works within a task context.".into(),
        parameters: schemars::schema_for!(AskUserInput),
        strict: false,
    });
    tools.insert(Tool {
        name: "await_tasks".into(),
        description:
            "Block until the specified tasks finish. Returns collected results for each task."
                .into(),
        parameters: schemars::schema_for!(AwaitTasksInput),
        strict: false,
    });
}

impl DaemonHook {
    pub(crate) async fn dispatch_spawn_task(
        &self,
        args: &str,
        agent: &str,
        parent_task_id: Option<u64>,
    ) -> String {
        let input: SpawnTaskInput = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments: {e}"),
        };
        let registry = self.tasks.clone();
        let (task_id, status) = registry.lock().await.submit(
            input.agent.into(),
            input.message,
            agent.into(),
            parent_task_id,
            registry.clone(),
        );
        serde_json::json!({ "task_id": task_id, "status": status.to_string() }).to_string()
    }

    pub(crate) async fn dispatch_check_tasks(&self, args: &str) -> String {
        let input: CheckTasksInput = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments: {e}"),
        };
        let status_filter = input.status.as_deref().and_then(parse_task_status);
        let registry = self.tasks.lock().await;
        let tasks = registry.list(
            input.agent.as_deref(),
            status_filter,
            input.parent_id.map(Some),
        );
        let entries: Vec<serde_json::Value> = tasks
            .iter()
            .map(|t| {
                serde_json::json!({
                    "task_id": t.id,
                    "agent": t.agent.as_str(),
                    "status": t.status.to_string(),
                    "description": t.description,
                    "parent_id": t.parent_id,
                    "result": t.result,
                    "error": t.error,
                    "created_by": t.created_by.as_str(),
                    "alive_secs": t.created_at.elapsed().as_secs(),
                    "prompt_tokens": t.prompt_tokens,
                    "completion_tokens": t.completion_tokens,
                })
            })
            .collect();
        serde_json::to_string(&entries).unwrap_or_else(|e| format!("serialization error: {e}"))
    }

    pub(crate) async fn dispatch_create_task(&self, args: &str, agent: &str) -> String {
        let input: CreateTaskInput = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments: {e}"),
        };
        let mut registry = self.tasks.lock().await;
        let task_id = registry.create(
            input.agent.into(),
            input.description,
            agent.into(),
            None,
            TaskStatus::Queued,
            false,
        );
        serde_json::json!({ "task_id": task_id, "status": "queued" }).to_string()
    }

    pub(crate) async fn dispatch_ask_user(&self, args: &str, task_id: Option<u64>) -> String {
        let input: AskUserInput = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments: {e}"),
        };
        let Some(tid) = task_id else {
            return "ask_user can only be called from within a task context".to_owned();
        };
        let rx = {
            let mut registry = self.tasks.lock().await;
            match registry.block(tid, input.question) {
                Some(rx) => rx,
                None => return format!("task {tid} not found"),
            }
        };
        match rx.await {
            Ok(response) => response,
            Err(_) => "user did not respond (channel closed)".to_owned(),
        }
    }

    pub(crate) async fn dispatch_await_tasks(&self, args: &str, task_id: Option<u64>) -> String {
        let input: AwaitTasksInput = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments: {e}"),
        };
        if input.task_ids.is_empty() {
            return "no task IDs provided".to_owned();
        }
        // Subscribe to status changes for each requested task.
        let mut receivers = Vec::new();
        {
            let registry = self.tasks.lock().await;
            for &tid in &input.task_ids {
                match registry.subscribe_status(tid) {
                    Some(rx) => receivers.push((tid, rx)),
                    None => return format!("task {tid} not found"),
                }
            }
        }
        // If running in a task context, mark ourselves as blocked.
        if let Some(tid) = task_id {
            let mut registry = self.tasks.lock().await;
            registry.set_status(tid, TaskStatus::Blocked);
        }
        // Wait for all tasks to reach Finished or Failed.
        for (_, rx) in &mut receivers {
            let mut rx = rx.clone();
            loop {
                let status = *rx.borrow_and_update();
                if status == TaskStatus::Finished || status == TaskStatus::Failed {
                    break;
                }
                if rx.changed().await.is_err() {
                    break;
                }
            }
        }
        // Unblock ourselves.
        if let Some(tid) = task_id {
            let mut registry = self.tasks.lock().await;
            registry.set_status(tid, TaskStatus::InProgress);
        }
        // Collect results.
        let registry = self.tasks.lock().await;
        let results: Vec<serde_json::Value> = input
            .task_ids
            .iter()
            .map(|&tid| {
                if let Some(t) = registry.get(tid) {
                    serde_json::json!({
                        "task_id": tid,
                        "status": t.status.to_string(),
                        "result": t.result,
                        "error": t.error,
                    })
                } else {
                    serde_json::json!({ "task_id": tid, "status": "not_found" })
                }
            })
            .collect();
        serde_json::to_string(&results).unwrap_or_else(|e| format!("serialization error: {e}"))
    }
}
