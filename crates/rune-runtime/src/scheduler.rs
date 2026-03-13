//! Job scheduler for cron, one-shot, and recurring jobs.
//!
//! Implements Phase 4 parity: scheduling, heartbeats, reminders, isolated runs.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use cron::Schedule as CronSchedule;
use rune_core::JobId;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{info, warn};

/// Schedule definition for a job.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Schedule {
    /// One-shot at a specific time.
    At { at: DateTime<Utc> },
    /// Recurring interval.
    Every {
        every_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        anchor_ms: Option<u64>,
    },
    /// Cron expression.
    Cron {
        expr: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        tz: Option<String>,
    },
}

/// What a job does when it fires.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JobPayload {
    /// Inject text as a system event into a session.
    SystemEvent { text: String },
    /// Run an agent turn in an isolated session.
    AgentTurn {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        timeout_seconds: Option<u64>,
    },
}

/// Session target for job execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionTarget {
    Main,
    Isolated,
}

/// A scheduled job definition.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    pub id: JobId,
    pub name: Option<String>,
    pub schedule: Schedule,
    pub payload: JobPayload,
    pub session_target: SessionTarget,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub last_run_at: Option<DateTime<Utc>>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub run_count: u64,
}

/// Record of a job execution.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JobRun {
    pub job_id: JobId,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub status: JobRunStatus,
    pub output: Option<String>,
}

/// Status of a job run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobRunStatus {
    Running,
    Completed,
    Failed,
    Skipped,
}

/// The scheduler manages jobs and their execution lifecycle.
pub struct Scheduler {
    jobs: Arc<Mutex<HashMap<JobId, Job>>>,
    runs: Arc<Mutex<Vec<JobRun>>>,
}

impl Scheduler {
    /// Create a new empty scheduler.
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(Mutex::new(HashMap::new())),
            runs: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Add a new job. Returns the job ID.
    pub async fn add_job(&self, job: Job) -> JobId {
        let id = job.id;
        info!(job_id = %id, name = ?job.name, "adding job");
        self.jobs.lock().await.insert(id, job);
        id
    }

    /// List all jobs, optionally including disabled ones.
    pub async fn list_jobs(&self, include_disabled: bool) -> Vec<Job> {
        let jobs = self.jobs.lock().await;
        let mut result: Vec<Job> = if include_disabled {
            jobs.values().cloned().collect()
        } else {
            jobs.values().filter(|j| j.enabled).cloned().collect()
        };
        result.sort_by_key(|j| j.created_at);
        result
    }

    /// Get a specific job by ID.
    pub async fn get_job(&self, id: &JobId) -> Option<Job> {
        self.jobs.lock().await.get(id).cloned()
    }

    /// Update a job.
    pub async fn update_job(&self, id: &JobId, update: JobUpdate) -> Option<Job> {
        let mut jobs = self.jobs.lock().await;
        let job = jobs.get_mut(id)?;

        if let Some(name) = update.name {
            job.name = Some(name);
        }
        if let Some(enabled) = update.enabled {
            job.enabled = enabled;
        }
        if let Some(schedule) = update.schedule {
            job.schedule = schedule;
        }
        if let Some(payload) = update.payload {
            job.payload = payload;
        }

        Some(job.clone())
    }

    /// Remove a job.
    pub async fn remove_job(&self, id: &JobId) -> Option<Job> {
        info!(job_id = %id, "removing job");
        self.jobs.lock().await.remove(id)
    }

    /// Enable or disable a job.
    pub async fn set_enabled(&self, id: &JobId, enabled: bool) -> Option<Job> {
        let mut jobs = self.jobs.lock().await;
        let job = jobs.get_mut(id)?;
        job.enabled = enabled;
        Some(job.clone())
    }

    /// Record a job run starting.
    pub async fn start_run(&self, job_id: JobId) -> JobRun {
        let run = JobRun {
            job_id,
            started_at: Utc::now(),
            finished_at: None,
            status: JobRunStatus::Running,
            output: None,
        };
        self.runs.lock().await.push(run.clone());

        // Update job metadata
        if let Some(job) = self.jobs.lock().await.get_mut(&job_id) {
            job.last_run_at = Some(run.started_at);
            job.run_count += 1;
        }

        run
    }

    /// Complete a job run.
    pub async fn complete_run(&self, job_id: JobId, status: JobRunStatus, output: Option<String>) {
        let mut runs = self.runs.lock().await;
        // Find the most recent running entry for this job
        for run in runs.iter_mut().rev() {
            if run.job_id == job_id && run.status == JobRunStatus::Running {
                run.finished_at = Some(Utc::now());
                run.status = status;
                run.output = output;
                break;
            }
        }
    }

    /// Get run history for a job.
    pub async fn get_runs(&self, job_id: &JobId, limit: Option<usize>) -> Vec<JobRun> {
        let runs = self.runs.lock().await;
        let mut job_runs: Vec<JobRun> = runs
            .iter()
            .filter(|r| r.job_id == *job_id)
            .cloned()
            .collect();
        job_runs.reverse(); // newest first
        if let Some(limit) = limit {
            job_runs.truncate(limit);
        }
        job_runs
    }

    /// Get all due jobs (jobs whose next_run_at is in the past or now).
    pub async fn get_due_jobs(&self) -> Vec<Job> {
        let now = Utc::now();
        let jobs = self.jobs.lock().await;
        jobs.values()
            .filter(|j| j.enabled && j.next_run_at.is_some_and(|next| next <= now))
            .cloned()
            .collect()
    }

    /// Compute and set the next run time for a job after a firing attempt.
    pub async fn advance_next_run(&self, id: &JobId) {
        let mut jobs = self.jobs.lock().await;
        if let Some(job) = jobs.get_mut(id) {
            let now = Utc::now();
            let base = job.last_run_at.or(job.next_run_at).unwrap_or(now);
            match &job.schedule {
                Schedule::At { .. } => {
                    // One-shot: disable after firing
                    job.enabled = false;
                    job.next_run_at = None;
                }
                Schedule::Every {
                    every_ms,
                    anchor_ms,
                } => {
                    job.next_run_at =
                        Some(compute_next_interval_run(*every_ms, *anchor_ms, base, now));
                }
                Schedule::Cron { expr, tz } => {
                    job.next_run_at = compute_next_cron_run(expr, tz.as_deref(), base, now);
                    if job.next_run_at.is_none() {
                        warn!(job_id = %job.id, expr = %expr, tz = ?tz, "failed to compute next cron run; disabling job");
                        job.enabled = false;
                    }
                }
            }
        }
    }
}

fn compute_next_interval_run(
    every_ms: u64,
    anchor_ms: Option<u64>,
    base: DateTime<Utc>,
    now: DateTime<Utc>,
) -> DateTime<Utc> {
    let duration = chrono::Duration::milliseconds(every_ms as i64);

    if let Some(anchor_ms) = anchor_ms {
        let Some(anchor) = DateTime::<Utc>::from_timestamp_millis(anchor_ms as i64) else {
            return now + duration;
        };

        if anchor > now {
            return anchor;
        }

        let elapsed_ms = (now - anchor).num_milliseconds();
        if elapsed_ms < 0 {
            return anchor;
        }

        let steps = (elapsed_ms / every_ms as i64) + 1;
        return anchor + chrono::Duration::milliseconds(steps * every_ms as i64);
    }

    let candidate = base + duration;
    if candidate <= now {
        now + duration
    } else {
        candidate
    }
}

fn compute_next_cron_run(
    expr: &str,
    tz: Option<&str>,
    base: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let timezone = parse_timezone(tz)?;
    let schedule = expr.parse::<CronSchedule>().ok()?;
    let after = base.max(now);
    let after_local = after.with_timezone(&timezone);
    schedule
        .after(&after_local)
        .next()
        .map(|next| next.with_timezone(&Utc))
}

fn parse_timezone(tz: Option<&str>) -> Option<Tz> {
    match tz {
        None => Some(chrono_tz::UTC),
        Some(value) => value.parse::<Tz>().ok(),
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

/// Partial update for a job.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct JobUpdate {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub schedule: Option<Schedule>,
    pub payload: Option<JobPayload>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    fn make_job(name: &str) -> Job {
        Job {
            id: JobId::new(),
            name: Some(name.into()),
            schedule: Schedule::Every {
                every_ms: 60_000,
                anchor_ms: None,
            },
            payload: JobPayload::SystemEvent {
                text: "test event".into(),
            },
            session_target: SessionTarget::Main,
            enabled: true,
            created_at: Utc::now(),
            last_run_at: None,
            next_run_at: Some(Utc::now() - chrono::Duration::seconds(10)),
            run_count: 0,
        }
    }

    #[tokio::test]
    async fn add_and_list_jobs() {
        let scheduler = Scheduler::new();
        let job = make_job("test-job");
        let id = scheduler.add_job(job).await;

        let jobs = scheduler.list_jobs(false).await;
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].id, id);
    }

    #[tokio::test]
    async fn disabled_jobs_filtered_by_default() {
        let scheduler = Scheduler::new();
        let mut job = make_job("disabled");
        job.enabled = false;
        scheduler.add_job(job).await;

        assert_eq!(scheduler.list_jobs(false).await.len(), 0);
        assert_eq!(scheduler.list_jobs(true).await.len(), 1);
    }

    #[tokio::test]
    async fn remove_job() {
        let scheduler = Scheduler::new();
        let job = make_job("removable");
        let id = scheduler.add_job(job).await;

        assert!(scheduler.remove_job(&id).await.is_some());
        assert_eq!(scheduler.list_jobs(true).await.len(), 0);
    }

    #[tokio::test]
    async fn update_job() {
        let scheduler = Scheduler::new();
        let job = make_job("updatable");
        let id = scheduler.add_job(job).await;

        let updated = scheduler
            .update_job(
                &id,
                JobUpdate {
                    name: Some("renamed".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        assert_eq!(updated.name, Some("renamed".into()));
    }

    #[tokio::test]
    async fn run_lifecycle() {
        let scheduler = Scheduler::new();
        let job = make_job("runnable");
        let id = scheduler.add_job(job).await;

        let run = scheduler.start_run(id).await;
        assert_eq!(run.status, JobRunStatus::Running);

        scheduler
            .complete_run(id, JobRunStatus::Completed, Some("done".into()))
            .await;

        let runs = scheduler.get_runs(&id, None).await;
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, JobRunStatus::Completed);
        assert_eq!(runs[0].output.as_deref(), Some("done"));

        // Job metadata updated
        let job = scheduler.get_job(&id).await.unwrap();
        assert_eq!(job.run_count, 1);
        assert!(job.last_run_at.is_some());
    }

    #[tokio::test]
    async fn get_due_jobs() {
        let scheduler = Scheduler::new();

        // Due job (next_run_at in the past)
        let due = make_job("due");
        scheduler.add_job(due).await;

        // Not due (next_run_at in the future)
        let mut future = make_job("future");
        future.next_run_at = Some(Utc::now() + chrono::Duration::hours(1));
        scheduler.add_job(future).await;

        let due_jobs = scheduler.get_due_jobs().await;
        assert_eq!(due_jobs.len(), 1);
        assert_eq!(due_jobs[0].name.as_deref(), Some("due"));
    }

    #[tokio::test]
    async fn advance_disables_one_shot() {
        let scheduler = Scheduler::new();
        let mut job = make_job("one-shot");
        job.schedule = Schedule::At {
            at: Utc::now() - chrono::Duration::seconds(10),
        };
        let id = scheduler.add_job(job).await;

        scheduler.advance_next_run(&id).await;

        let job = scheduler.get_job(&id).await.unwrap();
        assert!(!job.enabled);
        assert!(job.next_run_at.is_none());
    }

    #[tokio::test]
    async fn advance_sets_next_for_interval() {
        let scheduler = Scheduler::new();
        let job = make_job("interval");
        let id = scheduler.add_job(job).await;

        scheduler.advance_next_run(&id).await;

        let job = scheduler.get_job(&id).await.unwrap();
        assert!(job.next_run_at.is_some());
        assert!(job.next_run_at.unwrap() > Utc::now());
    }

    #[tokio::test]
    async fn schedule_serialization_roundtrip() {
        let schedules = vec![
            Schedule::At { at: Utc::now() },
            Schedule::Every {
                every_ms: 5000,
                anchor_ms: None,
            },
            Schedule::Cron {
                expr: "0 0 9 * * MON".into(),
                tz: Some("Europe/Sarajevo".into()),
            },
        ];

        for schedule in schedules {
            let json = serde_json::to_string(&schedule).unwrap();
            let restored: Schedule = serde_json::from_str(&json).unwrap();
            assert_eq!(schedule, restored);
        }
    }

    #[test]
    fn compute_next_interval_respects_anchor() {
        let anchor = Utc.with_ymd_and_hms(2026, 3, 13, 15, 0, 0).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 3, 13, 15, 2, 30).unwrap();

        let next =
            compute_next_interval_run(60_000, Some(anchor.timestamp_millis() as u64), anchor, now);

        assert_eq!(next, Utc.with_ymd_and_hms(2026, 3, 13, 15, 3, 0).unwrap());
    }

    #[test]
    fn compute_next_cron_run_honors_timezone() {
        let base = Utc.with_ymd_and_hms(2026, 3, 13, 7, 30, 0).unwrap();
        let next =
            compute_next_cron_run("0 0 9 * * *", Some("Europe/Sarajevo"), base, base).unwrap();
        let local = next.with_timezone(&"Europe/Sarajevo".parse::<Tz>().unwrap());

        assert_eq!(local.hour(), 9);
        assert_eq!(local.minute(), 0);
        assert_eq!(local.second(), 0);
        assert_eq!(
            local.date_naive(),
            base.with_timezone(&local.timezone()).date_naive()
        );
    }

    #[tokio::test]
    async fn advance_sets_next_for_cron() {
        let scheduler = Scheduler::new();
        let mut job = make_job("cron");
        job.schedule = Schedule::Cron {
            expr: "0 0 9 * * *".into(),
            tz: Some("UTC".into()),
        };
        job.last_run_at = Some(Utc.with_ymd_and_hms(2026, 3, 13, 8, 0, 0).unwrap());
        job.next_run_at = Some(Utc.with_ymd_and_hms(2026, 3, 13, 8, 0, 0).unwrap());
        let id = scheduler.add_job(job).await;

        scheduler.advance_next_run(&id).await;

        let job = scheduler.get_job(&id).await.unwrap();
        let next = job.next_run_at.expect("cron next run should be set");
        assert_eq!(next.hour(), 9);
        assert_eq!(next.minute(), 0);
        assert_eq!(next.second(), 0);
    }

    #[tokio::test]
    async fn invalid_cron_expression_disables_job() {
        let scheduler = Scheduler::new();
        let mut job = make_job("invalid-cron");
        job.schedule = Schedule::Cron {
            expr: "not a cron".into(),
            tz: Some("UTC".into()),
        };
        let id = scheduler.add_job(job).await;

        scheduler.advance_next_run(&id).await;

        let job = scheduler.get_job(&id).await.unwrap();
        assert!(!job.enabled);
        assert!(job.next_run_at.is_none());
    }

    #[tokio::test]
    async fn invalid_timezone_disables_job() {
        let scheduler = Scheduler::new();
        let mut job = make_job("invalid-tz");
        job.schedule = Schedule::Cron {
            expr: "0 0 9 * * *".into(),
            tz: Some("Mars/Olympus".into()),
        };
        let id = scheduler.add_job(job).await;

        scheduler.advance_next_run(&id).await;

        let job = scheduler.get_job(&id).await.unwrap();
        assert!(!job.enabled);
        assert!(job.next_run_at.is_none());
    }
}
