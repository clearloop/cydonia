//! Cron scheduler — protocol client + hook for periodic agent tasks.
//!
//! Implements [`Client`](protocol::api::Client) to fire scheduled jobs via
//! `SendRequest`, and [`Hook`](runtime::Hook) to expose a `create_cron` tool
//! that agents can use to schedule new jobs dynamically.

use chrono::Utc;
use compact_str::CompactString;
use cron::Schedule;
use protocol::api::{Client, Server};
use protocol::message::SendRequest;
use std::str::FromStr;
use std::sync::Arc;
use tokio::{
    sync::{RwLock, broadcast},
    task::JoinHandle,
    time,
};

mod client;
pub mod hook;

/// A parsed cron job ready for scheduling.
#[derive(Debug, Clone)]
pub struct CronJob {
    /// Job name.
    pub name: CompactString,
    /// Parsed cron schedule.
    pub schedule: Schedule,
    /// Target agent name.
    pub agent: CompactString,
    /// Message to send on each fire.
    pub message: String,
}

impl CronJob {
    /// Parse a [`CronJob`] from raw fields.
    pub fn new(
        name: CompactString,
        schedule_expr: &str,
        agent: CompactString,
        message: String,
    ) -> anyhow::Result<Self> {
        let schedule = Schedule::from_str(schedule_expr)
            .map_err(|e| anyhow::anyhow!("invalid cron expression '{schedule_expr}': {e}"))?;
        Ok(Self {
            name,
            schedule,
            agent,
            message,
        })
    }
}

/// Cron handler — owns the live job list for dynamic scheduling.
pub struct CronHandler {
    jobs: Arc<RwLock<Vec<CronJob>>>,
}

impl CronHandler {
    /// Create a handler from an initial set of jobs.
    pub fn new(jobs: Vec<CronJob>) -> Self {
        Self {
            jobs: Arc::new(RwLock::new(jobs)),
        }
    }

    /// Get a clone of the jobs arc (for the scheduler task).
    pub fn jobs_arc(&self) -> Arc<RwLock<Vec<CronJob>>> {
        Arc::clone(&self.jobs)
    }

    /// Snapshot the current job list.
    pub async fn jobs(&self) -> Vec<CronJob> {
        self.jobs.read().await.clone()
    }
}

/// Cron scheduler that fires jobs on their schedules.
struct CronScheduler {
    jobs: Vec<CronJob>,
}

impl CronScheduler {
    /// Create a scheduler from a list of cron jobs.
    fn new(jobs: Vec<CronJob>) -> Self {
        Self { jobs }
    }

    /// Start the scheduler. Calls `on_fire` for each job when it fires.
    ///
    /// Before sleeping, the scheduler identifies which jobs are due at the
    /// soonest upcoming time. After waking it fires exactly those jobs,
    /// avoiding the ambiguity of re-querying `upcoming()` post-sleep.
    fn start<F, Fut>(self, on_fire: F, mut shutdown: broadcast::Receiver<()>) -> JoinHandle<()>
    where
        F: Fn(CronJob) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        tokio::spawn(async move {
            if self.jobs.is_empty() {
                tracing::info!("cron scheduler started with no jobs");
                let _ = shutdown.recv().await;
                return;
            }

            tracing::info!("cron scheduler started with {} job(s)", self.jobs.len());
            loop {
                let now = Utc::now();
                let mut due_jobs: Vec<usize> = Vec::new();
                let mut soonest = None::<chrono::DateTime<Utc>>;

                for (i, job) in self.jobs.iter().enumerate() {
                    if let Some(next) = job.schedule.upcoming(Utc).next() {
                        match soonest {
                            None => {
                                soonest = Some(next);
                                due_jobs.clear();
                                due_jobs.push(i);
                            }
                            Some(s) if next < s => {
                                soonest = Some(next);
                                due_jobs.clear();
                                due_jobs.push(i);
                            }
                            Some(s) if (next - s).num_seconds().abs() <= 0 => {
                                due_jobs.push(i);
                            }
                            _ => {}
                        }
                    }
                }

                let Some(soonest_time) = soonest else {
                    tracing::warn!("no upcoming cron fires, scheduler stopping");
                    return;
                };

                let wait = (soonest_time - now).to_std().unwrap_or_default();
                tokio::select! {
                    _ = time::sleep(wait) => {
                        for &i in &due_jobs {
                            tracing::info!("cron firing job '{}'", self.jobs[i].name);
                            on_fire(self.jobs[i].clone()).await;
                        }
                    }
                    _ = shutdown.recv() => {
                        tracing::info!("cron scheduler shutting down");
                        return;
                    }
                }
            }
        })
    }
}

/// Start the cron scheduler with an in-process protocol client.
///
/// Takes a snapshot of jobs and a `Server` impl (e.g. `Gateway`) to dispatch
/// `SendRequest`s through the protocol layer. Jobs added dynamically via the
/// `create_cron` tool are not picked up by a running scheduler — they take
/// effect after the next daemon restart.
pub fn spawn<S: Server + Clone + Send + 'static>(
    jobs: Vec<CronJob>,
    server: S,
    shutdown: broadcast::Receiver<()>,
) {
    let scheduler = CronScheduler::new(jobs);

    scheduler.start(
        move |job| {
            let mut client = client::CronClient::new(server.clone());
            async move {
                let req = SendRequest {
                    agent: job.agent.clone(),
                    content: job.message.clone(),
                };
                match client.send(req).await {
                    Ok(response) => {
                        tracing::info!(
                            job = %job.name,
                            agent = %job.agent,
                            response_len = response.content.len(),
                            "cron job completed"
                        );
                    }
                    Err(e) => {
                        tracing::error!(job = %job.name, "cron dispatch failed: {e}");
                    }
                }
            }
        },
        shutdown,
    );
}
