//! HTTP route handlers for the gateway API.

use std::sync::Arc;
use std::time::Instant;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
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
use rune_runtime::{LaneStats, Skill, SkillScanSummary};
use rune_store::models::{SessionRow, TurnRow};
use rune_tools::process_tool::{PersistedProcessInfo, ProcessInfo};
use serde_json::Value;

use crate::error::GatewayError;
use crate::pairing::{PairedDevice, PairingError, PairingRequest};
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
    pub lane_stats: Option<LaneStatsResponse>,
    pub skills: SkillStatusResponse,
    pub config_paths: StatusPaths,
}

#[derive(Serialize)]
pub struct StatusPaths {
    pub sessions_dir: String,
    pub memory_dir: String,
    pub logs_dir: String,
}

#[derive(Serialize)]
pub struct LaneStatsResponse {
    pub main_active: usize,
    pub main_capacity: usize,
    pub subagent_active: usize,
    pub subagent_capacity: usize,
    pub cron_active: usize,
    pub cron_capacity: usize,
}

#[derive(Serialize)]
pub struct SkillStatusResponse {
    pub loaded: usize,
    pub enabled: usize,
    pub skills_dir: String,
}

/// Daemon status with useful runtime metadata.
pub async fn status(State(state): State<AppState>) -> Result<Json<StatusResponse>, GatewayError> {
    let sessions = state
        .session_repo
        .list(i64::MAX / 4, 0)
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;
    let cron_job_count = state.scheduler.list_jobs(true).await.len();

    let skills = state.skill_registry.list().await;
    let lane_stats = state.turn_executor.lane_stats().map(lane_stats_response);

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
        lane_stats,
        skills: SkillStatusResponse {
            loaded: skills.len(),
            enabled: skills.iter().filter(|skill| skill.enabled).count(),
            skills_dir: state.skill_loader.skills_dir().display().to_string(),
        },
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

// SPA serving — embedded UI dist
use include_dir::{Dir, include_dir};

static UI_DIST: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../ui/dist");

pub async fn spa_index() -> Response {
    spa_response_for_path("")
}

pub async fn spa_handler(uri: axum::http::Uri) -> Response {
    spa_response_for_path(uri.path().trim_start_matches('/'))
}

fn spa_response_for_path(path: &str) -> Response {
    // Try to serve the exact file first
    if !path.is_empty() {
        if let Some(file) = UI_DIST.get_file(path) {
            let content_type = match path.rsplit('.').next() {
                Some("html") => "text/html; charset=utf-8",
                Some("js") => "application/javascript; charset=utf-8",
                Some("css") => "text/css; charset=utf-8",
                Some("svg") => "image/svg+xml",
                Some("png") => "image/png",
                Some("ico") => "image/x-icon",
                Some("json") => "application/json",
                Some("woff2") => "font/woff2",
                Some("woff") => "font/woff",
                _ => "application/octet-stream",
            };
            return (
                StatusCode::OK,
                [(header::CONTENT_TYPE, content_type)],
                file.contents(),
            )
                .into_response();
        }
    }

    // Fall back to index.html for client-side routing
    match UI_DIST.get_file("index.html") {
        Some(index) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            index.contents(),
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "UI not built").into_response(),
    }
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
            // Try serving from embedded UI dist (Vite-built assets)
            let dist_path = format!("assets/{}", path);
            return Ok(spa_response_for_path(&dist_path));
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

/// `POST /gateway/start` — acknowledges the control-plane request in the current single-process gateway model.
pub async fn gateway_start() -> Json<ActionResponse> {
    Json(ActionResponse {
        success: true,
        message: "gateway already running in foreground mode".to_string(),
    })
}

/// `POST /gateway/stop` — acknowledges the control-plane request in the current single-process gateway model.
pub async fn gateway_stop() -> Json<ActionResponse> {
    Json(ActionResponse {
        success: true,
        message: "gateway stop acknowledged; external service supervision pending".to_string(),
    })
}

/// `POST /gateway/restart` — acknowledges the control-plane request in the current single-process gateway model.
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
        state_changed: true,
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
        state_changed: false,
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
        state_changed: true,
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

/// `PATCH /sessions/{id}` — update session metadata fields used by operator surfaces.
pub async fn patch_session(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchSessionRequest>,
) -> Result<Json<SessionResponse>, GatewayError> {
    let patch = serde_json::json!({
        "label": body.label,
        "thinking_level": body.thinking_level,
        "verbose": body.verbose,
        "reasoning": body.reasoning,
    });

    let row = state
        .session_engine
        .patch_metadata(id, patch)
        .await
        .map_err(|e| GatewayError::SessionNotFound(e.to_string()))?;

    let turns = state
        .turn_repo
        .list_by_session(row.id)
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;
    let aggregate = aggregate_turns(&turns);

    let _ = state.event_tx.send(SessionEvent {
        session_id: row.id.to_string(),
        kind: "session_updated".to_string(),
        payload: json!({
            "session_id": row.id,
            "metadata": row.metadata,
        }),
        state_changed: true,
    });

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

/// `DELETE /sessions/{id}` — delete session and transcript history.
pub async fn delete_session(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ActionResponse>, GatewayError> {
    state
        .session_engine
        .delete_session(id)
        .await
        .map_err(|e| GatewayError::SessionNotFound(e.to_string()))?;

    let _ = state.event_tx.send(SessionEvent {
        session_id: id.to_string(),
        kind: "session_deleted".to_string(),
        payload: json!({
            "session_id": id,
        }),
        state_changed: true,
    });

    Ok(Json(ActionResponse {
        success: true,
        message: format!("session {id} deleted"),
    }))
}

// ── Messages ──────────────────────────────────────────────────────────────────

/// Request body for `POST /sessions/{id}/messages`.
#[derive(Deserialize)]
pub struct SendMessageRequest {
    pub content: String,
    pub model: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PatchSessionRequest {
    pub label: Option<String>,
    pub thinking_level: Option<String>,
    pub verbose: Option<bool>,
    pub reasoning: Option<String>,
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
        state_changed: true,
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

fn lane_stats_response(stats: LaneStats) -> LaneStatsResponse {
    LaneStatsResponse {
        main_active: stats.main_active,
        main_capacity: stats.main_capacity,
        subagent_active: stats.subagent_active,
        subagent_capacity: stats.subagent_capacity,
        cron_active: stats.cron_active,
        cron_capacity: stats.cron_capacity,
    }
}

fn skill_to_response(skill: Skill) -> SkillResponse {
    SkillResponse {
        name: skill.name,
        description: skill.description,
        enabled: skill.enabled,
        source_dir: skill.source_dir.display().to_string(),
        binary_path: skill.binary_path.map(|path| path.display().to_string()),
    }
}

fn skill_reload_response(summary: SkillScanSummary) -> SkillReloadResponse {
    SkillReloadResponse {
        success: true,
        discovered: summary.discovered,
        loaded: summary.loaded,
        removed: summary.removed,
    }
}

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

/// Operator-facing summary for a background process handle.
#[derive(Serialize)]
pub struct ProcessResponse {
    pub process_id: String,
    pub running: bool,
    pub exit_code: Option<i32>,
    pub live: bool,
    pub durable_status: Option<String>,
    pub persisted: Option<PersistedProcessResponse>,
    pub note: Option<String>,
}

/// Restart-visible persisted metadata for a background process handle.
#[derive(Serialize)]
pub struct PersistedProcessResponse {
    pub tool_call_id: String,
    pub tool_execution_id: String,
    pub command: String,
    pub workdir: String,
    pub started_at: String,
    pub ended_at: Option<String>,
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

fn persisted_process_to_response(persisted: PersistedProcessInfo) -> PersistedProcessResponse {
    PersistedProcessResponse {
        tool_call_id: persisted.tool_call_id,
        tool_execution_id: persisted.tool_execution_id,
        command: persisted.command,
        workdir: persisted.workdir,
        started_at: persisted.started_at,
        ended_at: persisted.ended_at,
    }
}

fn process_to_response(process: ProcessInfo) -> ProcessResponse {
    ProcessResponse {
        process_id: process.process_id,
        running: process.running,
        exit_code: process.exit_code,
        live: process.live,
        durable_status: process.durable_status,
        persisted: process.persisted.map(persisted_process_to_response),
        note: process.note,
    }
}

#[derive(Deserialize)]
pub struct ProcessLogQuery {
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

/// `GET /processes` — list live and restart-visible background process handles.
pub async fn list_processes(
    State(state): State<AppState>,
) -> Result<Json<Vec<ProcessResponse>>, GatewayError> {
    let processes = state.process_manager.list().await;
    Ok(Json(
        processes.into_iter().map(process_to_response).collect(),
    ))
}

/// `GET /processes/{id}` — inspect a single background process handle.
pub async fn get_process(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ProcessResponse>, GatewayError> {
    let process = state.process_manager.poll(&id).await.map_err(|e| match e {
        rune_tools::ToolError::ExecutionFailed(message)
            if message.contains("process not found") =>
        {
            GatewayError::BadRequest(message)
        }
        other => GatewayError::Internal(other.to_string()),
    })?;

    Ok(Json(process_to_response(process)))
}

/// `GET /processes/{id}/log` — fetch process log output or persisted post-restart metadata.
pub async fn get_process_log(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<ProcessLogQuery>,
) -> Result<Response, GatewayError> {
    let output = state
        .process_manager
        .log(&id, query.offset, query.limit)
        .await
        .map_err(|e| match e {
            rune_tools::ToolError::ExecutionFailed(message)
                if message.contains("process not found") =>
            {
                GatewayError::BadRequest(message)
            }
            other => GatewayError::Internal(other.to_string()),
        })?;

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        output,
    )
        .into_response())
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

// ── Skills ─────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SkillResponse {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub source_dir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binary_path: Option<String>,
}

#[derive(Serialize)]
pub struct SkillReloadResponse {
    pub success: bool,
    pub discovered: usize,
    pub loaded: usize,
    pub removed: usize,
}

pub async fn list_skills(
    State(state): State<AppState>,
) -> Result<Json<Vec<SkillResponse>>, GatewayError> {
    let mut skills = state.skill_registry.list().await;
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Json(skills.into_iter().map(skill_to_response).collect()))
}

pub async fn reload_skills(
    State(state): State<AppState>,
) -> Result<Json<SkillReloadResponse>, GatewayError> {
    let summary = state.skill_loader.scan_summary().await;
    Ok(Json(skill_reload_response(summary)))
}

pub async fn enable_skill(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<ActionResponse>, GatewayError> {
    if state.skill_registry.enable(&name).await {
        Ok(Json(ActionResponse {
            success: true,
            message: format!("skill '{name}' enabled"),
        }))
    } else {
        Err(GatewayError::BadRequest(format!("unknown skill: {name}")))
    }
}

pub async fn disable_skill(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<ActionResponse>, GatewayError> {
    if state.skill_registry.disable(&name).await {
        Ok(Json(ActionResponse {
            success: true,
            message: format!("skill '{name}' disabled"),
        }))
    } else {
        Err(GatewayError::BadRequest(format!("unknown skill: {name}")))
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
        state_changed: true,
    });

    Ok(StatusCode::OK)
}

// ── Models ────────────────────────────────────────────────────────────────────

/// `GET /models` — list all configured models across all providers.
pub async fn list_models(
    State(state): State<AppState>,
) -> Result<Json<Vec<DashboardModelItem>>, GatewayError> {
    // Reuse the same logic as dashboard_models.
    dashboard_models(State(state)).await
}

/// Response for the Ollama scan endpoint.
#[derive(Serialize)]
pub struct ScanModelsResponse {
    pub provider: String,
    pub models: Vec<ScannedModel>,
}

/// A single discovered model from a local provider.
#[derive(Serialize)]
pub struct ScannedModel {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<String>,
}

/// `POST /models/scan` — discover models from local providers (e.g. Ollama).
///
/// Scans any configured Ollama provider by calling `GET /api/tags` on its
/// native API endpoint. Returns the list of locally available models.
pub async fn scan_models(
    State(state): State<AppState>,
) -> Result<Json<Vec<ScanModelsResponse>>, GatewayError> {
    let mut results = Vec::new();

    for provider_cfg in &state.config.models.providers {
        let kind = if provider_cfg.kind.is_empty() {
            provider_cfg.name.as_str()
        } else {
            provider_cfg.kind.as_str()
        };

        if kind.to_lowercase() != "ollama" {
            continue;
        }

        let ollama_base = if provider_cfg.base_url.is_empty() {
            "http://localhost:11434".to_string()
        } else {
            provider_cfg
                .base_url
                .trim_end_matches('/')
                .strip_suffix("/v1")
                .unwrap_or(&provider_cfg.base_url)
                .to_string()
        };

        let url = format!("{ollama_base}/api/tags");
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_default();

        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                #[derive(serde::Deserialize)]
                struct OllamaTagsResponse {
                    models: Vec<OllamaModelEntry>,
                }
                #[derive(serde::Deserialize)]
                struct OllamaModelEntry {
                    name: String,
                    #[serde(default)]
                    size: u64,
                    #[serde(default)]
                    modified_at: String,
                }

                if let Ok(tags) = resp.json::<OllamaTagsResponse>().await {
                    results.push(ScanModelsResponse {
                        provider: provider_cfg.name.clone(),
                        models: tags
                            .models
                            .into_iter()
                            .map(|m| ScannedModel {
                                name: m.name,
                                size: if m.size > 0 { Some(m.size) } else { None },
                                modified_at: if m.modified_at.is_empty() {
                                    None
                                } else {
                                    Some(m.modified_at)
                                },
                            })
                            .collect(),
                    });
                }
            }
            Ok(resp) => {
                let status = resp.status().as_u16();
                return Err(GatewayError::Internal(format!(
                    "Ollama /api/tags returned HTTP {status} for provider '{}'",
                    provider_cfg.name
                )));
            }
            Err(e) => {
                return Err(GatewayError::Internal(format!(
                    "failed to reach Ollama at {url} for provider '{}': {e}",
                    provider_cfg.name
                )));
            }
        }
    }

    Ok(Json(results))
}

// ── Device Pairing ──────────────────────────────────────────────────────────

/// Request body for `POST /devices/pair/request`.
#[derive(Deserialize)]
pub struct PairRequestBody {
    pub device_name: String,
    pub public_key: String,
}

/// `POST /devices/pair/request` — initiate a new device pairing.
///
/// The device supplies its name and Ed25519 public key (hex-encoded).
/// Returns a [`PairingRequest`] containing a random challenge nonce that
/// the device must sign with its private key.
pub async fn device_pair_request(
    State(state): State<AppState>,
    Json(body): Json<PairRequestBody>,
) -> Result<Json<PairingRequest>, GatewayError> {
    let req = state
        .device_registry
        .request_pairing(body.device_name, body.public_key)
        .await
        .map_err(pairing_err)?;

    Ok(Json(req))
}

/// Request body for `POST /devices/pair/approve`.
#[derive(Deserialize)]
pub struct PairApproveBody {
    pub request_id: Uuid,
    pub challenge_response: String,
}

/// `POST /devices/pair/approve` — approve a pending pairing request.
///
/// The caller supplies the request ID and the Ed25519 signature of the
/// challenge nonce (hex-encoded).  On success the response contains the
/// newly paired device **including the full bearer token**.
pub async fn device_pair_approve(
    State(state): State<AppState>,
    Json(body): Json<PairApproveBody>,
) -> Result<Json<PairedDevice>, GatewayError> {
    let device = state
        .device_registry
        .approve_pairing(body.request_id, body.challenge_response)
        .await
        .map_err(pairing_err)?;

    Ok(Json(device))
}

/// Request body for `POST /devices/pair/reject`.
#[derive(Deserialize)]
pub struct PairRejectBody {
    pub request_id: Uuid,
}

/// `POST /devices/pair/reject` — reject and discard a pending pairing request.
pub async fn device_pair_reject(
    State(state): State<AppState>,
    Json(body): Json<PairRejectBody>,
) -> Result<Json<ActionResponse>, GatewayError> {
    state
        .device_registry
        .reject_pairing(body.request_id)
        .await
        .map_err(pairing_err)?;

    Ok(Json(ActionResponse {
        success: true,
        message: format!("pairing request {} rejected", body.request_id),
    }))
}

/// `GET /devices/pair/pending` — list all pending pairing requests.
pub async fn device_pair_pending(
    State(state): State<AppState>,
) -> Result<Json<Vec<PairingRequest>>, GatewayError> {
    let pending = state.device_registry.list_pending().await;
    Ok(Json(pending))
}

/// Response type for device listings; masks the token field.
#[derive(Serialize)]
pub struct DeviceListEntry {
    pub id: Uuid,
    pub name: String,
    pub public_key: String,
    pub role: crate::pairing::DeviceRole,
    pub scopes: Vec<String>,
    /// Masked token — only the first 8 characters are shown.
    pub token_prefix: String,
    pub token_expires_at: chrono::DateTime<chrono::Utc>,
    pub paired_at: chrono::DateTime<chrono::Utc>,
    pub last_seen_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<PairedDevice> for DeviceListEntry {
    fn from(d: PairedDevice) -> Self {
        let token_prefix = if d.token.len() >= 8 {
            format!("{}...", &d.token[..8])
        } else {
            "***".to_string()
        };
        Self {
            id: d.id,
            name: d.name,
            public_key: d.public_key,
            role: d.role,
            scopes: d.scopes,
            token_prefix,
            token_expires_at: d.token_expires_at,
            paired_at: d.paired_at,
            last_seen_at: d.last_seen_at,
        }
    }
}

/// `GET /devices` — list all paired devices with masked tokens.
pub async fn device_list(
    State(state): State<AppState>,
) -> Result<Json<Vec<DeviceListEntry>>, GatewayError> {
    let devices = state.device_registry.list_devices().await;
    let entries: Vec<DeviceListEntry> = devices.into_iter().map(DeviceListEntry::from).collect();
    Ok(Json(entries))
}

/// `DELETE /devices/{id}` — revoke a paired device.
pub async fn device_revoke(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ActionResponse>, GatewayError> {
    state
        .device_registry
        .revoke_device(id)
        .await
        .map_err(pairing_err)?;

    Ok(Json(ActionResponse {
        success: true,
        message: format!("device {id} revoked"),
    }))
}

/// `POST /devices/{id}/rotate-token` — rotate the bearer token for a device.
///
/// Returns the updated device **including the new full token**.
pub async fn device_rotate_token(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<PairedDevice>, GatewayError> {
    let device = state
        .device_registry
        .rotate_token(id)
        .await
        .map_err(pairing_err)?;

    Ok(Json(device))
}

/// Map [`PairingError`] variants to appropriate [`GatewayError`] variants.
fn pairing_err(e: PairingError) -> GatewayError {
    match &e {
        PairingError::RequestNotFound(_) | PairingError::DeviceNotFound(_) => {
            GatewayError::BadRequest(e.to_string())
        }
        PairingError::RequestExpired(_) => GatewayError::BadRequest(e.to_string()),
        PairingError::InvalidPublicKey(_) | PairingError::InvalidSignature(_) => {
            GatewayError::BadRequest(e.to_string())
        }
        PairingError::VerificationFailed => GatewayError::Unauthorized,
    }
}
