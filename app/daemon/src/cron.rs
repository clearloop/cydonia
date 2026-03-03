//! Cron scheduler for periodic agent tasks.
//!
//! Each cron job runs in an isolated session with no state carried
//! between runs. The scheduler is decoupled from the runtime —
//! it produces events (fires jobs), and the Gateway wires them to
//! agent dispatch.

use crate::gateway::GatewayHook;
use chrono::Utc;
use compact_str::CompactString;
use cron::Schedule;
use model::ProviderManager;
use runtime::Runtime;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use tokio::{sync::broadcast, task::JoinHandle, time};

/// A parsed cron job ready for scheduling.
#[derive(Debug, Clone)]
pub struct CronJob {
    /// Job name.
    pub name: CompactString,
    /// Parsed cron schedule.
    pub schedule: Schedule,
    /// Target agent name.
    pub agent: CompactString,
    /// Message template to send.
    pub message: String,
}

impl CronJob {
    /// Parse a [`CronJob`] from its fields.
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

    /// Create a [`CronJob`] from a [`CronEntry`](crate::loader::CronEntry).
    pub fn from_entry(entry: &crate::loader::CronEntry) -> anyhow::Result<Self> {
        Self::new(
            entry.name.clone(),
            &entry.schedule,
            entry.agent.clone(),
            entry.message.clone(),
        )
    }
}

/// Cron scheduler that fires jobs on their schedules.
pub struct CronScheduler {
    jobs: Vec<CronJob>,
}

impl CronScheduler {
    /// Create a scheduler from a list of cron jobs.
    pub fn new(jobs: Vec<CronJob>) -> Self {
        Self { jobs }
    }

    /// Start the scheduler. Calls `on_fire` for each job when it fires.
    ///
    /// Returns a [`JoinHandle`]. The scheduler stops when `shutdown` is
    /// received or the handle is aborted.
    ///
    /// Before sleeping, the scheduler identifies which jobs are due at the
    /// soonest upcoming time. After waking it fires exactly those jobs,
    /// avoiding the ambiguity of re-querying `upcoming()` post-sleep.
    pub fn start<F, Fut>(self, on_fire: F, mut shutdown: broadcast::Receiver<()>) -> JoinHandle<()>
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

    /// Get the list of jobs.
    pub fn jobs(&self) -> &[CronJob] {
        &self.jobs
    }
}

/// Load cron entries from disk and start the scheduler.
pub fn spawn(
    cron_dir: &Path,
    runtime: &Arc<Runtime<ProviderManager, GatewayHook>>,
    shutdown: broadcast::Receiver<()>,
) {
    let entries = match crate::loader::load_cron_dir(cron_dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("failed to load cron entries: {e}");
            return;
        }
    };

    let mut jobs = Vec::new();
    for entry in &entries {
        match CronJob::from_entry(entry) {
            Ok(job) => {
                tracing::info!("registered cron job '{}' → agent '{}'", job.name, job.agent);
                jobs.push(job);
            }
            Err(e) => {
                tracing::warn!("skipping cron entry '{}': {e}", entry.name);
            }
        }
    }

    let scheduler = CronScheduler::new(jobs);
    let rt = Arc::clone(runtime);

    scheduler.start(
        move |job| {
            let rt = Arc::clone(&rt);
            async move {
                match rt.send_to(&job.agent, &job.message).await {
                    Ok(response) => {
                        let content = response.final_response.unwrap_or_default();
                        tracing::info!(
                            job = %job.name,
                            agent = %job.agent,
                            response_len = content.len(),
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
