//! Job scheduler for cron, one-shot, and recurring jobs.
//!
//! Implements Phase 4 parity: scheduling, heartbeats, reminders, isolated runs.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use cron::Schedule as CronSchedule;
use rune_core::{JobId, SchedulerDeliveryMode, SchedulerPayloadKind, SchedulerRunTrigger};
use rune_store::{
    JobRepo, JobRunRepo, StoreError,
    models::{JobRow, JobRunRow, NewJob, NewJobRun},
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;

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
    pub delivery_mode: SchedulerDeliveryMode,
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
    pub trigger_kind: SchedulerRunTrigger,
    pub status: JobRunStatus,
    pub output: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredJobRecord {
    pub name: Option<String>,
    pub payload: JobPayload,
    #[serde(default = "default_scheduler_delivery_mode")]
    pub delivery_mode: SchedulerDeliveryMode,
    pub session_target: SessionTarget,
    #[serde(default)]
    pub run_count: u64,
}

const fn default_scheduler_delivery_mode() -> SchedulerDeliveryMode {
    SchedulerDeliveryMode::None
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

impl JobPayload {
    fn kind(&self) -> SchedulerPayloadKind {
        match self {
            Self::SystemEvent { .. } => SchedulerPayloadKind::SystemEvent,
            Self::AgentTurn { .. } => SchedulerPayloadKind::AgentTurn,
        }
    }
}

/// The scheduler manages jobs and their execution lifecycle.
enum SchedulerBackend {
    Memory(Arc<Mutex<HashMap<JobId, Job>>>),
    Repo {
        jobs: Arc<dyn JobRepo>,
        runs: Option<Arc<dyn JobRunRepo>>,
    },
}

/// The scheduler manages jobs and their execution lifecycle.
pub struct Scheduler {
    backend: SchedulerBackend,
    runs: Arc<Mutex<Vec<JobRun>>>,
}

impl Scheduler {
    /// Create a new empty scheduler.
    pub fn new() -> Self {
        Self {
            backend: SchedulerBackend::Memory(Arc::new(Mutex::new(HashMap::new()))),
            runs: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Create a scheduler backed by the durable job repository.
    pub fn new_with_repo(job_repo: Arc<dyn JobRepo>) -> Self {
        Self {
            backend: SchedulerBackend::Repo {
                jobs: job_repo,
                runs: None,
            },
            runs: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Create a scheduler backed by durable job and job-run repositories.
    pub fn new_with_repos(job_repo: Arc<dyn JobRepo>, job_run_repo: Arc<dyn JobRunRepo>) -> Self {
        Self {
            backend: SchedulerBackend::Repo {
                jobs: job_repo,
                runs: Some(job_run_repo),
            },
            runs: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Add a new job. Returns the job ID.
    pub async fn add_job(&self, job: Job) -> JobId {
        let id = job.id;
        info!(job_id = %id, name = ?job.name, "adding job");
        match &self.backend {
            SchedulerBackend::Memory(jobs) => {
                jobs.lock().await.insert(id, job);
            }
            SchedulerBackend::Repo { jobs: repo, .. } => {
                let now = job.created_at;
                let schedule_json = serde_json::to_string(&job.schedule).ok();
                let payload = stored_job_payload(&job);
                let new_job = NewJob {
                    id: Uuid::from(job.id),
                    job_type: "cron".to_string(),
                    schedule: schedule_json,
                    due_at: job.next_run_at,
                    enabled: job.enabled,
                    payload_kind: job.payload.kind().as_str().to_string(),
                    delivery_mode: job.delivery_mode.as_str().to_string(),
                    payload,
                    created_at: now,
                    updated_at: now,
                };
                if let Err(error) = repo.create(new_job).await {
                    warn!(job_id = %id, error = %error, "failed to persist cron job");
                }
            }
        }
        id
    }

    /// List all jobs, optionally including disabled ones.
    pub async fn list_jobs(&self, include_disabled: bool) -> Vec<Job> {
        match &self.backend {
            SchedulerBackend::Memory(jobs) => {
                let jobs = jobs.lock().await;
                let mut result: Vec<Job> = if include_disabled {
                    jobs.values().cloned().collect()
                } else {
                    jobs.values().filter(|j| j.enabled).cloned().collect()
                };
                result.sort_by_key(|j| j.created_at);
                result
            }
            SchedulerBackend::Repo { jobs: repo, .. } => {
                match repo.list_by_type("cron", include_disabled).await {
                    Ok(rows) => {
                        let mut jobs: Vec<Job> = rows
                            .into_iter()
                            .filter_map(|row| row_to_job(row).ok())
                            .collect();
                        jobs.sort_by_key(|j| j.created_at);
                        jobs
                    }
                    Err(error) => {
                        warn!(error = %error, "failed to list persisted cron jobs");
                        Vec::new()
                    }
                }
            }
        }
    }

    /// Get a specific job by ID.
    pub async fn get_job(&self, id: &JobId) -> Option<Job> {
        match &self.backend {
            SchedulerBackend::Memory(jobs) => jobs.lock().await.get(id).cloned(),
            SchedulerBackend::Repo { jobs: repo, .. } => repo
                .find_by_id(Uuid::from(*id))
                .await
                .ok()
                .and_then(|row| row_to_job(row).ok()),
        }
    }

    /// Update a job.
    pub async fn update_job(&self, id: &JobId, update: JobUpdate) -> Option<Job> {
        match &self.backend {
            SchedulerBackend::Memory(jobs) => {
                let mut jobs = jobs.lock().await;
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
                if let Some(delivery_mode) = update.delivery_mode {
                    job.delivery_mode = delivery_mode;
                }

                Some(job.clone())
            }
            SchedulerBackend::Repo { jobs: repo, .. } => {
                let mut job = self.get_job(id).await?;
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
                if let Some(delivery_mode) = update.delivery_mode {
                    job.delivery_mode = delivery_mode;
                }
                let updated_at = Utc::now();
                let payload = stored_job_payload(&job);
                repo.update_job(
                    Uuid::from(*id),
                    job.enabled,
                    job.next_run_at,
                    job.payload.kind().as_str(),
                    job.delivery_mode.as_str(),
                    payload,
                    updated_at,
                    job.last_run_at,
                    job.next_run_at,
                )
                .await
                .ok()
                .and_then(|row| row_to_job(row).ok())
            }
        }
    }

    /// Remove a job.
    pub async fn remove_job(&self, id: &JobId) -> Option<Job> {
        info!(job_id = %id, "removing job");
        match &self.backend {
            SchedulerBackend::Memory(jobs) => jobs.lock().await.remove(id),
            SchedulerBackend::Repo { jobs: repo, .. } => {
                let existing = self.get_job(id).await?;
                match repo.delete(Uuid::from(*id)).await {
                    Ok(true) => Some(existing),
                    Ok(false) => None,
                    Err(error) => {
                        warn!(job_id = %id, error = %error, "failed to remove persisted cron job");
                        None
                    }
                }
            }
        }
    }

    /// Enable or disable a job.
    pub async fn set_enabled(&self, id: &JobId, enabled: bool) -> Option<Job> {
        self.update_job(
            id,
            JobUpdate {
                enabled: Some(enabled),
                ..Default::default()
            },
        )
        .await
    }

    /// Record a job run starting.
    pub async fn start_run(&self, job_id: JobId, trigger_kind: SchedulerRunTrigger) -> JobRun {
        let run = JobRun {
            job_id,
            started_at: Utc::now(),
            finished_at: None,
            trigger_kind,
            status: JobRunStatus::Running,
            output: None,
        };
        self.runs.lock().await.push(run.clone());

        // Update job metadata
        match &self.backend {
            SchedulerBackend::Memory(jobs) => {
                if let Some(job) = jobs.lock().await.get_mut(&job_id) {
                    job.last_run_at = Some(run.started_at);
                    job.run_count += 1;
                }
            }
            SchedulerBackend::Repo { jobs: repo, runs } => {
                if let Some(mut job) = self.get_job(&job_id).await {
                    job.last_run_at = Some(run.started_at);
                    job.run_count += 1;
                    let payload = stored_job_payload(&job);
                    if let Err(error) = repo
                        .update_job(
                            Uuid::from(job_id),
                            job.enabled,
                            job.next_run_at,
                            job.payload.kind().as_str(),
                            job.delivery_mode.as_str(),
                            payload,
                            run.started_at,
                            job.last_run_at,
                            job.next_run_at,
                        )
                        .await
                    {
                        warn!(job_id = %job_id, error = %error, "failed to persist cron run start metadata");
                    }
                }

                if let Some(run_repo) = runs {
                    let durable = NewJobRun {
                        id: Uuid::now_v7(),
                        job_id: Uuid::from(job_id),
                        started_at: run.started_at,
                        finished_at: None,
                        trigger_kind: run.trigger_kind.as_str().to_string(),
                        status: job_run_status_str(&run.status).to_string(),
                        output: None,
                        created_at: run.started_at,
                    };
                    if let Err(error) = run_repo.create(durable).await {
                        warn!(job_id = %job_id, error = %error, "failed to persist durable cron run start");
                    }
                }
            }
        }

        run
    }

    /// Complete a job run.
    pub async fn complete_run(&self, job_id: JobId, status: JobRunStatus, output: Option<String>) {
        let finished_at = Utc::now();
        let mut runs = self.runs.lock().await;
        // Find the most recent running entry for this job
        for run in runs.iter_mut().rev() {
            if run.job_id == job_id && run.status == JobRunStatus::Running {
                run.finished_at = Some(finished_at);
                run.status = status.clone();
                run.output = output.clone();
                break;
            }
        }
        drop(runs);

        if let SchedulerBackend::Repo {
            runs: Some(run_repo),
            ..
        } = &self.backend
        {
            match run_repo.list_by_job(Uuid::from(job_id), Some(1)).await {
                Ok(rows) => {
                    if let Some(latest) = rows.into_iter().find(|row| row.finished_at.is_none())
                        && let Err(error) = run_repo
                            .complete(
                                latest.id,
                                job_run_status_str(&status),
                                output.as_deref(),
                                finished_at,
                            )
                            .await
                    {
                        warn!(job_id = %job_id, error = %error, "failed to persist durable cron run completion");
                    }
                }
                Err(error) => {
                    warn!(job_id = %job_id, error = %error, "failed to load durable cron run history for completion");
                }
            }
        }
    }

    /// Get run history for a job.
    pub async fn get_runs(&self, job_id: &JobId, limit: Option<usize>) -> Vec<JobRun> {
        if let SchedulerBackend::Repo {
            runs: Some(run_repo),
            ..
        } = &self.backend
        {
            return match run_repo
                .list_by_job(Uuid::from(*job_id), limit.map(|value| value as i64))
                .await
            {
                Ok(rows) => rows.into_iter().map(row_to_job_run).collect(),
                Err(error) => {
                    warn!(job_id = %job_id, error = %error, "failed to list durable cron run history");
                    Vec::new()
                }
            };
        }

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
        match &self.backend {
            SchedulerBackend::Memory(jobs) => {
                let jobs = jobs.lock().await;
                jobs.values()
                    .filter(|j| j.enabled && j.next_run_at.is_some_and(|next| next <= now))
                    .cloned()
                    .collect()
            }
            SchedulerBackend::Repo { jobs: repo, .. } => {
                match repo.list_by_type("cron", false).await {
                    Ok(rows) => rows
                        .into_iter()
                        .filter_map(|row| row_to_job(row).ok())
                        .filter(|j| j.enabled && j.next_run_at.is_some_and(|next| next <= now))
                        .collect(),
                    Err(error) => {
                        warn!(error = %error, "failed to list due persisted cron jobs");
                        Vec::new()
                    }
                }
            }
        }
    }

    /// Compute and set the next run time for a job after a firing attempt.
    pub async fn advance_next_run(&self, id: &JobId) {
        match &self.backend {
            SchedulerBackend::Memory(jobs) => {
                let mut jobs = jobs.lock().await;
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
            SchedulerBackend::Repo { jobs: repo, .. } => {
                if let Some(mut job) = self.get_job(id).await {
                    let now = Utc::now();
                    let base = job.last_run_at.or(job.next_run_at).unwrap_or(now);
                    match &job.schedule {
                        Schedule::At { .. } => {
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
                    let payload = stored_job_payload(&job);
                    if let Err(error) = repo
                        .update_job(
                            Uuid::from(*id),
                            job.enabled,
                            job.next_run_at,
                            job.payload.kind().as_str(),
                            job.delivery_mode.as_str(),
                            payload,
                            now,
                            job.last_run_at,
                            job.next_run_at,
                        )
                        .await
                    {
                        warn!(job_id = %id, error = %error, "failed to persist cron next-run advancement");
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
    pub delivery_mode: Option<SchedulerDeliveryMode>,
}

// ── Reminders ─────────────────────────────────────────────────────────────────

/// Reminder lifecycle outcome.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReminderStatus {
    #[default]
    Pending,
    Delivered,
    Cancelled,
    Missed,
}

impl ReminderStatus {
    fn is_terminal(&self) -> bool {
        !matches!(self, Self::Pending)
    }
}

/// A one-shot reminder: fire-at timestamp, message, and target.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reminder {
    pub id: JobId,
    pub message: String,
    /// Target session or channel identifier.
    pub target: String,
    /// When to fire.
    pub fire_at: DateTime<Utc>,
    /// Whether the reminder has been delivered.
    pub delivered: bool,
    /// When the reminder was created.
    pub created_at: DateTime<Utc>,
    /// When the reminder was delivered (if at all).
    pub delivered_at: Option<DateTime<Utc>>,
    /// Reminder lifecycle outcome.
    #[serde(default)]
    pub status: ReminderStatus,
    /// When the reminder reached a terminal outcome.
    pub outcome_at: Option<DateTime<Utc>>,
    /// Last recorded terminal error, if any.
    pub last_error: Option<String>,
}

impl Reminder {
    /// Create a new reminder.
    pub fn new(
        message: impl Into<String>,
        target: impl Into<String>,
        fire_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id: JobId::new(),
            message: message.into(),
            target: target.into(),
            fire_at,
            delivered: false,
            created_at: Utc::now(),
            delivered_at: None,
            status: ReminderStatus::Pending,
            outcome_at: None,
            last_error: None,
        }
    }

    /// Check whether this reminder is due.
    pub fn is_due(&self) -> bool {
        matches!(self.status, ReminderStatus::Pending) && self.fire_at <= Utc::now()
    }

    fn mark_delivered(&mut self, at: DateTime<Utc>) {
        self.delivered = true;
        self.delivered_at = Some(at);
        self.status = ReminderStatus::Delivered;
        self.outcome_at = Some(at);
        self.last_error = None;
    }

    fn mark_cancelled(&mut self, at: DateTime<Utc>) {
        self.delivered = false;
        self.status = ReminderStatus::Cancelled;
        self.outcome_at = Some(at);
    }

    fn mark_missed(&mut self, at: DateTime<Utc>, error: String) {
        self.delivered = false;
        self.status = ReminderStatus::Missed;
        self.outcome_at = Some(at);
        self.last_error = Some(error);
    }
}

#[derive(Clone, Debug)]
pub struct ReminderDeliveryAttempt {
    run_id: Option<Uuid>,
    started_at: DateTime<Utc>,
}

enum ReminderStoreBackend {
    Memory(Arc<Mutex<Vec<Reminder>>>),
    Repo {
        jobs: Arc<dyn JobRepo>,
        runs: Option<Arc<dyn JobRunRepo>>,
    },
}

/// Reminder store managed by the scheduler.
///
/// Uses PostgreSQL-backed `jobs` rows when a `JobRepo` is provided and falls
/// back to an in-memory store for lightweight tests.
pub struct ReminderStore {
    backend: ReminderStoreBackend,
}

impl ReminderStore {
    /// Create a new empty in-memory reminder store.
    pub fn new() -> Self {
        Self {
            backend: ReminderStoreBackend::Memory(Arc::new(Mutex::new(Vec::new()))),
        }
    }

    /// Create a reminder store backed by the durable job repository.
    pub fn new_with_repo(job_repo: Arc<dyn JobRepo>) -> Self {
        Self {
            backend: ReminderStoreBackend::Repo {
                jobs: job_repo,
                runs: None,
            },
        }
    }

    /// Create a reminder store backed by durable job and job-run repositories.
    pub fn new_with_repos(job_repo: Arc<dyn JobRepo>, job_run_repo: Arc<dyn JobRunRepo>) -> Self {
        Self {
            backend: ReminderStoreBackend::Repo {
                jobs: job_repo,
                runs: Some(job_run_repo),
            },
        }
    }

    /// Add a reminder. Returns its ID.
    pub async fn add(&self, reminder: Reminder) -> JobId {
        let id = reminder.id;
        info!(reminder_id = %id, fire_at = %reminder.fire_at, "adding reminder");
        match &self.backend {
            ReminderStoreBackend::Memory(reminders) => {
                reminders.lock().await.push(reminder);
            }
            ReminderStoreBackend::Repo { jobs: repo, .. } => {
                let now = reminder.created_at;
                let payload = serde_json::to_value(&reminder)
                    .expect("Reminder serialization should not fail");
                let new_job = NewJob {
                    id: Uuid::from(reminder.id),
                    job_type: "reminder".to_string(),
                    schedule: None,
                    due_at: Some(reminder.fire_at),
                    enabled: !reminder.delivered,
                    payload_kind: SchedulerPayloadKind::Reminder.as_str().to_string(),
                    delivery_mode: SchedulerDeliveryMode::Announce.as_str().to_string(),
                    payload,
                    created_at: now,
                    updated_at: now,
                };
                if let Err(error) = repo.create(new_job).await {
                    warn!(reminder_id = %id, error = %error, "failed to persist reminder");
                }
            }
        }
        id
    }

    /// List all reminders, optionally including delivered ones.
    pub async fn list(&self, include_delivered: bool) -> Vec<Reminder> {
        match &self.backend {
            ReminderStoreBackend::Memory(reminders) => {
                let reminders = reminders.lock().await;
                let mut result: Vec<Reminder> = if include_delivered {
                    reminders.clone()
                } else {
                    reminders
                        .iter()
                        .filter(|r| !r.status.is_terminal())
                        .cloned()
                        .collect()
                };
                result.sort_by_key(|r| r.fire_at);
                result
            }
            ReminderStoreBackend::Repo { jobs: repo, .. } => {
                match repo.list_by_type("reminder", true).await {
                    Ok(rows) => {
                        let mut reminders: Vec<Reminder> = rows
                            .into_iter()
                            .filter_map(|row| row_to_reminder(row).ok())
                            .filter(|reminder| include_delivered || !reminder.status.is_terminal())
                            .collect();
                        reminders.sort_by_key(|r| r.fire_at);
                        reminders
                    }
                    Err(error) => {
                        warn!(error = %error, "failed to list persisted reminders");
                        Vec::new()
                    }
                }
            }
        }
    }

    /// Get a specific reminder.
    pub async fn get(&self, id: &JobId) -> Option<Reminder> {
        match &self.backend {
            ReminderStoreBackend::Memory(reminders) => {
                reminders.lock().await.iter().find(|r| r.id == *id).cloned()
            }
            ReminderStoreBackend::Repo { jobs: repo, .. } => repo
                .find_by_id(Uuid::from(*id))
                .await
                .ok()
                .and_then(|row| row_to_reminder(row).ok()),
        }
    }

    /// Cancel a reminder. Returns the updated reminder if found.
    pub async fn cancel(&self, id: &JobId) -> Option<Reminder> {
        match &self.backend {
            ReminderStoreBackend::Memory(reminders) => {
                let mut reminders = reminders.lock().await;
                if let Some(reminder) = reminders
                    .iter_mut()
                    .find(|reminder| reminder.id == *id && !reminder.status.is_terminal())
                {
                    info!(reminder_id = %id, "cancelling reminder");
                    reminder.mark_cancelled(Utc::now());
                    Some(reminder.clone())
                } else {
                    None
                }
            }
            ReminderStoreBackend::Repo { .. } => {
                let mut reminder = self.get(id).await?;
                if reminder.status.is_terminal() {
                    return None;
                }

                let cancelled_at = Utc::now();
                reminder.mark_cancelled(cancelled_at);
                self.persist_terminal_reminder(
                    &reminder,
                    cancelled_at,
                    None,
                    JobRunStatus::Skipped,
                    None,
                    None,
                )
                .await
            }
        }
    }

    /// Get all due (unfired) reminders.
    pub async fn get_due(&self) -> Vec<Reminder> {
        match &self.backend {
            ReminderStoreBackend::Memory(reminders) => {
                let reminders = reminders.lock().await;
                reminders.iter().filter(|r| r.is_due()).cloned().collect()
            }
            ReminderStoreBackend::Repo { .. } => self
                .list(false)
                .await
                .into_iter()
                .filter(Reminder::is_due)
                .collect(),
        }
    }

    /// Record a reminder delivery attempt starting.
    pub async fn start_delivery_attempt(&self, id: &JobId) -> ReminderDeliveryAttempt {
        let started_at = Utc::now();
        let run_id = match &self.backend {
            ReminderStoreBackend::Memory(_) => None,
            ReminderStoreBackend::Repo {
                runs: Some(run_repo),
                ..
            } => {
                let durable = NewJobRun {
                    id: Uuid::now_v7(),
                    job_id: Uuid::from(*id),
                    started_at,
                    finished_at: None,
                    trigger_kind: SchedulerRunTrigger::Due.as_str().to_string(),
                    status: job_run_status_str(&JobRunStatus::Running).to_string(),
                    output: None,
                    created_at: started_at,
                };
                match run_repo.create(durable).await {
                    Ok(row) => Some(row.id),
                    Err(error) => {
                        warn!(reminder_id = %id, error = %error, "failed to persist reminder run start");
                        None
                    }
                }
            }
            ReminderStoreBackend::Repo { runs: None, .. } => None,
        };

        ReminderDeliveryAttempt { run_id, started_at }
    }

    /// Mark a reminder as delivered.
    pub async fn mark_delivered(
        &self,
        id: &JobId,
        attempt: &ReminderDeliveryAttempt,
        output: Option<String>,
    ) -> Option<Reminder> {
        match &self.backend {
            ReminderStoreBackend::Memory(reminders) => {
                let mut reminders = reminders.lock().await;
                if let Some(reminder) = reminders.iter_mut().find(|r| r.id == *id) {
                    reminder.mark_delivered(Utc::now());
                    Some(reminder.clone())
                } else {
                    None
                }
            }
            ReminderStoreBackend::Repo { .. } => {
                let mut reminder = self.get(id).await?;
                reminder.mark_delivered(Utc::now());
                self.persist_terminal_reminder(
                    &reminder,
                    reminder.outcome_at.unwrap_or_else(Utc::now),
                    Some(attempt.started_at),
                    JobRunStatus::Completed,
                    attempt.run_id,
                    output,
                )
                .await
            }
        }
    }

    /// Mark a reminder as missed after a failed one-shot delivery attempt.
    pub async fn mark_missed(
        &self,
        id: &JobId,
        attempt: &ReminderDeliveryAttempt,
        error: impl Into<String>,
    ) -> Option<Reminder> {
        let error = error.into();
        match &self.backend {
            ReminderStoreBackend::Memory(reminders) => {
                let mut reminders = reminders.lock().await;
                if let Some(reminder) = reminders.iter_mut().find(|r| r.id == *id) {
                    reminder.mark_missed(Utc::now(), error);
                    Some(reminder.clone())
                } else {
                    None
                }
            }
            ReminderStoreBackend::Repo { .. } => {
                let mut reminder = self.get(id).await?;
                reminder.mark_missed(Utc::now(), error.clone());
                self.persist_terminal_reminder(
                    &reminder,
                    reminder.outcome_at.unwrap_or_else(Utc::now),
                    Some(attempt.started_at),
                    JobRunStatus::Failed,
                    attempt.run_id,
                    Some(error),
                )
                .await
            }
        }
    }

    async fn persist_terminal_reminder(
        &self,
        reminder: &Reminder,
        updated_at: DateTime<Utc>,
        last_run_at: Option<DateTime<Utc>>,
        run_status: JobRunStatus,
        run_id: Option<Uuid>,
        output: Option<String>,
    ) -> Option<Reminder> {
        let ReminderStoreBackend::Repo { jobs, runs } = &self.backend else {
            return None;
        };

        let payload =
            serde_json::to_value(reminder).expect("Reminder serialization should not fail");
        let persisted = match jobs
            .update_job(
                Uuid::from(reminder.id),
                false,
                Some(reminder.fire_at),
                SchedulerPayloadKind::Reminder.as_str(),
                SchedulerDeliveryMode::Announce.as_str(),
                payload,
                updated_at,
                last_run_at,
                None,
            )
            .await
        {
            Ok(row) => row_to_reminder(row).ok(),
            Err(error) => {
                warn!(reminder_id = %reminder.id, error = %error, "failed to persist reminder outcome");
                None
            }
        };

        if let (Some(run_repo), Some(run_id)) = (runs, run_id) {
            if let Err(error) = run_repo
                .complete(
                    run_id,
                    job_run_status_str(&run_status),
                    output.as_deref(),
                    updated_at,
                )
                .await
            {
                warn!(reminder_id = %reminder.id, error = %error, "failed to persist reminder run completion");
            }
        }

        persisted
    }
}

impl Default for ReminderStore {
    fn default() -> Self {
        Self::new()
    }
}

fn row_to_job(row: JobRow) -> Result<Job, StoreError> {
    let stored: StoredJobRecord = serde_json::from_value(row.payload.clone())
        .map_err(|error| StoreError::Serialization(error.to_string()))?;
    let schedule = row
        .schedule
        .as_deref()
        .ok_or_else(|| StoreError::Serialization("job row missing schedule".to_string()))
        .and_then(|value| {
            serde_json::from_str::<Schedule>(value)
                .map_err(|error| StoreError::Serialization(error.to_string()))
        })?;

    Ok(Job {
        id: JobId::from(row.id),
        name: stored.name,
        schedule,
        payload: stored.payload,
        delivery_mode: row.delivery_mode.parse().unwrap_or(stored.delivery_mode),
        session_target: stored.session_target,
        enabled: row.enabled,
        created_at: row.created_at,
        last_run_at: row.last_run_at,
        next_run_at: row.next_run_at.or(row.due_at),
        run_count: stored.run_count,
    })
}

fn row_to_reminder(row: rune_store::models::JobRow) -> Result<Reminder, StoreError> {
    let mut reminder: Reminder = serde_json::from_value(row.payload.clone())
        .map_err(|error| StoreError::Serialization(error.to_string()))?;
    reminder.id = JobId::from(row.id);
    reminder.fire_at = row.due_at.unwrap_or(reminder.fire_at);
    if matches!(reminder.status, ReminderStatus::Pending)
        && (reminder.delivered || reminder.delivered_at.is_some() || !row.enabled)
    {
        reminder.status = ReminderStatus::Delivered;
    }
    reminder.delivered = matches!(reminder.status, ReminderStatus::Delivered);
    if reminder.delivered && reminder.delivered_at.is_none() {
        reminder.delivered_at = row.last_run_at;
    }
    if reminder.outcome_at.is_none() {
        reminder.outcome_at = match reminder.status {
            ReminderStatus::Pending => None,
            ReminderStatus::Delivered => reminder.delivered_at.or(row.last_run_at),
            ReminderStatus::Cancelled | ReminderStatus::Missed => Some(row.updated_at),
        };
    }
    Ok(reminder)
}

fn row_to_job_run(row: JobRunRow) -> JobRun {
    JobRun {
        job_id: JobId::from(row.job_id),
        started_at: row.started_at,
        finished_at: row.finished_at,
        trigger_kind: row.trigger_kind.parse().unwrap_or(SchedulerRunTrigger::Due),
        status: parse_job_run_status(&row.status),
        output: row.output,
    }
}

fn stored_job_payload(job: &Job) -> serde_json::Value {
    serde_json::to_value(StoredJobRecord {
        name: job.name.clone(),
        payload: job.payload.clone(),
        delivery_mode: job.delivery_mode,
        session_target: job.session_target,
        run_count: job.run_count,
    })
    .unwrap_or_else(|_| serde_json::json!({}))
}

fn job_run_status_str(status: &JobRunStatus) -> &'static str {
    match status {
        JobRunStatus::Running => "running",
        JobRunStatus::Completed => "completed",
        JobRunStatus::Failed => "failed",
        JobRunStatus::Skipped => "skipped",
    }
}

fn parse_job_run_status(value: &str) -> JobRunStatus {
    match value {
        "completed" => JobRunStatus::Completed,
        "failed" => JobRunStatus::Failed,
        "skipped" => JobRunStatus::Skipped,
        _ => JobRunStatus::Running,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Timelike};

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
            delivery_mode: SchedulerDeliveryMode::None,
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

        let run = scheduler.start_run(id, SchedulerRunTrigger::Due).await;
        assert_eq!(run.status, JobRunStatus::Running);
        assert_eq!(run.trigger_kind, SchedulerRunTrigger::Due);

        scheduler
            .complete_run(id, JobRunStatus::Completed, Some("done".into()))
            .await;

        let runs = scheduler.get_runs(&id, None).await;
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, JobRunStatus::Completed);
        assert_eq!(runs[0].trigger_kind, SchedulerRunTrigger::Due);
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

    // ── Reminder tests ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn reminder_add_and_list() {
        let store = ReminderStore::new();
        let reminder = Reminder::new("Buy milk", "main", Utc::now() + chrono::Duration::hours(1));
        let id = store.add(reminder).await;

        let list = store.list(false).await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, id);
        assert_eq!(list[0].message, "Buy milk");
    }

    #[tokio::test]
    async fn reminder_cancel() {
        let store = ReminderStore::new();
        let reminder = Reminder::new("Cancel me", "main", Utc::now() + chrono::Duration::hours(1));
        let id = store.add(reminder).await;

        let cancelled = store.cancel(&id).await;
        assert!(cancelled.is_some());
        let cancelled = cancelled.unwrap();
        assert_eq!(cancelled.status, ReminderStatus::Cancelled);
        assert!(cancelled.outcome_at.is_some());
        assert_eq!(store.list(false).await.len(), 0);
        assert_eq!(store.list(true).await.len(), 1);
    }

    #[tokio::test]
    async fn reminder_cancel_nonexistent() {
        let store = ReminderStore::new();
        let fake_id = JobId::new();
        assert!(store.cancel(&fake_id).await.is_none());
    }

    #[tokio::test]
    async fn reminder_get_due() {
        let store = ReminderStore::new();
        // Due (fire_at in the past)
        let due = Reminder::new(
            "Due now",
            "main",
            Utc::now() - chrono::Duration::seconds(10),
        );
        store.add(due).await;

        // Not due (fire_at in the future)
        let future = Reminder::new("Later", "main", Utc::now() + chrono::Duration::hours(1));
        store.add(future).await;

        let due_list = store.get_due().await;
        assert_eq!(due_list.len(), 1);
        assert_eq!(due_list[0].message, "Due now");
    }

    #[tokio::test]
    async fn reminder_mark_delivered() {
        let store = ReminderStore::new();
        let reminder = Reminder::new(
            "Deliver me",
            "main",
            Utc::now() - chrono::Duration::seconds(10),
        );
        let id = store.add(reminder).await;
        let attempt = store.start_delivery_attempt(&id).await;

        let delivered = store
            .mark_delivered(&id, &attempt, Some("done".into()))
            .await
            .unwrap();
        assert!(delivered.delivered);
        assert!(delivered.delivered_at.is_some());
        assert_eq!(delivered.status, ReminderStatus::Delivered);
        assert!(delivered.outcome_at.is_some());
        assert!(delivered.last_error.is_none());

        // Should not appear in non-delivered list
        assert_eq!(store.list(false).await.len(), 0);
        // But should appear in full list
        assert_eq!(store.list(true).await.len(), 1);
    }

    #[tokio::test]
    async fn reminder_mark_missed() {
        let store = ReminderStore::new();
        let reminder = Reminder::new("Miss me", "main", Utc::now() - chrono::Duration::seconds(5));
        let id = store.add(reminder).await;
        let attempt = store.start_delivery_attempt(&id).await;

        let missed = store
            .mark_missed(&id, &attempt, "session unavailable")
            .await
            .unwrap();

        assert_eq!(missed.status, ReminderStatus::Missed);
        assert!(!missed.delivered);
        assert_eq!(missed.last_error.as_deref(), Some("session unavailable"));
        assert_eq!(store.list(false).await.len(), 0);
        assert_eq!(store.list(true).await.len(), 1);
    }

    #[tokio::test]
    async fn reminder_is_due_checks_status() {
        let mut reminder =
            Reminder::new("Done", "main", Utc::now() - chrono::Duration::seconds(10));
        assert!(reminder.is_due());
        reminder.status = ReminderStatus::Delivered;
        assert!(!reminder.is_due());
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
