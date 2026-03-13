//! HTTP route handlers for the gateway API.

use std::sync::Arc;
use std::time::Instant;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::info;
use uuid::Uuid;

use rune_core::SessionKind;

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

    Ok(Json(StatusResponse {
        status: "running",
        version: env!("CARGO_PKG_VERSION"),
        bind: format!("{}:{}", state.config.gateway.host, state.config.gateway.port),
        auth_enabled: state.config.gateway.auth_token.is_some(),
        configured_model_providers: state.config.models.providers.len(),
        active_model_backend: "in-memory-demo",
        registered_tools: state.tool_count,
        session_count: sessions.len(),
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
        .and_then(|t| t.payload.get("content").and_then(|v| v.as_str()).map(String::from));

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
