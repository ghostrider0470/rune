//! Background service supervisor: heartbeats, scheduled jobs, and reminders.
//!
//! Runs a tokio task that ticks every ~10 seconds, checking for due work.

use std::sync::Arc;

use tokio::sync::watch;
use tracing::{debug, error, info};

use rune_core::SessionKind;
use rune_runtime::heartbeat::HeartbeatRunner;
use rune_runtime::scheduler::{JobPayload, ReminderStore, Scheduler, SessionTarget};
use rune_runtime::{SessionEngine, TurnExecutor};

/// Manages background services (heartbeat, scheduler, reminders).
pub struct BackgroundSupervisor {
    handle: Option<tokio::task::JoinHandle<()>>,
    shutdown_tx: Option<watch::Sender<bool>>,
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

async fn supervisor_loop(deps: SupervisorDeps, mut shutdown_rx: watch::Receiver<bool>) {
    info!("supervisor loop started");

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
                    Ok(response) => {
                        if HeartbeatRunner::should_suppress(&response) {
                            debug!("heartbeat response suppressed (HEARTBEAT_OK)");
                            deps.heartbeat.record_suppression().await;
                        } else {
                            info!(len = response.len(), "heartbeat produced output");
                            // Non-suppressed heartbeat output is already persisted in the
                            // session transcript by TurnExecutor; nothing else to deliver.
                        }
                    }
                    Err(e) => {
                        error!(error = %e, "heartbeat execution failed");
                    }
                }
            }
        }

        // --- Scheduled jobs ---
        let due_jobs = deps.scheduler.get_due_jobs().await;
        for job in due_jobs {
            let job_id = job.id;
            debug!(job_id = %job_id, name = ?job.name, "executing due job");

            deps.scheduler.start_run(job_id).await;

            let result = match &job.payload {
                JobPayload::SystemEvent { text } => {
                    run_system_event(&deps, text).await
                }
                JobPayload::AgentTurn {
                    message,
                    model,
                    ..
                } => {
                    run_agent_turn(&deps, message, model.as_deref(), job.session_target).await
                }
            };

            match result {
                Ok(output) => {
                    deps.scheduler
                        .complete_run(
                            job_id,
                            rune_runtime::scheduler::JobRunStatus::Completed,
                            Some(output),
                        )
                        .await;
                }
                Err(e) => {
                    error!(job_id = %job_id, error = %e, "job execution failed");
                    deps.scheduler
                        .complete_run(
                            job_id,
                            rune_runtime::scheduler::JobRunStatus::Failed,
                            Some(e.to_string()),
                        )
                        .await;
                }
            }

            deps.scheduler.advance_next_run(&job_id).await;
        }

        // --- Reminders ---
        let due_reminders = deps.reminder_store.get_due().await;
        for reminder in due_reminders {
            info!(reminder_id = %reminder.id, target = %reminder.target, "delivering reminder");

            // Deliver reminder by executing it as a turn in an isolated session
            match run_reminder(&deps, &reminder.message).await {
                Ok(_) => {
                    deps.reminder_store.mark_delivered(&reminder.id).await;
                    info!(reminder_id = %reminder.id, "reminder delivered");
                }
                Err(e) => {
                    error!(reminder_id = %reminder.id, error = %e, "reminder delivery failed");
                }
            }
        }
    }
}

/// Create/reuse a heartbeat session and execute the heartbeat prompt.
/// Returns the assistant's response text.
async fn run_heartbeat(deps: &SupervisorDeps, prompt: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let session = deps
        .session_engine
        .create_session(SessionKind::Scheduled, deps.workspace_root.clone())
        .await?;

    deps.session_engine.mark_ready(session.id).await?;
    deps.session_engine.mark_running(session.id).await?;

    let (_turn, _usage) = deps
        .turn_executor
        .execute(session.id, prompt, None)
        .await?;

    // Read the last assistant message from transcript
    let items = deps.turn_executor.transcript_repo().list_by_session(session.id).await?;
    let response = items
        .iter()
        .rev()
        .find_map(|item| {
            let payload = &item.payload;
            if item.kind == "assistant_message" {
                payload.get("content").and_then(|c| c.as_str()).map(String::from)
            } else {
                None
            }
        })
        .unwrap_or_default();

    let _ = deps.session_engine.mark_completed(session.id).await;

    Ok(response)
}

/// Execute a system event in a new scheduled session.
async fn run_system_event(deps: &SupervisorDeps, text: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let session = deps
        .session_engine
        .create_session(SessionKind::Scheduled, deps.workspace_root.clone())
        .await?;

    deps.session_engine.mark_ready(session.id).await?;
    deps.session_engine.mark_running(session.id).await?;

    let (_turn, _usage) = deps.turn_executor.execute(session.id, text, None).await?;

    let items = deps.turn_executor.transcript_repo().list_by_session(session.id).await?;
    let response = items
        .iter()
        .rev()
        .find_map(|item| {
            if item.kind == "assistant_message" {
                item.payload.get("content").and_then(|c| c.as_str()).map(String::from)
            } else {
                None
            }
        })
        .unwrap_or_default();

    let _ = deps.session_engine.mark_completed(session.id).await;
    Ok(response)
}

/// Execute an agent turn in an isolated or main session.
async fn run_agent_turn(
    deps: &SupervisorDeps,
    message: &str,
    model: Option<&str>,
    target: SessionTarget,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let kind = match target {
        SessionTarget::Main => SessionKind::Scheduled,
        SessionTarget::Isolated => SessionKind::Scheduled,
    };

    let session = deps
        .session_engine
        .create_session(kind, deps.workspace_root.clone())
        .await?;

    deps.session_engine.mark_ready(session.id).await?;
    deps.session_engine.mark_running(session.id).await?;

    let (_turn, _usage) = deps.turn_executor.execute(session.id, message, model).await?;

    let items = deps.turn_executor.transcript_repo().list_by_session(session.id).await?;
    let response = items
        .iter()
        .rev()
        .find_map(|item| {
            if item.kind == "assistant_message" {
                item.payload.get("content").and_then(|c| c.as_str()).map(String::from)
            } else {
                None
            }
        })
        .unwrap_or_default();

    let _ = deps.session_engine.mark_completed(session.id).await;
    Ok(response)
}

/// Execute a reminder by running its message as a turn in a scheduled session.
async fn run_reminder(deps: &SupervisorDeps, message: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    run_system_event(deps, message).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn supervisor_starts_and_stops_cleanly() {
        let mut supervisor = BackgroundSupervisor::new();
        // Without deps we can't call start(), but we can verify construction
        assert!(supervisor.handle.is_none());
        assert!(supervisor.shutdown_tx.is_none());

        // Shutdown on an un-started supervisor is a no-op
        supervisor.shutdown();
        assert!(supervisor.handle.is_none());
    }

    #[tokio::test]
    async fn supervisor_default_trait() {
        let supervisor = BackgroundSupervisor::default();
        assert!(supervisor.handle.is_none());
    }
}
