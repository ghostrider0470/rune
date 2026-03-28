//! Background service supervisor: heartbeats, scheduled jobs, and reminders.
//!
//! Runs a tokio task that ticks every ~10 seconds, checking for due work.

use std::sync::Arc;

use serde_json::json;
use tokio::sync::{broadcast, watch};
use tracing::{debug, error, info, warn};

use rune_core::{SchedulerDeliveryMode, SchedulerRunTrigger, SessionKind, TriggerKind};
use rune_runtime::heartbeat::{HeartbeatResponseAction, HeartbeatRunner};
use rune_runtime::scheduler::{Job, JobPayload, ReminderStore, Scheduler, SessionTarget};
use rune_runtime::{PluginScanner, SessionEngine, TurnExecutor};
use rune_store::models::SessionRow;

use crate::pairing::DeviceRegistry;
use crate::state::SessionEvent;

/// Default lease duration (seconds) for durable job/reminder claims.
/// Claims older than this are considered expired and reclaimable by another
/// supervisor instance (crash recovery).
const CLAIM_LEASE_SECS: i64 = 300;

/// Manages background services (heartbeat, scheduler, reminders).
pub struct BackgroundSupervisor {
    handle: Option<tokio::task::JoinHandle<()>>,
    shutdown_tx: Option<watch::Sender<bool>>,
}

/// Callback for delivering heartbeat/scheduled output to the operator's channel.
#[async_trait::async_trait]
pub trait OperatorDelivery: Send + Sync {
    /// Send a text message to the operator's primary channel (e.g. Telegram).
    async fn deliver(&self, text: &str) -> Result<(), String>;
}

/// Delivers messages to a Telegram chat via the Bot API.
pub struct TelegramOperatorDelivery {
    bot_token: String,
    chat_id: String,
    client: reqwest::Client,
}

impl TelegramOperatorDelivery {
    pub fn new(bot_token: impl Into<String>, chat_id: impl Into<String>) -> Self {
        Self {
            bot_token: bot_token.into(),
            chat_id: chat_id.into(),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl OperatorDelivery for TelegramOperatorDelivery {
    async fn deliver(&self, text: &str) -> Result<(), String> {
        let url = format!("https://api.telegram.org/bot{}/sendMessage", self.bot_token);
        let params = serde_json::json!({
            "chat_id": self.chat_id,
            "text": text,
            "parse_mode": "Markdown",
        });

        let resp = self
            .client
            .post(&url)
            .json(&params)
            .send()
            .await
            .map_err(|e| format!("telegram send failed: {e}"))?;

        if !resp.status().is_success() {
            // Retry without Markdown if parsing failed
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            let desc = body
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if desc.contains("parse entities") || desc.contains("can't parse") {
                let plain = serde_json::json!({
                    "chat_id": self.chat_id,
                    "text": text,
                });
                let _ = self.client.post(&url).json(&plain).send().await;
            }
        }

        Ok(())
    }
}

/// Dependencies the supervisor loop needs.
#[derive(Clone)]
pub struct SupervisorDeps {
    pub heartbeat: Arc<HeartbeatRunner>,
    pub scheduler: Arc<Scheduler>,
    pub reminder_store: Arc<ReminderStore>,
    pub session_engine: Arc<SessionEngine>,
    pub turn_executor: Arc<TurnExecutor>,
    pub workspace_root: Option<String>,
    pub device_registry: Arc<DeviceRegistry>,
    /// Broadcast channel for session/runtime events (delivery announce).
    pub event_tx: broadcast::Sender<SessionEvent>,
    /// Optional delivery to the operator's user-facing channel.
    pub operator_delivery: Option<Arc<dyn OperatorDelivery>>,
    /// Optional plugin scanner for hot-reload.
    pub plugin_scanner: Option<Arc<PluginScanner>>,
    /// How many supervisor ticks (each ~10s) between re-scans. 0 = disabled.
    pub plugin_scan_interval_ticks: u64,
    /// Optional inter-agent comms client.
    pub comms: Option<Arc<rune_runtime::CommsClient>>,
}

impl BackgroundSupervisor {
    /// Create a new supervisor. No background services are started yet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            handle: None,
            shutdown_tx: None,
        }
    }

    /// Start the background supervisor loop with the given dependencies.
    pub fn start(&mut self, deps: SupervisorDeps) {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        self.shutdown_tx = Some(shutdown_tx);

        info!("background supervisor starting");
        self.handle = Some(tokio::spawn(supervisor_loop(deps, shutdown_rx)));
    }

    /// Request graceful shutdown of all background services.
    pub fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(true);
        }
        if let Some(handle) = self.handle.take() {
            handle.abort();
            info!("background supervisor shut down");
        }
    }
}

impl Default for BackgroundSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) async fn execute_job(
    deps: &SupervisorDeps,
    job: &Job,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    match &job.payload {
        JobPayload::SystemEvent { text } => run_system_event(deps, text).await,
        JobPayload::AgentTurn { message, model, .. } => {
            let enriched = enrich_with_last_run(deps, job, message).await;
            run_agent_turn(deps, &enriched, model.as_deref(), job.session_target).await
        }
    }
}

/// Prepend last-run output to a cron message for continuity across ticks.
async fn enrich_with_last_run(deps: &SupervisorDeps, job: &Job, message: &str) -> String {
    let runs = deps.scheduler.get_runs(&job.id, Some(1)).await;
    let last_output = runs.first().and_then(|r| {
        if r.status == rune_runtime::scheduler::JobRunStatus::Completed {
            r.output.as_deref()
        } else {
            None
        }
    });

    match last_output {
        Some(output) if !output.is_empty() => {
            let truncated = if output.len() > 2000 {
                &output[..2000]
            } else {
                output
            };
            format!("## Last run result:\n{truncated}\n\n## Your task:\n{message}")
        }
        _ => message.to_string(),
    }
}

pub(crate) async fn run_job_lifecycle(
    deps: &SupervisorDeps,
    job: &Job,
    advance_next_run: bool,
    run_trigger: SchedulerRunTrigger,
) -> (rune_runtime::scheduler::JobRunStatus, String) {
    let job_id = job.id;
    deps.scheduler.start_run(job_id, run_trigger).await;

    let result = execute_job(deps, job).await;

    let (status, output) = match result {
        Ok(output) => (rune_runtime::scheduler::JobRunStatus::Completed, output),
        Err(error) => {
            error!(job_id = %job_id, error = %error, "job execution failed");
            (
                rune_runtime::scheduler::JobRunStatus::Failed,
                error.to_string(),
            )
        }
    };

    deps.scheduler
        .complete_run(job_id, status.clone(), Some(output.clone()))
        .await;

    if advance_next_run {
        deps.scheduler.advance_next_run(&job_id).await;
    }

    // Deliver the result based on the job's delivery_mode.
    deliver_result(deps, job, &status, &output, run_trigger).await;

    (status, output)
}

/// Deliver a job result according to the job's `delivery_mode`.
async fn deliver_result(
    deps: &SupervisorDeps,
    job: &Job,
    status: &rune_runtime::scheduler::JobRunStatus,
    output: &str,
    trigger: SchedulerRunTrigger,
) {
    deliver_result_standalone(
        &deps.event_tx,
        deps.operator_delivery.as_deref(),
        job,
        status,
        output,
        trigger,
    )
    .await;

    // Write cron result to comms peer inbox (Announce mode only).
    if matches!(job.delivery_mode, SchedulerDeliveryMode::Announce) {
        if let Some(ref comms) = deps.comms {
            let status_str = match status {
                rune_runtime::scheduler::JobRunStatus::Completed => "completed",
                rune_runtime::scheduler::JobRunStatus::Failed => "FAILED",
                _ => "finished",
            };
            let truncated = if output.len() > 3000 {
                &output[..3000]
            } else {
                output
            };
            let subject = format!(
                "[cron:{}] {}",
                job.name.as_deref().unwrap_or("unknown"),
                status_str
            );
            if let Err(e) = comms.send("result", &subject, truncated, "p2").await {
                warn!(error = %e, "failed to write cron result to comms");
            }
        }
    }
}

/// Core delivery logic, callable without full `SupervisorDeps` (for testing).
async fn deliver_result_standalone(
    event_tx: &broadcast::Sender<SessionEvent>,
    operator_delivery: Option<&dyn OperatorDelivery>,
    job: &Job,
    status: &rune_runtime::scheduler::JobRunStatus,
    output: &str,
    trigger: SchedulerRunTrigger,
) {
    match job.delivery_mode {
        SchedulerDeliveryMode::None => {
            // Silent execution — no additional delivery.
        }
        SchedulerDeliveryMode::Announce => {
            let _ = event_tx.send(SessionEvent {
                session_id: job.id.to_string(),
                kind: "cron_run_completed".to_string(),
                payload: json!({
                    "job_id": job.id.to_string(),
                    "job_name": job.name,
                    "delivery_mode": "announce",
                    "trigger": trigger.as_str(),
                    "status": status,
                    "output": output,
                }),
                state_changed: true,
            });
            debug!(job_id = %job.id, "announce delivery sent");

            // Also deliver to operator's Telegram (proactive updates)
            if let Some(delivery) = operator_delivery {
                let status_str = match status {
                    rune_runtime::scheduler::JobRunStatus::Completed => "completed",
                    rune_runtime::scheduler::JobRunStatus::Failed => "FAILED",
                    _ => "finished",
                };
                let truncated = if output.len() > 3000 {
                    &output[..3000]
                } else {
                    output
                };
                let msg = format!(
                    "*[{}]* {} ({})\n\n{}",
                    job.name.as_deref().unwrap_or("cron"),
                    status_str,
                    trigger.as_str(),
                    truncated,
                );
                if let Err(e) = delivery.deliver(&msg).await {
                    warn!(job_id = %job.id, error = %e, "operator delivery for cron result failed");
                }
            }
        }
        SchedulerDeliveryMode::Webhook => {
            let Some(url) = job.webhook_url.as_deref() else {
                warn!(job_id = %job.id, "webhook delivery_mode but no webhook_url configured; skipping");
                return;
            };
            let payload = json!({
                "job_id": job.id.to_string(),
                "job_name": job.name,
                "delivery_mode": "webhook",
                "trigger": trigger.as_str(),
                "status": status,
                "output": output,
            });
            match reqwest::Client::new()
                .post(url)
                .json(&payload)
                .timeout(std::time::Duration::from_secs(30))
                .send()
                .await
            {
                Ok(resp) => {
                    if resp.status().is_success() {
                        info!(job_id = %job.id, url = %url, "webhook delivery succeeded");
                    } else {
                        warn!(
                            job_id = %job.id, url = %url, status = %resp.status(),
                            "webhook delivery returned non-success status"
                        );
                    }
                }
                Err(error) => {
                    error!(
                        job_id = %job.id, url = %url, error = %error,
                        "webhook delivery failed"
                    );
                }
            }
        }
    }
}

/// Process messages from the comms inbox.
async fn check_comms_inbox(deps: &SupervisorDeps) {
    let Some(ref comms) = deps.comms else {
        return;
    };

    let messages = comms.read_inbox().await;
    if messages.is_empty() {
        return;
    }

    info!(count = messages.len(), "processing comms inbox messages");

    for (path, msg) in messages {
        debug!(
            id = %msg.id,
            msg_type = %msg.msg_type,
            from = %msg.from,
            subject = %msg.subject,
            "processing comms message"
        );

        match msg.msg_type.as_str() {
            "ack" | "result" => {
                // Archive silently.
                if let Err(e) = comms.archive(&path).await {
                    warn!(error = %e, "failed to archive comms message");
                }
            }
            "status" => {
                // Respond with gateway status.
                let sessions_count = match deps
                    .session_engine
                    .session_repo()
                    .list_active_channel_sessions()
                    .await
                {
                    Ok(s) => s.len(),
                    Err(_) => 0,
                };
                let jobs = deps.scheduler.list_jobs(false).await;
                let body = format!(
                    "Gateway status:\n- Active sessions: {}\n- Cron jobs: {}\n- Uptime: running",
                    sessions_count,
                    jobs.len()
                );
                if let Err(e) = comms
                    .send("result", &format!("re: {}", msg.subject), &body, "p2")
                    .await
                {
                    warn!(error = %e, "failed to send comms status response");
                }
                if let Err(e) = comms.archive(&path).await {
                    warn!(error = %e, "failed to archive comms message");
                }
            }
            "task" | "question" | "directive" | "answer" | "proposal" => {
                // Route all substantive message types through an agent turn
                // so that Rune actually reasons about the content.
                let prompt = format!(
                    "[Inter-Agent Comms] Incoming {} from {} via the .comms/ mailbox.\n\
                    Priority: {}\nSubject: {}\n\n{}\n\n\
                    Respond with a clear, actionable answer. Your response will be sent back as a result message.",
                    msg.msg_type, msg.from, msg.priority, msg.subject, msg.body
                );
                match run_agent_turn(deps, &prompt, None, SessionTarget::Isolated).await {
                    Ok(response) => {
                        let truncated = if response.len() > 3000 {
                            &response[..3000]
                        } else {
                            &response
                        };
                        if let Err(e) = comms
                            .send(
                                "result",
                                &format!("re: {}", msg.subject),
                                truncated,
                                &msg.priority,
                            )
                            .await
                        {
                            warn!(error = %e, "failed to send comms result");
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "comms agent turn failed");
                        let _ = comms
                            .send(
                                "result",
                                &format!("re: {} [error]", msg.subject),
                                &format!("Agent turn failed: {e}"),
                                "p1",
                            )
                            .await;
                    }
                }
                if let Err(e) = comms.archive(&path).await {
                    warn!(error = %e, "failed to archive comms message");
                }
            }
            other => {
                warn!(msg_type = other, "unknown comms message type, archiving");
                if let Err(e) = comms.archive(&path).await {
                    warn!(error = %e, "failed to archive comms message");
                }
            }
        }
    }
}

async fn supervisor_loop(deps: SupervisorDeps, mut shutdown_rx: watch::Receiver<bool>) {
    info!("supervisor loop started");

    let mut tick_count: u64 = 0;

    loop {
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_secs(10)) => {}
            result = shutdown_rx.changed() => {
                if result.is_err() || *shutdown_rx.borrow() {
                    info!("supervisor loop shutting down");
                    return;
                }
            }
        }

        // --- Heartbeat ---
        if deps.heartbeat.is_due().await {
            debug!("heartbeat is due, firing");
            let tick_result = deps.heartbeat.tick().await;
            if let Some(prompt) = tick_result.prompt {
                match run_heartbeat(&deps, &prompt).await {
                    Ok(response) => match deps.heartbeat.record_response(&response).await {
                        HeartbeatResponseAction::SuppressNoop => {
                            debug!("heartbeat response suppressed (HEARTBEAT_OK)");
                        }
                        HeartbeatResponseAction::SuppressDuplicate => {
                            debug!("heartbeat response suppressed (duplicate)");
                        }
                        HeartbeatResponseAction::Deliver => {
                            info!(
                                len = response.len(),
                                "heartbeat produced output — delivering to operator"
                            );
                            if let Some(ref delivery) = deps.operator_delivery {
                                if let Err(e) = delivery.deliver(&response).await {
                                    error!(error = %e, "failed to deliver heartbeat to operator");
                                }
                            } else {
                                debug!(
                                    "heartbeat output ready but no operator delivery configured"
                                );
                            }
                        }
                    },
                    Err(e) => {
                        error!(error = %e, "heartbeat execution failed");
                    }
                }
            }
        }

        // --- Pairing request pruning ---
        if let Err(error) = deps.device_registry.prune_expired_requests().await {
            error!(error = %error, "failed to prune expired pairing requests");
        }

        // --- Scheduled jobs (durable claim prevents duplicate execution) ---
        let due_jobs = deps.scheduler.claim_due_jobs(CLAIM_LEASE_SECS).await;
        for job in due_jobs {
            let deps = deps.clone();
            tokio::spawn(async move {
                let job_id = job.id;
                debug!(job_id = %job_id, name = ?job.name, "executing claimed job");
                let _ = run_job_lifecycle(&deps, &job, true, SchedulerRunTrigger::Due).await;
                deps.scheduler.release_claim(&job_id).await;
            });
        }

        // --- Reminders (durable claim prevents duplicate delivery) ---
        let due_reminders = deps.reminder_store.claim_due(CLAIM_LEASE_SECS).await;
        for reminder in due_reminders {
            info!(reminder_id = %reminder.id, target = %reminder.target, "delivering reminder");
            let attempt = deps
                .reminder_store
                .start_delivery_attempt(&reminder.id)
                .await;

            // Deliver reminder via the session determined by its target field.
            match run_reminder(&deps, &reminder.message, &reminder.target).await {
                Ok(output) => {
                    if deps
                        .reminder_store
                        .mark_delivered(&reminder.id, &attempt, Some(output))
                        .await
                        .is_some()
                    {
                        info!(reminder_id = %reminder.id, "reminder delivered");
                    } else {
                        warn!(reminder_id = %reminder.id, "reminder delivered but outcome persistence failed");
                    }
                }
                Err(e) => {
                    let error_text = e.to_string();
                    if deps
                        .reminder_store
                        .mark_missed(&reminder.id, &attempt, error_text.clone())
                        .await
                        .is_some()
                    {
                        error!(reminder_id = %reminder.id, error = %error_text, "reminder delivery missed");
                    } else {
                        error!(reminder_id = %reminder.id, error = %error_text, "reminder delivery failed and outcome persistence failed");
                    }
                }
            }
            deps.reminder_store.release_claim(&reminder.id).await;
        }

        // --- Stale session cleanup (every ~100s / 10 ticks) ---
        tick_count += 1;
        if tick_count % 10 == 0 {
            match deps
                .session_engine
                .session_repo()
                .mark_stale_completed(3600)
                .await
            {
                Ok(0) => {}
                Ok(n) => info!(count = n, "cleaned up stale running sessions"),
                Err(e) => warn!(error = %e, "stale session cleanup failed"),
            }

            // Also clean up dangling turns (interrupted by crash/restart)
            match deps.turn_executor.turn_repo().mark_stale_failed(3600).await {
                Ok(0) => {}
                Ok(n) => info!(count = n, "cleaned up stale dangling turns"),
                Err(e) => warn!(error = %e, "stale turn cleanup failed"),
            }
        }

        // --- Plugin re-scan ---
        if let Some(ref scanner) = deps.plugin_scanner {
            if deps.plugin_scan_interval_ticks > 0
                && tick_count % deps.plugin_scan_interval_ticks == 0
            {
                let summary = scanner.scan().await;
                if summary.claude_plugins > 0 || summary.native_plugins > 0 {
                    debug!(
                        native = summary.native_plugins,
                        claude = summary.claude_plugins,
                        "plugin re-scan complete"
                    );
                }
            }
        }

        // --- Inter-agent comms inbox check ---
        check_comms_inbox(&deps).await;
    }
}

/// Create/reuse a heartbeat session and execute the heartbeat prompt.
/// Returns the assistant's response text.
async fn run_heartbeat(
    deps: &SupervisorDeps,
    prompt: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let session = get_or_create_heartbeat_session(deps).await?;

    deps.session_engine.mark_running(session.id).await?;

    let (_turn, _usage) = deps
        .turn_executor
        .execute_triggered(session.id, prompt, None, TriggerKind::Heartbeat, None)
        .await?;

    // Read the last assistant message from transcript
    let items = deps
        .turn_executor
        .transcript_repo()
        .list_by_session(session.id)
        .await?;
    let response = items
        .iter()
        .rev()
        .find_map(|item| {
            let payload = &item.payload;
            if item.kind == "assistant_message" {
                payload
                    .get("content")
                    .and_then(|c| c.as_str())
                    .map(String::from)
            } else {
                None
            }
        })
        .unwrap_or_default();

    let _ = deps.session_engine.mark_completed(session.id).await;

    Ok(response)
}

/// Execute a system event in the stable main scheduled session.
async fn run_system_event(
    deps: &SupervisorDeps,
    text: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let session = get_or_create_main_scheduled_session(deps).await?;
    execute_in_session(deps, &session, text, None, false, TriggerKind::CronJob).await
}

/// Execute an agent turn in an isolated or main session.
async fn run_agent_turn(
    deps: &SupervisorDeps,
    message: &str,
    model: Option<&str>,
    target: SessionTarget,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    match target {
        SessionTarget::Main => {
            let session = get_or_create_main_scheduled_session(deps).await?;
            execute_in_session(deps, &session, message, model, false, TriggerKind::CronJob).await
        }
        SessionTarget::Isolated => {
            let parent = get_or_create_main_scheduled_session(deps).await?;
            let session = deps
                .session_engine
                .create_session_full(
                    SessionKind::Subagent,
                    deps.workspace_root.clone(),
                    Some(parent.id),
                    None,
                    None,
                    None,
                )
                .await?;
            execute_in_session(deps, &session, message, model, true, TriggerKind::CronJob).await
        }
    }
}

/// Execute a reminder by running its message in the session determined by `target`.
///
/// Target routing:
///   - `"main"` (default) → stable main scheduled session
///   - `"isolated"` → one-shot subagent session under the main scheduled session
///   - anything else → treated as `"main"` with a warning
async fn run_reminder(
    deps: &SupervisorDeps,
    message: &str,
    target: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    match target {
        "isolated" => {
            let parent = get_or_create_main_scheduled_session(deps).await?;
            let session = deps
                .session_engine
                .create_session_full(
                    SessionKind::Subagent,
                    deps.workspace_root.clone(),
                    Some(parent.id),
                    None,
                    None,
                    None,
                )
                .await?;
            execute_in_session(deps, &session, message, None, true, TriggerKind::Reminder).await
        }
        other => {
            if other != "main" {
                warn!(target = %other, "unrecognized reminder target; falling back to main");
            }
            let session = get_or_create_main_scheduled_session(deps).await?;
            execute_in_session(deps, &session, message, None, false, TriggerKind::Reminder).await
        }
    }
}

async fn get_or_create_heartbeat_session(
    deps: &SupervisorDeps,
) -> Result<SessionRow, Box<dyn std::error::Error + Send + Sync>> {
    const HEARTBEAT_CHANNEL_REF: &str = "system:heartbeat";

    if let Some(session) = deps
        .session_engine
        .get_session_by_channel_ref(HEARTBEAT_CHANNEL_REF)
        .await?
    {
        return Ok(session);
    }

    let session = deps
        .session_engine
        .create_session_full(
            SessionKind::Scheduled,
            deps.workspace_root.clone(),
            None,
            Some(HEARTBEAT_CHANNEL_REF.to_string()),
            None,
            None,
        )
        .await?;

    Ok(session)
}

async fn get_or_create_main_scheduled_session(
    deps: &SupervisorDeps,
) -> Result<SessionRow, Box<dyn std::error::Error + Send + Sync>> {
    const MAIN_SCHEDULED_CHANNEL_REF: &str = "system:scheduled-main";

    if let Some(session) = deps
        .session_engine
        .get_session_by_channel_ref(MAIN_SCHEDULED_CHANNEL_REF)
        .await?
    {
        return Ok(session);
    }

    let session = deps
        .session_engine
        .create_session_full(
            SessionKind::Scheduled,
            deps.workspace_root.clone(),
            None,
            Some(MAIN_SCHEDULED_CHANNEL_REF.to_string()),
            None,
            None,
        )
        .await?;

    Ok(session)
}

async fn execute_in_session(
    deps: &SupervisorDeps,
    session: &SessionRow,
    message: &str,
    model: Option<&str>,
    complete_when_done: bool,
    trigger_kind: TriggerKind,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    if session.status == "created" {
        return Err("scheduled session unexpectedly remained in created state".into());
    }
    if matches!(session.status.as_str(), "ready") {
        deps.session_engine.mark_running(session.id).await?;
    }

    let (_turn, _usage) = deps
        .turn_executor
        .execute_triggered(session.id, message, model, trigger_kind, None)
        .await?;

    let items = deps
        .turn_executor
        .transcript_repo()
        .list_by_session(session.id)
        .await?;
    let response = items
        .iter()
        .rev()
        .find_map(|item| {
            if item.kind == "assistant_message" {
                item.payload
                    .get("content")
                    .and_then(|c| c.as_str())
                    .map(String::from)
            } else {
                None
            }
        })
        .unwrap_or_default();

    if complete_when_done {
        let _ = deps.session_engine.mark_completed(session.id).await;
    }

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune_core::JobId;
    use rune_runtime::scheduler::{Job, JobPayload, JobRunStatus, Schedule, SessionTarget};

    #[tokio::test]
    async fn supervisor_starts_and_stops_cleanly() {
        let mut supervisor = BackgroundSupervisor::new();
        assert!(supervisor.handle.is_none());
        assert!(supervisor.shutdown_tx.is_none());

        supervisor.shutdown();
        assert!(supervisor.handle.is_none());
    }

    #[tokio::test]
    async fn supervisor_default_trait() {
        let supervisor = BackgroundSupervisor::default();
        assert!(supervisor.handle.is_none());
    }

    fn make_test_job(delivery_mode: SchedulerDeliveryMode, webhook_url: Option<String>) -> Job {
        Job {
            id: JobId::new(),
            name: Some("test-job".into()),
            schedule: Schedule::Every {
                every_ms: 60_000,
                anchor_ms: None,
            },
            payload: JobPayload::SystemEvent {
                text: "test event".into(),
            },
            delivery_mode,
            webhook_url,
            session_target: SessionTarget::Main,
            enabled: true,
            created_at: chrono::Utc::now(),
            last_run_at: None,
            next_run_at: None,
            run_count: 0,
        }
    }

    #[tokio::test]
    async fn deliver_announce_broadcasts_event() {
        let (event_tx, mut rx) = broadcast::channel(16);
        let job = make_test_job(SchedulerDeliveryMode::Announce, None);

        deliver_result_standalone(
            &event_tx,
            None,
            &job,
            &JobRunStatus::Completed,
            "hello world",
            SchedulerRunTrigger::Due,
        )
        .await;

        let event = rx
            .try_recv()
            .expect("announce mode should broadcast an event");
        assert_eq!(event.kind, "cron_run_completed");
        assert_eq!(event.payload["delivery_mode"], "announce");
        assert_eq!(event.payload["output"], "hello world");
        assert!(event.state_changed);
    }

    #[tokio::test]
    async fn deliver_none_does_not_broadcast() {
        let (event_tx, mut rx) = broadcast::channel(16);
        let job = make_test_job(SchedulerDeliveryMode::None, None);

        deliver_result_standalone(
            &event_tx,
            None,
            &job,
            &JobRunStatus::Completed,
            "silent output",
            SchedulerRunTrigger::Due,
        )
        .await;

        assert!(
            rx.try_recv().is_err(),
            "none mode should NOT broadcast any event"
        );
    }

    #[tokio::test]
    async fn deliver_webhook_without_url_does_not_panic() {
        let (event_tx, mut rx) = broadcast::channel(16);
        let job = make_test_job(SchedulerDeliveryMode::Webhook, None);

        deliver_result_standalone(
            &event_tx,
            None,
            &job,
            &JobRunStatus::Completed,
            "output",
            SchedulerRunTrigger::Due,
        )
        .await;

        assert!(
            rx.try_recv().is_err(),
            "webhook mode without URL should not broadcast"
        );
    }
}
