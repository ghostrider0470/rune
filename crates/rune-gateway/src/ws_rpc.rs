//! WebSocket RPC method dispatcher.
//!
//! Maps method names arriving over the WebSocket req/res protocol to existing
//! service calls backed by [`AppState`]. Each method accepts `serde_json::Value`
//! params and returns `Result<Value, RpcError>`.

use serde_json::{Value, json};
use tracing::warn;
use uuid::Uuid;

use rune_core::SessionKind;
use rune_runtime::SkillScanSummary;

use crate::a2ui::{A2uiActionParams, A2uiEvent, A2uiFormSubmitParams, broadcast_a2ui_event};
use crate::state::AppState;
use crate::ws::active_ws_connections;

// ── Error type ───────────────────────────────────────────────────────────────

/// Error returned from an RPC method.
#[derive(Debug, Clone)]
pub struct RpcError {
    pub code: String,
    pub message: String,
}

impl RpcError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self {
            code: "bad_request".into(),
            message: msg.into(),
        }
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self {
            code: "not_found".into(),
            message: msg.into(),
        }
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self {
            code: "internal".into(),
            message: msg.into(),
        }
    }

    pub fn method_not_found(method: &str) -> Self {
        Self {
            code: "method_not_found".into(),
            message: format!("unknown method: {method}"),
        }
    }
}

// ── Dispatcher ───────────────────────────────────────────────────────────────

/// Holds an [`AppState`] reference and dispatches RPC methods.
pub struct RpcDispatcher {
    state: AppState,
}

#[async_trait::async_trait]
pub trait RpcDispatch: Send + Sync {
    async fn dispatch(&self, method: &str, params: Value) -> Result<Value, RpcError>;
}

impl RpcDispatcher {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    pub async fn dispatch(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        self.dispatch_impl(method, params).await
    }

    /// Dispatch a method call to the appropriate handler.
    ///
    /// `subscribe` and `unsubscribe` are handled at the connection level in
    /// `ws.rs`; they are not routed here.
    async fn dispatch_impl(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        match method {
            "session.list" => self.session_list(params).await,
            "session.get" => self.session_get(params).await,
            "session.create" => self.session_create(params).await,
            "session.send" => self.session_send(params).await,
            "session.transcript" => self.session_transcript(params).await,
            "session.status" => self.session_status(params).await,
            "a2ui.form_submit" => self.a2ui_form_submit(params).await,
            "a2ui.action" => self.a2ui_action(params).await,
            "cron.list" => self.cron_list(params).await,
            "cron.status" => self.cron_status(params).await,
            "runtime.lanes" => self.runtime_lanes().await,
            "skills.list" => self.skills_list().await,
            "skills.reload" => self.skills_reload().await,
            "skills.enable" => self.skills_enable(params).await,
            "skills.disable" => self.skills_disable(params).await,
            "health" => self.health().await,
            "status" => self.full_status().await,
            other => {
                warn!(method = %other, "unknown WS RPC method");
                Err(RpcError::method_not_found(other))
            }
        }
    }

    // ── Session methods ──────────────────────────────────────────────────

    /// List sessions. Optional params: `limit`, `channel`, `active` (minutes).
    async fn session_list(&self, params: Value) -> Result<Value, RpcError> {
        let limit = params
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(100)
            .min(500) as i64;

        let rows = self
            .state
            .session_repo
            .list(limit, 0)
            .await
            .map_err(|e| RpcError::internal(e.to_string()))?;

        let channel_filter = params.get("channel").and_then(|v| v.as_str());

        let active_cutoff = params
            .get("active")
            .and_then(|v| v.as_u64())
            .map(|minutes| chrono::Utc::now() - chrono::Duration::minutes(minutes as i64));

        let items: Vec<Value> = rows
            .into_iter()
            .filter(|row| {
                channel_filter
                    .map(|ch| row.channel_ref.as_deref() == Some(ch))
                    .unwrap_or(true)
            })
            .filter(|row| {
                active_cutoff
                    .map(|cutoff| row.last_activity_at >= cutoff)
                    .unwrap_or(true)
            })
            .map(|row| {
                json!({
                    "id": row.id.to_string(),
                    "kind": row.kind,
                    "status": row.status,
                    "channel": row.channel_ref,
                    "created_at": row.created_at.to_rfc3339(),
                })
            })
            .collect();

        Ok(json!(items))
    }

    /// Get a single session. Params: `session_id` (required).
    async fn session_get(&self, params: Value) -> Result<Value, RpcError> {
        let session_id = require_uuid(&params, "session_id")?;

        let row = self
            .state
            .session_engine
            .get_session(session_id)
            .await
            .map_err(|e| RpcError::not_found(e.to_string()))?;

        let turns = self
            .state
            .turn_repo
            .list_by_session(row.id)
            .await
            .map_err(|e| RpcError::internal(e.to_string()))?;

        let turn_count = turns.len() as u64;
        let prompt_tokens: u64 = turns
            .iter()
            .map(|t| t.usage_prompt_tokens.unwrap_or(0).max(0) as u64)
            .sum();
        let completion_tokens: u64 = turns
            .iter()
            .map(|t| t.usage_completion_tokens.unwrap_or(0).max(0) as u64)
            .sum();
        let latest_model = turns
            .iter()
            .max_by_key(|t| t.started_at)
            .and_then(|t| t.model_ref.clone());

        let latest_turn = turns.iter().max_by_key(|t| t.started_at);
        let last_turn_started_at = latest_turn.map(|t| t.started_at.to_rfc3339());
        let last_turn_ended_at = latest_turn.and_then(|t| t.ended_at.map(|dt| dt.to_rfc3339()));

        Ok(json!({
            "id": row.id,
            "kind": row.kind,
            "status": row.status,
            "requester_session_id": row.requester_session_id,
            "channel_ref": row.channel_ref,
            "created_at": row.created_at.to_rfc3339(),
            "updated_at": row.updated_at.to_rfc3339(),
            "turn_count": turn_count,
            "latest_model": latest_model,
            "usage_prompt_tokens": prompt_tokens,
            "usage_completion_tokens": completion_tokens,
            "last_turn_started_at": last_turn_started_at,
            "last_turn_ended_at": last_turn_ended_at,
        }))
    }

    /// Create a new session. Params: `kind` (optional, default "direct"),
    /// `workspace_root`, `requester_session_id`, `channel_ref`.
    async fn session_create(&self, params: Value) -> Result<Value, RpcError> {
        let kind_str = params
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("direct");
        let kind = parse_session_kind(kind_str)?;

        let workspace_root = params
            .get("workspace_root")
            .and_then(|v| v.as_str())
            .map(String::from);
        let requester_session_id = params
            .get("requester_session_id")
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok());
        let channel_ref = params
            .get("channel_ref")
            .and_then(|v| v.as_str())
            .map(String::from);

        let row = self
            .state
            .session_engine
            .create_session_full(kind, workspace_root, requester_session_id, channel_ref)
            .await
            .map_err(|e| RpcError::internal(e.to_string()))?;

        // Broadcast creation event.
        let _ = self.state.event_tx.send(crate::state::SessionEvent {
            session_id: row.id.to_string(),
            kind: "session_created".to_string(),
            payload: json!({
                "session_id": row.id,
                "kind": row.kind,
                "status": row.status,
            }),
            state_changed: true,
        });

        Ok(json!({
            "id": row.id,
            "kind": row.kind,
            "status": row.status,
            "created_at": row.created_at.to_rfc3339(),
        }))
    }

    /// Send a message to a session. Params: `session_id` (required), `content` (required), `model` (optional).
    async fn session_send(&self, params: Value) -> Result<Value, RpcError> {
        let session_id = require_uuid(&params, "session_id")?;
        let content = params
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::bad_request("missing required param: content"))?;
        let model = params.get("model").and_then(|v| v.as_str());

        // Verify session exists.
        self.state
            .session_engine
            .get_session(session_id)
            .await
            .map_err(|e| RpcError::not_found(e.to_string()))?;

        let started = std::time::Instant::now();

        let (turn_row, usage) = self
            .state
            .turn_executor
            .execute(session_id, content, model)
            .await
            .map_err(|e| RpcError::internal(e.to_string()))?;

        // Extract assistant reply from transcript.
        let transcript = self
            .state
            .transcript_repo
            .list_by_session(session_id)
            .await
            .map_err(|e| RpcError::internal(e.to_string()))?;

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

        // Broadcast turn completion event.
        let _ = self.state.event_tx.send(crate::state::SessionEvent {
            session_id: session_id.to_string(),
            kind: "turn_completed".to_string(),
            payload: json!({
                "session_id": session_id,
                "turn_id": turn_row.id,
                "assistant_reply": assistant_reply,
                "prompt_tokens": usage.prompt_tokens,
                "completion_tokens": usage.completion_tokens,
            }),
            state_changed: true,
        });

        Ok(json!({
            "turn_id": turn_row.id,
            "assistant_reply": assistant_reply,
            "usage": {
                "prompt_tokens": usage.prompt_tokens,
                "completion_tokens": usage.completion_tokens,
            },
            "latency_ms": started.elapsed().as_millis() as u64,
        }))
    }

    /// Get session transcript. Params: `session_id` (required).
    async fn session_transcript(&self, params: Value) -> Result<Value, RpcError> {
        let session_id = require_uuid(&params, "session_id")?;

        // Verify session exists.
        self.state
            .session_engine
            .get_session(session_id)
            .await
            .map_err(|e| RpcError::not_found(e.to_string()))?;

        let items = self
            .state
            .transcript_repo
            .list_by_session(session_id)
            .await
            .map_err(|e| RpcError::internal(e.to_string()))?;

        let entries: Vec<Value> = items
            .into_iter()
            .map(|item| {
                json!({
                    "id": item.id,
                    "turn_id": item.turn_id,
                    "seq": item.seq,
                    "kind": item.kind,
                    "payload": item.payload,
                    "created_at": item.created_at.to_rfc3339(),
                })
            })
            .collect();

        Ok(json!(entries))
    }

    /// Get session status card. Params: `session_id` (required).
    async fn session_status(&self, params: Value) -> Result<Value, RpcError> {
        let session_id = require_uuid(&params, "session_id")?;

        let row = self
            .state
            .session_engine
            .get_session(session_id)
            .await
            .map_err(|e| RpcError::not_found(e.to_string()))?;

        let turns = self
            .state
            .turn_repo
            .list_by_session(row.id)
            .await
            .map_err(|e| RpcError::internal(e.to_string()))?;

        let turn_count = turns.len() as u64;
        let prompt_tokens: u64 = turns
            .iter()
            .map(|t| t.usage_prompt_tokens.unwrap_or(0).max(0) as u64)
            .sum();
        let completion_tokens: u64 = turns
            .iter()
            .map(|t| t.usage_completion_tokens.unwrap_or(0).max(0) as u64)
            .sum();
        let latest_turn = turns.iter().max_by_key(|t| t.started_at);
        let latest_model = latest_turn.and_then(|t| t.model_ref.clone());
        let last_turn_started_at = latest_turn.map(|t| t.started_at.to_rfc3339());
        let last_turn_ended_at = latest_turn.and_then(|t| t.ended_at.map(|dt| dt.to_rfc3339()));

        let metadata = &row.metadata;
        let model_override = metadata_str(metadata, "selected_model").map(str::to_string);
        let current_model = latest_model.clone().or_else(|| model_override.clone());
        let approval_mode = metadata_str(metadata, "approval_mode").unwrap_or("on-miss");
        let security_mode = metadata_str(metadata, "security_mode").unwrap_or("allowlist");
        let reasoning = metadata_str(metadata, "reasoning").unwrap_or("off");
        let verbose = metadata
            .get("verbose")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let elevated = metadata
            .get("elevated")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let subagent_lifecycle = metadata_str(metadata, "subagent_lifecycle");
        let subagent_runtime_status = metadata_str(metadata, "subagent_runtime_status");
        let subagent_runtime_attached = metadata
            .get("subagent_runtime_attached")
            .and_then(Value::as_bool);
        let subagent_status_updated_at = metadata_str(metadata, "subagent_status_updated_at");
        let subagent_last_note = metadata_str(metadata, "subagent_last_note");

        let mut unresolved =
            vec!["cost posture is estimate-only; provider pricing is not wired yet".to_string()];
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

        Ok(json!({
            "session_id": row.id.to_string(),
            "runtime": format!(
                "kind={} | channel={} | status={}",
                row.kind,
                row.channel_ref.as_deref().unwrap_or("local"),
                row.status
            ),
            "status": row.status,
            "kind": row.kind,
            "channel_ref": row.channel_ref,
            "current_model": current_model,
            "model_override": model_override,
            "turn_count": turn_count,
            "prompt_tokens": prompt_tokens,
            "completion_tokens": completion_tokens,
            "total_tokens": prompt_tokens + completion_tokens,
            "estimated_cost": "not available",
            "uptime_seconds": self.state.started_at.elapsed().as_secs(),
            "last_turn_started_at": last_turn_started_at,
            "last_turn_ended_at": last_turn_ended_at,
            "approval_mode": approval_mode,
            "security_mode": security_mode,
            "reasoning": reasoning,
            "verbose": verbose,
            "elevated": elevated,
            "subagent_lifecycle": subagent_lifecycle,
            "subagent_runtime_status": subagent_runtime_status,
            "subagent_runtime_attached": subagent_runtime_attached,
            "subagent_status_updated_at": subagent_status_updated_at,
            "subagent_last_note": subagent_last_note,
            "unresolved": unresolved,
        }))
    }

    async fn a2ui_form_submit(&self, params: Value) -> Result<Value, RpcError> {
        let params: A2uiFormSubmitParams =
            serde_json::from_value(params).map_err(|err| RpcError::bad_request(err.to_string()))?;
        let event = A2uiEvent::FormSubmit {
            session_id: params.session_id.clone(),
            callback_id: params.callback_id,
            data: params.data,
            timestamp: chrono::Utc::now(),
        };
        broadcast_a2ui_event(&self.state.event_tx, &event).map_err(RpcError::internal)?;

        Ok(json!({
            "accepted": true,
            "message": "Form submitted to agent event bus",
            "session_id": params.session_id,
        }))
    }

    async fn a2ui_action(&self, params: Value) -> Result<Value, RpcError> {
        let params: A2uiActionParams =
            serde_json::from_value(params).map_err(|err| RpcError::bad_request(err.to_string()))?;
        let event = A2uiEvent::Action {
            session_id: params.session_id.clone(),
            component_id: params.component_id.clone(),
            action_target: params.action_target.clone(),
            timestamp: chrono::Utc::now(),
        };
        broadcast_a2ui_event(&self.state.event_tx, &event).map_err(RpcError::internal)?;

        Ok(json!({
            "accepted": true,
            "message": "Action submitted to agent event bus",
            "session_id": params.session_id,
            "component_id": params.component_id,
            "action_target": params.action_target,
        }))
    }

    // ── Cron methods ─────────────────────────────────────────────────────

    /// List cron jobs. Optional params: `include_disabled` (bool).
    async fn cron_list(&self, params: Value) -> Result<Value, RpcError> {
        let include_disabled = params
            .get("include_disabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let jobs = self.state.scheduler.list_jobs(include_disabled).await;

        let items: Vec<Value> = jobs
            .into_iter()
            .map(|job| {
                json!({
                    "id": job.id.to_string(),
                    "name": job.name,
                    "enabled": job.enabled,
                    "created_at": job.created_at.to_rfc3339(),
                    "last_run_at": job.last_run_at.map(|dt| dt.to_rfc3339()),
                    "next_run_at": job.next_run_at.map(|dt| dt.to_rfc3339()),
                    "run_count": job.run_count,
                })
            })
            .collect();

        Ok(json!(items))
    }

    /// Cron scheduler status summary.
    async fn cron_status(&self, _params: Value) -> Result<Value, RpcError> {
        let jobs = self.state.scheduler.list_jobs(true).await;
        let due_jobs = self.state.scheduler.get_due_jobs().await;

        Ok(json!({
            "total_jobs": jobs.len(),
            "enabled_jobs": jobs.iter().filter(|j| j.enabled).count(),
            "due_jobs": due_jobs.len(),
        }))
    }

    async fn skills_list(&self) -> Result<Value, RpcError> {
        let mut skills = self.state.skill_registry.list().await;
        skills.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(json!(
            skills
                .into_iter()
                .map(|skill| json!({
                    "name": skill.name,
                    "description": skill.description,
                    "enabled": skill.enabled,
                    "source_dir": skill.source_dir.display().to_string(),
                    "binary_path": skill.binary_path.map(|path| path.display().to_string()),
                }))
                .collect::<Vec<_>>()
        ))
    }

    async fn skills_reload(&self) -> Result<Value, RpcError> {
        let summary = self.state.skill_loader.scan_summary().await;
        Ok(skill_reload_json(summary))
    }

    async fn skills_enable(&self, params: Value) -> Result<Value, RpcError> {
        let name = require_string(&params, "name")?;
        if self.state.skill_registry.enable(name).await {
            Ok(json!({ "name": name, "enabled": true }))
        } else {
            Err(RpcError::not_found(format!("unknown skill: {name}")))
        }
    }

    async fn skills_disable(&self, params: Value) -> Result<Value, RpcError> {
        let name = require_string(&params, "name")?;
        if self.state.skill_registry.disable(name).await {
            Ok(json!({ "name": name, "enabled": false }))
        } else {
            Err(RpcError::not_found(format!("unknown skill: {name}")))
        }
    }

    // ── System methods ───────────────────────────────────────────────────

    /// Current runtime lane utilisation and capacities.
    async fn runtime_lanes(&self) -> Result<Value, RpcError> {
        let lane_stats = self.state.turn_executor.lane_stats().map(|stats| {
            json!({
                "main": {
                    "active": stats.main_active,
                    "capacity": stats.main_capacity,
                },
                "subagent": {
                    "active": stats.subagent_active,
                    "capacity": stats.subagent_capacity,
                },
                "cron": {
                    "active": stats.cron_active,
                    "capacity": stats.cron_capacity,
                },
            })
        });

        Ok(json!({
            "enabled": lane_stats.is_some(),
            "lanes": lane_stats,
        }))
    }

    /// Health check.
    async fn health(&self) -> Result<Value, RpcError> {
        let sessions = self
            .state
            .session_repo
            .list(i64::MAX / 4, 0)
            .await
            .map_err(|e| RpcError::internal(e.to_string()))?;

        Ok(json!({
            "status": "ok",
            "service": "rune-gateway",
            "version": env!("CARGO_PKG_VERSION"),
            "uptime_seconds": self.state.started_at.elapsed().as_secs(),
            "session_count": sessions.len(),
            "ws_subscribers": self.state.event_tx.receiver_count(),
            "ws_connections": active_ws_connections(),
        }))
    }

    /// Full daemon status.
    async fn full_status(&self) -> Result<Value, RpcError> {
        let sessions = self
            .state
            .session_repo
            .list(i64::MAX / 4, 0)
            .await
            .map_err(|e| RpcError::internal(e.to_string()))?;
        let cron_job_count = self.state.scheduler.list_jobs(true).await.len();
        let skills = self.state.skill_registry.list().await;
        let lane_stats = self.state.turn_executor.lane_stats().map(|stats| {
            json!({
                "main_active": stats.main_active,
                "main_capacity": stats.main_capacity,
                "subagent_active": stats.subagent_active,
                "subagent_capacity": stats.subagent_capacity,
                "cron_active": stats.cron_active,
                "cron_capacity": stats.cron_capacity,
            })
        });

        let config = self.state.config.read().await;
        Ok(json!({
            "status": "running",
            "version": env!("CARGO_PKG_VERSION"),
            "bind": format!("{}:{}", config.gateway.host, config.gateway.port),
            "auth_enabled": config.gateway.auth_token.is_some(),
            "configured_model_providers": config.models.providers.len(),
            "registered_tools": self.state.capabilities.tool_count,
            "session_count": sessions.len(),
            "cron_job_count": cron_job_count,
            "ws_subscribers": self.state.event_tx.receiver_count(),
            "ws_connections": active_ws_connections(),
            "uptime_seconds": self.state.started_at.elapsed().as_secs(),
            "lane_stats": lane_stats,
            "skills": {
                "loaded": skills.len(),
                "enabled": skills.iter().filter(|skill| skill.enabled).count(),
                "skills_dir": self.state.skill_loader.skills_dir().display().to_string(),
            },
            "config_paths": {
                "sessions_dir": config.paths.sessions_dir.display().to_string(),
                "memory_dir": config.paths.memory_dir.display().to_string(),
                "logs_dir": config.paths.logs_dir.display().to_string(),
            },
        }))
    }
}

#[async_trait::async_trait]
impl RpcDispatch for RpcDispatcher {
    async fn dispatch(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        self.dispatch_impl(method, params).await
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Extract a required UUID parameter from params.
fn require_uuid(params: &Value, key: &str) -> Result<Uuid, RpcError> {
    let raw = params
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| RpcError::bad_request(format!("missing required param: {key}")))?;
    Uuid::parse_str(raw)
        .map_err(|_| RpcError::bad_request(format!("invalid UUID for {key}: {raw}")))
}

fn require_string<'a>(params: &'a Value, key: &str) -> Result<&'a str, RpcError> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| RpcError::bad_request(format!("missing required param: {key}")))
}

fn skill_reload_json(summary: SkillScanSummary) -> Value {
    json!({
        "success": true,
        "discovered": summary.discovered,
        "loaded": summary.loaded,
        "removed": summary.removed,
    })
}

/// Extract a string value from a JSON metadata object.
fn metadata_str<'a>(metadata: &'a Value, key: &str) -> Option<&'a str> {
    metadata
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

/// Parse a session kind string.
fn parse_session_kind(s: &str) -> Result<SessionKind, RpcError> {
    match s.to_lowercase().as_str() {
        "direct" => Ok(SessionKind::Direct),
        "channel" => Ok(SessionKind::Channel),
        "scheduled" => Ok(SessionKind::Scheduled),
        "subagent" => Ok(SessionKind::Subagent),
        other => Err(RpcError::bad_request(format!(
            "unknown session kind: {other}"
        ))),
    }
}
