//! Cron tool handler — exposes `create_cron` as a `(Tool, Handler)` pair.

use crate::CronJob;
use anyhow::Result;
use std::{pin::Pin, sync::Arc};
use tokio::sync::RwLock;
use wcore::{Handler, model::Tool};

/// Tool name for creating cron jobs.
const CREATE_CRON: &str = "create_cron";

/// Build the `create_cron` tool schema.
pub fn create_cron_tool() -> Tool {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string", "description": "Cron job name" },
            "schedule": { "type": "string", "description": "Cron schedule expression (e.g. '0 0 9 * * *')" },
            "agent": { "type": "string", "description": "Target agent name" },
            "message": { "type": "string", "description": "Message to send on each fire" }
        },
        "required": ["name", "schedule", "agent", "message"]
    });
    Tool {
        name: CREATE_CRON.into(),
        description: "Schedule a recurring cron job that sends a message to an agent.".into(),
        parameters: serde_json::from_value(schema).unwrap(),
        strict: false,
    }
}

/// Create a `(Tool, Handler)` pair for the `create_cron` tool.
///
/// The handler captures the live job list and adds new jobs dynamically.
/// Register the returned pair on Runtime.
pub fn create_cron_handler(jobs: Arc<RwLock<Vec<CronJob>>>) -> (Tool, Handler) {
    create_cron_handler_with_notify(jobs, |_| {})
}

/// Create a `(Tool, Handler)` pair with a notification callback (DD#9).
///
/// After adding a new job to the live list, calls `on_create(job.clone())`
/// so the caller can route the side-effect (e.g. send a `GatewayEvent`).
pub fn create_cron_handler_with_notify<F>(
    jobs: Arc<RwLock<Vec<CronJob>>>,
    on_create: F,
) -> (Tool, Handler)
where
    F: Fn(CronJob) + Send + Sync + 'static,
{
    let tool = create_cron_tool();
    let on_create = Arc::new(on_create);
    let handler: Handler = Arc::new(move |args: String| {
        let jobs = Arc::clone(&jobs);
        let on_create = Arc::clone(&on_create);
        Box::pin(async move {
            match handle_create_cron(&jobs, &args).await {
                Ok((msg, job)) => {
                    on_create(job);
                    msg
                }
                Err(e) => format!("create_cron failed: {e}"),
            }
        }) as Pin<Box<dyn std::future::Future<Output = String> + Send>>
    });
    (tool, handler)
}

/// Handle a `create_cron` tool call — parse args, create job, add to live list.
///
/// Returns both the success message and the created job (for notification).
async fn handle_create_cron(jobs: &RwLock<Vec<CronJob>>, args: &str) -> Result<(String, CronJob)> {
    let parsed: serde_json::Value = serde_json::from_str(args)?;
    let name = parsed["name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'name'"))?;
    let schedule = parsed["schedule"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'schedule'"))?;
    let agent = parsed["agent"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'agent'"))?;
    let message = parsed["message"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'message'"))?;

    let job = CronJob::new(name.into(), schedule, agent.into(), message.to_owned())?;

    tracing::info!(
        "dynamically created cron job '{}' → agent '{}'",
        name,
        agent
    );
    let msg = format!("created cron job '{name}' → agent '{agent}' on schedule '{schedule}'");
    jobs.write().await.push(job.clone());

    Ok((msg, job))
}
