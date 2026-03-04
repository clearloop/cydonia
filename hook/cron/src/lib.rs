//! Cron scheduler — periodic agent tasks with dynamic job addition.
//!
//! Exposes a `create_cron` tool that agents can use to schedule new jobs
//! dynamically. Jobs fire via a caller-provided callback, and the running
//! scheduler picks up dynamically created jobs without daemon restart.

use chrono::Utc;
use compact_str::CompactString;
use cron::Schedule;
use protocol::{
    api::{Client, Server},
    message::SendRequest,
};
use std::{str::FromStr, sync::Arc};
use tokio::{
    sync::{RwLock, broadcast, mpsc},
    task::JoinHandle,
    time,
};

mod client;
pub mod hook;
pub mod parser;

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
///
/// The `on_create` callback is called whenever a new cron job is created
/// via the `create_cron` tool. Callers that don't need side-effects pass `|_| {}`.
pub struct CronHandler {
    jobs: Arc<RwLock<Vec<CronJob>>>,
    on_create: Arc<dyn Fn(CronJob) + Send + Sync>,
}

impl CronHandler {
    /// Create a handler from an initial set of jobs and a creation callback.
    ///
    /// `on_create` is called after each dynamic `create_cron` tool invocation.
    /// Pass `|_| {}` if no side-effect is needed.
    pub fn new<F: Fn(CronJob) + Send + Sync + 'static>(jobs: Vec<CronJob>, on_create: F) -> Self {
        Self {
            jobs: Arc::new(RwLock::new(jobs)),
            on_create: Arc::new(on_create),
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

impl wcore::Hook for CronHandler {
    fn on_register_tools(
        &self,
        registry: &mut wcore::ToolRegistry,
    ) -> impl std::future::Future<Output = ()> + Send {
        let (tool, handler) = hook::create_cron_handler_with_notify(Arc::clone(&self.jobs), {
            let cb = Arc::clone(&self.on_create);
            move |job| cb(job)
        });
        registry.insert(tool, handler);
        async {}
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
    /// Accepts an optional `mpsc::UnboundedReceiver<CronJob>` for dynamic
    /// job addition. New jobs are merged into the live list between fire
    /// cycles. Before sleeping, the scheduler identifies which jobs are due
    /// at the soonest upcoming time. After waking it fires exactly those
    /// jobs, avoiding the ambiguity of re-querying `upcoming()` post-sleep.
    fn start<F, Fut>(
        mut self,
        on_fire: F,
        mut add_rx: mpsc::UnboundedReceiver<CronJob>,
        mut shutdown: broadcast::Receiver<()>,
    ) -> JoinHandle<()>
    where
        F: Fn(CronJob) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        tokio::spawn(async move {
            tracing::info!("cron scheduler started with {} job(s)", self.jobs.len());
            loop {
                // Drain any dynamically added jobs before computing schedule.
                while let Ok(job) = add_rx.try_recv() {
                    tracing::info!("cron scheduler: added dynamic job '{}'", job.name);
                    self.jobs.push(job);
                }

                if self.jobs.is_empty() {
                    // No jobs yet — wait for a dynamic add or shutdown.
                    tokio::select! {
                        Some(job) = add_rx.recv() => {
                            tracing::info!("cron scheduler: added dynamic job '{}'", job.name);
                            self.jobs.push(job);
                            continue;
                        }
                        _ = shutdown.recv() => {
                            tracing::info!("cron scheduler shutting down");
                            return;
                        }
                    }
                }

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
                    Some(job) = add_rx.recv() => {
                        tracing::info!("cron scheduler: added dynamic job '{}'", job.name);
                        self.jobs.push(job);
                        // Re-loop to recalculate schedule with new job.
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
/// `SendRequest`s through the protocol layer. Dynamic job addition is not
/// supported through this function — use [`spawn_with_callback`] instead.
pub fn spawn<S: Server + Clone + Send + 'static>(
    jobs: Vec<CronJob>,
    server: S,
    shutdown: broadcast::Receiver<()>,
) {
    let scheduler = CronScheduler::new(jobs);
    let (_add_tx, add_rx) = mpsc::unbounded_channel();

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
        add_rx,
        shutdown,
    );
}

/// Start the cron scheduler with a caller-provided fire callback.
///
/// Returns an `mpsc::UnboundedSender<CronJob>` for dynamically adding jobs
/// to the running scheduler. The scheduler picks up new jobs between fire
/// cycles without requiring a restart.
pub fn spawn_with_callback<F, Fut>(
    jobs: Vec<CronJob>,
    on_fire: F,
    shutdown: broadcast::Receiver<()>,
) -> mpsc::UnboundedSender<CronJob>
where
    F: Fn(CronJob) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let scheduler = CronScheduler::new(jobs);
    let (add_tx, add_rx) = mpsc::unbounded_channel();
    scheduler.start(on_fire, add_rx, shutdown);
    add_tx
}
