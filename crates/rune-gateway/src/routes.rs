//! HTTP route handlers for the gateway API.

use std::sync::Arc;
use std::time::Instant;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use tracing::info;
use uuid::Uuid;

use rune_core::{JobId, SchedulerDeliveryMode, SchedulerRunTrigger, SessionKind};
use rune_runtime::heartbeat::HeartbeatState;
use rune_runtime::scheduler::{
    Job, JobPayload, JobRun, JobRunStatus, JobUpdate, Reminder, ReminderStatus, Schedule,
    SessionTarget, compute_initial_next_run,
};
use rune_runtime::{LaneStats, Skill, SkillScanSummary};
use rune_store::models::{SessionRow, TurnRow};
use rune_tools::memory_tool::MemoryToolExecutor;
use rune_tools::process_tool::{PersistedProcessInfo, ProcessInfo};
use rune_tools::{ToolCall, ToolExecutor};
use serde_json::Value;

use crate::error::GatewayError;
use crate::events::{ApprovalEvent, RuntimeEvent, broadcast_runtime_event};
use crate::ms365::{
    CreateCalendarEventRequest, CreatePlannerTaskRequest, CreateTodoTaskRequest, FileItem,
    FileMetadata, FileSearchItem, ForwardMailRequest, Ms365CalendarServiceError,
    Ms365FilesServiceError, Ms365MailServiceError, Ms365PlannerServiceError, Ms365TodoServiceError,
    Ms365UsersServiceError, PlannerTask, ReplyMailRequest, RespondCalendarEventRequest,
    SendMailRequest, TodoTask, UpdateCalendarEventRequest, UpdatePlannerTaskRequest,
    UpdateTodoTaskRequest, UserProfile, UserSummary,
};
use crate::pairing::{DeviceRole, PairingError, PairingRequest, StoredPairedDevice};
use crate::state::{AppState, SessionEvent};
use crate::ws::active_ws_connections;
use crate::{SupervisorDeps, run_job_lifecycle};

// ── Health & Status ───────────────────────────────────────────────────────────

/// Response for `GET /health` and `GET /ready`.
#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub service: &'static str,
    pub version: &'static str,
    pub uptime_seconds: u64,
    pub session_count: usize,
    pub ws_subscribers: usize,
    pub ws_connections: usize,
    pub mode: &'static str,
    pub storage_backend: String,
}

#[derive(Serialize)]
pub struct ReadinessResponse {
    pub status: &'static str,
    pub service: &'static str,
    pub version: &'static str,
    pub mode: &'static str,
    pub storage_backend: String,
    pub checks: Vec<DoctorCheck>,
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
        ws_connections: active_ws_connections(),
        mode: state.capabilities.mode.as_str(),
        storage_backend: state.capabilities.storage_backend.clone(),
    }))
}

/// Readiness check for startup/service probes.
pub async fn ready(State(state): State<AppState>) -> Result<Response, GatewayError> {
    let config = state.config.read().await;
    let checks = readiness_checks(&config);
    let failing = checks.iter().any(|check| check.status == "fail");
    let status = if failing { "degraded" } else { "ok" };
    let body = ReadinessResponse {
        status,
        service: "rune-gateway",
        version: env!("CARGO_PKG_VERSION"),
        mode: state.capabilities.mode.as_str(),
        storage_backend: state.capabilities.storage_backend.clone(),
        checks,
    };
    let code = if failing {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };
    Ok((code, Json(body)).into_response())
}

/// Response for `GET /status`.
#[derive(Serialize)]
pub struct StatusResponse {
    pub status: &'static str,
    pub version: &'static str,
    pub bind: String,
    pub auth_enabled: bool,
    pub configured_model_providers: usize,
    pub active_model_backend: String,
    pub registered_tools: usize,
    pub session_count: usize,
    pub cron_job_count: usize,
    pub ws_subscribers: usize,
    pub ws_connections: usize,
    pub uptime_seconds: u64,
    pub lane_stats: Option<LaneStatsResponse>,
    pub skills: SkillStatusResponse,
    pub config_paths: StatusPaths,
    pub capabilities: CapabilitiesResponse,
}

#[derive(Serialize)]
pub struct UpdateCheckResponse {
    pub available: bool,
    pub current_version: String,
    pub latest_version: Option<String>,
    pub detail: String,
}

#[derive(Serialize)]
pub struct UpdateApplyResponse {
    pub success: bool,
    pub detail: String,
}

#[derive(Serialize)]
pub struct UpdateStatusResponse {
    pub current_version: String,
    pub detail: String,
}

#[derive(Serialize)]
pub struct CapabilitiesResponse {
    pub mode: &'static str,
    pub storage_backend: String,
    pub pgvector: bool,
    pub memory_mode: String,
    pub browser: bool,
    pub mcp_servers: usize,
    pub tts: bool,
    pub stt: bool,
    pub channels: Vec<String>,
    pub approval_mode: String,
    pub security_posture: String,
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
    let config = state.config.read().await;

    Ok(Json(StatusResponse {
        status: "running",
        version: env!("CARGO_PKG_VERSION"),
        bind: format!("{}:{}", config.gateway.host, config.gateway.port),
        auth_enabled: config.gateway.auth_token.is_some(),
        configured_model_providers: config.models.providers.len(),
        active_model_backend: if config.models.providers.is_empty() {
            config
                .models
                .zero_config_ollama_base_url(std::env::var("OLLAMA_HOST").ok().as_deref())
                .map(|_| "zero-config-ollama".to_string())
                .unwrap_or_else(|| "demo-echo".to_string())
        } else {
            "configured-provider".to_string()
        },
        registered_tools: state.capabilities.tool_count,
        session_count: sessions.len(),
        cron_job_count,
        ws_subscribers: state.event_tx.receiver_count(),
        ws_connections: active_ws_connections(),
        uptime_seconds: state.started_at.elapsed().as_secs(),
        lane_stats,
        skills: SkillStatusResponse {
            loaded: skills.len(),
            enabled: skills.iter().filter(|skill| skill.enabled).count(),
            skills_dir: state.skill_loader.skills_dir().display().to_string(),
        },
        config_paths: StatusPaths {
            sessions_dir: config.paths.sessions_dir.display().to_string(),
            memory_dir: config.paths.memory_dir.display().to_string(),
            logs_dir: config.paths.logs_dir.display().to_string(),
        },
        capabilities: CapabilitiesResponse {
            mode: state.capabilities.mode.as_str(),
            storage_backend: state.capabilities.storage_backend.clone(),
            pgvector: state.capabilities.pgvector,
            memory_mode: state.capabilities.memory_mode.clone(),
            browser: state.capabilities.browser,
            mcp_servers: state.capabilities.mcp_servers,
            tts: state.capabilities.tts,
            stt: state.capabilities.stt,
            channels: state.capabilities.channels.clone(),
            approval_mode: state.capabilities.approval_mode.clone(),
            security_posture: state.capabilities.security_posture.clone(),
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

// SPA serving — runtime UI dist lookup so cargo check works even when ui/dist is absent.

pub async fn spa_index() -> Response {
    spa_response_for_path("")
}

pub async fn spa_handler(headers: axum::http::HeaderMap, uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // If the request is for an API path or doesn't accept HTML, return 404
    // so it doesn't accidentally serve the SPA for missing API endpoints.
    let accepts_html = headers
        .get(axum::http::header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("text/html"))
        .unwrap_or(false);

    if !accepts_html && !path.is_empty() {
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    }

    spa_response_for_path(path)
}

fn spa_response_for_path(path: &str) -> Response {
    let dist_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/dist");
    let requested = if path.is_empty() { "index.html" } else { path };
    let requested_path = dist_root.join(requested);

    if requested_path.is_file() {
        return file_response(requested_path, requested);
    }

    let index_path = dist_root.join("index.html");
    if index_path.is_file() {
        return file_response(index_path, "index.html");
    }

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        minimal_spa_html(),
    )
        .into_response()
}

fn minimal_spa_html() -> String {
    r#"<!doctype html><html><head><meta charset="utf-8"><title>Rune Admin</title><link rel="icon" href="/favicon"></head><body><div id="root">Rune UI not built yet.</div></body></html>"#.to_string()
}

fn file_response(path: std::path::PathBuf, request_path: &str) -> Response {
    match std::fs::read(&path) {
        Ok(bytes) => {
            let content_type = match request_path.rsplit('.').next() {
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
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, content_type)],
                bytes,
            )
                .into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "UI asset missing").into_response(),
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
    let config = state.config.read().await;

    Ok(Json(DashboardSummaryResponse {
        gateway_status: "running",
        bind: format!("{}:{}", config.gateway.host, config.gateway.port),
        uptime_seconds: state.started_at.elapsed().as_secs(),
        default_model: resolved_default_model(&config),
        provider_count: config.models.providers.len(),
        configured_model_count: config.models.inventory().len(),
        session_count: sessions.len(),
        auth_enabled: config.gateway.auth_token.is_some(),
        ws_subscribers: state.event_tx.receiver_count(),
        channels: configured_channels(&config),
    }))
}

pub async fn dashboard_models(
    State(state): State<AppState>,
) -> Result<Json<Vec<DashboardModelItem>>, GatewayError> {
    let config = state.config.read().await;
    let default_model = resolved_default_model(&config);
    let mut items = config
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
    let config = state.config.read().await;
    let mut items = Vec::new();
    let now = Utc::now().to_rfc3339();

    if config.models.providers.is_empty() {
        items.push(DashboardDiagnosticItem {
            level: "warn",
            source: "models",
            message: config
                .models
                .zero_config_ollama_base_url(std::env::var("OLLAMA_HOST").ok().as_deref())
                .map(|base| format!("No explicit model providers configured; zero-config Ollama auto-detect is active ({base})."))
                .unwrap_or_else(|| "No model providers configured; gateway is using the demo echo backend.".to_string()),
            observed_at: now.clone(),
        });
    }

    if configured_channels(&config).is_empty() {
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
    #[serde(rename = "includeDisabled", alias = "include_disabled")]
    pub include_disabled: Option<bool>,
}

#[derive(Deserialize)]
pub struct SessionsListQuery {
    #[serde(rename = "active")]
    pub active_minutes: Option<u64>,
    pub channel: Option<String>,
    pub kind: Option<String>,
    /// Filter by parent/requester session ID (returns children of this session).
    pub parent: Option<Uuid>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct CronWakeRequest {
    pub text: String,
    pub mode: Option<String>,
    #[serde(rename = "contextMessages", alias = "context_messages")]
    pub context_messages: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct CronJobRequest {
    pub name: Option<String>,
    pub schedule: CronScheduleRequest,
    pub payload: CronPayloadRequest,
    #[serde(rename = "sessionTarget", alias = "session_target")]
    pub session_target: String,
    #[serde(default, rename = "deliveryMode", alias = "delivery_mode")]
    pub delivery_mode: Option<SchedulerDeliveryMode>,
    #[serde(default, rename = "webhookUrl", alias = "webhook_url")]
    pub webhook_url: Option<String>,
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
    #[serde(default, rename = "deliveryMode", alias = "delivery_mode")]
    pub delivery_mode: Option<SchedulerDeliveryMode>,
    #[serde(default, rename = "webhookUrl", alias = "webhook_url")]
    pub webhook_url: Option<String>,
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
    pub delivery_mode: SchedulerDeliveryMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_url: Option<String>,
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
    pub trigger_kind: String,
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

pub async fn cron_get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<CronJobResponse>, GatewayError> {
    let job_id = JobId::from(id);
    let job = state
        .scheduler
        .get_job(&job_id)
        .await
        .ok_or_else(|| GatewayError::JobNotFound(job_id.to_string()))?;
    Ok(Json(job_to_response(job)))
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
    let next_run_at = compute_initial_next_run(&schedule);
    let mut job = Job {
        id,
        name: body.name,
        schedule,
        payload,
        delivery_mode: body.delivery_mode.unwrap_or(SchedulerDeliveryMode::None),
        webhook_url: body.webhook_url,
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
        delivery_mode: body.delivery_mode,
        webhook_url: body.webhook_url,
    };

    state
        .scheduler
        .update_job(&job_id, update)
        .await
        .ok_or_else(|| GatewayError::JobNotFound(job_id.to_string()))?;

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

    let workspace_root = state.config.read().await.agents.defaults.workspace.clone();
    let deps = SupervisorDeps {
        heartbeat: state.heartbeat.clone(),
        scheduler: state.scheduler.clone(),
        reminder_store: state.reminder_store.clone(),
        session_engine: state.session_engine.clone(),
        turn_executor: state.turn_executor.clone(),
        workspace_root,
        device_registry: state.device_registry.clone(),
        event_tx: state.event_tx.clone(),
        operator_delivery: None,
        plugin_scanner: None,
        plugin_scan_interval_ticks: 0,
    };

    let (_status, _output) =
        run_job_lifecycle(&deps, &job, true, SchedulerRunTrigger::Manual).await;

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
    let mode = normalize_wake_mode(body.mode.as_deref())?;

    let _ = state.event_tx.send(SessionEvent {
        session_id: "system".to_string(),
        kind: "wake_event".to_string(),
        payload: json!({
            "text": body.text,
            "mode": mode,
            "context_messages": body.context_messages,
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
    pub kind: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requester_session_id: Option<String>,
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
    let kind_filter = query.kind.as_deref();
    let parent_filter = query.parent;

    let rows = state
        .session_repo
        .list(limit, 0)
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;

    let mut items = Vec::new();
    for row in rows
        .into_iter()
        .filter(|row| {
            kind_filter
                .map(|kind| row.kind.eq_ignore_ascii_case(kind))
                .unwrap_or(true)
        })
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
        .filter(|row| {
            parent_filter
                .map(|parent_id| row.requester_session_id == Some(parent_id))
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
            kind: row.kind,
            status: row.status,
            requester_session_id: row.requester_session_id.map(|id| id.to_string()),
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
            "approval requests, operator-triggered resume, and restart-safe mid-resume continuation are durable".to_string(),
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

/// A single node in a session delegation tree.
#[derive(Serialize)]
pub struct SessionTreeNode {
    pub id: String,
    pub kind: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    pub created_at: String,
    pub turn_count: u32,
    pub children: Vec<SessionTreeNode>,
}

/// `GET /sessions/{id}/tree` — return the delegation tree rooted at a session.
pub async fn get_session_tree(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<SessionTreeNode>, GatewayError> {
    let root = state
        .session_engine
        .get_session(id)
        .await
        .map_err(|e| GatewayError::SessionNotFound(e.to_string()))?;

    // Fetch all sessions and build the tree in-memory.
    let all_rows = state
        .session_repo
        .list(500, 0)
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;

    // Build parent -> children index.
    let mut children_map: std::collections::HashMap<Uuid, Vec<&rune_store::models::SessionRow>> =
        std::collections::HashMap::new();
    for row in &all_rows {
        if let Some(parent_id) = row.requester_session_id {
            children_map.entry(parent_id).or_default().push(row);
        }
    }

    // Collect all session IDs in the subtree for turn-count lookup.
    fn collect_ids(
        session_id: Uuid,
        children_map: &std::collections::HashMap<Uuid, Vec<&rune_store::models::SessionRow>>,
        out: &mut Vec<Uuid>,
    ) {
        out.push(session_id);
        if let Some(kids) = children_map.get(&session_id) {
            for child in kids {
                collect_ids(child.id, children_map, out);
            }
        }
    }
    let mut subtree_ids = Vec::new();
    collect_ids(root.id, &children_map, &mut subtree_ids);

    // Pre-compute turn counts.
    let mut turn_counts = std::collections::HashMap::new();
    for sid in &subtree_ids {
        let turns = state
            .turn_repo
            .list_by_session(*sid)
            .await
            .map_err(|e| GatewayError::Internal(e.to_string()))?;
        turn_counts.insert(*sid, turns.len() as u32);
    }

    fn build_node(
        row: &rune_store::models::SessionRow,
        children_map: &std::collections::HashMap<Uuid, Vec<&rune_store::models::SessionRow>>,
        turn_counts: &std::collections::HashMap<Uuid, u32>,
    ) -> SessionTreeNode {
        let children = children_map
            .get(&row.id)
            .map(|kids| {
                kids.iter()
                    .map(|child| build_node(child, children_map, turn_counts))
                    .collect()
            })
            .unwrap_or_default();
        SessionTreeNode {
            id: row.id.to_string(),
            kind: row.kind.clone(),
            status: row.status.clone(),
            channel: row.channel_ref.clone(),
            created_at: row.created_at.to_rfc3339(),
            turn_count: turn_counts.get(&row.id).copied().unwrap_or(0),
            children,
        }
    }

    let tree = build_node(&root, &children_map, &turn_counts);
    Ok(Json(tree))
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

// ── Agent (subagent) control ──────────────────────────────────────────────────

/// Request body for `POST /agents/{id}/steer`.
#[derive(Deserialize)]
pub struct AgentSteerRequest {
    pub message: String,
}

/// Response for `POST /agents/{id}/steer`.
#[derive(Serialize)]
pub struct AgentSteerResponse {
    pub session_id: String,
    pub accepted: bool,
    pub detail: String,
}

/// Request body for `POST /agents/{id}/kill`.
#[derive(Deserialize)]
pub struct AgentKillRequest {
    pub reason: Option<String>,
}

/// Response for `POST /agents/{id}/kill`.
#[derive(Serialize)]
pub struct AgentKillResponse {
    pub session_id: String,
    pub killed: bool,
    pub detail: String,
}

/// `POST /agents/{id}/steer` — inject a steering instruction into a running subagent.
pub async fn agent_steer(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<AgentSteerRequest>,
) -> Result<Json<AgentSteerResponse>, GatewayError> {
    let session = state
        .session_repo
        .find_by_id(id)
        .await
        .map_err(|_| GatewayError::SessionNotFound(format!("agent session {id} not found")))?;

    let now = chrono::Utc::now();
    let note = format!("[steer] operator instruction injected: {}", body.message);

    // Append a status_note transcript item for auditability.
    state
        .transcript_repo
        .append(rune_store::models::NewTranscriptItem {
            id: Uuid::now_v7(),
            session_id: id,
            turn_id: None,
            seq: 0,
            kind: "status_note".into(),
            payload: json!({ "content": note }),
            created_at: now,
        })
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;

    // Update subagent lifecycle metadata.
    let mut metadata = session.metadata.clone();
    metadata["subagent_lifecycle"] = json!("steered");
    metadata["subagent_runtime_status"] = json!("running");
    metadata["subagent_runtime_attached"] = json!(true);
    metadata["subagent_status_updated_at"] = json!(now.to_rfc3339());
    metadata["subagent_last_note"] = json!(note);

    state
        .session_repo
        .update_metadata(id, metadata, now)
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;

    let _ = state.event_tx.send(SessionEvent {
        session_id: id.to_string(),
        kind: "agent_steered".to_string(),
        payload: json!({
            "session_id": id,
            "message": body.message,
        }),
        state_changed: true,
    });

    Ok(Json(AgentSteerResponse {
        session_id: id.to_string(),
        accepted: true,
        detail: format!("steering instruction delivered to session {}", id),
    }))
}

/// `POST /agents/{id}/kill` — cancel/terminate a running subagent session.
pub async fn agent_kill(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<AgentKillRequest>,
) -> Result<Json<AgentKillResponse>, GatewayError> {
    let session = state
        .session_repo
        .find_by_id(id)
        .await
        .map_err(|_| GatewayError::SessionNotFound(format!("agent session {id} not found")))?;

    let now = chrono::Utc::now();
    let reason = body.reason.as_deref().unwrap_or("operator-initiated");
    let note = format!("[kill] session cancelled: {reason}");

    // Mark session as cancelled.
    state
        .session_repo
        .update_status(id, "cancelled", now)
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;

    // Append a status_note transcript item.
    state
        .transcript_repo
        .append(rune_store::models::NewTranscriptItem {
            id: Uuid::now_v7(),
            session_id: id,
            turn_id: None,
            seq: 0,
            kind: "status_note".into(),
            payload: json!({ "content": note }),
            created_at: now,
        })
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;

    // Update subagent lifecycle metadata.
    let mut metadata = session.metadata.clone();
    metadata["subagent_lifecycle"] = json!("cancelled");
    metadata["subagent_runtime_status"] = json!("stopped");
    metadata["subagent_runtime_attached"] = json!(false);
    metadata["subagent_status_updated_at"] = json!(now.to_rfc3339());
    metadata["subagent_last_note"] = json!(note);

    state
        .session_repo
        .update_metadata(id, metadata, now)
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;

    let _ = state.event_tx.send(SessionEvent {
        session_id: id.to_string(),
        kind: "agent_killed".to_string(),
        payload: json!({
            "session_id": id,
            "reason": reason,
        }),
        state_changed: true,
    });

    Ok(Json(AgentKillResponse {
        session_id: id.to_string(),
        killed: true,
        detail: format!("session {} cancelled: {reason}", id),
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

fn normalize_wake_mode(mode: Option<&str>) -> Result<String, GatewayError> {
    match mode
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .as_deref()
    {
        None => Ok("next-heartbeat".to_string()),
        Some("next-heartbeat") | Some("next_heartbeat") => Ok("next-heartbeat".to_string()),
        Some("now") => Ok("now".to_string()),
        Some(other) => Err(GatewayError::BadRequest(format!(
            "unknown wake mode: {other}"
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

fn job_to_response(job: Job) -> CronJobResponse {
    CronJobResponse {
        id: job.id.to_string(),
        name: job.name,
        schedule: job.schedule,
        payload: job.payload,
        delivery_mode: job.delivery_mode,
        webhook_url: job.webhook_url,
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
        trigger_kind: run.trigger_kind.to_string(),
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

fn resolved_default_model(config: &rune_config::AppConfig) -> Option<String> {
    config
        .agents
        .default_agent()
        .and_then(|agent| config.agents.effective_model(agent))
        .map(str::to_string)
        .or_else(|| config.models.default_model.clone())
}

fn configured_channels(config: &rune_config::AppConfig) -> Vec<String> {
    let mut channels = config.channels.enabled.clone();
    if config.channels.telegram_token.is_some() && !channels.iter().any(|c| c == "telegram") {
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
    pub handle_ref: Option<String>,
    pub host_ref: Option<String>,
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

    if let Some(session_id) = decided
        .handle_ref
        .as_deref()
        .and_then(|value| Uuid::parse_str(value).ok())
    {
        let _ = broadcast_runtime_event(
            &state.event_tx,
            RuntimeEvent::Approval(ApprovalEvent::Resolved {
                session_id,
                approval_id: decided.id,
                decision: normalised.clone(),
            }),
        );
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
        handle_ref: approval.handle_ref,
        host_ref: approval.host_ref,
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

/// `POST /processes/{id}/kill` — kill a running background process.
pub async fn kill_process(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ActionResponse>, GatewayError> {
    state.process_manager.kill(&id).await.map_err(|e| match e {
        rune_tools::ToolError::ExecutionFailed(message)
            if message.contains("process not found") =>
        {
            GatewayError::BadRequest(message)
        }
        other => GatewayError::Internal(other.to_string()),
    })?;

    Ok(Json(ActionResponse {
        success: true,
        message: format!("process {id} killed"),
    }))
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

/// `POST /heartbeat/interval` — set the heartbeat interval in seconds.
pub async fn heartbeat_set_interval(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<ActionResponse>, GatewayError> {
    let secs = body
        .get("interval_secs")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| GatewayError::BadRequest("interval_secs required".into()))?;
    if secs < 60 {
        return Err(GatewayError::BadRequest(
            "interval_secs must be >= 60".into(),
        ));
    }
    state.heartbeat.set_interval(secs).await;
    Ok(Json(ActionResponse {
        success: true,
        message: format!("heartbeat interval set to {secs}s"),
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
    pub status: ReminderStatus,
    pub delivered: bool,
    pub created_at: String,
    pub delivered_at: Option<String>,
    pub outcome_at: Option<String>,
    pub last_error: Option<String>,
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
        status: r.status,
        delivered: r.delivered,
        created_at: r.created_at.to_rfc3339(),
        delivered_at: r.delivered_at.map(|dt| dt.to_rfc3339()),
        outcome_at: r.outcome_at.map(|dt| dt.to_rfc3339()),
        last_error: r.last_error,
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

pub async fn get_skill(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<SkillResponse>, GatewayError> {
    let skill = state
        .skill_registry
        .get(&name)
        .await
        .ok_or_else(|| GatewayError::SkillNotFound(name.clone()))?;

    Ok(Json(skill_to_response(skill)))
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
        Err(GatewayError::SkillNotFound(name))
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
        Err(GatewayError::SkillNotFound(name))
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
        .read()
        .await
        .channels
        .telegram_token
        .clone()
        .unwrap_or_default();

    if token != expected_token {
        return Err(GatewayError::Unauthorized);
    }

    let mut payload = update.clone();

    if let Some(message) = update.get("message") {
        let media_file = message
            .get("voice")
            .and_then(|v| v.get("file_id"))
            .or_else(|| message.get("audio").and_then(|v| v.get("file_id")))
            .or_else(|| message.get("video_note").and_then(|v| v.get("file_id")))
            .or_else(|| message.get("video").and_then(|v| v.get("file_id")))
            .or_else(|| message.get("animation").and_then(|v| v.get("file_id")));

        let media_mime = message
            .get("voice")
            .and_then(|v| v.get("mime_type"))
            .or_else(|| message.get("audio").and_then(|v| v.get("mime_type")))
            .or_else(|| message.get("video").and_then(|v| v.get("mime_type")))
            .or_else(|| message.get("animation").and_then(|v| v.get("mime_type")))
            .and_then(|v| v.as_str())
            .map(str::to_owned);

        if let Some(file_id) = media_file.and_then(|v| v.as_str()) {
            if let Some(engine_lock) = &state.stt_engine {
                let file_url = format!(
                    "https://api.telegram.org/bot{}/getFile?file_id={}",
                    expected_token, file_id
                );

                let client = reqwest::Client::new();
                if let Ok(resp) = client.get(&file_url).send().await {
                    if let Ok(body) = resp.json::<serde_json::Value>().await {
                        if let Some(file_path) = body
                            .get("result")
                            .and_then(|r| r.get("file_path"))
                            .and_then(|v| v.as_str())
                        {
                            let download_url = format!(
                                "https://api.telegram.org/file/bot{}/{}",
                                expected_token, file_path
                            );
                            if let Ok(file_resp) = client.get(&download_url).send().await {
                                if let Ok(bytes) = file_resp.bytes().await {
                                    let mime_type = media_mime.clone().unwrap_or_else(|| {
                                        if message.get("voice").is_some() {
                                            "audio/ogg".to_string()
                                        } else if message.get("audio").is_some() {
                                            "audio/mpeg".to_string()
                                        } else {
                                            "audio/ogg".to_string()
                                        }
                                    });

                                    let engine = engine_lock.read().await;
                                    if let Ok(result) = engine.transcribe(&bytes, &mime_type).await
                                    {
                                        if let Some(root) = payload.as_object_mut() {
                                            root.insert(
                                                "media_transcription".to_string(),
                                                serde_json::json!({
                                                    "text": result.text,
                                                    "language": result.language,
                                                    "duration_seconds": result.duration_seconds,
                                                    "mime_type": mime_type,
                                                    "file_id": file_id,
                                                    "file_path": file_path,
                                                }),
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Emit the raw update as a session event for observability.
    let _ = state.event_tx.send(crate::state::SessionEvent {
        session_id: "telegram".to_string(),
        kind: "telegram_update".to_string(),
        payload: payload.clone(),
        state_changed: true,
    });

    if let Some(message) = payload.get("message") {
        let chat_id = message
            .get("chat")
            .and_then(|chat| chat.get("id"))
            .and_then(|id| id.as_i64())
            .ok_or_else(|| {
                GatewayError::BadRequest("telegram message missing chat.id".to_string())
            })?;

        let sender = message
            .get("from")
            .and_then(|from| {
                from.get("username")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .or_else(|| from.get("id").map(|v| v.to_string()))
            })
            .unwrap_or_else(|| "unknown".to_string());

        let routing_key = format!("{}:{}", chat_id, sender);

        let session = if let Some(existing) = state
            .session_repo
            .find_by_channel_ref(&routing_key)
            .await
            .map_err(|e| GatewayError::Internal(e.to_string()))?
        {
            existing
        } else {
            let config_guard = state.config.read().await;
            let workspace = config_guard
                .agents
                .default_agent()
                .and_then(|a| config_guard.agents.effective_workspace(a))
                .map(String::from);
            drop(config_guard);

            state
                .session_engine
                .create_session_full(
                    rune_core::SessionKind::Channel,
                    workspace,
                    None,
                    Some(routing_key.clone()),
                )
                .await
                .map_err(|e| GatewayError::Internal(e.to_string()))?
        };

        let mut content = message
            .get("text")
            .and_then(|v| v.as_str())
            .or_else(|| message.get("caption").and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string();

        if let Some(transcribed) = payload
            .get("media_transcription")
            .and_then(|v| v.get("text"))
            .and_then(|v| v.as_str())
        {
            if content.trim().is_empty() {
                content = transcribed.to_string();
            } else {
                content = format!(
                    "{}

[Voice transcription]
{}",
                    content, transcribed
                );
            }
        }

        if !content.trim().is_empty() {
            let (turn_row, usage) = state
                .turn_executor
                .execute(session.id, &content, None)
                .await
                .map_err(|e| GatewayError::Internal(e.to_string()))?;

            let transcript = state
                .transcript_repo
                .list_by_session(session.id)
                .await
                .map_err(|e| GatewayError::Internal(e.to_string()))?;

            let assistant_reply = transcript
                .iter()
                .rev()
                .find(|t| t.turn_id == Some(turn_row.id) && t.kind == "assistant_message")
                .and_then(|t| t.payload.get("content").and_then(|v| v.as_str()))
                .map(String::from);

            if let Some(reply) = assistant_reply {
                let message_id = message
                    .get("message_id")
                    .and_then(|v| v.as_i64())
                    .map(|id| id.to_string())
                    .unwrap_or_default();
                let client = reqwest::Client::new();
                let send_url =
                    format!("https://api.telegram.org/bot{}/sendMessage", expected_token);
                let mut params = serde_json::json!({
                    "chat_id": chat_id,
                    "text": reply,
                    "parse_mode": "Markdown",
                });
                if let Ok(reply_id) = message_id.parse::<i64>() {
                    params["reply_parameters"] = serde_json::json!({ "message_id": reply_id });
                }
                if let Ok(resp) = client.post(&send_url).json(&params).send().await {
                    if let Ok(body) = resp.json::<serde_json::Value>().await {
                        let markdown_failed = body
                            .get("ok")
                            .and_then(|v| v.as_bool())
                            .map(|ok| !ok)
                            .unwrap_or(false)
                            && body
                                .get("description")
                                .and_then(|v| v.as_str())
                                .map(|d| d.contains("parse entities") || d.contains("can't parse"))
                                .unwrap_or(false);
                        if markdown_failed {
                            params.as_object_mut().unwrap().remove("parse_mode");
                            let _ = client.post(&send_url).json(&params).send().await;
                        }
                    }
                }

                let _ = state.event_tx.send(SessionEvent {
                    session_id: session.id.to_string(),
                    kind: "turn_completed".to_string(),
                    payload: json!({
                        "session_id": session.id,
                        "turn_id": turn_row.id,
                        "assistant_reply": reply,
                        "prompt_tokens": usage.prompt_tokens,
                        "completion_tokens": usage.completion_tokens,
                        "channel": "telegram",
                        "routing_key": routing_key,
                    }),
                    state_changed: true,
                });
            }
        }
    }

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
    let providers = state.config.read().await.models.providers.clone();
    let mut results = Vec::new();

    for provider_cfg in &providers {
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

#[derive(Serialize)]
pub struct PairRequestResponse {
    pub request_id: Uuid,
    pub challenge: String,
    pub expires_at: DateTime<Utc>,
}

/// `POST /devices/pair/request` — initiate a new device pairing.
///
/// The device supplies its name and Ed25519 public key (hex-encoded).
/// Returns a random challenge nonce that the device must sign with its private key.
pub async fn device_pair_request(
    State(state): State<AppState>,
    Json(body): Json<PairRequestBody>,
) -> Result<Json<PairRequestResponse>, GatewayError> {
    let req = state
        .device_registry
        .request_pairing(body.device_name, body.public_key)
        .await
        .map_err(pairing_err)?;

    Ok(Json(PairRequestResponse {
        request_id: req.id,
        challenge: req.challenge,
        expires_at: req.expires_at,
    }))
}

/// Request body for `POST /devices/pair/approve`.
#[derive(Deserialize)]
pub struct PairApproveBody {
    pub request_id: Uuid,
    pub challenge_response: String,
    #[serde(default = "default_device_role")]
    pub role: String,
    #[serde(default = "default_device_scopes")]
    pub scopes: Vec<String>,
}

fn default_device_role() -> String {
    "operator".into()
}

fn default_device_scopes() -> Vec<String> {
    vec![
        "sessions:read".into(),
        "sessions:write".into(),
        "status:read".into(),
    ]
}

#[derive(Serialize)]
pub struct PairApproveResponse {
    pub device_id: Uuid,
    pub name: String,
    pub role: String,
    pub scopes: Vec<String>,
    pub token: String,
    pub token_expires_at: DateTime<Utc>,
}

/// `POST /devices/pair/approve` — approve a pending pairing request.
///
/// The caller supplies the request ID and the Ed25519 signature of the
/// challenge nonce (hex-encoded).  On success the response contains the
/// newly paired device **including the full bearer token**.
pub async fn device_pair_approve(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PairApproveBody>,
) -> Result<Json<PairApproveResponse>, GatewayError> {
    let config = state.config.read().await;
    require_gateway_operator_token(&headers, &config)?;
    drop(config);
    let role = DeviceRole::parse(&body.role);
    let device = state
        .device_registry
        .approve_pairing(
            body.request_id,
            body.challenge_response,
            Some(role.clone()),
            Some(body.scopes.clone()),
        )
        .await
        .map_err(pairing_err)?;

    Ok(Json(PairApproveResponse {
        device_id: device.id,
        name: device.name,
        role: role.as_str().to_string(),
        scopes: body.scopes,
        token: device.token,
        token_expires_at: device.token_expires_at,
    }))
}

/// Request body for `POST /devices/pair/reject`.
#[derive(Deserialize)]
pub struct PairRejectBody {
    pub request_id: Uuid,
}

/// `POST /devices/pair/reject` — reject and discard a pending pairing request.
#[derive(Serialize)]
pub struct PairRejectResponse {
    pub rejected: bool,
}

pub async fn device_pair_reject(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PairRejectBody>,
) -> Result<Json<PairRejectResponse>, GatewayError> {
    let config = state.config.read().await;
    require_gateway_operator_token(&headers, &config)?;
    drop(config);
    state
        .device_registry
        .reject_pairing(body.request_id)
        .await
        .map_err(pairing_err)?;

    Ok(Json(PairRejectResponse { rejected: true }))
}

/// `GET /devices/pair/pending` — list all pending pairing requests.
pub async fn device_pair_pending(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<PairingRequest>>, GatewayError> {
    let config = state.config.read().await;
    require_gateway_operator_token(&headers, &config)?;
    drop(config);
    let pending = state
        .device_registry
        .list_pending()
        .await
        .map_err(pairing_err)?;
    Ok(Json(pending))
}

#[derive(Serialize)]
pub struct PendingRequestEntry {
    pub id: Uuid,
    pub device_name: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// Response type for device listings; masks the token field.
#[derive(Serialize)]
pub struct DeviceListEntry {
    pub id: Uuid,
    pub name: String,
    pub public_key: String,
    pub role: String,
    pub scopes: Vec<String>,
    pub token_masked: String,
    pub token_expires_at: chrono::DateTime<chrono::Utc>,
    pub paired_at: chrono::DateTime<chrono::Utc>,
    pub last_seen_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Serialize)]
pub struct DeviceListResponse {
    pub devices: Vec<DeviceListEntry>,
    pub pending_requests: Vec<PendingRequestEntry>,
}

impl From<StoredPairedDevice> for DeviceListEntry {
    fn from(d: StoredPairedDevice) -> Self {
        let prefix_len = d.token_hash.len().min(6);
        let token_masked = format!("{}****", &d.token_hash[..prefix_len]);
        Self {
            id: d.id,
            name: d.name,
            public_key: d.public_key,
            role: d.role.as_str().to_string(),
            scopes: d.scopes,
            token_masked,
            token_expires_at: d.token_expires_at,
            paired_at: d.paired_at,
            last_seen_at: d.last_seen_at,
        }
    }
}

/// `GET /devices` — list all paired devices with masked tokens.
pub async fn device_list(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<DeviceListResponse>, GatewayError> {
    let config = state.config.read().await;
    require_gateway_operator_token(&headers, &config)?;
    drop(config);
    let devices = state
        .device_registry
        .list_devices()
        .await
        .map_err(pairing_err)?;
    let pending = state
        .device_registry
        .list_pending()
        .await
        .map_err(pairing_err)?;
    Ok(Json(DeviceListResponse {
        devices: devices.into_iter().map(DeviceListEntry::from).collect(),
        pending_requests: pending
            .into_iter()
            .map(|request| PendingRequestEntry {
                id: request.id,
                device_name: request.device_name,
                created_at: request.created_at,
                expires_at: request.expires_at,
            })
            .collect(),
    }))
}

#[derive(Serialize)]
pub struct DeviceDeleteResponse {
    pub deleted: bool,
}

/// `DELETE /devices/{id}` — revoke a paired device.
pub async fn device_revoke(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<DeviceDeleteResponse>, GatewayError> {
    let config = state.config.read().await;
    require_gateway_operator_token(&headers, &config)?;
    drop(config);
    state
        .device_registry
        .revoke_device(id)
        .await
        .map_err(pairing_err)?;

    Ok(Json(DeviceDeleteResponse { deleted: true }))
}

/// `POST /devices/{id}/rotate-token` — rotate the bearer token for a device.
///
/// Returns the updated device **including the new full token**.
#[derive(Serialize)]
pub struct TokenRotateResponse {
    pub device_id: Uuid,
    pub token: String,
    pub token_expires_at: DateTime<Utc>,
}

pub async fn device_rotate_token(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<TokenRotateResponse>, GatewayError> {
    let config = state.config.read().await;
    require_gateway_operator_token(&headers, &config)?;
    drop(config);
    let device = state
        .device_registry
        .rotate_token(id)
        .await
        .map_err(pairing_err)?;

    Ok(Json(TokenRotateResponse {
        device_id: device.id,
        token: device.token,
        token_expires_at: device.token_expires_at,
    }))
}

/// Map [`PairingError`] variants to appropriate [`GatewayError`] variants.
fn pairing_err(e: PairingError) -> GatewayError {
    match &e {
        PairingError::RequestNotFound(_)
        | PairingError::DeviceNotFound(_)
        | PairingError::RequestExpired(_)
        | PairingError::InvalidPublicKey(_)
        | PairingError::InvalidSignature(_)
        | PairingError::EmptyDeviceName
        | PairingError::DuplicatePublicKey => GatewayError::BadRequest(e.to_string()),
        PairingError::VerificationFailed => GatewayError::BadRequest(e.to_string()),
        PairingError::Store(_) => GatewayError::Internal(e.to_string()),
    }
}

fn require_gateway_operator_token(
    headers: &HeaderMap,
    config: &rune_config::AppConfig,
) -> Result<(), GatewayError> {
    let expected = config.gateway.auth_token.as_deref().ok_or_else(|| {
        GatewayError::Forbidden("device management requires gateway operator auth".to_string())
    })?;

    let Some(token) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    else {
        return Err(GatewayError::Unauthorized);
    };

    if token == expected {
        Ok(())
    } else {
        Err(GatewayError::Unauthorized)
    }
}

// ── TTS Routes ──────────────────────────────────────────────────────────────

/// Response for `GET /tts/status`.
#[derive(Serialize)]
pub struct TtsStatusResponse {
    pub available: bool,
    pub enabled: bool,
    pub provider: String,
    pub voice: String,
    pub model: String,
    pub auto_mode: String,
    pub voices: Vec<TtsVoiceEntry>,
}

#[derive(Serialize)]
pub struct TtsVoiceEntry {
    pub id: String,
    pub name: String,
    pub language: Option<String>,
}

pub async fn tts_status(
    State(state): State<AppState>,
) -> Result<Json<TtsStatusResponse>, GatewayError> {
    let Some(ref engine_lock) = state.tts_engine else {
        let config = state.config.read().await;
        return Ok(Json(TtsStatusResponse {
            available: false,
            enabled: false,
            provider: config.media.tts.provider.clone(),
            voice: config.media.tts.voice.clone(),
            model: config.media.tts.model.clone(),
            auto_mode: format!("{:?}", config.media.tts.auto_mode).to_lowercase(),
            voices: vec![],
        }));
    };

    let engine = engine_lock.read().await;
    let voices = engine
        .available_voices()
        .into_iter()
        .map(|v| TtsVoiceEntry {
            id: v.id,
            name: v.name,
            language: v.language,
        })
        .collect();
    let cfg = engine.config();
    Ok(Json(TtsStatusResponse {
        available: true,
        enabled: engine.is_enabled(),
        provider: cfg.provider.clone(),
        voice: cfg.voice.clone(),
        model: cfg.model.clone(),
        auto_mode: format!("{:?}", cfg.auto_mode).to_lowercase(),
        voices,
    }))
}

#[derive(Deserialize)]
pub struct TtsSynthesizeRequest {
    pub text: String,
    pub voice: Option<String>,
    pub model: Option<String>,
    pub channel: Option<String>,
    pub chat_id: Option<String>,
    pub reply_to: Option<String>,
    pub as_voice: Option<bool>,
}

pub async fn tts_synthesize(
    State(state): State<AppState>,
    Json(body): Json<TtsSynthesizeRequest>,
) -> Result<Response, GatewayError> {
    let engine_lock = state
        .tts_engine
        .as_ref()
        .ok_or_else(|| GatewayError::BadRequest("TTS engine not configured".to_string()))?;

    let engine = engine_lock.read().await;
    let audio = match (body.voice.as_deref(), body.model.as_deref()) {
        (Some(voice), Some(model)) => engine.convert_with(&body.text, voice, model).await,
        _ => engine.convert(&body.text).await,
    }
    .map_err(|e| GatewayError::Internal(e.to_string()))?;

    if body.channel.as_deref() == Some("telegram") {
        let chat_id = body.chat_id.as_deref().ok_or_else(|| {
            GatewayError::BadRequest("chat_id is required for Telegram TTS delivery".to_string())
        })?;
        let bot_token = state
            .config
            .read()
            .await
            .channels
            .telegram_token
            .clone()
            .ok_or_else(|| GatewayError::BadRequest("Telegram not configured".to_string()))?;
        let adapter: rune_channels::TelegramAdapter =
            crate::telegram_adapter_from_token(&bot_token);
        let receipt = adapter
            .send_audio_bytes(
                chat_id,
                &audio,
                body.reply_to.as_deref(),
                body.as_voice.unwrap_or(true),
                None,
            )
            .await
            .map_err(|e| GatewayError::Internal(e.to_string()))?;

        return Ok(Json(json!({
            "delivered": true,
            "provider_message_id": receipt.provider_message_id,
            "bytes": audio.len(),
            "channel": "telegram"
        }))
        .into_response());
    }

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "audio/mpeg")],
        audio,
    )
        .into_response())
}

pub async fn tts_enable(State(state): State<AppState>) -> Result<Json<Value>, GatewayError> {
    let engine_lock = state
        .tts_engine
        .as_ref()
        .ok_or_else(|| GatewayError::BadRequest("TTS engine not configured".to_string()))?;
    engine_lock.write().await.enable();
    Ok(Json(json!({ "enabled": true })))
}

pub async fn tts_disable(State(state): State<AppState>) -> Result<Json<Value>, GatewayError> {
    let engine_lock = state
        .tts_engine
        .as_ref()
        .ok_or_else(|| GatewayError::BadRequest("TTS engine not configured".to_string()))?;
    engine_lock.write().await.disable();
    Ok(Json(json!({ "enabled": false })))
}

// ── STT Routes ──────────────────────────────────────────────────────────────

/// Response for `GET /stt/status`.
#[derive(Serialize)]
pub struct SttStatusResponse {
    pub available: bool,
    pub enabled: bool,
    pub provider: String,
    pub model: String,
}

pub async fn stt_status(
    State(state): State<AppState>,
) -> Result<Json<SttStatusResponse>, GatewayError> {
    let Some(ref engine_lock) = state.stt_engine else {
        let config = state.config.read().await;
        return Ok(Json(SttStatusResponse {
            available: false,
            enabled: false,
            provider: config.media.stt.provider.clone(),
            model: config.media.stt.model.clone(),
        }));
    };

    let engine = engine_lock.read().await;
    let cfg = engine.config();
    Ok(Json(SttStatusResponse {
        available: true,
        enabled: engine.is_enabled(),
        provider: cfg.provider.clone(),
        model: cfg.model.clone(),
    }))
}

#[derive(Serialize)]
pub struct TranscribeResponse {
    pub text: String,
    pub language: Option<String>,
    pub duration_seconds: Option<f64>,
}

pub async fn stt_transcribe(
    State(state): State<AppState>,
    mut multipart: axum::extract::Multipart,
) -> Result<Json<TranscribeResponse>, GatewayError> {
    let engine_lock = state
        .stt_engine
        .as_ref()
        .ok_or_else(|| GatewayError::BadRequest("STT engine not configured".to_string()))?;

    // Extract the first file field from the multipart body.
    let mut audio_bytes: Option<Vec<u8>> = None;
    let mut mime_type = "audio/wav".to_string();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| GatewayError::BadRequest(e.to_string()))?
    {
        if field.name() == Some("file") {
            if let Some(ct) = field.content_type() {
                mime_type = ct.to_string();
            }
            let data = field
                .bytes()
                .await
                .map_err(|e| GatewayError::BadRequest(e.to_string()))?;
            audio_bytes = Some(data.to_vec());
            break;
        }
    }

    let audio = audio_bytes.ok_or_else(|| {
        GatewayError::BadRequest("missing 'file' field in multipart body".to_string())
    })?;

    let engine = engine_lock.read().await;
    let result = engine
        .transcribe(&audio, &mime_type)
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;

    Ok(Json(TranscribeResponse {
        text: result.text,
        language: result.language,
        duration_seconds: result.duration_seconds,
    }))
}

pub async fn stt_enable(State(state): State<AppState>) -> Result<Json<Value>, GatewayError> {
    let engine_lock = state
        .stt_engine
        .as_ref()
        .ok_or_else(|| GatewayError::BadRequest("STT engine not configured".to_string()))?;
    engine_lock.write().await.enable();
    Ok(Json(json!({ "enabled": true })))
}

pub async fn stt_disable(State(state): State<AppState>) -> Result<Json<Value>, GatewayError> {
    let engine_lock = state
        .stt_engine
        .as_ref()
        .ok_or_else(|| GatewayError::BadRequest("STT engine not configured".to_string()))?;
    engine_lock.write().await.disable();
    Ok(Json(json!({ "enabled": false })))
}

// ── Config Editor ───────────────────────────────────────────────────────────

/// `GET /config` — return the current configuration with secrets redacted.
pub async fn get_config(State(state): State<AppState>) -> Result<Json<Value>, GatewayError> {
    let config = state.config.read().await;
    let redacted = config.redacted();
    let value =
        serde_json::to_value(&redacted).map_err(|e| GatewayError::Internal(e.to_string()))?;
    Ok(Json(value))
}

/// `PUT /config` — replace the live configuration.
pub async fn update_config(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, GatewayError> {
    let new_config: rune_config::AppConfig = serde_json::from_value(body)
        .map_err(|e| GatewayError::BadRequest(format!("invalid config: {e}")))?;

    let mut config = state.config.write().await;
    *config = new_config;
    drop(config);

    let config = state.config.read().await;
    let redacted = config.redacted();
    let value =
        serde_json::to_value(&redacted).map_err(|e| GatewayError::Internal(e.to_string()))?;
    Ok(Json(value))
}

// ── Turns ───────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct TurnsListQuery {
    pub session_id: Option<Uuid>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Serialize)]
pub struct TurnResponse {
    pub id: Uuid,
    pub session_id: Uuid,
    pub trigger_kind: String,
    pub status: String,
    pub model_ref: Option<String>,
    pub usage_prompt_tokens: Option<i32>,
    pub usage_completion_tokens: Option<i32>,
    pub started_at: String,
    pub ended_at: Option<String>,
}

/// `GET /api/turns` — list turns, optionally filtered by session_id.
pub async fn list_turns(
    State(state): State<AppState>,
    Query(query): Query<TurnsListQuery>,
) -> Result<Json<Vec<TurnResponse>>, GatewayError> {
    let session_id = query
        .session_id
        .ok_or_else(|| GatewayError::BadRequest("session_id query parameter is required".into()))?;

    let rows = state
        .turn_repo
        .list_by_session(session_id)
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;

    let limit = query.limit.unwrap_or(100).min(500) as usize;
    let offset = query.offset.unwrap_or(0).max(0) as usize;

    let items: Vec<TurnResponse> = rows
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(turn_to_response)
        .collect();

    Ok(Json(items))
}

/// `GET /api/turns/{id}` — get a single turn by ID.
pub async fn get_turn(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<TurnResponse>, GatewayError> {
    let row = state
        .turn_repo
        .find_by_id(id)
        .await
        .map_err(|e| GatewayError::BadRequest(format!("turn not found: {e}")))?;

    Ok(Json(turn_to_response(row)))
}

fn turn_to_response(row: TurnRow) -> TurnResponse {
    TurnResponse {
        id: row.id,
        session_id: row.session_id,
        trigger_kind: row.trigger_kind,
        status: row.status,
        model_ref: row.model_ref,
        usage_prompt_tokens: row.usage_prompt_tokens,
        usage_completion_tokens: row.usage_completion_tokens,
        started_at: row.started_at.to_rfc3339(),
        ended_at: row.ended_at.map(|t| t.to_rfc3339()),
    }
}

// ── Tools ───────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ToolRegistryItem {
    pub name: String,
    pub description: String,
    pub category: String,
}

/// `GET /api/tools` — list registered tools from the skill registry.
pub async fn list_tools(
    State(state): State<AppState>,
) -> Result<Json<Vec<ToolRegistryItem>>, GatewayError> {
    let skills = state.skill_registry.list().await;
    let items: Vec<ToolRegistryItem> = skills
        .into_iter()
        .map(|s| ToolRegistryItem {
            name: s.name.clone(),
            description: s.description.clone(),
            category: if s.enabled {
                "enabled".to_string()
            } else {
                "disabled".to_string()
            },
        })
        .collect();

    Ok(Json(items))
}

/// `GET /api/tools/{id}` — get a tool execution by ID (stub).
pub async fn get_tool_execution(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, GatewayError> {
    let execution_id = Uuid::parse_str(&id)
        .map_err(|_| GatewayError::BadRequest(format!("invalid tool execution id: {id}")))?;
    let execution = state
        .tool_execution_repo
        .find_by_id(execution_id)
        .await
        .map_err(|error| match error {
            rune_store::StoreError::NotFound { .. } => {
                GatewayError::BadRequest(format!("no tool execution found for id: {id}"))
            }
            other => GatewayError::Internal(other.to_string()),
        })?;

    Ok(Json(serde_json::to_value(execution).map_err(|error| {
        GatewayError::Internal(error.to_string())
    })?))
}

// ── Microsoft 365 ───────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct Ms365AuthStatusResponse {
    pub authenticated: bool,
    pub tenant_id: Option<String>,
    pub client_id: Option<String>,
    pub user_principal: Option<String>,
    pub scopes: Vec<String>,
    pub token_expires_at: Option<String>,
    pub token_valid: bool,
}

/// `GET /ms365/auth/status` — return Microsoft 365 auth/config readiness.
pub async fn ms365_auth_status(
    State(state): State<AppState>,
) -> Result<Json<Ms365AuthStatusResponse>, GatewayError> {
    let config = state.config.read().await;

    Ok(Json(Ms365AuthStatusResponse {
        authenticated: false,
        tenant_id: config.ms365.tenant_id.clone(),
        client_id: config.ms365.client_id.clone(),
        user_principal: config.ms365.user_principal.clone(),
        scopes: config.ms365.scopes.clone(),
        token_expires_at: None,
        token_valid: false,
    }))
}

#[derive(Serialize)]
pub struct Ms365FilesListResponse {
    pub items: Vec<FileItem>,
    pub path: String,
    pub total: u32,
}

#[derive(Serialize)]
pub struct Ms365FilesReadResponse {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub is_folder: bool,
    pub mime_type: Option<String>,
    pub last_modified: String,
    pub created_at: String,
    pub web_url: Option<String>,
    pub parent_path: Option<String>,
    pub download_url: Option<String>,
}

#[derive(Serialize)]
pub struct Ms365FilesSearchResponse {
    pub items: Vec<FileSearchItem>,
    pub query: String,
    pub total: u32,
}

#[derive(Serialize)]
pub struct Ms365UsersReadResponse {
    pub user: UserProfile,
}

#[derive(Serialize)]
pub struct Ms365UsersListResponse {
    pub users: Vec<UserSummary>,
    pub total: u32,
}

/// `GET /ms365/files` — list files in a OneDrive folder path.
pub async fn ms365_files_list(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Ms365FilesListResponse>, GatewayError> {
    let path = params.get("path").map(String::as_str).unwrap_or("/");
    let limit = params
        .get("limit")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(25)
        .clamp(1, 100);

    let list = state
        .ms365_files_service
        .list(path, limit)
        .await
        .map_err(map_ms365_files_service_error)?;

    Ok(Json(Ms365FilesListResponse {
        items: list.items,
        path: list.path,
        total: list.total,
    }))
}

/// `GET /ms365/files/{id}` — read OneDrive file metadata by item ID.
pub async fn ms365_files_read(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Ms365FilesReadResponse>, GatewayError> {
    let file = state
        .ms365_files_service
        .read(&id)
        .await
        .map_err(map_ms365_files_service_error)?;

    Ok(Json(ms365_file_read_response(file)))
}

/// `GET /ms365/files/search` — search OneDrive files by name/content.
pub async fn ms365_files_search(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Ms365FilesSearchResponse>, GatewayError> {
    let query = params.get("query").map(String::as_str).unwrap_or("");
    let limit = params
        .get("limit")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(25)
        .clamp(1, 100);

    let results = state
        .ms365_files_service
        .search(query, limit)
        .await
        .map_err(map_ms365_files_service_error)?;

    Ok(Json(Ms365FilesSearchResponse {
        items: results.items,
        query: results.query,
        total: results.total,
    }))
}

/// `GET /ms365/files/{id}/content` — stream OneDrive file bytes by item ID.
pub async fn ms365_files_content(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, GatewayError> {
    let content = state
        .ms365_files_service
        .download_content(&id)
        .await
        .map_err(map_ms365_files_service_error)?;

    Ok((
        [
            (header::CONTENT_TYPE, content.content_type),
            (
                header::CONTENT_DISPOSITION,
                format!(
                    "attachment; filename=\"{}\"",
                    content.filename.replace('"', "")
                ),
            ),
        ],
        content.bytes,
    )
        .into_response())
}

/// `GET /ms365/users/me` — return the authenticated user's profile.
pub async fn ms365_users_me(
    State(state): State<AppState>,
) -> Result<Json<Ms365UsersReadResponse>, GatewayError> {
    let user = state
        .ms365_users_service
        .me()
        .await
        .map_err(map_ms365_users_service_error)?;

    Ok(Json(Ms365UsersReadResponse { user }))
}

/// `GET /ms365/users` — list users in the organization directory.
pub async fn ms365_users_list(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Ms365UsersListResponse>, GatewayError> {
    let limit = params
        .get("limit")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(25)
        .clamp(1, 100);

    let list = state
        .ms365_users_service
        .list(limit)
        .await
        .map_err(map_ms365_users_service_error)?;

    Ok(Json(Ms365UsersListResponse {
        users: list.users,
        total: list.total,
    }))
}

/// `GET /ms365/users/{id}` — read a single user's profile by ID or UPN.
pub async fn ms365_users_read(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Ms365UsersReadResponse>, GatewayError> {
    let user = state
        .ms365_users_service
        .read(&id)
        .await
        .map_err(map_ms365_users_service_error)?;

    Ok(Json(Ms365UsersReadResponse { user }))
}

#[derive(Serialize)]
pub struct Ms365MailMutationResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Serialize)]
pub struct Ms365CalendarMutationResponse {
    pub success: bool,
    pub message: String,
}

/// `POST /ms365/calendar/events` — create a Microsoft 365 calendar event.
pub async fn ms365_calendar_create_event(
    State(state): State<AppState>,
    Json(request): Json<CreateCalendarEventRequest>,
) -> Result<(StatusCode, Json<Ms365CalendarMutationResponse>), GatewayError> {
    state
        .ms365_calendar_service
        .create_event(request)
        .await
        .map_err(map_ms365_calendar_service_error)?;

    Ok((
        StatusCode::CREATED,
        Json(Ms365CalendarMutationResponse {
            success: true,
            message: "Calendar event created".to_string(),
        }),
    ))
}

/// `POST /ms365/calendar/events/{id}` — update a Microsoft 365 calendar event.
pub async fn ms365_calendar_update_event(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<UpdateCalendarEventRequest>,
) -> Result<Json<Ms365CalendarMutationResponse>, GatewayError> {
    state
        .ms365_calendar_service
        .update_event(&id, request)
        .await
        .map_err(map_ms365_calendar_service_error)?;

    Ok(Json(Ms365CalendarMutationResponse {
        success: true,
        message: "Calendar event updated".to_string(),
    }))
}

/// `DELETE /ms365/calendar/events/{id}` — delete a Microsoft 365 calendar event.
pub async fn ms365_calendar_delete_event(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Ms365CalendarMutationResponse>, GatewayError> {
    state
        .ms365_calendar_service
        .delete_event(&id)
        .await
        .map_err(map_ms365_calendar_service_error)?;

    Ok(Json(Ms365CalendarMutationResponse {
        success: true,
        message: "Calendar event deleted".to_string(),
    }))
}

/// `POST /ms365/calendar/events/{id}/delete` — compatibility alias for existing clients.
pub async fn ms365_calendar_delete_event_compat(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Ms365CalendarMutationResponse>, GatewayError> {
    ms365_calendar_delete_event(State(state), Path(id)).await
}

/// `POST /ms365/calendar/events/{id}/respond` — respond to a Microsoft 365 calendar invitation.
pub async fn ms365_calendar_respond_event(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<RespondCalendarEventRequest>,
) -> Result<Json<Ms365CalendarMutationResponse>, GatewayError> {
    state
        .ms365_calendar_service
        .respond_to_event(&id, request)
        .await
        .map_err(map_ms365_calendar_service_error)?;

    Ok(Json(Ms365CalendarMutationResponse {
        success: true,
        message: "Calendar response sent".to_string(),
    }))
}

/// `POST /ms365/mail/send` — send a Microsoft 365 mail message.
pub async fn ms365_mail_send(
    State(state): State<AppState>,
    Json(request): Json<SendMailRequest>,
) -> Result<Json<Ms365MailMutationResponse>, GatewayError> {
    state
        .ms365_mail_service
        .send_mail(request)
        .await
        .map_err(map_ms365_mail_service_error)?;

    Ok(Json(Ms365MailMutationResponse {
        success: true,
        message: "Message sent".to_string(),
    }))
}

/// `POST /ms365/mail/messages/{id}/reply` — reply to a Microsoft 365 mail message.
pub async fn ms365_mail_reply(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<ReplyMailRequest>,
) -> Result<Json<Ms365MailMutationResponse>, GatewayError> {
    state
        .ms365_mail_service
        .reply_to_message(&id, request)
        .await
        .map_err(map_ms365_mail_service_error)?;

    Ok(Json(Ms365MailMutationResponse {
        success: true,
        message: "Reply sent".to_string(),
    }))
}

/// `POST /ms365/mail/messages/{id}/forward` — forward a Microsoft 365 mail message.
pub async fn ms365_mail_forward(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<ForwardMailRequest>,
) -> Result<Json<Ms365MailMutationResponse>, GatewayError> {
    state
        .ms365_mail_service
        .forward_message(&id, request)
        .await
        .map_err(map_ms365_mail_service_error)?;

    Ok(Json(Ms365MailMutationResponse {
        success: true,
        message: "Message forwarded".to_string(),
    }))
}

#[derive(Serialize)]
pub struct Ms365PlannerTaskMutationResponse {
    pub task: PlannerTask,
}

#[derive(Serialize)]
pub struct Ms365TodoTaskMutationResponse {
    pub task: TodoTask,
}

/// `POST /ms365/planner/tasks` — create a Microsoft Planner task.
pub async fn ms365_planner_create_task(
    State(state): State<AppState>,
    Json(request): Json<CreatePlannerTaskRequest>,
) -> Result<(StatusCode, Json<Ms365PlannerTaskMutationResponse>), GatewayError> {
    let task = state
        .ms365_planner_service
        .create_task(request)
        .await
        .map_err(map_ms365_planner_service_error)?;

    Ok((
        StatusCode::CREATED,
        Json(Ms365PlannerTaskMutationResponse { task }),
    ))
}

/// `POST /ms365/planner/tasks/{id}` — update a Microsoft Planner task.
pub async fn ms365_planner_update_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<UpdatePlannerTaskRequest>,
) -> Result<Json<Ms365PlannerTaskMutationResponse>, GatewayError> {
    let task = state
        .ms365_planner_service
        .update_task(&id, request)
        .await
        .map_err(map_ms365_planner_service_error)?;

    Ok(Json(Ms365PlannerTaskMutationResponse { task }))
}

/// `POST /ms365/planner/tasks/{id}/complete` — mark a Microsoft Planner task complete.
pub async fn ms365_planner_complete_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Ms365PlannerTaskMutationResponse>, GatewayError> {
    let task = state
        .ms365_planner_service
        .complete_task(&id)
        .await
        .map_err(map_ms365_planner_service_error)?;

    Ok(Json(Ms365PlannerTaskMutationResponse { task }))
}

/// `POST /ms365/todo/lists/{list_id}/tasks` — create a Microsoft To-Do task.
pub async fn ms365_todo_create_task(
    State(state): State<AppState>,
    Path(list_id): Path<String>,
    Json(request): Json<CreateTodoTaskRequest>,
) -> Result<(StatusCode, Json<Ms365TodoTaskMutationResponse>), GatewayError> {
    let task = state
        .ms365_todo_service
        .create_task(&list_id, request)
        .await
        .map_err(map_ms365_todo_service_error)?;

    Ok((
        StatusCode::CREATED,
        Json(Ms365TodoTaskMutationResponse { task }),
    ))
}

/// `POST /ms365/todo/lists/{list_id}/tasks/{id}` — update a Microsoft To-Do task.
pub async fn ms365_todo_update_task(
    State(state): State<AppState>,
    Path((list_id, id)): Path<(String, String)>,
    Json(request): Json<UpdateTodoTaskRequest>,
) -> Result<Json<Ms365TodoTaskMutationResponse>, GatewayError> {
    let task = state
        .ms365_todo_service
        .update_task(&list_id, &id, request)
        .await
        .map_err(map_ms365_todo_service_error)?;

    Ok(Json(Ms365TodoTaskMutationResponse { task }))
}

/// `POST /ms365/todo/lists/{list_id}/tasks/{id}/complete` — mark a Microsoft To-Do task complete.
pub async fn ms365_todo_complete_task(
    State(state): State<AppState>,
    Path((list_id, id)): Path<(String, String)>,
) -> Result<Json<Ms365TodoTaskMutationResponse>, GatewayError> {
    let task = state
        .ms365_todo_service
        .complete_task(&list_id, &id)
        .await
        .map_err(map_ms365_todo_service_error)?;

    Ok(Json(Ms365TodoTaskMutationResponse { task }))
}

fn ms365_file_read_response(file: FileMetadata) -> Ms365FilesReadResponse {
    Ms365FilesReadResponse {
        id: file.id,
        name: file.name,
        size: file.size,
        is_folder: file.is_folder,
        mime_type: file.mime_type,
        last_modified: file.last_modified,
        created_at: file.created_at,
        web_url: file.web_url,
        parent_path: file.parent_path,
        download_url: file.download_url,
    }
}

fn map_ms365_files_service_error(error: Ms365FilesServiceError) -> GatewayError {
    match error {
        Ms365FilesServiceError::Validation(message) | Ms365FilesServiceError::NotFound(message) => {
            GatewayError::BadRequest(message)
        }
        Ms365FilesServiceError::NotConfigured(message)
        | Ms365FilesServiceError::Upstream(message) => GatewayError::Internal(message),
        Ms365FilesServiceError::Unauthorized => GatewayError::Unauthorized,
        Ms365FilesServiceError::Forbidden(message) => GatewayError::Forbidden(message),
    }
}

fn map_ms365_calendar_service_error(error: Ms365CalendarServiceError) -> GatewayError {
    match error {
        Ms365CalendarServiceError::Validation(message)
        | Ms365CalendarServiceError::NotFound(message) => GatewayError::BadRequest(message),
        Ms365CalendarServiceError::NotConfigured(message)
        | Ms365CalendarServiceError::Upstream(message) => GatewayError::Internal(message),
        Ms365CalendarServiceError::Unauthorized => GatewayError::Unauthorized,
        Ms365CalendarServiceError::Forbidden(message) => GatewayError::Forbidden(message),
    }
}

fn map_ms365_planner_service_error(error: Ms365PlannerServiceError) -> GatewayError {
    match error {
        Ms365PlannerServiceError::Validation(message)
        | Ms365PlannerServiceError::NotFound(message) => GatewayError::BadRequest(message),
        Ms365PlannerServiceError::NotConfigured(message)
        | Ms365PlannerServiceError::Upstream(message) => GatewayError::Internal(message),
        Ms365PlannerServiceError::Unauthorized => GatewayError::Unauthorized,
        Ms365PlannerServiceError::Forbidden(message) => GatewayError::Forbidden(message),
    }
}

fn map_ms365_mail_service_error(error: Ms365MailServiceError) -> GatewayError {
    match error {
        Ms365MailServiceError::Validation(message) | Ms365MailServiceError::NotFound(message) => {
            GatewayError::BadRequest(message)
        }
        Ms365MailServiceError::NotConfigured(message)
        | Ms365MailServiceError::Upstream(message) => GatewayError::Internal(message),
        Ms365MailServiceError::Unauthorized => GatewayError::Unauthorized,
        Ms365MailServiceError::Forbidden(message) => GatewayError::Forbidden(message),
    }
}

fn map_ms365_todo_service_error(error: Ms365TodoServiceError) -> GatewayError {
    match error {
        Ms365TodoServiceError::Validation(message) | Ms365TodoServiceError::NotFound(message) => {
            GatewayError::BadRequest(message)
        }
        Ms365TodoServiceError::NotConfigured(message)
        | Ms365TodoServiceError::Upstream(message) => GatewayError::Internal(message),
        Ms365TodoServiceError::Unauthorized => GatewayError::Unauthorized,
        Ms365TodoServiceError::Forbidden(message) => GatewayError::Forbidden(message),
    }
}

fn map_ms365_users_service_error(error: Ms365UsersServiceError) -> GatewayError {
    match error {
        Ms365UsersServiceError::Validation(message) | Ms365UsersServiceError::NotFound(message) => {
            GatewayError::BadRequest(message)
        }
        Ms365UsersServiceError::NotConfigured(message)
        | Ms365UsersServiceError::Upstream(message) => GatewayError::Internal(message),
        Ms365UsersServiceError::Unauthorized => GatewayError::Unauthorized,
        Ms365UsersServiceError::Forbidden(message) => GatewayError::Forbidden(message),
    }
}

// ── Auth ────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct AuthTokenInfo {
    pub authenticated: bool,
    pub auth_enabled: bool,
    pub device_count: usize,
}

/// `GET /api/auth` — return token / auth status information.
pub async fn auth_token_info(
    State(state): State<AppState>,
) -> Result<Json<AuthTokenInfo>, GatewayError> {
    let config = state.config.read().await;
    let auth_enabled = config.gateway.auth_token.is_some();
    drop(config);

    let devices = state
        .device_repo
        .list_devices()
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;

    Ok(Json(AuthTokenInfo {
        authenticated: true,
        auth_enabled,
        device_count: devices.len(),
    }))
}

// ── Channels ────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ChannelItem {
    pub name: String,
    pub kind: String,
    pub enabled: bool,
}

#[derive(Serialize)]
pub struct ChannelStatusResponse {
    pub configured: Vec<ChannelItem>,
    pub active_sessions: usize,
}

/// `GET /api/channels` — list configured channel adapters.
pub async fn list_channels(
    State(state): State<AppState>,
) -> Result<Json<Vec<ChannelItem>>, GatewayError> {
    let config = state.config.read().await;
    let channels = configured_channels(&config);
    let items: Vec<ChannelItem> = channels
        .into_iter()
        .map(|name| ChannelItem {
            kind: name.clone(),
            name,
            enabled: true,
        })
        .collect();
    Ok(Json(items))
}

/// `GET /api/channels/status` — channel subsystem status.
pub async fn channels_status(
    State(state): State<AppState>,
) -> Result<Json<ChannelStatusResponse>, GatewayError> {
    let config = state.config.read().await;
    let channels = configured_channels(&config);
    drop(config);

    let rows = state
        .session_repo
        .list(i64::MAX / 4, 0)
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;
    let active_sessions = rows.iter().filter(|r| r.channel_ref.is_some()).count();

    let items: Vec<ChannelItem> = channels
        .into_iter()
        .map(|name| ChannelItem {
            kind: name.clone(),
            name,
            enabled: true,
        })
        .collect();

    Ok(Json(ChannelStatusResponse {
        configured: items,
        active_sessions,
    }))
}

// ── Memory ──────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct MemoryStatusResponse {
    pub memory_mode: String,
    pub memory_dir: String,
    pub pgvector: bool,
}

#[derive(Deserialize)]
pub struct MemorySearchQuery {
    pub q: Option<String>,
    #[serde(rename = "limit")]
    pub _limit: Option<usize>,
}

#[derive(Serialize)]
pub struct MemorySearchResponse {
    pub query: String,
    pub results: Vec<Value>,
    pub message: String,
}

pub(crate) fn parse_memory_search_output(output: &str) -> Vec<Value> {
    output
        .split("\n---\n")
        .filter_map(|chunk| {
            let chunk = chunk.trim();
            if chunk.is_empty() || chunk.starts_with("No results found for query:") {
                return None;
            }

            let (source_line, snippet) = chunk.split_once("\n")?;
            let source = source_line.strip_prefix("Source: ")?.trim();
            Some(json!({
                "source": source,
                "snippet": snippet.trim(),
            }))
        })
        .collect()
}

/// `GET /api/memory/status` — memory subsystem status.
pub async fn memory_status(
    State(state): State<AppState>,
) -> Result<Json<MemoryStatusResponse>, GatewayError> {
    let config = state.config.read().await;
    Ok(Json(MemoryStatusResponse {
        memory_mode: state.capabilities.memory_mode.clone(),
        memory_dir: config.paths.memory_dir.display().to_string(),
        pgvector: state.capabilities.pgvector,
    }))
}

/// `GET /api/memory/search` — search memory (stub; backend integration pending).
pub async fn memory_search(
    State(state): State<AppState>,
    Query(query): Query<MemorySearchQuery>,
) -> Result<Json<MemorySearchResponse>, GatewayError> {
    let q = query.q.unwrap_or_default();
    if q.is_empty() {
        return Err(GatewayError::BadRequest(
            "q query parameter is required".into(),
        ));
    }

    let limit = query._limit.unwrap_or(10).clamp(1, 50);
    let workspace_root = {
        let config = state.config.read().await;
        config
            .agents
            .defaults
            .workspace
            .clone()
            .unwrap_or_else(|| ".".to_string())
    };

    let call = ToolCall {
        tool_call_id: rune_core::ToolCallId::new(),
        tool_name: "memory_search".to_string(),
        arguments: json!({
            "query": q.clone(),
            "maxResults": limit,
        }),
    };

    let tool = MemoryToolExecutor::new(workspace_root);
    let result = tool
        .execute(call)
        .await
        .map_err(|error| GatewayError::Internal(error.to_string()))?;
    let results = parse_memory_search_output(&result.output);
    let message = if results.is_empty() {
        format!("No results found for query: {q}")
    } else {
        format!("Found {} memory result(s)", results.len())
    };

    Ok(Json(MemorySearchResponse {
        query: q,
        results,
        message,
    }))
}

/// `GET /api/memory/graph` — knowledge graph of Mem0 memories.
///
/// Returns nodes (memories) and edges (cosine similarity above threshold)
/// for Obsidian-style visualization.
pub async fn memory_graph(
    State(state): State<AppState>,
    Query(params): Query<MemoryGraphQuery>,
) -> Result<Json<serde_json::Value>, GatewayError> {
    let mem0 = state
        .turn_executor
        .mem0()
        .ok_or_else(|| GatewayError::BadRequest("mem0 not enabled".into()))?;

    let threshold = params.threshold.unwrap_or(0.45);
    let neighbors = params.neighbors.unwrap_or(5).min(20) as i64;

    let graph = mem0.graph(threshold, neighbors).await;

    Ok(Json(json!({
        "nodes": graph.nodes.iter().map(|n| json!({
            "id": n.id.to_string(),
            "fact": n.fact,
            "category": n.category,
            "session_id": n.source_session_id.map(|s| s.to_string()),
            "created_at": n.created_at.to_rfc3339(),
            "access_count": n.access_count,
        })).collect::<Vec<_>>(),
        "edges": graph.edges.iter().map(|e| json!({
            "source": e.source.to_string(),
            "target": e.target.to_string(),
            "similarity": e.similarity,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct MemoryGraphQuery {
    pub threshold: Option<f64>,
    pub neighbors: Option<usize>,
}

/// `DELETE /api/memory/:id` — delete a memory node.
pub async fn memory_delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, GatewayError> {
    let mem0 = state
        .turn_executor
        .mem0()
        .ok_or_else(|| GatewayError::BadRequest("mem0 not enabled".into()))?;

    mem0.delete_memory(&id)
        .await
        .map_err(|e| GatewayError::Internal(format!("failed to delete memory: {e}")))?;

    Ok(Json(json!({"success": true, "id": id})))
}

// ── Logs ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct LogsQuery {
    #[serde(rename = "level")]
    pub _level: Option<String>,
    #[serde(rename = "source")]
    pub _source: Option<String>,
    #[serde(rename = "limit")]
    pub _limit: Option<usize>,
    #[serde(rename = "since")]
    pub _since: Option<String>,
}

#[derive(Serialize)]
pub struct LogsQueryResponse {
    pub entries: Vec<Value>,
    pub message: String,
}

/// `GET /api/logs` — query structured logs (stub; log aggregation pending).
pub async fn query_logs(
    State(state): State<AppState>,
    Query(_query): Query<LogsQuery>,
) -> Result<Json<LogsQueryResponse>, GatewayError> {
    let config = state.config.read().await;
    let logs_dir = config.paths.logs_dir.display().to_string();
    drop(config);

    Ok(Json(LogsQueryResponse {
        entries: vec![],
        message: format!("structured log query not yet aggregated; logs directory: {logs_dir}"),
    }))
}

// ── Doctor ───────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct DoctorCheck {
    pub name: String,
    pub status: &'static str,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct DoctorPathSummary {
    pub profile: &'static str,
    pub mode: &'static str,
    pub auto_create_missing: bool,
}

#[derive(Serialize)]
pub struct DoctorReport {
    pub overall: &'static str,
    pub checks: Vec<DoctorCheck>,
    pub paths: DoctorPathSummary,
    pub run_at: String,
}

fn probe_writable(dir: &std::path::Path) -> bool {
    let probe = dir.join(".rune_gateway_doctor_probe");
    match std::fs::write(&probe, b"probe") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

fn gateway_path_hint(
    path: &std::path::Path,
    mode: &rune_config::RuntimeMode,
    writable: bool,
) -> String {
    match (mode, writable) {
        (rune_config::RuntimeMode::Standalone, false) => {
            format!("Fix permissions: chmod u+w {}", path.display())
        }
        (rune_config::RuntimeMode::Standalone, true) => {
            format!("Create the path: mkdir -p {}", path.display())
        }
        (_, false) => format!(
            "Ensure the volume at {} is writable; check mount flags and container user UID",
            path.display()
        ),
        (_, true) => format!(
            "Mount a writable volume at {} (for example -v /host/path:{})",
            path.display(),
            path.display()
        ),
    }
}

fn readiness_checks(config: &rune_config::AppConfig) -> Vec<DoctorCheck> {
    let mut checks = Vec::new();
    let resolved_mode = config.mode.resolve(config);

    let path_checks = storage_path_checks(config);
    let mut hard_fail_names = vec![
        "paths.db_dir",
        "paths.sessions_dir",
        "paths.memory_dir",
        "paths.media_dir",
        "paths.logs_dir",
    ];
    if resolved_mode == rune_config::RuntimeMode::Server {
        hard_fail_names.extend([
            "paths.skills_dir",
            "paths.plugins_dir",
            "paths.backups_dir",
            "paths.config_dir",
            "paths.secrets_dir",
        ]);
    }

    for mut check in path_checks {
        if hard_fail_names.iter().any(|name| *name == check.name) {
            check.status = match check.status {
                "pass" => "pass",
                _ => "fail",
            };
        }
        checks.push(check);
    }

    checks.push(DoctorCheck {
        name: "model_backend".to_string(),
        status: if !config.models.providers.is_empty() || resolved_mode == rune_config::RuntimeMode::Standalone {
            "pass"
        } else {
            "warn"
        },
        message: if !config.models.providers.is_empty() {
            format!("{} provider(s) configured", config.models.providers.len())
        } else if resolved_mode == rune_config::RuntimeMode::Standalone {
            "No explicit providers configured; standalone startup may still succeed via zero-config Ollama or echo fallback".to_string()
        } else {
            "No explicit model providers configured; server deployments should provision at least one provider".to_string()
        },
    });

    checks
}

fn storage_path_checks(config: &rune_config::AppConfig) -> Vec<DoctorCheck> {
    let mode = config.mode.resolve(config);
    [
        ("paths.db_dir", &config.paths.db_dir, true),
        ("paths.sessions_dir", &config.paths.sessions_dir, true),
        ("paths.memory_dir", &config.paths.memory_dir, true),
        ("paths.media_dir", &config.paths.media_dir, true),
        ("paths.skills_dir", &config.paths.skills_dir, true),
        ("paths.plugins_dir", &config.paths.plugins_dir, true),
        ("paths.logs_dir", &config.paths.logs_dir, true),
        ("paths.backups_dir", &config.paths.backups_dir, true),
        ("paths.config_dir", &config.paths.config_dir, true),
        ("paths.secrets_dir", &config.paths.secrets_dir, true),
    ]
    .into_iter()
    .map(|(name, path, required_persistent)| {
        if !path.exists() {
            DoctorCheck {
                name: name.to_string(),
                status: if required_persistent { "warn" } else { "info" },
                message: format!(
                    "{} is missing — {}",
                    path.display(),
                    gateway_path_hint(path, &mode, true)
                ),
            }
        } else if !path.is_dir() {
            DoctorCheck {
                name: name.to_string(),
                status: "fail",
                message: format!(
                    "{} exists but is not a directory — remove and recreate it as a directory",
                    path.display()
                ),
            }
        } else if !probe_writable(path) {
            DoctorCheck {
                name: name.to_string(),
                status: "fail",
                message: format!(
                    "{} is not writable (write probe failed) — {}",
                    path.display(),
                    gateway_path_hint(path, &mode, false)
                ),
            }
        } else {
            DoctorCheck {
                name: name.to_string(),
                status: "pass",
                message: format!("{} is present and writable", path.display()),
            }
        }
    })
    .collect()
}

/// `POST /api/doctor/run` — execute diagnostic checks.
pub async fn doctor_run(State(state): State<AppState>) -> Result<Json<DoctorReport>, GatewayError> {
    let mut checks = Vec::new();

    let config = state.config.read().await;
    let resolved_mode = config.mode.resolve(&config);
    let paths_summary = DoctorPathSummary {
        profile: config.paths.profile().as_str(),
        mode: resolved_mode.as_str(),
        auto_create_missing: resolved_mode == rune_config::RuntimeMode::Standalone,
    };
    let provider_ok = !config.models.providers.is_empty();
    checks.push(DoctorCheck {
        name: "model_providers".to_string(),
        status: if provider_ok { "pass" } else { "warn" },
        message: if provider_ok {
            format!("{} provider(s) configured", config.models.providers.len())
        } else {
            config
                .models
                .zero_config_ollama_base_url(std::env::var("OLLAMA_HOST").ok().as_deref())
                .map(|base| format!("no explicit model providers configured; zero-config Ollama available at {base}"))
                .unwrap_or_else(|| "no model providers configured; using demo echo backend".to_string())
        },
    });

    let auth_ok = config.gateway.auth_token.is_some();
    checks.push(DoctorCheck {
        name: "auth".to_string(),
        status: if auth_ok { "pass" } else { "warn" },
        message: if auth_ok {
            "bearer auth enabled".to_string()
        } else {
            "no auth token configured; gateway is unauthenticated".to_string()
        },
    });
    checks.extend(storage_path_checks(&config));
    drop(config);

    let session_check = state.session_repo.list(1, 0).await;
    let (session_store_status, session_store_message) = match session_check {
        Ok(_) => ("pass", "session store reachable".to_string()),
        Err(error) => ("fail", format!("session store error: {error}")),
    };
    checks.push(DoctorCheck {
        name: "session_store".to_string(),
        status: session_store_status,
        message: session_store_message,
    });

    checks.push(DoctorCheck {
        name: "tts".to_string(),
        status: if state.tts_engine.is_some() {
            "pass"
        } else {
            "info"
        },
        message: if state.tts_engine.is_some() {
            "TTS engine configured".to_string()
        } else {
            "TTS engine not configured".to_string()
        },
    });

    checks.push(DoctorCheck {
        name: "stt".to_string(),
        status: if state.stt_engine.is_some() {
            "pass"
        } else {
            "info"
        },
        message: if state.stt_engine.is_some() {
            "STT engine configured".to_string()
        } else {
            "STT engine not configured".to_string()
        },
    });

    // ── Approval / security mode visibility (#64) ────────────────────
    let approval_mode = &state.capabilities.approval_mode;
    let security_posture = &state.capabilities.security_posture;
    let is_yolo = approval_mode == "yolo";
    let is_no_sandbox = security_posture == "no-sandbox" || security_posture == "unrestricted";

    checks.push(DoctorCheck {
        name: "approval_mode".to_string(),
        status: if is_yolo { "warn" } else { "pass" },
        message: format!("approval mode: {approval_mode}"),
    });
    checks.push(DoctorCheck {
        name: "security_posture".to_string(),
        status: if is_no_sandbox { "warn" } else { "pass" },
        message: format!("security posture: {security_posture}"),
    });

    let overall = if checks.iter().any(|c| c.status == "fail") {
        "unhealthy"
    } else if checks.iter().any(|c| c.status == "warn") {
        "degraded"
    } else {
        "healthy"
    };

    Ok(Json(DoctorReport {
        overall,
        checks,
        paths: paths_summary,
        run_at: Utc::now().to_rfc3339(),
    }))
}

/// `GET /api/doctor/results` — return the most recent doctor report (stub).
pub async fn doctor_results(
    State(state): State<AppState>,
) -> Result<Json<DoctorReport>, GatewayError> {
    doctor_run(State(state)).await
}

// ── Configure / Setup ────────────────────────────────────────────────────────

/// A single configuration item reported by the configure surface.
#[derive(Serialize)]
pub struct ConfigureItem {
    pub name: String,
    pub status: &'static str,
    pub message: String,
}

/// Full response from the configure/setup endpoints.
#[derive(Serialize)]
pub struct ConfigureGatewayResponse {
    pub success: bool,
    pub detail: String,
    pub items: Vec<ConfigureItem>,
}

/// Inspect current configuration and report what is configured, skipped, or needed.
fn build_configure_items(
    config: &rune_config::AppConfig,
    capabilities: &rune_config::Capabilities,
    tts_available: bool,
    stt_available: bool,
) -> Vec<ConfigureItem> {
    let mut items = Vec::new();

    // Model providers
    let provider_count = config.models.providers.len();
    items.push(ConfigureItem {
        name: "model_providers".into(),
        status: if provider_count > 0 {
            "configured"
        } else {
            "needed"
        },
        message: if provider_count > 0 {
            format!("{provider_count} provider(s) configured")
        } else {
            config
                .models
                .zero_config_ollama_base_url(std::env::var("OLLAMA_HOST").ok().as_deref())
                .map(|base| {
                    format!("no explicit model providers; zero-config Ollama available at {base}")
                })
                .unwrap_or_else(|| "no model providers; using demo echo backend".into())
        },
    });

    // Auth
    let auth = config.gateway.auth_token.is_some();
    items.push(ConfigureItem {
        name: "auth".into(),
        status: if auth { "configured" } else { "needed" },
        message: if auth {
            "bearer auth enabled".into()
        } else {
            "no auth token; gateway is unauthenticated".into()
        },
    });

    // Storage paths
    let sessions_ok = config.paths.sessions_dir.exists();
    items.push(ConfigureItem {
        name: "sessions_dir".into(),
        status: if sessions_ok { "configured" } else { "needed" },
        message: format!(
            "{} ({})",
            config.paths.sessions_dir.display(),
            if sessions_ok { "exists" } else { "missing" }
        ),
    });

    let memory_ok = config.paths.memory_dir.exists();
    items.push(ConfigureItem {
        name: "memory_dir".into(),
        status: if memory_ok { "configured" } else { "needed" },
        message: format!(
            "{} ({})",
            config.paths.memory_dir.display(),
            if memory_ok { "exists" } else { "missing" }
        ),
    });

    // TTS / STT (optional)
    items.push(ConfigureItem {
        name: "tts".into(),
        status: if tts_available {
            "configured"
        } else {
            "skipped"
        },
        message: if tts_available {
            "TTS engine available".into()
        } else {
            "TTS not configured (optional)".into()
        },
    });
    items.push(ConfigureItem {
        name: "stt".into(),
        status: if stt_available {
            "configured"
        } else {
            "skipped"
        },
        message: if stt_available {
            "STT engine available".into()
        } else {
            "STT not configured (optional)".into()
        },
    });

    // Channels (optional)
    let ch = &capabilities.channels;
    items.push(ConfigureItem {
        name: "channels".into(),
        status: if ch.is_empty() {
            "skipped"
        } else {
            "configured"
        },
        message: if ch.is_empty() {
            "no channels enabled (optional)".into()
        } else {
            format!("{} channel(s): {}", ch.len(), ch.join(", "))
        },
    });

    // MCP servers (optional)
    let mcp = capabilities.mcp_servers;
    items.push(ConfigureItem {
        name: "mcp_servers".into(),
        status: if mcp > 0 { "configured" } else { "skipped" },
        message: if mcp > 0 {
            format!("{mcp} MCP server(s) enabled")
        } else {
            "no MCP servers (optional)".into()
        },
    });

    items
}

/// `POST /configure` — inspect configuration and report operator-meaningful status.
pub async fn configure(
    State(state): State<AppState>,
) -> Result<Json<ConfigureGatewayResponse>, GatewayError> {
    let config = state.config.read().await;
    let items = build_configure_items(
        &config,
        &state.capabilities,
        state.tts_engine.is_some(),
        state.stt_engine.is_some(),
    );
    drop(config);

    let needed = items.iter().filter(|i| i.status == "needed").count();
    let success = needed == 0;
    let detail = if success {
        "all required configuration present".into()
    } else {
        format!("{needed} item(s) still need configuration")
    };

    Ok(Json(ConfigureGatewayResponse {
        success,
        detail,
        items,
    }))
}

/// `POST /setup` — alias for configure (first-run setup wizard surface).
pub async fn setup(
    State(state): State<AppState>,
) -> Result<Json<ConfigureGatewayResponse>, GatewayError> {
    configure(State(state)).await
}

/// `GET /update/check` — report the currently running version and whether a newer Git HEAD exists.
pub async fn update_check() -> Result<Json<UpdateCheckResponse>, GatewayError> {
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let detail = if let Some((current, latest)) = git_update_versions() {
        let available = current != latest;
        let detail = if available {
            format!("local checkout is behind Git HEAD ({current} -> {latest})")
        } else {
            format!("local checkout matches Git HEAD ({current})")
        };
        return Ok(Json(UpdateCheckResponse {
            available,
            current_version: current,
            latest_version: Some(latest),
            detail,
        }));
    } else {
        "gateway build is not running from a Git checkout; remote update availability unknown"
            .to_string()
    };

    Ok(Json(UpdateCheckResponse {
        available: false,
        current_version,
        latest_version: None,
        detail,
    }))
}

/// `POST /update/apply` — guide operators to the supported source/self-update flows.
pub async fn update_apply() -> Result<Json<UpdateApplyResponse>, GatewayError> {
    Ok(Json(UpdateApplyResponse {
        success: false,
        detail: "automatic in-process update apply is not supported yet; pull the checkout, rebuild `rune` + `rune-gateway`, then restart the service (or run scripts/self-update.sh from the repo)".to_string(),
    }))
}

/// `GET /update/status` — report the running build version and checkout/source status.
pub async fn update_status() -> Result<Json<UpdateStatusResponse>, GatewayError> {
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let detail = if let Some((current, latest)) = git_update_versions() {
        if current == latest {
            format!("running from Git checkout at HEAD {current}")
        } else {
            format!("running from Git checkout at {current}; latest HEAD is {latest}")
        }
    } else {
        "running from packaged build or detached source; Git HEAD status unavailable".to_string()
    };

    Ok(Json(UpdateStatusResponse {
        current_version,
        detail,
    }))
}

fn git_update_versions() -> Option<(String, String)> {
    use std::process::Command;

    let current = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())?;

    let latest = Command::new("git")
        .args(["rev-parse", "--short=12", "@{upstream}"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| current.clone());

    Some((current, latest))
}

// ── Plugins ──────────────────────────────────────────────────────────────────

pub async fn plugins_list(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, GatewayError> {
    let Some(ref mgr) = state.plugin_manager else {
        return Ok(Json(serde_json::json!({"plugins": []})));
    };
    let plugins = mgr.status().await;
    Ok(Json(serde_json::json!({"plugins": plugins})))
}

pub async fn plugins_get(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, GatewayError> {
    let Some(ref mgr) = state.plugin_manager else {
        return Err(GatewayError::PluginNotFound(
            "plugin manager not initialized".to_string(),
        ));
    };
    match mgr.get_plugin(&name).await {
        Some(plugin) => Ok(Json(serde_json::to_value(plugin).unwrap_or_default())),
        None => Err(GatewayError::PluginNotFound(name)),
    }
}

pub async fn plugins_enable(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, GatewayError> {
    let Some(ref mgr) = state.plugin_manager else {
        return Err(GatewayError::PluginNotFound(
            "plugin manager not initialized".to_string(),
        ));
    };
    let success = mgr.enable(&name).await;
    Ok(Json(serde_json::json!({"success": success})))
}

pub async fn plugins_disable(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, GatewayError> {
    let Some(ref mgr) = state.plugin_manager else {
        return Err(GatewayError::PluginNotFound(
            "plugin manager not initialized".to_string(),
        ));
    };
    let success = mgr.disable(&name).await;
    Ok(Json(serde_json::json!({"success": success})))
}

pub async fn plugins_reload(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, GatewayError> {
    let Some(ref mgr) = state.plugin_manager else {
        return Err(GatewayError::PluginNotFound(
            "plugin manager not initialized".to_string(),
        ));
    };
    let summary = mgr.reload().await;
    Ok(Json(serde_json::json!({
        "success": true,
        "native_plugins": summary.native_plugins,
        "claude_plugins": summary.claude_plugins,
        "skills": summary.skills_registered,
        "agents": summary.agents_registered,
        "commands": summary.commands_registered,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn probe_writable_on_existing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(probe_writable(tmp.path()));
    }

    #[test]
    fn probe_writable_on_missing_dir() {
        let missing = PathBuf::from("/tmp/rune_gateway_test_nonexistent_dir_probe");
        assert!(!probe_writable(&missing));
    }

    #[test]
    fn hint_standalone_missing_path() {
        let p = PathBuf::from("/home/user/.rune/db");
        let hint = gateway_path_hint(&p, &rune_config::RuntimeMode::Standalone, true);
        assert!(
            hint.contains("mkdir -p"),
            "expected mkdir hint, got: {hint}"
        );
    }

    #[test]
    fn hint_standalone_unwritable_path() {
        let p = PathBuf::from("/home/user/.rune/db");
        let hint = gateway_path_hint(&p, &rune_config::RuntimeMode::Standalone, false);
        assert!(hint.contains("chmod"), "expected chmod hint, got: {hint}");
    }

    #[test]
    fn hint_server_missing_path() {
        let p = PathBuf::from("/data/db");
        let hint = gateway_path_hint(&p, &rune_config::RuntimeMode::Server, true);
        assert!(hint.contains("volume"), "expected volume hint, got: {hint}");
    }

    #[test]
    fn hint_server_unwritable_path() {
        let p = PathBuf::from("/data/db");
        let hint = gateway_path_hint(&p, &rune_config::RuntimeMode::Server, false);
        assert!(
            hint.contains("mount flags"),
            "expected mount flags hint, got: {hint}"
        );
    }

    #[test]
    fn storage_checks_pass_for_writable_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().to_path_buf();
        // Create all 9 subdirs
        for sub in &[
            "db", "sessions", "memory", "media", "skills", "plugins", "logs", "backups", "config",
            "secrets",
        ] {
            std::fs::create_dir(base.join(sub)).unwrap();
        }
        let config = rune_config::AppConfig {
            mode: rune_config::RuntimeMode::Standalone,
            paths: rune_config::PathsConfig {
                db_dir: base.join("db"),
                sessions_dir: base.join("sessions"),
                memory_dir: base.join("memory"),
                media_dir: base.join("media"),
                skills_dir: base.join("skills"),
                plugins_dir: base.join("plugins"),
                logs_dir: base.join("logs"),
                backups_dir: base.join("backups"),
                config_dir: base.join("config"),
                secrets_dir: base.join("secrets"),
            },
            ..Default::default()
        };
        let checks = storage_path_checks(&config);
        assert_eq!(checks.len(), 10);
        for c in &checks {
            assert_eq!(
                c.status, "pass",
                "expected pass for {}: {}",
                c.name, c.message
            );
        }
    }

    #[test]
    fn storage_checks_warn_on_missing_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("nonexistent");
        let config = rune_config::AppConfig {
            mode: rune_config::RuntimeMode::Standalone,
            paths: rune_config::PathsConfig {
                db_dir: base.join("db"),
                sessions_dir: base.join("sessions"),
                memory_dir: base.join("memory"),
                media_dir: base.join("media"),
                skills_dir: base.join("skills"),
                plugins_dir: base.join("plugins"),
                logs_dir: base.join("logs"),
                backups_dir: base.join("backups"),
                config_dir: base.join("config"),
                secrets_dir: base.join("secrets"),
            },
            ..Default::default()
        };
        let checks = storage_path_checks(&config);
        for c in &checks {
            assert_eq!(
                c.status, "warn",
                "expected warn for {}: {}",
                c.name, c.message
            );
        }
    }
}
