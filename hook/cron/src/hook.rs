//! Hook implementation — exposes `create_cron` tool for dynamic job scheduling.

use crate::{CronHandler, CronJob};
use anyhow::Result;
use std::future::Future;
use tokio::sync::RwLock;
use wcore::model::Tool;

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

impl runtime::Hook for CronHandler {
    fn tools(&self, _agent: &str) -> Vec<Tool> {
        vec![create_cron_tool()]
    }

    fn dispatch(
        &self,
        _agent: &str,
        calls: &[(&str, &str)],
    ) -> impl Future<Output = Vec<Result<String>>> + Send {
        let jobs = self.jobs_arc();
        let calls: Vec<(String, String)> = calls
            .iter()
            .map(|(m, p)| (m.to_string(), p.to_string()))
            .collect();

        async move {
            let mut results = Vec::with_capacity(calls.len());
            for (method, params) in &calls {
                let result = if method == CREATE_CRON {
                    handle_create_cron(&jobs, params).await
                } else {
                    Ok(format!("unknown tool: {method}"))
                };
                results.push(result);
            }
            results
        }
    }
}

/// Dispatch a single cron tool call by method name.
pub async fn dispatch_call(
    jobs: &RwLock<Vec<CronJob>>,
    method: &str,
    params: &str,
) -> Result<String> {
    if method == CREATE_CRON {
        handle_create_cron(jobs, params).await
    } else {
        Ok(format!("unknown cron tool: {method}"))
    }
}

/// Handle a `create_cron` tool call — parse args, create job, add to live list.
async fn handle_create_cron(jobs: &RwLock<Vec<CronJob>>, args: &str) -> Result<String> {
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
    jobs.write().await.push(job);

    Ok(format!(
        "created cron job '{name}' → agent '{agent}' on schedule '{schedule}'"
    ))
}
