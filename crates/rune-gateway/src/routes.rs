//! HTTP route handlers for the gateway API.

use std::sync::Arc;
use std::time::Instant;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::info;
use uuid::Uuid;

use rune_core::{JobId, SessionKind};
use rune_runtime::scheduler::{
    Job, JobPayload, JobRun, JobRunStatus, JobUpdate, Schedule, SessionTarget,
};

use crate::error::GatewayError;
use crate::state::{AppState, SessionEvent};

// ── Health & Status ───────────────────────────────────────────────────────────

/// Response for `GET /health`.
#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub service: &'static str,
    pub version: &'static str,
    pub uptime_seconds: u64,
    pub session_count: usize,
    pub ws_subscribers: usize,
}

/// Health check with runtime counters.
pub async fn health(State(state): State<AppState>) -> Result<Json<HealthResponse>, GatewayError> {
    let sessions = state
        .session_repo
        .list(i64::MAX / 4, 0)
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;

    Ok(Json(HealthResponse {
        status: "ok",
        service: "rune-gateway",
        version: env!("CARGO_PKG_VERSION"),
        uptime_seconds: state.started_at.elapsed().as_secs(),
        session_count: sessions.len(),
        ws_subscribers: state.event_tx.receiver_count(),
    }))
}

/// Response for `GET /status`.
#[derive(Serialize)]
pub struct StatusResponse {
    pub status: &'static str,
    pub version: &'static str,
    pub bind: String,
    pub auth_enabled: bool,
    pub configured_model_providers: usize,
    pub active_model_backend: &'static str,
    pub registered_tools: usize,
    pub session_count: usize,
    pub cron_job_count: usize,
    pub ws_subscribers: usize,
    pub uptime_seconds: u64,
    pub config_paths: StatusPaths,
}

#[derive(Serialize)]
pub struct StatusPaths {
    pub sessions_dir: String,
    pub memory_dir: String,
    pub logs_dir: String,
}

/// Daemon status with useful runtime metadata.
pub async fn status(State(state): State<AppState>) -> Result<Json<StatusResponse>, GatewayError> {
    let sessions = state
        .session_repo
        .list(i64::MAX / 4, 0)
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;
    let cron_job_count = state.scheduler.list_jobs(true).await.len();

    Ok(Json(StatusResponse {
        status: "running",
        version: env!("CARGO_PKG_VERSION"),
        bind: format!(
            "{}:{}",
            state.config.gateway.host, state.config.gateway.port
        ),
        auth_enabled: state.config.gateway.auth_token.is_some(),
        configured_model_providers: state.config.models.providers.len(),
        active_model_backend: "in-memory-demo",
        registered_tools: state.tool_count,
        session_count: sessions.len(),
        cron_job_count,
        ws_subscribers: state.event_tx.receiver_count(),
        uptime_seconds: state.started_at.elapsed().as_secs(),
        config_paths: StatusPaths {
            sessions_dir: state.config.paths.sessions_dir.display().to_string(),
            memory_dir: state.config.paths.memory_dir.display().to_string(),
            logs_dir: state.config.paths.logs_dir.display().to_string(),
        },
    }))
}

/// Response for control actions that are acknowledged but not yet fully wired.
#[derive(Serialize)]
pub struct ActionResponse {
    pub success: bool,
    pub message: String,
}

/// `POST /gateway/start` — parity placeholder for CLI/gateway contract alignment.
pub async fn gateway_start() -> Json<ActionResponse> {
    Json(ActionResponse {
        success: true,
        message: "gateway already running in foreground mode".to_string(),
    })
}

/// `POST /gateway/stop` — parity placeholder for CLI/gateway contract alignment.
pub async fn gateway_stop() -> Json<ActionResponse> {
    Json(ActionResponse {
        success: true,
        message: "gateway stop acknowledged; external service supervision pending".to_string(),
    })
}

/// `POST /gateway/restart` — parity placeholder for CLI/gateway contract alignment.
pub async fn gateway_restart() -> Json<ActionResponse> {
    Json(ActionResponse {
        success: true,
        message: "gateway restart acknowledged; external service supervision pending".to_string(),
    })
}

// ── Cron / Scheduler ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CronListQuery {
    #[serde(rename = "includeDisabled")]
    pub include_disabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct CronJobRequest {
    pub name: Option<String>,
    pub schedule: CronScheduleRequest,
    pub payload: CronPayloadRequest,
    #[serde(rename = "sessionTarget")]
    pub session_target: String,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CronScheduleRequest {
    At {
        at: DateTime<Utc>,
    },
    Every {
        every_ms: u64,
        anchor_ms: Option<u64>,
    },
    Cron {
        expr: String,
        tz: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CronPayloadRequest {
    SystemEvent {
        text: String,
    },
    AgentTurn {
        message: String,
        model: Option<String>,
        timeout_seconds: Option<u64>,
    },
}

#[derive(Debug, Deserialize)]
pub struct CronUpdateRequest {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub schedule: Option<CronScheduleRequest>,
    pub payload: Option<CronPayloadRequest>,
}

#[derive(Serialize)]
pub struct CronStatusResponse {
    pub total_jobs: usize,
    pub enabled_jobs: usize,
    pub due_jobs: usize,
}

#[derive(Serialize)]
pub struct CronJobResponse {
    pub id: String,
    pub name: Option<String>,
    pub schedule: Schedule,
    pub payload: JobPayload,
    pub session_target: SessionTarget,
    pub enabled: bool,
    pub created_at: String,
    pub last_run_at: Option<String>,
    pub next_run_at: Option<String>,
    pub run_count: u64,
}

#[derive(Serialize)]
pub struct CronRunResponse {
    pub job_id: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub status: JobRunStatus,
    pub output: Option<String>,
}

#[derive(Serialize)]
pub struct CronMutationResponse {
    pub success: bool,
    pub job_id: String,
    pub message: String,
}

pub async fn cron_status(
    State(state): State<AppState>,
) -> Result<Json<CronStatusResponse>, GatewayError> {
    let jobs = state.scheduler.list_jobs(true).await;
    let due_jobs = state.scheduler.get_due_jobs().await;
    Ok(Json(CronStatusResponse {
        total_jobs: jobs.len(),
        enabled_jobs: jobs.iter().filter(|job| job.enabled).count(),
        due_jobs: due_jobs.len(),
    }))
}

pub async fn cron_list(
    State(state): State<AppState>,
    Query(query): Query<CronListQuery>,
) -> Result<Json<Vec<CronJobResponse>>, GatewayError> {
    let include_disabled = query.include_disabled.unwrap_or(false);
    let jobs = state.scheduler.list_jobs(include_disabled).await;
    Ok(Json(jobs.into_iter().map(job_to_response).collect()))
}

pub async fn cron_add(
    State(state): State<AppState>,
    Json(body): Json<CronJobRequest>,
) -> Result<(StatusCode, Json<CronMutationResponse>), GatewayError> {
    let schedule = convert_schedule(body.schedule);
    let payload = convert_payload(body.payload);
    let session_target = parse_session_target(&body.session_target)?;
    validate_job_contract(&session_target, &payload)?;

    let now = Utc::now();
    let id = JobId::new();
    let next_run_at = initial_next_run(&schedule);
    let mut job = Job {
        id,
        name: body.name,
        schedule,
        payload,
        session_target,
        enabled: body.enabled.unwrap_or(true),
        created_at: now,
        last_run_at: None,
        next_run_at,
        run_count: 0,
    };

    if matches!(job.schedule, Schedule::At { .. }) && !job.enabled {
        job.next_run_at = None;
    }

    let job_id = state.scheduler.add_job(job).await;

    Ok((
        StatusCode::CREATED,
        Json(CronMutationResponse {
            success: true,
            job_id: job_id.to_string(),
            message: "cron job created".to_string(),
        }),
    ))
}

pub async fn cron_update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<CronUpdateRequest>,
) -> Result<Json<CronMutationResponse>, GatewayError> {
    let job_id = JobId::from(id);
    let existing = state
        .scheduler
        .get_job(&job_id)
        .await
        .ok_or_else(|| GatewayError::JobNotFound(job_id.to_string()))?;

    let new_payload = body.payload.map(convert_payload);
    let new_schedule = body.schedule.map(convert_schedule);
    let effective_payload = new_payload
        .clone()
        .unwrap_or_else(|| existing.payload.clone());
    validate_job_contract(&existing.session_target, &effective_payload)?;

    let update = JobUpdate {
        name: body.name,
        enabled: body.enabled,
        schedule: new_schedule,
        payload: new_payload,
    };

    state
        .scheduler
        .update_job(&job_id, update)
        .await
        .ok_or_else(|| GatewayError::JobNotFound(job_id.to_string()))?;

    if let Some(updated) = state.scheduler.get_job(&job_id).await {
        if updated.next_run_at.is_none() && updated.enabled {
            state.scheduler.advance_next_run(&job_id).await;
        }
    }

    Ok(Json(CronMutationResponse {
        success: true,
        job_id: job_id.to_string(),
        message: "cron job updated".to_string(),
    }))
}

pub async fn cron_remove(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<CronMutationResponse>, GatewayError> {
    let job_id = JobId::from(id);
    state
        .scheduler
        .remove_job(&job_id)
        .await
        .ok_or_else(|| GatewayError::JobNotFound(job_id.to_string()))?;

    Ok(Json(CronMutationResponse {
        success: true,
        job_id: job_id.to_string(),
        message: "cron job removed".to_string(),
    }))
}

pub async fn cron_run(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<CronMutationResponse>, GatewayError> {
    let job_id = JobId::from(id);
    let job = state
        .scheduler
        .get_job(&job_id)
        .await
        .ok_or_else(|| GatewayError::JobNotFound(job_id.to_string()))?;

    let run = state.scheduler.start_run(job_id).await;
    let output = simulated_job_output(&job);
    state
        .scheduler
        .complete_run(job_id, JobRunStatus::Completed, Some(output.clone()))
        .await;
    state.scheduler.advance_next_run(&job_id).await;

    let _ = state.event_tx.send(SessionEvent {
        session_id: job_id.to_string(),
        kind: "cron_run_completed".to_string(),
        payload: json!({
            "job_id": job_id.to_string(),
            "started_at": run.started_at,
            "status": "completed",
            "output": output,
        }),
    });

    Ok(Json(CronMutationResponse {
        success: true,
        job_id: job_id.to_string(),
        message: "cron job triggered".to_string(),
    }))
}

pub async fn cron_runs(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<CronRunResponse>>, GatewayError> {
    let job_id = JobId::from(id);
    state
        .scheduler
        .get_job(&job_id)
        .await
        .ok_or_else(|| GatewayError::JobNotFound(job_id.to_string()))?;
    let runs = state.scheduler.get_runs(&job_id, None).await;
    Ok(Json(runs.into_iter().map(run_to_response).collect()))
}

// ── Sessions ──────────────────────────────────────────────────────────────────

/// Request body for `POST /sessions`.
#[derive(Deserialize)]
pub struct CreateSessionRequest {
    /// Session kind (defaults to `Direct`).
    #[serde(default = "default_kind")]
    pub kind: String,
    /// Optional workspace root.
    pub workspace_root: Option<String>,
}

fn default_kind() -> String {
    "direct".to_string()
}

/// Response for session creation / retrieval.
#[derive(Serialize)]
pub struct SessionResponse {
    pub id: Uuid,
    pub kind: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Lightweight session summary for list output.
#[derive(Serialize)]
pub struct SessionListItem {
    pub id: String,
    pub status: String,
    pub channel: Option<String>,
    pub created_at: String,
}

/// `GET /sessions` — list sessions.
pub async fn list_sessions(
    State(state): State<AppState>,
) -> Result<Json<Vec<SessionListItem>>, GatewayError> {
    let rows = state
        .session_repo
        .list(100, 0)
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;

    let items = rows
        .into_iter()
        .map(|row| SessionListItem {
            id: row.id.to_string(),
            status: row.status,
            channel: row.channel_ref,
            created_at: row.created_at.to_rfc3339(),
        })
        .collect();

    Ok(Json(items))
}

/// `POST /sessions` — create a new session.
pub async fn create_session(
    State(state): State<AppState>,
    Json(body): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<SessionResponse>), GatewayError> {
    let kind = parse_session_kind(&body.kind)?;

    let row = state
        .session_engine
        .create_session(kind, body.workspace_root)
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;

    let _ = state.event_tx.send(SessionEvent {
        session_id: row.id.to_string(),
        kind: "session_created".to_string(),
        payload: json!({
            "session_id": row.id,
            "kind": row.kind,
            "status": row.status,
        }),
    });

    info!(session_id = %row.id, "session created");

    Ok((
        StatusCode::CREATED,
        Json(SessionResponse {
            id: row.id,
            kind: row.kind,
            status: row.status,
            created_at: row.created_at.to_rfc3339(),
            updated_at: row.updated_at.to_rfc3339(),
        }),
    ))
}

/// `GET /sessions/{id}` — get session by ID.
pub async fn get_session(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<SessionResponse>, GatewayError> {
    let row = state
        .session_engine
        .get_session(id)
        .await
        .map_err(|e| GatewayError::SessionNotFound(e.to_string()))?;

    Ok(Json(SessionResponse {
        id: row.id,
        kind: row.kind,
        status: row.status,
        created_at: row.created_at.to_rfc3339(),
        updated_at: row.updated_at.to_rfc3339(),
    }))
}

// ── Messages ──────────────────────────────────────────────────────────────────

/// Request body for `POST /sessions/{id}/messages`.
#[derive(Deserialize)]
pub struct SendMessageRequest {
    pub content: String,
    pub model: Option<String>,
}

/// Response after processing a message.
#[derive(Serialize)]
pub struct MessageResponse {
    pub turn_id: Uuid,
    pub assistant_reply: Option<String>,
    pub usage: UsageInfo,
    pub latency_ms: u128,
}

/// Token usage summary.
#[derive(Serialize)]
pub struct UsageInfo {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

/// `POST /sessions/{id}/messages` — send a user message and get the assistant response.
pub async fn send_message(
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
    Json(body): Json<SendMessageRequest>,
) -> Result<Json<MessageResponse>, GatewayError> {
    let started = Instant::now();

    state
        .session_engine
        .get_session(session_id)
        .await
        .map_err(|e| GatewayError::SessionNotFound(e.to_string()))?;

    let (turn_row, usage) = state
        .turn_executor
        .execute(session_id, &body.content, body.model.as_deref())
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;

    let transcript = state
        .transcript_repo
        .list_by_session(session_id)
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;

    let assistant_reply = transcript
        .iter()
        .rev()
        .find(|t| t.turn_id == Some(turn_row.id) && t.kind == "assistant_message")
        .and_then(|t| {
            t.payload
                .get("content")
                .and_then(|v| v.as_str())
                .map(String::from)
        });

    let _ = state.event_tx.send(SessionEvent {
        session_id: session_id.to_string(),
        kind: "turn_completed".to_string(),
        payload: json!({
            "session_id": session_id,
            "turn_id": turn_row.id,
            "assistant_reply": assistant_reply.clone(),
            "prompt_tokens": usage.prompt_tokens,
            "completion_tokens": usage.completion_tokens,
        }),
    });

    info!(session_id = %session_id, turn_id = %turn_row.id, "message processed");

    Ok(Json(MessageResponse {
        turn_id: turn_row.id,
        assistant_reply,
        usage: UsageInfo {
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
        },
        latency_ms: started.elapsed().as_millis(),
    }))
}

// ── Transcript ────────────────────────────────────────────────────────────────

/// A single transcript entry in the response.
#[derive(Serialize)]
pub struct TranscriptEntry {
    pub id: Uuid,
    pub turn_id: Option<Uuid>,
    pub seq: i32,
    pub kind: String,
    pub payload: serde_json::Value,
    pub created_at: String,
}

/// `GET /sessions/{id}/transcript` — full session transcript.
pub async fn get_transcript(
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
) -> Result<Json<Vec<TranscriptEntry>>, GatewayError> {
    state
        .session_engine
        .get_session(session_id)
        .await
        .map_err(|e| GatewayError::SessionNotFound(e.to_string()))?;

    let items = state
        .transcript_repo
        .list_by_session(session_id)
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;

    let entries: Vec<TranscriptEntry> = items
        .into_iter()
        .map(|item| TranscriptEntry {
            id: item.id,
            turn_id: item.turn_id,
            seq: item.seq,
            kind: item.kind,
            payload: item.payload,
            created_at: item.created_at.to_rfc3339(),
        })
        .collect();

    Ok(Json(entries))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

#[allow(dead_code)]
fn _started_at_for_tests() -> Arc<Instant> {
    Arc::new(Instant::now())
}

fn parse_session_kind(s: &str) -> Result<SessionKind, GatewayError> {
    match s.to_lowercase().as_str() {
        "direct" => Ok(SessionKind::Direct),
        "channel" => Ok(SessionKind::Channel),
        "scheduled" => Ok(SessionKind::Scheduled),
        "subagent" => Ok(SessionKind::Subagent),
        other => Err(GatewayError::BadRequest(format!(
            "unknown session kind: {other}"
        ))),
    }
}

fn parse_session_target(s: &str) -> Result<SessionTarget, GatewayError> {
    match s.to_lowercase().as_str() {
        "main" => Ok(SessionTarget::Main),
        "isolated" => Ok(SessionTarget::Isolated),
        other => Err(GatewayError::BadRequest(format!(
            "unknown session target: {other}"
        ))),
    }
}

fn convert_schedule(request: CronScheduleRequest) -> Schedule {
    match request {
        CronScheduleRequest::At { at } => Schedule::At { at },
        CronScheduleRequest::Every {
            every_ms,
            anchor_ms,
        } => Schedule::Every {
            every_ms,
            anchor_ms,
        },
        CronScheduleRequest::Cron { expr, tz } => Schedule::Cron { expr, tz },
    }
}

fn convert_payload(request: CronPayloadRequest) -> JobPayload {
    match request {
        CronPayloadRequest::SystemEvent { text } => JobPayload::SystemEvent { text },
        CronPayloadRequest::AgentTurn {
            message,
            model,
            timeout_seconds,
        } => JobPayload::AgentTurn {
            message,
            model,
            timeout_seconds,
        },
    }
}

fn validate_job_contract(
    session_target: &SessionTarget,
    payload: &JobPayload,
) -> Result<(), GatewayError> {
    match (session_target, payload) {
        (SessionTarget::Main, JobPayload::SystemEvent { .. }) => Ok(()),
        (SessionTarget::Isolated, JobPayload::AgentTurn { .. }) => Ok(()),
        (SessionTarget::Main, _) => Err(GatewayError::BadRequest(
            "sessionTarget=main requires payload.kind=system_event".to_string(),
        )),
        (SessionTarget::Isolated, _) => Err(GatewayError::BadRequest(
            "sessionTarget=isolated requires payload.kind=agent_turn".to_string(),
        )),
    }
}

fn initial_next_run(schedule: &Schedule) -> Option<DateTime<Utc>> {
    match schedule {
        Schedule::At { at } => Some(*at),
        Schedule::Every { every_ms, .. } => {
            Some(Utc::now() + chrono::Duration::milliseconds(*every_ms as i64))
        }
        Schedule::Cron { .. } => Some(Utc::now() + chrono::Duration::hours(1)),
    }
}

fn simulated_job_output(job: &Job) -> String {
    match &job.payload {
        JobPayload::SystemEvent { text } => format!("system event delivered: {text}"),
        JobPayload::AgentTurn { message, .. } => format!("isolated agent turn queued: {message}"),
    }
}

fn job_to_response(job: Job) -> CronJobResponse {
    CronJobResponse {
        id: job.id.to_string(),
        name: job.name,
        schedule: job.schedule,
        payload: job.payload,
        session_target: job.session_target,
        enabled: job.enabled,
        created_at: job.created_at.to_rfc3339(),
        last_run_at: job.last_run_at.map(|dt| dt.to_rfc3339()),
        next_run_at: job.next_run_at.map(|dt| dt.to_rfc3339()),
        run_count: job.run_count,
    }
}

fn run_to_response(run: JobRun) -> CronRunResponse {
    CronRunResponse {
        job_id: run.job_id.to_string(),
        started_at: run.started_at.to_rfc3339(),
        finished_at: run.finished_at.map(|dt| dt.to_rfc3339()),
        status: run.status,
        output: run.output,
    }
}

// ── Telegram Webhook ────────────────────────────────────────────────

/// `POST /webhook/telegram/{token}` — receive Telegram Bot API updates.
///
/// The token in the URL is validated against the configured bot token
/// to prevent unauthorized webhook calls.
pub async fn telegram_webhook(
    State(state): State<AppState>,
    Path(token): Path<String>,
    Json(update): Json<serde_json::Value>,
) -> Result<StatusCode, GatewayError> {
    // Validate the webhook token matches the configured bot token
    let expected_token = state
        .config
        .channels
        .telegram_token
        .as_deref()
        .unwrap_or_default();

    if token != expected_token {
        return Err(GatewayError::Unauthorized);
    }

    // Emit the update as a session event for processing
    let _ = state.event_tx.send(crate::state::SessionEvent {
        session_id: "telegram".to_string(),
        kind: "telegram_update".to_string(),
        payload: update,
    });

    Ok(StatusCode::OK)
}
