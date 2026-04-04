//! WebSocket RPC method dispatcher.
//!
//! Maps method names arriving over the WebSocket req/res protocol to existing
//! service calls backed by [`AppState`]. Each method accepts `serde_json::Value`
//! params and returns `Result<Value, RpcError>`.

use serde_json::{Value, json};
use std::time::Duration;
use tracing::warn;
use uuid::Uuid;

use rune_core::SessionKind;
use rune_runtime::SkillScanSummary;
use rune_tools::{ToolCall, ToolExecutor};

use crate::a2ui::{A2uiActionParams, A2uiEvent, A2uiFormSubmitParams, broadcast_a2ui_event};
use crate::events::{RuntimeEvent, TurnEvent, UsageSummary, broadcast_runtime_event};
use crate::routes::{session_next_task_reason, session_resume_hint, session_status_reason};
use crate::state::AppState;
use crate::ws::active_ws_connections;

// ── Error type ───────────────────────────────────────────────────────────────

/// Error returned from an RPC method.
#[derive(Debug, Clone)]
pub struct RpcError {
    pub code: String,
    pub message: String,
    pub data: Option<Value>,
}

impl RpcError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self {
            code: "bad_request".into(),
            message: msg.into(),
            data: None,
        }
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self {
            code: "not_found".into(),
            message: msg.into(),
            data: None,
        }
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self {
            code: "internal".into(),
            message: msg.into(),
            data: None,
        }
    }

    pub fn method_not_found(method: &str) -> Self {
        Self {
            code: "method_not_found".into(),
            message: format!("unknown method: {method}"),
            data: None,
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
            "session.resolve" => self.session_resolve(params).await,
            "session.create" => self.session_create(params).await,
            "session.send" => self.session_send(params).await,
            "session.transcript" => self.session_transcript(params).await,
            "session.status" => self.session_status(params).await,
            "a2ui.form_submit" => self.a2ui_form_submit(params).await,
            "a2ui.action" => self.a2ui_action(params).await,
            "cron.list" => self.cron_list(params).await,
            "cron.get" => self.cron_get(params).await,
            "cron.status" => self.cron_status(params).await,
            "runtime.lanes" => self.runtime_lanes().await,
            "runtime.context_budget" => self.runtime_context_budget(params).await,
            "runtime.context_budget.gc" => self.runtime_context_budget_gc(params).await,
            "agent.steer" => self.agent_steer(params).await,
            "agent.kill" => self.agent_kill(params).await,
            "skills.list" => self.skills_list().await,
            "skills.reload" => self.skills_reload().await,
            "skills.enable" => self.skills_enable(params).await,
            "skills.disable" => self.skills_disable(params).await,
            "health" => self.health().await,
            "status" => self.full_status().await,
            // ── Parity WS-RPC methods (#39) ─────────────────────────────
            "turns.list" => self.turns_list(params).await,
            "turns.get" => self.turns_get(params).await,
            "tools.list" => self.tools_list().await,
            "tools.get" => self.tools_get(params).await,
            "approvals.list" => self.approvals_list().await,
            "approvals.decide" => self.approvals_decide(params).await,
            "processes.list" => self.processes_list().await,
            "processes.get" => self.processes_get(params).await,
            "processes.log" => self.processes_log(params).await,
            "processes.kill" => self.processes_kill(params).await,
            "channels.list" => self.channels_list().await,
            "channels.status" => self.channels_status().await,
            "auth.info" => self.auth_info().await,
            "memory.status" => self.memory_status().await,
            "memory.search" => self.memory_search(params).await,
            "memory.graph" => self.memory_graph(params).await,
            "logs.query" => self.logs_query(params).await,
            "doctor.run" => self.doctor_run().await,
            "doctor.results" => self.doctor_results().await,
            "dashboard.summary" => self.dashboard_summary().await,
            "dashboard.models" => self.dashboard_models().await,
            "dashboard.sessions" => self.dashboard_sessions().await,
            "dashboard.diagnostics" => self.dashboard_diagnostics().await,
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
        let browser_session = params
            .get("browser_session")
            .and_then(|v| v.as_str())
            .filter(|value| !value.is_empty());

        let active_cutoff = params
            .get("active")
            .and_then(|v| v.as_u64())
            .map(|minutes| chrono::Utc::now() - chrono::Duration::minutes(minutes as i64));

        let include_metadata = params
            .get("include_metadata")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let items: Vec<Value> = rows
            .into_iter()
            .filter(|row| {
                channel_filter
                    .map(|ch| row.channel_ref.as_deref() == Some(ch))
                    .unwrap_or(true)
            })
            .filter(|row| {
                browser_session
                    .map(|token| row.channel_ref.as_deref() == Some(&format!("webchat:{token}")))
                    .unwrap_or(true)
            })
            .filter(|row| {
                active_cutoff
                    .map(|cutoff| row.last_activity_at >= cutoff)
                    .unwrap_or(true)
            })
            .map(|row| {
                let mut item = json!({
                    "id": row.id.to_string(),
                    "kind": row.kind,
                    "status": row.status,
                    "channel": row.channel_ref,
                    "created_at": row.created_at.to_rfc3339(),
                });
                if include_metadata {
                    item["metadata"] = row.metadata;
                }
                item
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
    /// Resolve or create a session for a durable channel reference. Params:
    /// `channel_ref` (required), `kind` (optional, default "channel"),
    /// `workspace_root`, `requester_session_id`, `metadata`.
    async fn session_resolve(&self, params: Value) -> Result<Value, RpcError> {
        let channel_ref = require_string(&params, "channel_ref")?;
        let kind_str = params
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("channel");
        let kind = parse_session_kind(kind_str)?;

        if let Some(existing) = self
            .state
            .session_engine
            .get_session_by_channel_ref(channel_ref)
            .await
            .map_err(|e| RpcError::internal(e.to_string()))?
        {
            let mut metadata = existing.metadata.clone();
            if let Some(patch) = params.get("metadata") {
                merge_json_object(&mut metadata, patch.clone())?;
                self.state
                    .session_engine
                    .patch_metadata(existing.id, metadata.clone())
                    .await
                    .map_err(|e| RpcError::internal(e.to_string()))?;
            }

            return Ok(json!({
                "id": existing.id,
                "kind": existing.kind,
                "status": existing.status,
                "created_at": existing.created_at.to_rfc3339(),
                "resumed": true,
                "metadata": metadata,
            }));
        }

        let workspace_root = params
            .get("workspace_root")
            .and_then(|v| v.as_str())
            .map(String::from);
        let requester_session_id = params
            .get("requester_session_id")
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok());

        let row = self
            .state
            .session_engine
            .create_session_full(
                kind,
                workspace_root,
                requester_session_id,
                Some(channel_ref.to_string()),
                None,
                None,
            )
            .await
            .map_err(|e| RpcError::internal(e.to_string()))?;

        if let Some(metadata) = params.get("metadata") {
            self.state
                .session_engine
                .patch_metadata(row.id, metadata.clone())
                .await
                .map_err(|e| RpcError::internal(e.to_string()))?;
        }

        let _ = self.state.event_tx.send(crate::state::SessionEvent {
            session_id: row.id.to_string(),
            kind: "session_created".to_string(),
            payload: json!({
                "session_id": row.id,
                "kind": row.kind,
                "status": row.status,
                "channel_ref": row.channel_ref,
            }),
            state_changed: true,
        });

        Ok(json!({
            "id": row.id,
            "kind": row.kind,
            "status": row.status,
            "created_at": row.created_at.to_rfc3339(),
            "resumed": false,
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
        let mode = params
            .get("mode")
            .and_then(|v| v.as_str())
            .map(String::from);

        let row = self
            .state
            .session_engine
            .create_session_full(
                kind,
                workspace_root,
                requester_session_id,
                channel_ref,
                mode,
                None,
            )
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
        let session = self
            .state
            .session_engine
            .get_session(session_id)
            .await
            .map_err(|e| RpcError::not_found(e.to_string()))?;
        let content = params
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| RpcError::bad_request("missing required param: content"))?;
        let model = params.get("model").and_then(|v| v.as_str());

        let rate_limit_key = session
            .channel_ref
            .as_deref()
            .and_then(|channel| channel.strip_prefix("webchat:"))
            .filter(|token| !token.is_empty() && *token != "anonymous")
            .map(str::to_owned)
            .unwrap_or_else(|| format!("session:{}", session_id));
        if let Err(retry_after) = self.state.webchat_rate_limiter.check(rate_limit_key).await {
            return Err(rate_limited_error(retry_after));
        }

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

        // Broadcast typed turn completion event.
        let _ = broadcast_runtime_event(
            &self.state.event_tx,
            RuntimeEvent::Turn(TurnEvent::Completed {
                session_id,
                turn_id: turn_row.id,
                usage: Some(UsageSummary {
                    prompt_tokens: u64::from(usage.prompt_tokens),
                    completion_tokens: u64::from(usage.completion_tokens),
                }),
            }),
        );

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
        let status_reason = session_status_reason(&row.status, metadata, approval_mode);
        let next_task_reason = session_next_task_reason(&row.status, metadata);
        let resume_hint = session_resume_hint(&row.status, metadata);
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
        let parent_session_id = row.requester_session_id.map(|id| id.to_string());
        let session_mode = metadata_str(metadata, "mode");
        let orchestration_status = metadata_str(metadata, "orchestration_status")
            .or_else(|| metadata_str(metadata, "subagent_lifecycle"));
        let delegation_roles = metadata
            .get("delegation_roles")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let delegation_depth = if parent_session_id.is_some() {
            Some(
                metadata
                    .get("delegation_depth")
                    .and_then(Value::as_u64)
                    .map(|value| value as u32)
                    .unwrap_or(1),
            )
        } else {
            metadata
                .get("delegation_depth")
                .and_then(Value::as_u64)
                .map(|value| value as u32)
        };
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
            unresolved
                .push(rune_runtime::restart_continuity::RESTART_CONTINUITY_SUMMARY.to_string());
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
            "status_reason": status_reason,
            "next_task_reason": next_task_reason,
            "resume_hint": resume_hint,
            "kind": row.kind,
            "channel_ref": row.channel_ref,
            "parent_session_id": parent_session_id,
            "session_mode": session_mode,
            "orchestration_status": orchestration_status,
            "delegation_roles": delegation_roles,
            "delegation_depth": delegation_depth,
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

    // ── Agent (subagent) control methods ───────────────────────────────────

    /// Steer a running subagent. Params: `session_id` (required), `message` (required).
    async fn agent_steer(&self, params: Value) -> Result<Value, RpcError> {
        let session_id = require_uuid(&params, "session_id")?;
        let message = require_string(&params, "message")?;

        let session = self
            .state
            .session_repo
            .find_by_id(session_id)
            .await
            .map_err(|_| RpcError::not_found(format!("agent session {session_id} not found")))?;

        let now = chrono::Utc::now();
            let parent_session_id = session
            .requester_session_id
            .map(|parent| parent.to_string());
        let note = format!("[steer] operator instruction injected: {message}");

        self.state
            .transcript_repo
            .append(rune_store::models::NewTranscriptItem {
                id: Uuid::now_v7(),
                session_id,
                turn_id: None,
                seq: 0,
                kind: "status_note".into(),
                payload: json!({ "content": note }),
                created_at: now,
            })
            .await
            .map_err(|e| RpcError::internal(e.to_string()))?;

        let mut metadata = session.metadata.clone();
        metadata["subagent_lifecycle"] = json!("steered");
        metadata["subagent_runtime_status"] = json!("running");
        metadata["subagent_runtime_attached"] = json!(true);
        metadata["subagent_status_updated_at"] = json!(now.to_rfc3339());
        metadata["subagent_last_note"] = json!(note);

        self.state
            .session_repo
            .update_metadata(session_id, metadata, now)
            .await
            .map_err(|e| RpcError::internal(e.to_string()))?;

        Ok(json!({
            "session_id": session_id.to_string(),
            "parent_session_id": parent_session_id,
            "accepted": true,
            "detail": format!("steering instruction delivered to session {session_id}"),
        }))
    }

    /// Kill/cancel a running subagent. Params: `session_id` (required), `reason` (optional).
    async fn agent_kill(&self, params: Value) -> Result<Value, RpcError> {
        let session_id = require_uuid(&params, "session_id")?;
        let reason = params
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("operator-initiated");

        let session = self
            .state
            .session_repo
            .find_by_id(session_id)
            .await
            .map_err(|_| RpcError::not_found(format!("agent session {session_id} not found")))?;

        let now = chrono::Utc::now();
        let previous_status = session.status.clone();
        let parent_session_id = session
            .requester_session_id
            .map(|parent| parent.to_string());
        let note = format!("[kill] session cancelled: {reason}");

        let current = self
            .state
            .session_repo
            .find_by_id(session_id)
            .await
            .map_err(|_| RpcError::not_found(format!("agent session {session_id} not found")))?;

        let current_status = current
            .status
            .parse::<rune_core::SessionStatus>()
            .map_err(|_| {
                RpcError::internal(format!(
                    "invalid persisted session status: {}",
                    current.status
                ))
            })?;
        current_status
            .transition(rune_core::SessionStatus::Cancelled)
            .map_err(|e| RpcError::bad_request(e.to_string()))?;

        self.state
            .session_repo
            .update_status(session_id, "cancelled", now)
            .await
            .map_err(|e| RpcError::internal(e.to_string()))?;

        self.state
            .transcript_repo
            .append(rune_store::models::NewTranscriptItem {
                id: Uuid::now_v7(),
                session_id,
                turn_id: None,
                seq: 0,
                kind: "status_note".into(),
                payload: json!({ "content": note }),
                created_at: now,
            })
            .await
            .map_err(|e| RpcError::internal(e.to_string()))?;

        let mut metadata = session.metadata.clone();
        metadata["subagent_lifecycle"] = json!("cancelled");
        metadata["subagent_runtime_status"] = json!("stopped");
        metadata["subagent_runtime_attached"] = json!(false);
        metadata["subagent_status_updated_at"] = json!(now.to_rfc3339());
        metadata["subagent_last_note"] = json!(note);

        self.state
            .session_repo
            .update_metadata(session_id, metadata, now)
            .await
            .map_err(|e| RpcError::internal(e.to_string()))?;

        Ok(json!({
            "session_id": session_id.to_string(),
            "parent_session_id": parent_session_id,
            "killed": true,
            "detail": format!("session {session_id} cancelled: {reason}"),
            "previous_status": previous_status,
            "cancelled_at": now.to_rfc3339(),
            "can_resume": false,
        }))
    }

    // ── Cron methods ─────────────────────────────────────────────────────

    /// List cron jobs. Optional params: `include_disabled` (bool).
    async fn cron_list(&self, params: Value) -> Result<Value, RpcError> {
        let include_disabled = params
            .get("include_disabled")
            .or_else(|| params.get("includeDisabled"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let jobs = self.state.scheduler.list_jobs(include_disabled).await;

        let items: Vec<Value> = jobs
            .into_iter()
            .map(|job| {
                json!({
                    "id": job.id.to_string(),
                    "name": job.name,
                    "schedule": job.schedule,
                    "payload": job.payload,
                    "delivery_mode": job.delivery_mode,
                    "webhook_url": job.webhook_url,
                    "session_target": job.session_target,
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

    /// Inspect a single cron job. Params: `id` or `job_id` (required UUID).
    async fn cron_get(&self, params: Value) -> Result<Value, RpcError> {
        let job_id = params
            .get("id")
            .or_else(|| params.get("job_id"))
            .and_then(|value| value.as_str())
            .ok_or_else(|| RpcError::bad_request("missing required field: id"))?;
        let job_id = Uuid::parse_str(job_id)
            .map_err(|err| RpcError::bad_request(format!("invalid job id: {err}")))?;
        let job_id = rune_core::JobId::from(job_id);

        let job = self
            .state
            .scheduler
            .get_job(&job_id)
            .await
            .ok_or_else(|| RpcError::not_found(format!("job not found: {job_id}")))?;

        Ok(json!({
            "id": job.id.to_string(),
            "name": job.name,
            "schedule": job.schedule,
            "payload": job.payload,
            "delivery_mode": job.delivery_mode,
            "webhook_url": job.webhook_url,
            "session_target": job.session_target,
            "enabled": job.enabled,
            "created_at": job.created_at.to_rfc3339(),
            "last_run_at": job.last_run_at.map(|dt| dt.to_rfc3339()),
            "next_run_at": job.next_run_at.map(|dt| dt.to_rfc3339()),
            "run_count": job.run_count,
        }))
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
                "priority": {
                    "active": stats.priority_active,
                    "capacity": stats.priority_capacity,
                },
                "subagent": {
                    "active": stats.subagent_active,
                    "capacity": stats.subagent_capacity,
                },
                "cron": {
                    "active": stats.cron_active,
                    "capacity": stats.cron_capacity,
                },
                "heartbeat": {
                    "active": stats.heartbeat_active,
                    "capacity": stats.heartbeat_capacity,
                },
                "tools": {
                    "active": stats.tool_active,
                    "capacity": stats.tool_capacity,
                    "per_project_capacity": stats.project_tool_capacity,
                },
            })
        });

        Ok(json!({
            "enabled": lane_stats.is_some(),
            "lanes": lane_stats,
        }))
    }

    async fn runtime_context_budget(&self, params: Value) -> Result<Value, RpcError> {
        let mut budget = build_context_budget(&params)?;
        let report = rune_runtime::BudgetReport::from(&budget);
        let checkpoint = params.get("checkpoint").map(|value| {
            let status = value
                .get("status")
                .and_then(|inner| inner.as_str())
                .unwrap_or("ready");
            let key_decisions = value
                .get("key_decisions")
                .and_then(|inner| inner.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let next_step = value
                .get("next_step")
                .and_then(|inner| inner.as_str())
                .unwrap_or("continue");
            budget.create_checkpoint(status, key_decisions, next_step)
        });

        let config = self.state.config.read().await;
        let context = &config.context;
        let compaction = &config.runtime.compaction;

        Ok(json!({
            "report": report,
            "assembly": {
                "total_budget": context.identity + context.task + context.project + context.shared,
                "compaction_trigger_tokens": compaction.effective_compress_after(),
                "warn_at_tokens": compaction.effective_warn_at_tokens(),
                "usable_prompt_budget": compaction.usable_prompt_budget()
            },
            "checkpoint": checkpoint,
            "tiers": {
                "identity": {
                    "token_budget": context.identity,
                    "priority": 0,
                    "staleness_policy": "always_fresh"
                },
                "task": {
                    "token_budget": context.task,
                    "priority": 1,
                    "staleness_policy": "per_turn"
                },
                "project": {
                    "token_budget": context.project,
                    "priority": 2,
                    "staleness_policy": "per_session"
                },
                "shared": {
                    "token_budget": context.shared,
                    "priority": 3,
                    "staleness_policy": "on_demand"
                },
                "historical": {
                    "token_budget": 0,
                    "priority": 4,
                    "staleness_policy": "retrieval_only"
                }
            }
        }))
    }

    async fn runtime_context_budget_gc(&self, params: Value) -> Result<Value, RpcError> {
        let mut budget = build_context_budget(&params)?;
        let checkpoint_status = params
            .get("status")
            .and_then(|value| value.as_str())
            .unwrap_or("ready");
        let key_decisions = params
            .get("key_decisions")
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let next_step = params
            .get("next_step")
            .and_then(|value| value.as_str())
            .unwrap_or("continue");

        let checkpoint_store = params
            .get("checkpoint_store_path")
            .and_then(|value| value.as_str())
            .map(rune_runtime::CheckpointStore::new);

        let before = rune_runtime::BudgetReport::from(&budget);
        let (checkpoint, gc_result) = rune_runtime::heartbeat_gc_with_store(
            &mut budget,
            checkpoint_status,
            key_decisions,
            next_step,
            checkpoint_store.as_ref(),
        );
        let after = rune_runtime::BudgetReport::from(&budget);
        let checkpoint_storage_key = checkpoint_store
            .as_ref()
            .map(|store| store.path().display().to_string());

        Ok(json!({
            "before": before,
            "after": after,
            "checkpoint": checkpoint,
            "checkpoint_storage_key": checkpoint_storage_key,
            "gc": gc_result,
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

    // ── Turns ────────────────────────────────────────────────────────────

    /// List turns for a session. Params: `session_id` (required), `limit`, `offset`.
    async fn turns_list(&self, params: Value) -> Result<Value, RpcError> {
        let session_id = require_uuid(&params, "session_id")?;
        let limit = params
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(100)
            .min(500) as usize;
        let offset = params.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

        let rows = self
            .state
            .turn_repo
            .list_by_session(session_id)
            .await
            .map_err(|e| RpcError::internal(e.to_string()))?;

        let items: Vec<Value> = rows
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|row| {
                json!({
                    "id": row.id,
                    "session_id": row.session_id,
                    "trigger_kind": row.trigger_kind,
                    "status": row.status,
                    "model_ref": row.model_ref,
                    "usage_prompt_tokens": row.usage_prompt_tokens,
                    "usage_completion_tokens": row.usage_completion_tokens,
                    "started_at": row.started_at.to_rfc3339(),
                    "ended_at": row.ended_at.map(|t| t.to_rfc3339()),
                })
            })
            .collect();

        Ok(json!(items))
    }

    /// Get a single turn. Params: `turn_id` (required).
    async fn turns_get(&self, params: Value) -> Result<Value, RpcError> {
        let turn_id = require_uuid(&params, "turn_id")?;

        let row = self
            .state
            .turn_repo
            .find_by_id(turn_id)
            .await
            .map_err(|e| RpcError::not_found(e.to_string()))?;

        Ok(json!({
            "id": row.id,
            "session_id": row.session_id,
            "trigger_kind": row.trigger_kind,
            "status": row.status,
            "model_ref": row.model_ref,
            "usage_prompt_tokens": row.usage_prompt_tokens,
            "usage_completion_tokens": row.usage_completion_tokens,
            "started_at": row.started_at.to_rfc3339(),
            "ended_at": row.ended_at.map(|t| t.to_rfc3339()),
        }))
    }

    // ── Tools ───────────────────────────────────────────────────────────

    /// List registered tools/skills.
    async fn tools_list(&self) -> Result<Value, RpcError> {
        let skills = self.state.skill_registry.list().await;
        let items: Vec<Value> = skills
            .into_iter()
            .map(|s| {
                json!({
                    "name": s.name,
                    "description": s.description,
                    "enabled": s.enabled,
                })
            })
            .collect();
        Ok(json!(items))
    }

    /// Get a persisted tool execution. Params: `id` (required).
    async fn tools_get(&self, params: Value) -> Result<Value, RpcError> {
        let id = require_string(&params, "id")?;
        let execution_id = Uuid::parse_str(id)
            .map_err(|_| RpcError::bad_request(format!("invalid tool execution id: {id}")))?;

        let execution = self
            .state
            .tool_execution_repo
            .find_by_id(execution_id)
            .await
            .map_err(|error| match error {
                rune_store::StoreError::NotFound { .. } => {
                    RpcError::not_found(format!("no tool execution found for id: {id}"))
                }
                other => RpcError::internal(other.to_string()),
            })?;

        serde_json::to_value(execution).map_err(|error| RpcError::internal(error.to_string()))
    }

    // ── Approvals ───────────────────────────────────────────────────────

    /// List pending approval requests.
    async fn approvals_list(&self) -> Result<Value, RpcError> {
        let approvals = self
            .state
            .approval_repo
            .list(true)
            .await
            .map_err(|e| RpcError::internal(e.to_string()))?;

        let items: Vec<Value> = approvals
            .into_iter()
            .map(|a| {
                json!({
                    "id": a.id,
                    "subject_type": a.subject_type,
                    "subject_id": a.subject_id,
                    "reason": a.reason,
                    "decision": a.decision,
                    "decided_by": a.decided_by,
                    "handle_ref": a.handle_ref,
                    "host_ref": a.host_ref,
                    "presented_payload": a.presented_payload,
                    "created_at": a.created_at.to_rfc3339(),
                    "decided_at": a.decided_at.map(|t| t.to_rfc3339()),
                })
            })
            .collect();

        Ok(json!(items))
    }

    /// Decide an approval. Params: `id`, `decision`, optional `decided_by`.
    async fn approvals_decide(&self, params: Value) -> Result<Value, RpcError> {
        let id_str = require_string(&params, "id")?;
        let approval_id = Uuid::parse_str(id_str)
            .map_err(|_| RpcError::bad_request(format!("invalid approval id: {id_str}")))?;
        let decision = require_string(&params, "decision")?;
        let normalised = decision.replace('-', "_");

        let valid = ["allow_once", "allow_always", "deny"];
        if !valid.contains(&normalised.as_str()) {
            return Err(RpcError::bad_request(format!(
                "invalid decision '{decision}'; expected: allow-once, allow-always, deny"
            )));
        }

        let decided_by = params
            .get("decided_by")
            .and_then(|v| v.as_str())
            .unwrap_or("operator");

        let decided = self
            .state
            .approval_repo
            .decide(approval_id, &normalised, decided_by, chrono::Utc::now())
            .await
            .map_err(|e| RpcError::internal(e.to_string()))?;

        if normalised == "allow_always" && decided.subject_type == "tool_call" {
            self.state
                .tool_approval_repo
                .set_policy(&decided.reason, "allow_always")
                .await
                .map_err(|e| RpcError::internal(e.to_string()))?;
        }

        if decided.subject_type == "tool_call" {
            self.state
                .turn_executor
                .resume_approval(decided.id)
                .await
                .map_err(|e| RpcError::internal(e.to_string()))?;
        }

        let decided = self
            .state
            .approval_repo
            .find_by_id(approval_id)
            .await
            .map_err(|e| RpcError::internal(e.to_string()))?;

        Ok(json!({
            "id": decided.id,
            "decision": decided.decision,
            "decided_by": decided.decided_by,
            "decided_at": decided.decided_at.map(|t| t.to_rfc3339()),
            "presented_payload": decided.presented_payload,
        }))
    }

    // ── Processes ────────────────────────────────────────────────────────

    /// Fetch process log output with optional offset/limit. Params: `id`, optional `offset`, `limit`.
    async fn processes_log(&self, params: Value) -> Result<Value, RpcError> {
        let id = require_string(&params, "id")?;
        let offset = params
            .get("offset")
            .and_then(|value| value.as_u64())
            .map(|value| value as usize);
        let limit = params
            .get("limit")
            .and_then(|value| value.as_u64())
            .map(|value| value as usize);

        let output = self
            .state
            .process_manager
            .log(id, offset, limit)
            .await
            .map_err(|error| match error {
                rune_tools::ToolError::ExecutionFailed(message)
                    if message.contains("process not found") =>
                {
                    RpcError::bad_request(message)
                }
                other => RpcError::internal(other.to_string()),
            })?;

        Ok(json!({ "output": output }))
    }

    /// List background processes.
    async fn processes_list(&self) -> Result<Value, RpcError> {
        let processes = self.state.process_manager.list().await;
        let items: Vec<Value> = processes
            .into_iter()
            .map(|p| {
                json!({
                    "process_id": p.process_id,
                    "running": p.running,
                    "exit_code": p.exit_code,
                    "live": p.live,
                    "durable_status": p.durable_status,
                    "note": p.note,
                })
            })
            .collect();
        Ok(json!(items))
    }

    /// Get a single process. Params: `id` (required).
    async fn processes_get(&self, params: Value) -> Result<Value, RpcError> {
        let id = require_string(&params, "id")?;
        let p = self
            .state
            .process_manager
            .poll(id)
            .await
            .map_err(|e| RpcError::not_found(e.to_string()))?;

        Ok(json!({
            "process_id": p.process_id,
            "running": p.running,
            "exit_code": p.exit_code,
            "live": p.live,
            "durable_status": p.durable_status,
            "note": p.note,
        }))
    }

    /// Kill a background process. Params: `id` (required).
    async fn processes_kill(&self, params: Value) -> Result<Value, RpcError> {
        let id = require_string(&params, "id")?;
        self.state
            .process_manager
            .kill(id)
            .await
            .map_err(|e| RpcError::internal(e.to_string()))?;

        Ok(json!({
            "process_id": id,
            "killed": true,
        }))
    }

    // ── Channels ────────────────────────────────────────────────────────

    /// List configured channel adapters.
    async fn channels_list(&self) -> Result<Value, RpcError> {
        let config = self.state.config.read().await;
        let mut channels = config.channels.enabled.clone();
        if config.channels.telegram_token.is_some() && !channels.iter().any(|c| c == "telegram") {
            channels.push("telegram".to_string());
        }
        channels.sort();

        let items: Vec<Value> = channels
            .into_iter()
            .map(|name| json!({ "name": name, "kind": name, "enabled": true }))
            .collect();
        Ok(json!(items))
    }

    /// Channel subsystem status.
    async fn channels_status(&self) -> Result<Value, RpcError> {
        let config = self.state.config.read().await;
        let mut channels = config.channels.enabled.clone();
        if config.channels.telegram_token.is_some() && !channels.iter().any(|c| c == "telegram") {
            channels.push("telegram".to_string());
        }
        channels.sort();
        drop(config);

        let rows = self
            .state
            .session_repo
            .list(i64::MAX / 4, 0)
            .await
            .map_err(|e| RpcError::internal(e.to_string()))?;
        let active_sessions = rows.iter().filter(|r| r.channel_ref.is_some()).count();

        Ok(json!({
            "configured": channels,
            "active_sessions": active_sessions,
        }))
    }

    // ── Memory ──────────────────────────────────────────────────────────

    /// Auth token / gateway auth status.
    async fn auth_info(&self) -> Result<Value, RpcError> {
        let response = crate::routes::auth_token_info(axum::extract::State(self.state.clone()))
            .await
            .map_err(|error| RpcError::internal(error.to_string()))?;
        serde_json::to_value(response.0).map_err(|error| RpcError::internal(error.to_string()))
    }

    /// Memory subsystem status.
    async fn memory_status(&self) -> Result<Value, RpcError> {
        let config = self.state.config.read().await;
        let compaction = &config.runtime.compaction;
        Ok(json!({
            "memory_mode": self.state.capabilities.memory_mode,
            "memory_dir": config.paths.memory_dir.display().to_string(),
            "pgvector": self.state.capabilities.pgvector,
            "context_budget": {
                "max_tokens": compaction.effective_max_tokens(),
                "warn_at_tokens": compaction.effective_warn_at_tokens(),
                "compress_after": compaction.effective_compress_after(),
                "reserved_system": compaction.reserved_system,
                "reserved_task": compaction.reserved_task,
                "usable_prompt_budget": compaction.usable_prompt_budget(),
                "auto_inject_project": compaction.auto_inject_project,
                "memory_search_k": compaction.memory_search_k,
            }
        }))
    }

    /// Search memory via the same executor used by the HTTP gateway routes.
    async fn memory_search(&self, params: Value) -> Result<Value, RpcError> {
        let q = params.get("q").and_then(|v| v.as_str()).unwrap_or("");
        if q.is_empty() {
            return Err(RpcError::bad_request("missing required param: q"));
        }

        let limit = params
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(10)
            .clamp(1, 50) as usize;

        let workspace_root = {
            let config = self.state.config.read().await;
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
                "query": q,
                "maxResults": limit,
            }),
        };

        let tool = rune_tools::memory_tool::MemoryToolExecutor::new(workspace_root);
        let result = tool
            .execute(call)
            .await
            .map_err(|error| RpcError::internal(error.to_string()))?;
        let results = crate::routes::parse_memory_search_output(&result.output);
        let message = if results.is_empty() {
            format!("No results found for query: {q}")
        } else {
            format!("Found {} memory result(s)", results.len())
        };

        Ok(json!({
            "query": q,
            "results": results,
            "message": message,
        }))
    }

    /// Memory knowledge graph parity payload.
    async fn memory_graph(&self, params: Value) -> Result<Value, RpcError> {
        let query = crate::routes::MemoryGraphQuery {
            threshold: params.get("threshold").and_then(|value| value.as_f64()),
            neighbors: params
                .get("neighbors")
                .and_then(|value| value.as_u64())
                .map(|value| value as usize),
        };

        let response = crate::routes::memory_graph(
            axum::extract::State(self.state.clone()),
            axum::extract::Query(query),
        )
        .await
        .map_err(|error| match error {
            crate::error::GatewayError::BadRequest(message) => RpcError::bad_request(message),
            other => RpcError::internal(other.to_string()),
        })?;

        serde_json::to_value(response.0).map_err(|error| RpcError::internal(error.to_string()))
    }

    // ── Logs ────────────────────────────────────────────────────────────

    /// Query structured logs with the same parity payload as the HTTP route.
    async fn logs_query(&self, _params: Value) -> Result<Value, RpcError> {
        let config = self.state.config.read().await;
        let logs_dir = config.paths.logs_dir.display().to_string();
        drop(config);

        Ok(json!({
            "entries": [],
            "message": format!("structured log query not yet aggregated; logs directory: {logs_dir}"),
        }))
    }

    // ── Doctor ──────────────────────────────────────────────────────────

    /// Run doctor checks with the same result contract as the HTTP route.
    async fn doctor_run(&self) -> Result<Value, RpcError> {
        let report = crate::routes::doctor_run(axum::extract::State(self.state.clone()))
            .await
            .map_err(|error| RpcError::internal(error.to_string()))?;
        serde_json::to_value(report.0).map_err(|error| RpcError::internal(error.to_string()))
    }

    /// Latest doctor results parity payload.
    async fn doctor_results(&self) -> Result<Value, RpcError> {
        let report = crate::routes::doctor_results(axum::extract::State(self.state.clone()))
            .await
            .map_err(|error| RpcError::internal(error.to_string()))?;
        serde_json::to_value(report.0).map_err(|error| RpcError::internal(error.to_string()))
    }

    /// Dashboard summary parity payload.
    async fn dashboard_summary(&self) -> Result<Value, RpcError> {
        let response = crate::routes::dashboard_summary(axum::extract::State(self.state.clone()))
            .await
            .map_err(|error| RpcError::internal(error.to_string()))?;
        serde_json::to_value(response.0).map_err(|error| RpcError::internal(error.to_string()))
    }

    /// Dashboard models parity payload.
    async fn dashboard_models(&self) -> Result<Value, RpcError> {
        let response = crate::routes::dashboard_models(axum::extract::State(self.state.clone()))
            .await
            .map_err(|error| RpcError::internal(error.to_string()))?;
        serde_json::to_value(response.0).map_err(|error| RpcError::internal(error.to_string()))
    }

    /// Dashboard sessions parity payload.
    async fn dashboard_sessions(&self) -> Result<Value, RpcError> {
        let response = crate::routes::dashboard_sessions(axum::extract::State(self.state.clone()))
            .await
            .map_err(|error| RpcError::internal(error.to_string()))?;
        serde_json::to_value(response.0).map_err(|error| RpcError::internal(error.to_string()))
    }

    /// Dashboard diagnostics parity payload.
    async fn dashboard_diagnostics(&self) -> Result<Value, RpcError> {
        let response =
            crate::routes::dashboard_diagnostics(axum::extract::State(self.state.clone()))
                .await
                .map_err(|error| RpcError::internal(error.to_string()))?;
        serde_json::to_value(response.0).map_err(|error| RpcError::internal(error.to_string()))
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
                "priority_active": stats.priority_active,
                "priority_capacity": stats.priority_capacity,
                "subagent_active": stats.subagent_active,
                "subagent_capacity": stats.subagent_capacity,
                "cron_active": stats.cron_active,
                "cron_capacity": stats.cron_capacity,
                "heartbeat_active": stats.heartbeat_active,
                "heartbeat_capacity": stats.heartbeat_capacity,
                "tool_active": stats.tool_active,
                "tool_capacity": stats.tool_capacity,
                "project_tool_capacity": stats.project_tool_capacity,
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

fn build_context_budget(params: &Value) -> Result<rune_runtime::TokenBudget, RpcError> {
    let total_capacity = params
        .get("total_capacity")
        .and_then(|value| value.as_u64())
        .unwrap_or(8192)
        .clamp(1, 1_000_000) as usize;

    let mut budget = rune_runtime::TokenBudget::new(total_capacity);

    if let Some(items) = params.get("items").and_then(|value| value.as_array()) {
        for item in items {
            let partition = parse_partition(
                item.get("partition")
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| {
                        RpcError::bad_request(
                            "context budget items require partition values like objective/history/decision_log/background/reserve",
                        )
                    })?,
            )?;

            let id = item
                .get("id")
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    RpcError::bad_request("context budget items require a non-empty id")
                })?;

            let token_count = item
                .get("token_count")
                .and_then(|value| value.as_u64())
                .ok_or_else(|| {
                    RpcError::bad_request(
                        "context budget items require token_count as a non-negative integer",
                    )
                })? as usize;

            let importance = item
                .get("importance")
                .and_then(|value| value.as_f64())
                .unwrap_or(0.5) as f32;

            let mut budget_item = rune_runtime::BudgetItem::new(id, token_count, importance);
            if item
                .get("summarized")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
            {
                budget_item.summarized = true;
            }
            budget.add_item(partition, budget_item);
        }
    }

    Ok(budget)
}

fn parse_partition(value: &str) -> Result<rune_runtime::Partition, RpcError> {
    match value {
        "objective" => Ok(rune_runtime::Partition::Objective),
        "history" => Ok(rune_runtime::Partition::History),
        "decision_log" => Ok(rune_runtime::Partition::DecisionLog),
        "background" => Ok(rune_runtime::Partition::Background),
        "reserve" => Ok(rune_runtime::Partition::Reserve),
        _ => Err(RpcError::bad_request(
            "context budget items require partition values like objective/history/decision_log/background/reserve",
        )),
    }
}

fn skill_reload_json(summary: SkillScanSummary) -> Value {
    json!({
        "success": true,
        "discovered": summary.discovered,
        "loaded": summary.loaded,
        "removed": summary.removed,
    })
}

fn merge_json_object(target: &mut Value, patch: Value) -> Result<(), RpcError> {
    let target_obj = target
        .as_object_mut()
        .ok_or_else(|| RpcError::bad_request("session metadata must be a JSON object"))?;
    let patch_obj = patch
        .as_object()
        .ok_or_else(|| RpcError::bad_request("session metadata must be a JSON object"))?;
    for (key, value) in patch_obj {
        target_obj.insert(key.clone(), value.clone());
    }
    Ok(())
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

fn rate_limited_error(retry_after: Duration) -> RpcError {
    RpcError {
        code: "rate_limited".into(),
        message: format!(
            "webchat send limit reached; retry in {}s",
            retry_after.as_secs().max(1)
        ),
        data: Some(json!({
            "retry_after_seconds": retry_after.as_secs().max(1)
        })),
    }
}
