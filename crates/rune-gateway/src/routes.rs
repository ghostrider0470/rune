//! HTTP route handlers for the gateway API.

use std::sync::Arc;
use std::time::Instant;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::info;
use uuid::Uuid;

use rune_core::{JobId, SessionKind};
use rune_runtime::heartbeat::HeartbeatState;
use rune_runtime::scheduler::{
    Job, JobPayload, JobRun, JobRunStatus, JobUpdate, Reminder, Schedule, SessionTarget,
};
use rune_store::models::{SessionRow, TurnRow};
use serde_json::Value;

use crate::error::GatewayError;
use crate::state::{AppState, SessionEvent};
use crate::{SupervisorDeps, run_job_lifecycle};

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
        active_model_backend: if state.config.models.providers.is_empty() {
            "demo-echo"
        } else {
            "configured-provider"
        },
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

// ── Dashboard ────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct DashboardSummaryResponse {
    pub gateway_status: &'static str,
    pub bind: String,
    pub uptime_seconds: u64,
    pub default_model: Option<String>,
    pub provider_count: usize,
    pub configured_model_count: usize,
    pub session_count: usize,
    pub auth_enabled: bool,
    pub ws_subscribers: usize,
    pub channels: Vec<String>,
}

#[derive(Serialize)]
pub struct DashboardModelItem {
    pub provider_name: String,
    pub provider_kind: String,
    pub model_id: String,
    pub raw_model: String,
    pub is_default: bool,
}

#[derive(Serialize)]
pub struct DashboardSessionItem {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub channel_ref: Option<String>,
    pub routing_ref: Option<String>,
    pub created_at: String,
    pub last_activity_at: String,
}

#[derive(Serialize)]
pub struct DashboardDiagnosticItem {
    pub level: &'static str,
    pub source: &'static str,
    pub message: String,
    pub observed_at: String,
}

#[derive(Serialize)]
pub struct DashboardDiagnosticsResponse {
    pub structured_errors_available: bool,
    pub items: Vec<DashboardDiagnosticItem>,
}

pub async fn dashboard_page() -> Html<&'static str> {
    Html(DASHBOARD_HTML)
}

pub async fn branded_asset(Path(path): Path<String>) -> Result<Response, GatewayError> {
    let (content_type, bytes): (&'static str, &'static [u8]) = match path.as_str() {
        "hero.png" => ("image/png", include_bytes!("../../../assets/hero.png")),
        "rune-logo-favicon.svg" => (
            "image/svg+xml",
            include_bytes!("../../../assets/rune-logo-favicon.svg"),
        ),
        "rune-logo-icon.svg" => (
            "image/svg+xml",
            include_bytes!("../../../assets/rune-logo-icon.svg"),
        ),
        "rune-logo-wordmark-dark.svg" => (
            "image/svg+xml",
            include_bytes!("../../../assets/rune-logo-wordmark-dark.svg"),
        ),
        "rune-logo-wordmark-light.svg" => (
            "image/svg+xml",
            include_bytes!("../../../assets/rune-logo-wordmark-light.svg"),
        ),
        "rune-logo-wordmark.svg" => (
            "image/svg+xml",
            include_bytes!("../../../assets/rune-logo-wordmark.svg"),
        ),
        _ => {
            return Err(GatewayError::AssetNotFound(path));
        }
    };

    Ok(([(header::CONTENT_TYPE, content_type)], bytes).into_response())
}

pub async fn dashboard_summary(
    State(state): State<AppState>,
) -> Result<Json<DashboardSummaryResponse>, GatewayError> {
    let sessions = state
        .session_repo
        .list(i64::MAX / 4, 0)
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;

    Ok(Json(DashboardSummaryResponse {
        gateway_status: "running",
        bind: format!(
            "{}:{}",
            state.config.gateway.host, state.config.gateway.port
        ),
        uptime_seconds: state.started_at.elapsed().as_secs(),
        default_model: resolved_default_model(&state),
        provider_count: state.config.models.providers.len(),
        configured_model_count: state.config.models.inventory().len(),
        session_count: sessions.len(),
        auth_enabled: state.config.gateway.auth_token.is_some(),
        ws_subscribers: state.event_tx.receiver_count(),
        channels: configured_channels(&state),
    }))
}

pub async fn dashboard_models(
    State(state): State<AppState>,
) -> Result<Json<Vec<DashboardModelItem>>, GatewayError> {
    let default_model = resolved_default_model(&state);
    let mut items = state
        .config
        .models
        .inventory()
        .into_iter()
        .map(|entry| {
            let model_id = entry.model_id();
            let is_default = default_model.as_deref() == Some(model_id.as_str())
                || default_model.as_deref() == Some(entry.raw_model);
            DashboardModelItem {
                provider_name: entry.provider_name.to_string(),
                provider_kind: entry.provider_kind.to_string(),
                model_id,
                raw_model: entry.raw_model.to_string(),
                is_default,
            }
        })
        .collect::<Vec<_>>();

    items.sort_by(|a, b| a.model_id.cmp(&b.model_id));
    Ok(Json(items))
}

pub async fn dashboard_sessions(
    State(state): State<AppState>,
) -> Result<Json<Vec<DashboardSessionItem>>, GatewayError> {
    let rows = state
        .session_repo
        .list(50, 0)
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;

    Ok(Json(
        rows.into_iter().map(session_to_dashboard_item).collect(),
    ))
}

pub async fn dashboard_diagnostics(
    State(state): State<AppState>,
) -> Result<Json<DashboardDiagnosticsResponse>, GatewayError> {
    let mut items = Vec::new();
    let now = Utc::now().to_rfc3339();

    if state.config.models.providers.is_empty() {
        items.push(DashboardDiagnosticItem {
            level: "warn",
            source: "models",
            message: "No model providers configured; gateway is using the demo echo backend."
                .to_string(),
            observed_at: now.clone(),
        });
    }

    if configured_channels(&state).is_empty() {
        items.push(DashboardDiagnosticItem {
            level: "info",
            source: "channels",
            message: "No channel adapters are configured.".to_string(),
            observed_at: now.clone(),
        });
    }

    if items.is_empty() {
        items.push(DashboardDiagnosticItem {
            level: "info",
            source: "runtime",
            message:
                "No structured provider or channel errors are currently exposed by the runtime."
                    .to_string(),
            observed_at: now,
        });
    }

    Ok(Json(DashboardDiagnosticsResponse {
        structured_errors_available: false,
        items,
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

#[derive(Deserialize)]
pub struct SessionsListQuery {
    #[serde(rename = "active")]
    pub active_minutes: Option<u64>,
    pub channel: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct CronWakeRequest {
    pub text: String,
    pub mode: Option<String>,
    #[serde(rename = "contextMessages")]
    pub context_messages: Option<u64>,
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

#[derive(Serialize)]
pub struct CronWakeResponse {
    pub success: bool,
    pub mode: String,
    pub text: String,
    pub context_messages: Option<u64>,
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

    let deps = SupervisorDeps {
        heartbeat: state.heartbeat.clone(),
        scheduler: state.scheduler.clone(),
        reminder_store: state.reminder_store.clone(),
        session_engine: state.session_engine.clone(),
        turn_executor: state.turn_executor.clone(),
        workspace_root: state.config.agents.defaults.workspace.clone(),
    };

    let started_at = Utc::now();
    let (status, output) = run_job_lifecycle(&deps, &job, true).await;

    let _ = state.event_tx.send(SessionEvent {
        session_id: job_id.to_string(),
        kind: "cron_run_completed".to_string(),
        payload: json!({
            "job_id": job_id.to_string(),
            "started_at": started_at,
            "status": status,
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

pub async fn cron_wake(
    State(state): State<AppState>,
    Json(body): Json<CronWakeRequest>,
) -> Result<Json<CronWakeResponse>, GatewayError> {
    let mode = body.mode.unwrap_or_else(|| "next-heartbeat".to_string());

    let _ = state.event_tx.send(SessionEvent {
        session_id: "system".to_string(),
        kind: "wake_event".to_string(),
        payload: json!({
            "text": body.text,
            "mode": mode,
            "contextMessages": body.context_messages,
        }),
    });

    Ok(Json(CronWakeResponse {
        success: true,
        mode: mode.clone(),
        text: body.text,
        context_messages: body.context_messages,
        message: format!("wake event queued for {mode}"),
    }))
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
    /// Optional parent/requester session ID (for subagent/scheduled sessions).
    pub requester_session_id: Option<Uuid>,
    /// Optional channel reference (e.g. `telegram`, `discord`).
    pub channel_ref: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requester_session_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_ref: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub turn_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_model: Option<String>,
    pub usage_prompt_tokens: u64,
    pub usage_completion_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_turn_started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_turn_ended_at: Option<String>,
}

/// Lightweight session summary for list output.
#[derive(Serialize)]
pub struct SessionListItem {
    pub id: String,
    pub status: String,
    pub channel: Option<String>,
    pub created_at: String,
    pub turn_count: u32,
    pub usage_prompt_tokens: u64,
    pub usage_completion_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_model: Option<String>,
}

/// `GET /sessions` — list sessions.
pub async fn list_sessions(
    State(state): State<AppState>,
    Query(query): Query<SessionsListQuery>,
) -> Result<Json<Vec<SessionListItem>>, GatewayError> {
    let limit = query.limit.unwrap_or(100).min(500) as i64;
    let active_cutoff = query
        .active_minutes
        .map(|minutes| Utc::now() - chrono::Duration::minutes(minutes as i64));
    let channel_filter = query.channel.as_deref();

    let rows = state
        .session_repo
        .list(limit, 0)
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;

    let mut items = Vec::new();
    for row in rows
        .into_iter()
        .filter(|row| {
            channel_filter
                .map(|channel| row.channel_ref.as_deref() == Some(channel))
                .unwrap_or(true)
        })
        .filter(|row| {
            active_cutoff
                .map(|cutoff| row.last_activity_at >= cutoff)
                .unwrap_or(true)
        })
    {
        let turns = state
            .turn_repo
            .list_by_session(row.id)
            .await
            .map_err(|e| GatewayError::Internal(e.to_string()))?;
        let aggregate = aggregate_turns(&turns);
        items.push(SessionListItem {
            id: row.id.to_string(),
            status: row.status,
            channel: row.channel_ref,
            created_at: row.created_at.to_rfc3339(),
            turn_count: aggregate.turn_count,
            usage_prompt_tokens: aggregate.usage_prompt_tokens,
            usage_completion_tokens: aggregate.usage_completion_tokens,
            latest_model: aggregate.latest_model,
        });
    }

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
        .create_session_full(
            kind,
            body.workspace_root,
            body.requester_session_id,
            body.channel_ref,
        )
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
            requester_session_id: row.requester_session_id,
            channel_ref: row.channel_ref,
            created_at: row.created_at.to_rfc3339(),
            updated_at: row.updated_at.to_rfc3339(),
            turn_count: 0,
            latest_model: None,
            usage_prompt_tokens: 0,
            usage_completion_tokens: 0,
            last_turn_started_at: None,
            last_turn_ended_at: None,
        }),
    ))
}

/// First-class session status parity card for `/sessions/{id}/status`.
#[derive(Serialize)]
pub struct SessionStatusResponse {
    pub session_id: String,
    pub runtime: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_override: Option<String>,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_cost: Option<String>,
    pub turn_count: u32,
    pub uptime_seconds: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_turn_started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_turn_ended_at: Option<String>,
    pub reasoning: String,
    pub verbose: bool,
    pub elevated: bool,
    pub approval_mode: String,
    pub security_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent_lifecycle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent_runtime_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent_runtime_attached: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent_status_updated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent_last_note: Option<String>,
    pub unresolved: Vec<String>,
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

    let turns = state
        .turn_repo
        .list_by_session(row.id)
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;
    let aggregate = aggregate_turns(&turns);

    Ok(Json(SessionResponse {
        id: row.id,
        kind: row.kind,
        status: row.status,
        requester_session_id: row.requester_session_id,
        channel_ref: row.channel_ref,
        created_at: row.created_at.to_rfc3339(),
        updated_at: row.updated_at.to_rfc3339(),
        turn_count: aggregate.turn_count,
        latest_model: aggregate.latest_model,
        usage_prompt_tokens: aggregate.usage_prompt_tokens,
        usage_completion_tokens: aggregate.usage_completion_tokens,
        last_turn_started_at: aggregate.last_turn_started_at,
        last_turn_ended_at: aggregate.last_turn_ended_at,
    }))
}

/// `GET /sessions/{id}/status` — first-class session status parity card.
pub async fn get_session_status(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<SessionStatusResponse>, GatewayError> {
    let row = state
        .session_engine
        .get_session(id)
        .await
        .map_err(|e| GatewayError::SessionNotFound(e.to_string()))?;

    let turns = state
        .turn_repo
        .list_by_session(row.id)
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;
    let aggregate = aggregate_turns(&turns);

    let metadata = &row.metadata;
    let model_override = metadata_string(metadata, "selected_model");
    let current_model = aggregate
        .latest_model
        .clone()
        .or_else(|| model_override.clone());
    let approval_mode =
        metadata_string(metadata, "approval_mode").unwrap_or_else(|| "on-miss".to_string());
    let security_mode =
        metadata_string(metadata, "security_mode").unwrap_or_else(|| "allowlist".to_string());
    let reasoning = metadata_string(metadata, "reasoning").unwrap_or_else(|| "off".to_string());
    let verbose = metadata_bool(metadata, "verbose").unwrap_or(false);
    let elevated = metadata_bool(metadata, "elevated").unwrap_or(false);
    let subagent_lifecycle = metadata_string(metadata, "subagent_lifecycle");
    let subagent_runtime_status = metadata_string(metadata, "subagent_runtime_status");
    let subagent_runtime_attached = metadata_bool(metadata, "subagent_runtime_attached");
    let subagent_status_updated_at = metadata_string(metadata, "subagent_status_updated_at");
    let subagent_last_note = metadata_string(metadata, "subagent_last_note");

    let mut unresolved = Vec::new();
    unresolved.push("cost posture is estimate-only; provider pricing is not wired yet".to_string());
    if approval_mode == "on-miss" {
        unresolved.push(
            "approval requests and operator-triggered resume are durable, but restart-safe continuation for mid-resume approval flows is not parity-complete yet".to_string(),
        );
    }
    if security_mode == "allowlist" {
        unresolved.push(
            "host/node/sandbox parity and PTY fidelity are not yet parity-complete".to_string(),
        );
    }
    if row.kind == "subagent" {
        unresolved.push(
            "subagent runtime execution remains conservative; durable lifecycle inspection is available but full remote/runtime attachment parity is not complete".to_string(),
        );
    }

    Ok(Json(SessionStatusResponse {
        session_id: row.id.to_string(),
        runtime: format!(
            "kind={} | channel={} | status={}",
            row.kind,
            row.channel_ref.as_deref().unwrap_or("local"),
            row.status
        ),
        status: row.status,
        current_model,
        model_override,
        prompt_tokens: aggregate.usage_prompt_tokens,
        completion_tokens: aggregate.usage_completion_tokens,
        total_tokens: aggregate.usage_prompt_tokens + aggregate.usage_completion_tokens,
        estimated_cost: Some("not available".to_string()),
        turn_count: aggregate.turn_count,
        uptime_seconds: state.started_at.elapsed().as_secs(),
        last_turn_started_at: aggregate.last_turn_started_at,
        last_turn_ended_at: aggregate.last_turn_ended_at,
        reasoning,
        verbose,
        elevated,
        approval_mode,
        security_mode,
        subagent_lifecycle,
        subagent_runtime_status,
        subagent_runtime_attached,
        subagent_status_updated_at,
        subagent_last_note,
        unresolved,
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
    let now = Utc::now();
    match schedule {
        Schedule::At { at } => Some(*at),
        Schedule::Every {
            every_ms,
            anchor_ms,
        } => Some(compute_next_interval_run(*every_ms, *anchor_ms, now)),
        Schedule::Cron { expr, tz } => compute_next_cron_run(expr, tz.as_deref(), now),
    }
}

fn compute_next_interval_run(
    every_ms: u64,
    anchor_ms: Option<u64>,
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

    now + duration
}

fn compute_next_cron_run(
    expr: &str,
    tz: Option<&str>,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let schedule = expr.parse::<cron::Schedule>().ok()?;
    let timezone = match tz {
        None => chrono_tz::UTC,
        Some(value) => value.parse::<chrono_tz::Tz>().ok()?,
    };
    let after_local = now.with_timezone(&timezone);
    schedule
        .after(&after_local)
        .next()
        .map(|next| next.with_timezone(&Utc))
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

struct SessionTurnAggregate {
    turn_count: u32,
    latest_model: Option<String>,
    usage_prompt_tokens: u64,
    usage_completion_tokens: u64,
    last_turn_started_at: Option<String>,
    last_turn_ended_at: Option<String>,
}

fn aggregate_turns(turns: &[TurnRow]) -> SessionTurnAggregate {
    let turn_count = turns.len() as u32;
    let usage_prompt_tokens = turns
        .iter()
        .map(|turn| turn.usage_prompt_tokens.unwrap_or(0).max(0) as u64)
        .sum();
    let usage_completion_tokens = turns
        .iter()
        .map(|turn| turn.usage_completion_tokens.unwrap_or(0).max(0) as u64)
        .sum();
    let latest_turn = turns.iter().max_by_key(|turn| turn.started_at);

    SessionTurnAggregate {
        turn_count,
        latest_model: latest_turn.and_then(|turn| turn.model_ref.clone()),
        usage_prompt_tokens,
        usage_completion_tokens,
        last_turn_started_at: latest_turn.map(|turn| turn.started_at.to_rfc3339()),
        last_turn_ended_at: latest_turn.and_then(|turn| turn.ended_at.map(|dt| dt.to_rfc3339())),
    }
}

fn resolved_default_model(state: &AppState) -> Option<String> {
    state
        .config
        .agents
        .default_agent()
        .and_then(|agent| state.config.agents.effective_model(agent))
        .map(str::to_string)
        .or_else(|| state.config.models.default_model.clone())
}

fn configured_channels(state: &AppState) -> Vec<String> {
    let mut channels = state.config.channels.enabled.clone();
    if state.config.channels.telegram_token.is_some() && !channels.iter().any(|c| c == "telegram") {
        channels.push("telegram".to_string());
    }
    channels.sort();
    channels.dedup();
    channels
}

fn metadata_string(metadata: &Value, key: &str) -> Option<String> {
    metadata
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.is_empty())
}

fn metadata_bool(metadata: &Value, key: &str) -> Option<bool> {
    metadata.get(key).and_then(Value::as_bool)
}

fn session_to_dashboard_item(row: SessionRow) -> DashboardSessionItem {
    DashboardSessionItem {
        id: row.id.to_string(),
        kind: row.kind,
        status: row.status,
        routing_ref: row.channel_ref.clone(),
        channel_ref: row.channel_ref,
        created_at: row.created_at.to_rfc3339(),
        last_activity_at: row.last_activity_at.to_rfc3339(),
    }
}

const DASHBOARD_HTML: &str = r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <link rel="icon" href="/assets/rune-logo-favicon.svg" type="image/svg+xml">
  <title>Rune Operator Dashboard</title>
  <style>
    :root {
      --bg: #f5f7fb;
      --bg-soft: #eef3fb;
      --panel: rgba(255, 255, 255, 0.82);
      --panel-strong: rgba(255, 255, 255, 0.94);
      --panel-alt: rgba(247, 250, 255, 0.92);
      --ink: #1b2942;
      --text-strong: #101c33;
      --muted: #61718b;
      --line: rgba(102, 123, 159, 0.18);
      --line-strong: rgba(88, 108, 145, 0.28);
      --accent: #ff8c61;
      --accent-strong: #ff6b79;
      --accent-soft: rgba(255, 140, 97, 0.16);
      --violet: #8d7dff;
      --violet-soft: rgba(141, 125, 255, 0.14);
      --teal: #2fb6b0;
      --teal-soft: rgba(47, 182, 176, 0.14);
      --warn: #d98f1f;
      --warn-soft: rgba(217, 143, 31, 0.14);
      --danger: #df5e67;
      --danger-soft: rgba(223, 94, 103, 0.14);
      --ok: #249f97;
      --ok-soft: rgba(36, 159, 151, 0.14);
      --shadow-lg: 0 30px 80px rgba(24, 43, 77, 0.13);
      --shadow-md: 0 18px 48px rgba(24, 43, 77, 0.08);
      --radius-xl: 32px;
      --radius-lg: 24px;
      --radius-md: 18px;
      --space-2: 0.5rem;
      --space-3: 0.75rem;
      --space-4: 1rem;
      --space-5: 1.25rem;
      --space-6: 1.5rem;
      --space-8: 2rem;
      --space-10: 2.5rem;
      --space-12: 3rem;
      --space-16: 4rem;
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      min-height: 100vh;
      font-family: "Avenir Next", "IBM Plex Sans", "Segoe UI", sans-serif;
      color: var(--ink);
      background:
        radial-gradient(circle at top left, rgba(47, 182, 176, 0.18), transparent 22%),
        radial-gradient(circle at 88% 8%, rgba(255, 107, 121, 0.2), transparent 22%),
        radial-gradient(circle at 80% 28%, rgba(141, 125, 255, 0.16), transparent 20%),
        linear-gradient(180deg, #fcfdff 0%, var(--bg) 48%, #eef4fb 100%);
    }
    body::before {
      content: "";
      position: fixed;
      inset: 0;
      pointer-events: none;
      background-image:
        linear-gradient(rgba(115, 138, 177, 0.08) 1px, transparent 1px),
        linear-gradient(90deg, rgba(115, 138, 177, 0.08) 1px, transparent 1px);
      background-size: 32px 32px;
      mask-image: radial-gradient(circle at center, black 32%, transparent 82%);
      opacity: 0.4;
    }
    .shell {
      position: relative;
      max-width: 1360px;
      margin: 0 auto;
      padding: 24px 16px 64px;
    }
    .topbar {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--space-4);
      margin-bottom: var(--space-6);
      padding: 0 var(--space-2);
    }
    .topbar-note {
      color: var(--muted);
      font-size: 13px;
      letter-spacing: 0.02em;
    }
    .topbar-badge {
      display: inline-flex;
      align-items: center;
      gap: 8px;
      padding: 10px 14px;
      border-radius: 999px;
      border: 1px solid rgba(141, 125, 255, 0.22);
      background: rgba(255, 255, 255, 0.72);
      color: var(--text-strong);
      font-size: 12px;
      font-weight: 700;
      letter-spacing: 0.08em;
      text-transform: uppercase;
      box-shadow: var(--shadow-md);
    }
    .hero {
      position: relative;
      overflow: hidden;
      display: grid;
      grid-template-columns: minmax(0, 1.25fr) minmax(320px, 0.95fr);
      gap: var(--space-6);
      align-items: stretch;
      margin-bottom: var(--space-8);
      padding: var(--space-8);
      border: 1px solid rgba(255, 255, 255, 0.72);
      border-radius: var(--radius-xl);
      background:
        linear-gradient(135deg, rgba(255, 255, 255, 0.82), rgba(249, 251, 255, 0.92)),
        linear-gradient(140deg, rgba(255, 140, 97, 0.18), rgba(141, 125, 255, 0.12) 58%, rgba(47, 182, 176, 0.14));
      box-shadow: var(--shadow-lg);
      backdrop-filter: blur(22px);
    }
    .hero::after {
      content: "";
      position: absolute;
      inset: auto -60px -110px auto;
      width: 280px;
      height: 280px;
      border-radius: 999px;
      background: radial-gradient(circle, rgba(255, 140, 97, 0.26), transparent 68%);
      pointer-events: none;
    }
    .hero::before {
      content: "";
      position: absolute;
      inset: -35% auto auto -10%;
      width: 380px;
      height: 380px;
      border-radius: 50%;
      background: radial-gradient(circle, rgba(141, 125, 255, 0.18), transparent 70%);
      pointer-events: none;
    }
    .hero-copy {
      position: relative;
      z-index: 1;
      display: flex;
      flex-direction: column;
      gap: var(--space-6);
      min-width: 0;
    }
    .brand-lockup {
      display: flex;
      align-items: center;
      gap: var(--space-4);
      flex-wrap: wrap;
    }
    .brand-mark {
      width: 62px;
      height: 62px;
      border-radius: 20px;
      padding: 13px;
      background: linear-gradient(180deg, rgba(255, 140, 97, 0.12), rgba(141, 125, 255, 0.06));
      border: 1px solid rgba(255, 140, 97, 0.18);
      box-shadow: inset 0 1px 0 rgba(255,255,255,0.72);
    }
    .brand-wordmark {
      height: 32px;
      width: auto;
      display: block;
    }
    .eyebrow {
      display: inline-flex;
      align-items: center;
      gap: 8px;
      width: fit-content;
      padding: 9px 14px;
      border-radius: 999px;
      border: 1px solid rgba(47, 182, 176, 0.18);
      background: rgba(47, 182, 176, 0.1);
      color: #1d7f7b;
      font-size: 12px;
      font-weight: 700;
      letter-spacing: 0.08em;
      text-transform: uppercase;
    }
    .hero-heading {
      display: grid;
      gap: var(--space-4);
      max-width: 52rem;
    }
    h1 {
      margin: 0;
      color: var(--text-strong);
      font-size: clamp(2.45rem, 4vw, 4.45rem);
      line-height: 0.96;
      letter-spacing: -0.065em;
      max-width: 11ch;
    }
    .subhead {
      margin: 0;
      font-size: clamp(1rem, 1.25vw, 1.125rem);
      line-height: 1.75;
      color: var(--muted);
      max-width: 62ch;
    }
    .hero-meta {
      display: flex;
      flex-wrap: wrap;
      gap: 10px;
    }
    .pill {
      border: 1px solid rgba(255, 255, 255, 0.72);
      background: rgba(255, 255, 255, 0.76);
      border-radius: 999px;
      padding: 10px 14px;
      font-size: 12px;
      color: var(--muted);
      box-shadow: 0 10px 25px rgba(32, 54, 92, 0.06);
    }
    .pill strong {
      color: var(--text-strong);
      font-weight: 700;
    }
    .hero-stats {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
      gap: 12px;
    }
    .hero-stat {
      padding: 16px 18px;
      border-radius: 20px;
      border: 1px solid rgba(255, 255, 255, 0.76);
      background: linear-gradient(180deg, rgba(255,255,255,0.86), rgba(247,250,255,0.96));
      box-shadow: 0 16px 32px rgba(34, 58, 98, 0.07);
    }
    .hero-stat label {
      display: block;
      margin-bottom: 8px;
      color: var(--muted);
      font-size: 11px;
      font-weight: 700;
      letter-spacing: 0.09em;
      text-transform: uppercase;
    }
    .hero-stat strong {
      color: var(--text-strong);
      font-size: 1rem;
      line-height: 1.45;
      word-break: break-word;
    }
    .hero-visual {
      position: relative;
      min-height: 100%;
      border-radius: 28px;
      overflow: hidden;
      border: 1px solid rgba(255, 255, 255, 0.82);
      background:
        linear-gradient(180deg, rgba(17, 28, 50, 0.1), rgba(17, 28, 50, 0.5)),
        linear-gradient(135deg, rgba(255, 140, 97, 0.16), rgba(141, 125, 255, 0.14));
      box-shadow: inset 0 1px 0 rgba(255,255,255,0.5);
    }
    .hero-visual img {
      width: 100%;
      height: 100%;
      object-fit: cover;
      object-position: center;
      filter: saturate(1.05) contrast(1.04);
      transform: scale(1.01);
    }
    .hero-overlay {
      position: absolute;
      inset: auto 20px 20px 20px;
      display: grid;
      gap: 14px;
      padding: 20px;
      border-radius: 22px;
      background: linear-gradient(180deg, rgba(15, 25, 45, 0.82), rgba(20, 32, 56, 0.92));
      border: 1px solid rgba(255, 255, 255, 0.1);
      backdrop-filter: blur(14px);
    }
    .hero-overlay-title {
      font-size: 12px;
      text-transform: uppercase;
      letter-spacing: 0.1em;
      color: #ffc9bb;
    }
    .hero-overlay-value {
      font-size: clamp(1.8rem, 2.2vw, 2.5rem);
      font-weight: 700;
      line-height: 1;
      color: #fff9ff;
    }
    .hero-overlay-copy {
      color: rgba(232, 239, 255, 0.76);
      font-size: 13px;
      line-height: 1.6;
    }
    .hero-overlay-grid {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 10px;
    }
    .overlay-chip {
      padding: 10px 12px;
      border-radius: 14px;
      background: rgba(255, 255, 255, 0.08);
      border: 1px solid rgba(255, 255, 255, 0.08);
      color: rgba(240, 245, 255, 0.82);
      font-size: 12px;
      line-height: 1.45;
    }
    .overlay-chip strong {
      display: block;
      color: #fff;
      font-size: 13px;
      margin-top: 2px;
    }
    .section-heading {
      display: flex;
      justify-content: space-between;
      align-items: end;
      gap: var(--space-4);
      margin-bottom: var(--space-5);
      padding: 0 var(--space-2);
    }
    .section-label {
      margin: 0 0 8px;
      color: #8c6bff;
      font-size: 12px;
      font-weight: 700;
      letter-spacing: 0.1em;
      text-transform: uppercase;
    }
    .section-heading h2 {
      margin: 0;
      font-size: clamp(1.7rem, 2vw, 2.25rem);
      letter-spacing: -0.05em;
      color: var(--text-strong);
    }
    .section-heading p {
      margin: 10px 0 0;
      color: var(--muted);
      font-size: 14px;
      line-height: 1.7;
      max-width: 56ch;
    }
    .grid {
      display: grid;
      gap: var(--space-5);
      grid-template-columns: repeat(12, minmax(0, 1fr));
    }
    .card {
      grid-column: span 12;
      border: 1px solid rgba(255, 255, 255, 0.86);
      background: var(--panel);
      border-radius: var(--radius-lg);
      padding: 24px;
      box-shadow: var(--shadow-md);
      backdrop-filter: blur(18px);
    }
    .card-head {
      display: flex;
      gap: 12px;
      align-items: flex-start;
      justify-content: space-between;
      margin-bottom: 20px;
    }
    .card h2 {
      margin: 0 0 8px;
      color: var(--text-strong);
      font-size: 12px;
      text-transform: uppercase;
      letter-spacing: 0.1em;
    }
    .card-copy {
      margin: 0;
      color: var(--muted);
      font-size: 14px;
      line-height: 1.7;
    }
    .stats {
      display: grid;
      gap: 16px;
      grid-template-columns: repeat(auto-fit, minmax(190px, 1fr));
    }
    .stat {
      border: 1px solid rgba(106, 127, 163, 0.14);
      border-radius: var(--radius-md);
      padding: 18px;
      background:
        linear-gradient(180deg, rgba(255,255,255,0.98), rgba(247,250,255,0.88)),
        var(--panel-strong);
      min-height: 136px;
      display: flex;
      flex-direction: column;
      justify-content: space-between;
      box-shadow: inset 0 1px 0 rgba(255,255,255,0.72);
    }
    .stat label {
      display: block;
      color: var(--muted);
      font-size: 12px;
      text-transform: uppercase;
      letter-spacing: 0.08em;
      margin-bottom: 10px;
    }
    .stat strong {
      font-size: clamp(1.55rem, 2.5vw, 2.05rem);
      line-height: 1.05;
      color: var(--text-strong);
      letter-spacing: -0.04em;
      word-break: break-word;
    }
    .stat small {
      display: block;
      margin-top: 10px;
      color: var(--muted);
      font-size: 12px;
      line-height: 1.5;
    }
    .stack {
      display: grid;
      gap: 12px;
    }
    .surface {
      border-radius: 20px;
      border: 1px solid rgba(106, 127, 163, 0.14);
      background: var(--panel-alt);
      overflow: hidden;
      box-shadow: inset 0 1px 0 rgba(255,255,255,0.76);
    }
    table {
      width: 100%;
      border-collapse: collapse;
      font-size: 14px;
    }
    th, td {
      text-align: left;
      padding: 16px 18px;
      border-top: 1px solid rgba(106, 127, 163, 0.12);
      vertical-align: top;
    }
    th {
      color: var(--muted);
      font-size: 12px;
      text-transform: uppercase;
      letter-spacing: 0.08em;
      border-top: none;
      padding-top: 16px;
      padding-bottom: 12px;
    }
    td {
      color: var(--ink);
    }
    tr:hover td {
      background: rgba(141, 125, 255, 0.04);
    }
    .model-name,
    .session-id {
      display: grid;
      gap: 6px;
    }
    .model-name strong,
    .session-id strong {
      color: var(--text-strong);
      font-size: 14px;
      font-weight: 600;
    }
    .table-subtle {
      color: var(--muted);
      font-size: 12px;
      line-height: 1.5;
    }
    .chip-row {
      display: flex;
      flex-wrap: wrap;
      gap: 8px;
    }
    .chip {
      display: inline-flex;
      align-items: center;
      width: fit-content;
      border-radius: 999px;
      padding: 6px 11px;
      font-size: 12px;
      font-weight: 600;
      letter-spacing: 0.02em;
      border: 1px solid transparent;
    }
    .chip.status-running,
    .chip.status-ready,
    .chip.status-completed {
      background: var(--ok-soft);
      color: #19766f;
      border-color: rgba(36, 159, 151, 0.18);
    }
    .chip.status-failed,
    .chip.status-error,
    .chip.status-cancelled {
      background: var(--danger-soft);
      color: #9d3340;
      border-color: rgba(223, 94, 103, 0.18);
    }
    .chip.status-waiting,
    .chip.status-pending,
    .chip.status-tool_executing {
      background: var(--warn-soft);
      color: #8f5a0a;
      border-color: rgba(217, 143, 31, 0.18);
    }
    .chip.kind {
      background: rgba(141, 125, 255, 0.08);
      color: #5340cd;
      border-color: rgba(141, 125, 255, 0.14);
      text-transform: capitalize;
    }
    code {
      font-family: "IBM Plex Mono", "SFMono-Regular", ui-monospace, monospace;
      font-size: 12px;
      color: #824d56;
      background: rgba(255, 140, 97, 0.08);
      border: 1px solid rgba(255, 140, 97, 0.12);
      border-radius: 10px;
      padding: 2px 7px;
      word-break: break-all;
    }
    .diag {
      display: grid;
      gap: 14px;
    }
    .diag-item {
      border-radius: 20px;
      border: 1px solid rgba(106, 127, 163, 0.14);
      padding: 18px;
      background: var(--panel-strong);
    }
    .diag-item.info { background: rgba(255,255,255,0.78); }
    .diag-item.warn { background: rgba(255, 244, 226, 0.92); border-color: rgba(217, 143, 31, 0.26); }
    .diag-item.error { background: rgba(255, 238, 239, 0.94); border-color: rgba(223, 94, 103, 0.26); }
    .diag-head {
      display: flex;
      gap: 8px;
      justify-content: space-between;
      align-items: baseline;
      margin-bottom: 8px;
      font-family: "IBM Plex Mono", monospace;
      font-size: 12px;
      color: var(--muted);
    }
    .diag-message {
      color: var(--text-strong);
      line-height: 1.6;
    }
    .empty, .loading {
      color: var(--muted);
      padding: 18px 16px;
    }
    .status-rail {
      display: grid;
      gap: 12px;
      grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    }
    .rail-item {
      border: 1px solid rgba(255, 255, 255, 0.82);
      border-radius: 20px;
      padding: 16px 18px;
      background: linear-gradient(180deg, rgba(255,255,255,0.84), rgba(247,250,255,0.98));
      box-shadow: 0 18px 34px rgba(31, 50, 86, 0.06);
    }
    .rail-item span {
      display: block;
      margin-bottom: 8px;
      color: var(--muted);
      font-size: 12px;
      text-transform: uppercase;
      letter-spacing: 0.08em;
    }
    .rail-item strong {
      color: var(--text-strong);
      font-size: 15px;
      line-height: 1.5;
    }
    .footer-note {
      margin-top: 18px;
      color: var(--muted);
      font-size: 12px;
      line-height: 1.6;
    }
    @media (min-width: 900px) {
      .summary { grid-column: span 12; }
      .models { grid-column: span 4; }
      .sessions { grid-column: span 8; }
      .diagnostics { grid-column: span 12; }
    }
    @media (max-width: 980px) {
      .topbar,
      .section-heading {
        padding: 0;
      }
      .hero {
        grid-template-columns: 1fr;
      }
      .hero-visual {
        order: -1;
        min-height: 300px;
      }
    }
    @media (max-width: 720px) {
      .shell {
        padding: 16px 14px 32px;
      }
      .topbar {
        flex-direction: column;
        align-items: flex-start;
      }
      .hero,
      .card {
        padding: 18px;
      }
      .brand-lockup {
        align-items: flex-start;
      }
      .brand-wordmark {
        height: 28px;
      }
      h1 {
        max-width: 12ch;
      }
      .hero-stats,
      .status-rail,
      .hero-overlay-grid {
        grid-template-columns: 1fr;
      }
      .section-heading {
        align-items: flex-start;
      }
      .surface {
        overflow-x: auto;
      }
      table {
        min-width: 680px;
      }
      .hero-overlay {
        inset: auto 12px 12px 12px;
      }
    }
  </style>
</head>
<body>
  <main class="shell">
    <div class="topbar">
      <div class="topbar-note">Horizon-inspired operator surface for Rune's existing dashboard routes.</div>
      <div class="topbar-badge">Dashboard Theme Refresh</div>
    </div>
    <section class="hero">
      <div class="hero-copy">
        <div class="eyebrow">Local Operator Surface</div>
        <div class="brand-lockup">
          <img class="brand-mark" src="/assets/rune-logo-icon.svg" alt="Rune icon">
          <img class="brand-wordmark" src="/assets/rune-logo-wordmark-dark.svg" alt="Rune">
        </div>
        <div class="hero-heading">
          <h1>Operate sessions, models, and runtime health with actual signal.</h1>
          <p class="subhead">Local-first control plane for gateway status, configured models, recent sessions, and diagnostics. Same backend routes, cleaner hierarchy, better scanability, and brand-consistent presentation.</p>
        </div>
        <div class="hero-meta">
          <div class="pill">Route <strong>/dashboard</strong></div>
          <div class="pill">Mirror <strong>/ui</strong></div>
          <div class="pill">Data <strong>/api/dashboard/*</strong></div>
        </div>
        <div id="status-rail" class="status-rail">
          <div class="hero-stat"><label>Gateway</label><strong>Loading…</strong></div>
          <div class="hero-stat"><label>Bind</label><strong>Loading…</strong></div>
          <div class="hero-stat"><label>Channels</label><strong>Loading…</strong></div>
          <div class="hero-stat"><label>Auth</label><strong>Loading…</strong></div>
        </div>
      </div>
      <aside class="hero-visual" aria-hidden="true">
        <img src="/assets/hero.png" alt="">
        <div class="hero-overlay">
          <div class="hero-overlay-title">Rune Operator</div>
          <div id="hero-session-count" class="hero-overlay-value">Loading…</div>
          <div class="hero-overlay-copy">Snapshot of live operator context across runtime sessions, model routing, channel availability, and diagnostic posture.</div>
          <div class="hero-overlay-grid">
            <div class="overlay-chip">Theme<strong>Horizon-style polish</strong></div>
            <div class="overlay-chip">Surface<strong>Dashboard + /ui mirror</strong></div>
          </div>
        </div>
      </aside>
    </section>

    <div class="section-heading">
      <div>
        <p class="section-label">Overview</p>
        <h2>Operational clarity without changing the backend surface.</h2>
        <p>The dashboard keeps the same live JSON sources and runtime functionality, but the hierarchy now reads like a polished admin surface instead of a raw internal panel.</p>
      </div>
    </div>

    <section class="grid">
      <article class="card summary">
        <div class="card-head">
          <div>
            <h2>Overview</h2>
            <p class="card-copy">High-signal system summary for fast operator triage.</p>
          </div>
        </div>
        <div id="summary" class="stats">
          <div class="loading">Loading summary…</div>
        </div>
      </article>

      <article class="card models">
        <div class="card-head">
          <div>
            <h2>Model Inventory</h2>
            <p class="card-copy">Configured routes and default selection across providers.</p>
          </div>
        </div>
        <div class="stack">
          <div class="surface">
          <table>
            <thead>
              <tr>
                <th>Model</th>
                <th>Provider</th>
                <th>Kind</th>
              </tr>
            </thead>
            <tbody id="models-body">
              <tr><td colspan="3" class="loading">Loading models…</td></tr>
            </tbody>
          </table>
          </div>
        </div>
      </article>

      <article class="card sessions">
        <div class="card-head">
          <div>
            <h2>Recent Sessions</h2>
            <p class="card-copy">Current execution state, routing context, and latest activity.</p>
          </div>
        </div>
        <div class="surface">
        <table>
          <thead>
            <tr>
              <th>Session</th>
              <th>Kind / Status</th>
              <th>Channel / Routing</th>
              <th>Last Activity</th>
              <th>Created</th>
            </tr>
          </thead>
          <tbody id="sessions-body">
            <tr><td colspan="5" class="loading">Loading sessions…</td></tr>
          </tbody>
        </table>
        </div>
      </article>

      <article class="card diagnostics">
        <div class="card-head">
          <div>
            <h2>Diagnostics</h2>
            <p class="card-copy">Configuration warnings and runtime signals surfaced by the gateway.</p>
          </div>
        </div>
        <div id="diagnostics" class="diag">
          <div class="loading">Loading diagnostics…</div>
        </div>
        <div class="footer-note">This dashboard is intentionally lightweight and reads from the existing gateway JSON routes, so it stays operational without introducing a separate SPA runtime.</div>
      </article>
    </section>
  </main>
  <script>
    function escapeHtml(value) {
      return String(value ?? "")
        .replaceAll("&", "&amp;")
        .replaceAll("<", "&lt;")
        .replaceAll(">", "&gt;")
        .replaceAll('"', "&quot;")
        .replaceAll("'", "&#39;");
    }

    function fmtDate(value) {
      if (!value) return "n/a";
      const date = new Date(value);
      return Number.isNaN(date.getTime()) ? value : date.toLocaleString([], {
        month: "short",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit"
      });
    }

    function fmtDuration(seconds) {
      if (typeof seconds !== "number") return "n/a";
      const parts = [];
      const days = Math.floor(seconds / 86400);
      const hours = Math.floor((seconds % 86400) / 3600);
      const minutes = Math.floor((seconds % 3600) / 60);
      if (days) parts.push(days + "d");
      if (hours) parts.push(hours + "h");
      parts.push(minutes + "m");
      return parts.join(" ");
    }

    function toStatusClass(value) {
      return String(value || "unknown").toLowerCase().replaceAll(/[^a-z0-9]+/g, "_");
    }

    function renderStatusRail(summary) {
      const channels = Array.isArray(summary.channels) && summary.channels.length
        ? summary.channels.join(", ")
        : "No channels";
      document.getElementById("status-rail").innerHTML = [
        ["Gateway", summary.gateway_status || "unknown"],
        ["Bind", summary.bind || "n/a"],
        ["Channels", channels],
        ["Auth", summary.auth_enabled ? "Bearer protected" : "Open"]
      ].map(([label, value]) => `
        <div class="hero-stat">
          <label>${escapeHtml(label)}</label>
          <strong>${escapeHtml(value)}</strong>
        </div>
      `).join("");
      document.getElementById("hero-session-count").textContent =
        `${summary.session_count || 0} active session${summary.session_count === 1 ? "" : "s"}`;
    }

    async function loadJson(path) {
      const response = await fetch(path, { headers: { "accept": "application/json" } });
      if (!response.ok) throw new Error(path + " returned " + response.status);
      return response.json();
    }

    function renderSummary(summary) {
      const root = document.getElementById("summary");
      const entries = [
        ["Gateway status", summary.gateway_status, summary.auth_enabled ? "Protected operator access is enabled." : "Auth is disabled for protected routes."],
        ["Uptime", fmtDuration(summary.uptime_seconds), `Listening on ${summary.bind || "n/a"}`],
        ["Default model", summary.default_model || "none", `${summary.provider_count || 0} provider${summary.provider_count === 1 ? "" : "s"} configured`],
        ["Configured models", String(summary.configured_model_count || 0), "Inventory discovered from current model configuration."],
        ["Sessions", String(summary.session_count || 0), `${summary.ws_subscribers || 0} WebSocket subscriber${summary.ws_subscribers === 1 ? "" : "s"}`],
        ["Channels", String((summary.channels || []).length), (summary.channels || []).length ? (summary.channels || []).join(", ") : "No channel adapters configured"],
      ];
      root.innerHTML = entries.map(([label, value, note]) =>
        `<div class="stat"><div><label>${escapeHtml(label)}</label><strong>${escapeHtml(value)}</strong></div><small>${escapeHtml(note)}</small></div>`
      ).join("");
      renderStatusRail(summary);
    }

    function renderModels(models) {
      const body = document.getElementById("models-body");
      if (!models.length) {
        body.innerHTML = `<tr><td colspan="3" class="empty">No configured models.</td></tr>`;
        return;
      }
      body.innerHTML = models.map((model) => `
        <tr>
          <td>
            <div class="model-name">
              <strong>${escapeHtml(model.model_id)}</strong>
              <div class="table-subtle">${model.is_default ? "Default route" : escapeHtml(model.raw_model || "Mapped model")}</div>
            </div>
          </td>
          <td>${escapeHtml(model.provider_name)}</td>
          <td><span class="chip kind">${escapeHtml(model.provider_kind || "n/a")}</span></td>
        </tr>
      `).join("");
    }

    function renderSessions(sessions) {
      const body = document.getElementById("sessions-body");
      if (!sessions.length) {
        body.innerHTML = `<tr><td colspan="5" class="empty">No sessions found.</td></tr>`;
        return;
      }
      body.innerHTML = sessions.map((session) => `
        <tr>
          <td>
            <div class="session-id">
              <strong>${escapeHtml((session.id || "").slice(0, 8))}</strong>
              <div><code>${escapeHtml(session.id)}</code></div>
            </div>
          </td>
          <td>
            <div class="chip-row">
              <span class="chip kind">${escapeHtml(session.kind || "unknown")}</span>
              <span class="chip status-${toStatusClass(session.status)}">${escapeHtml(session.status || "unknown")}</span>
            </div>
          </td>
          <td>
            <div>${escapeHtml(session.channel_ref || "n/a")}</div>
            <div class="table-subtle">${session.routing_ref ? `<code>${escapeHtml(session.routing_ref)}</code>` : "No routing ref"}</div>
          </td>
          <td>${escapeHtml(fmtDate(session.last_activity_at))}</td>
          <td>${escapeHtml(fmtDate(session.created_at))}</td>
        </tr>
      `).join("");
    }

    function renderDiagnostics(data) {
      const root = document.getElementById("diagnostics");
      if (!data.items.length) {
        root.innerHTML = `<div class="diag-item info"><div class="diag-head"><span>INFO · runtime</span><span>now</span></div><div class="diag-message">No diagnostics were raised by the current dashboard probes.</div></div>`;
        return;
      }
      root.innerHTML = data.items.map((item) => `
        <div class="diag-item ${item.level}">
          <div class="diag-head">
            <span>${escapeHtml(String(item.level || "info").toUpperCase())} · ${escapeHtml(item.source || "runtime")}</span>
            <span>${escapeHtml(fmtDate(item.observed_at))}</span>
          </div>
          <div class="diag-message">${escapeHtml(item.message)}</div>
        </div>
      `).join("");
    }

    async function boot() {
      try {
        const [summary, models, sessions, diagnostics] = await Promise.all([
          loadJson("/api/dashboard/summary"),
          loadJson("/api/dashboard/models"),
          loadJson("/api/dashboard/sessions"),
          loadJson("/api/dashboard/diagnostics"),
        ]);
        renderSummary(summary);
        renderModels(models);
        renderSessions(sessions);
        renderDiagnostics(diagnostics);
      } catch (error) {
        document.getElementById("summary").innerHTML = `<div class="empty">${escapeHtml(error.message)}</div>`;
        document.getElementById("models-body").innerHTML = `<tr><td colspan="3" class="empty">Failed to load models.</td></tr>`;
        document.getElementById("sessions-body").innerHTML = `<tr><td colspan="5" class="empty">Failed to load sessions.</td></tr>`;
        document.getElementById("diagnostics").innerHTML = `<div class="empty">Failed to load diagnostics.</div>`;
        document.getElementById("status-rail").innerHTML = `<div class="rail-item"><span>Gateway</span><strong>Unavailable</strong></div>`;
        document.getElementById("hero-session-count").textContent = "Dashboard unavailable";
      }
    }

    boot();
  </script>
</body>
</html>
"##;

// ── Approvals ─────────────────────────────────────────────────────────

/// Response for a pending or resolved approval request.
#[derive(Serialize)]
pub struct ApprovalRequestResponse {
    pub id: String,
    pub subject_type: String,
    pub subject_id: String,
    pub reason: String,
    pub decision: Option<String>,
    pub decided_by: Option<String>,
    pub decided_at: Option<String>,
    pub approval_status: Option<String>,
    pub approval_status_updated_at: Option<String>,
    pub resumed_at: Option<String>,
    pub completed_at: Option<String>,
    pub resume_result_summary: Option<String>,
    pub command: Option<String>,
    pub presented_payload: Value,
    pub created_at: String,
}

/// Response for a single tool approval policy.
#[derive(Serialize)]
pub struct ApprovalPolicyResponse {
    pub tool_name: String,
    pub decision: String,
    pub decided_at: String,
}

/// Request body for `POST /approvals`.
#[derive(Deserialize)]
pub struct SubmitApprovalDecisionRequest {
    pub id: String,
    pub decision: String,
    pub decided_by: Option<String>,
}

/// Request body for `PUT /approvals/policies/{tool}`.
#[derive(Deserialize)]
pub struct SetApprovalPolicyRequest {
    pub decision: String,
}

/// `GET /approvals` — list durable approval requests.
pub async fn list_pending_approvals(
    State(state): State<AppState>,
) -> Result<Json<Vec<ApprovalRequestResponse>>, GatewayError> {
    let approvals = state
        .approval_repo
        .list(true)
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;

    Ok(Json(
        approvals.into_iter().map(approval_to_response).collect(),
    ))
}

/// `POST /approvals` — submit a decision for a durable approval request.
pub async fn submit_approval_decision(
    State(state): State<AppState>,
    Json(body): Json<SubmitApprovalDecisionRequest>,
) -> Result<Json<ApprovalRequestResponse>, GatewayError> {
    let approval_id = Uuid::parse_str(&body.id)
        .map_err(|_| GatewayError::BadRequest(format!("invalid approval id: {}", body.id)))?;

    let normalised = body.decision.replace('-', "_");
    let valid_decisions = ["allow_once", "allow_always", "deny"];
    if !valid_decisions.contains(&normalised.as_str()) {
        return Err(GatewayError::BadRequest(format!(
            "invalid decision '{}'; expected one of: allow-once, allow-always, deny",
            body.decision
        )));
    }

    let decided = state
        .approval_repo
        .decide(
            approval_id,
            &normalised,
            body.decided_by.as_deref().unwrap_or("operator"),
            Utc::now(),
        )
        .await
        .map_err(|e| match e {
            rune_store::StoreError::NotFound { .. } => {
                GatewayError::BadRequest(format!("no pending approval found for id: {}", body.id))
            }
            other => GatewayError::Internal(other.to_string()),
        })?;

    if normalised == "allow_always" && decided.subject_type == "tool_call" {
        state
            .tool_approval_repo
            .set_policy(&decided.reason, "allow_always")
            .await
            .map_err(|e| GatewayError::Internal(e.to_string()))?;
    }

    if decided.subject_type == "tool_call" {
        state
            .turn_executor
            .resume_approval(decided.id)
            .await
            .map_err(|e| GatewayError::Internal(e.to_string()))?;
    }

    let decided = state
        .approval_repo
        .find_by_id(approval_id)
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;

    Ok(Json(approval_to_response(decided)))
}

fn approval_payload_field(payload: &Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

fn approval_to_response(approval: rune_store::models::ApprovalRow) -> ApprovalRequestResponse {
    let approval_status = approval_payload_field(&approval.presented_payload, "approval_status")
        .or_else(|| approval_payload_field(&approval.presented_payload, "resume_status"));
    let approval_status_updated_at =
        approval_payload_field(&approval.presented_payload, "approval_status_updated_at");
    let resumed_at = approval_payload_field(&approval.presented_payload, "resumed_at");
    let completed_at = approval_payload_field(&approval.presented_payload, "completed_at");
    let resume_result_summary =
        approval_payload_field(&approval.presented_payload, "resume_result_summary");
    let command = approval_payload_field(&approval.presented_payload, "command");

    ApprovalRequestResponse {
        id: approval.id.to_string(),
        subject_type: approval.subject_type,
        subject_id: approval.subject_id.to_string(),
        reason: approval.reason,
        decision: approval.decision,
        decided_by: approval.decided_by,
        decided_at: approval.decided_at.map(|value| value.to_rfc3339()),
        approval_status,
        approval_status_updated_at,
        resumed_at,
        completed_at,
        resume_result_summary,
        command,
        presented_payload: approval.presented_payload,
        created_at: approval.created_at.to_rfc3339(),
    }
}

/// `GET /approvals/policies` — list all tool approval policies.
pub async fn list_approval_policies(
    State(state): State<AppState>,
) -> Result<Json<Vec<ApprovalPolicyResponse>>, GatewayError> {
    let policies = state
        .tool_approval_repo
        .list_policies()
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;

    Ok(Json(
        policies
            .into_iter()
            .map(|p| ApprovalPolicyResponse {
                tool_name: p.tool_name,
                decision: p.decision,
                decided_at: p.decided_at.to_rfc3339(),
            })
            .collect(),
    ))
}

/// `GET /approvals/policies/{tool}` — get approval policy for a specific tool.
pub async fn get_approval_policy(
    State(state): State<AppState>,
    Path(tool): Path<String>,
) -> Result<Json<ApprovalPolicyResponse>, GatewayError> {
    let policy = state
        .tool_approval_repo
        .get_policy(&tool)
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?
        .ok_or_else(|| GatewayError::BadRequest(format!("no approval policy for tool: {tool}")))?;

    Ok(Json(ApprovalPolicyResponse {
        tool_name: policy.tool_name,
        decision: policy.decision,
        decided_at: policy.decided_at.to_rfc3339(),
    }))
}

/// `PUT /approvals/policies/{tool}` — set approval policy for a tool.
pub async fn set_approval_policy(
    State(state): State<AppState>,
    Path(tool): Path<String>,
    Json(body): Json<SetApprovalPolicyRequest>,
) -> Result<Json<ApprovalPolicyResponse>, GatewayError> {
    let valid_decisions = ["allow_always", "allow-always", "deny"];
    let normalised = body.decision.replace('-', "_");
    if !valid_decisions.contains(&normalised.as_str()) {
        return Err(GatewayError::BadRequest(format!(
            "invalid decision '{}'; expected one of: allow-always, deny",
            body.decision
        )));
    }

    let policy = state
        .tool_approval_repo
        .set_policy(&tool, &normalised)
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;

    Ok(Json(ApprovalPolicyResponse {
        tool_name: policy.tool_name,
        decision: policy.decision,
        decided_at: policy.decided_at.to_rfc3339(),
    }))
}

/// `DELETE /approvals/policies/{tool}` — clear approval policy for a tool.
pub async fn clear_approval_policy(
    State(state): State<AppState>,
    Path(tool): Path<String>,
) -> Result<Json<ActionResponse>, GatewayError> {
    let deleted = state
        .tool_approval_repo
        .clear_policy(&tool)
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;

    if deleted {
        Ok(Json(ActionResponse {
            success: true,
            message: format!("approval policy for '{tool}' cleared"),
        }))
    } else {
        Err(GatewayError::BadRequest(format!(
            "no approval policy found for tool: {tool}"
        )))
    }
}

// ── Telegram Webhook ────────────────────────────────────────────────

// ── Heartbeat ─────────────────────────────────────────────────────────────────

/// `GET /heartbeat/status` — current heartbeat runner state.
pub async fn heartbeat_status(
    State(state): State<AppState>,
) -> Result<Json<HeartbeatState>, GatewayError> {
    let status = state.heartbeat.status().await;
    Ok(Json(status))
}

/// `POST /heartbeat/enable` — enable the heartbeat runner.
pub async fn heartbeat_enable(
    State(state): State<AppState>,
) -> Result<Json<ActionResponse>, GatewayError> {
    state.heartbeat.enable().await;
    Ok(Json(ActionResponse {
        success: true,
        message: "heartbeat enabled".to_string(),
    }))
}

/// `POST /heartbeat/disable` — disable the heartbeat runner.
pub async fn heartbeat_disable(
    State(state): State<AppState>,
) -> Result<Json<ActionResponse>, GatewayError> {
    state.heartbeat.disable().await;
    Ok(Json(ActionResponse {
        success: true,
        message: "heartbeat disabled".to_string(),
    }))
}

// ── Reminders ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ReminderAddRequest {
    pub message: String,
    /// ISO-8601 fire-at timestamp.
    pub fire_at: DateTime<Utc>,
    /// Target session or channel (defaults to "main").
    #[serde(default = "default_reminder_target")]
    pub target: String,
}

fn default_reminder_target() -> String {
    "main".to_string()
}

#[derive(Serialize)]
pub struct ReminderResponse {
    pub id: String,
    pub message: String,
    pub target: String,
    pub fire_at: String,
    pub delivered: bool,
    pub created_at: String,
    pub delivered_at: Option<String>,
}

#[derive(Deserialize)]
pub struct RemindersListQuery {
    #[serde(rename = "includeDelivered")]
    pub include_delivered: Option<bool>,
}

/// `GET /reminders` — list reminders.
pub async fn reminders_list(
    State(state): State<AppState>,
    Query(query): Query<RemindersListQuery>,
) -> Result<Json<Vec<ReminderResponse>>, GatewayError> {
    let include_delivered = query.include_delivered.unwrap_or(false);
    let reminders = state.reminder_store.list(include_delivered).await;
    Ok(Json(
        reminders.into_iter().map(reminder_to_response).collect(),
    ))
}

/// `POST /reminders` — add a reminder.
pub async fn reminders_add(
    State(state): State<AppState>,
    Json(body): Json<ReminderAddRequest>,
) -> Result<(StatusCode, Json<ReminderResponse>), GatewayError> {
    let reminder = Reminder::new(body.message, body.target, body.fire_at);
    let resp = reminder_to_response(reminder.clone());
    state.reminder_store.add(reminder).await;
    Ok((StatusCode::CREATED, Json(resp)))
}

/// `DELETE /reminders/{id}` — cancel a reminder.
pub async fn reminders_cancel(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ActionResponse>, GatewayError> {
    let job_id = rune_core::JobId::from(id);
    state
        .reminder_store
        .cancel(&job_id)
        .await
        .ok_or_else(|| GatewayError::JobNotFound(id.to_string()))?;

    Ok(Json(ActionResponse {
        success: true,
        message: format!("reminder {id} cancelled"),
    }))
}

fn reminder_to_response(r: Reminder) -> ReminderResponse {
    ReminderResponse {
        id: r.id.to_string(),
        message: r.message,
        target: r.target,
        fire_at: r.fire_at.to_rfc3339(),
        delivered: r.delivered,
        created_at: r.created_at.to_rfc3339(),
        delivered_at: r.delivered_at.map(|dt| dt.to_rfc3339()),
    }
}

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
