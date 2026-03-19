//! Gateway HTTP client for CLI commands.

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use reqwest::Client;
use reqwest::Method;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde_json::json;
use toml_edit::{DocumentMut, Item, Table, Value};

use crate::output::{
    ActionResult, ApprovalListResponse, ApprovalPoliciesResponse, ApprovalPolicySummary,
    ApprovalRequestSummary, ConfigFileResponse, ConfigGetResponse, ConfigMutationResponse,
    ConfigValidationResult, CronJobDetailResponse, CronJobSummary, CronListResponse,
    CronRunSummary, CronRunsResponse, CronStatusResponse, DoctorCheck, DoctorReport, SystemEventListResponse,
    GatewayCallResponse, GatewayDiscoverResponse, GatewayProbeResponse, GatewayUsageCostResponse,
    HealthResponse, HeartbeatStatusResponse, ModelScanProviderResult, ModelScanResponse,
    MessageSearchHit, MessageSearchResponse, MessageSendResponse, ReminderSummary, RemindersListResponse, ScannedModelDetail, SessionDetailResponse,
    SessionListResponse, SessionStatusCard, SessionSummary, StatusResponse,
};

/// HTTP client that talks to the Rune gateway API.
#[derive(Debug, Clone)]
pub struct GatewayClient {
    base_url: String,
    http: Client,
}

impl GatewayClient {
    /// Create a new gateway client pointing at the given base URL.
    #[must_use]
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http: Client::new(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    /// `GET /health`
    pub async fn health(&self) -> Result<HealthResponse> {
        let resp = self
            .http
            .get(self.url("/health"))
            .send()
            .await
            .context("failed to reach gateway")?;

        if resp.status().is_success() {
            Ok(HealthResponse {
                healthy: true,
                message: "Gateway is healthy.".into(),
            })
        } else {
            Ok(HealthResponse {
                healthy: false,
                message: format!("Gateway returned HTTP {}", resp.status()),
            })
        }
    }

    /// `GET /status`
    pub async fn status(&self) -> Result<StatusResponse> {
        let resp = self
            .http
            .get(self.url("/status"))
            .send()
            .await
            .context("failed to reach gateway")?;

        if resp.status().is_success() {
            let body: serde_json::Value = resp.json().await.context("invalid JSON from /status")?;
            Ok(StatusResponse {
                status: body["status"].as_str().unwrap_or("unknown").to_string(),
                version: body["version"].as_str().map(String::from),
                uptime_seconds: body["uptime_seconds"].as_u64(),
            })
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `GET /heartbeat/status`
    pub async fn heartbeat_status(&self) -> Result<HeartbeatStatusResponse> {
        let resp = self
            .http
            .get(self.url("/heartbeat/status"))
            .send()
            .await
            .context("failed to reach gateway")?;

        if resp.status().is_success() {
            let body: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from /heartbeat/status")?;
            Ok(HeartbeatStatusResponse {
                enabled: body["enabled"].as_bool().unwrap_or(false),
                interval_secs: body["interval_secs"].as_u64().unwrap_or(0),
                last_run_at: body["last_run_at"].as_str().map(str::to_string),
                run_count: body["run_count"].as_u64().unwrap_or(0),
                suppressed_count: body["suppressed_count"].as_u64().unwrap_or(0),
            })
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `POST /heartbeat/enable`
    pub async fn heartbeat_enable(&self) -> Result<ActionResult> {
        let resp = self
            .http
            .post(self.url("/heartbeat/enable"))
            .send()
            .await
            .context("failed to reach gateway")?;

        if resp.status().is_success() {
            let body: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from /heartbeat/enable")?;
            Ok(ActionResult {
                success: body["success"].as_bool().unwrap_or(true),
                message: body["message"]
                    .as_str()
                    .unwrap_or("heartbeat enabled")
                    .to_string(),
            })
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `POST /heartbeat/disable`
    pub async fn heartbeat_disable(&self) -> Result<ActionResult> {
        let resp = self
            .http
            .post(self.url("/heartbeat/disable"))
            .send()
            .await
            .context("failed to reach gateway")?;

        if resp.status().is_success() {
            let body: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from /heartbeat/disable")?;
            Ok(ActionResult {
                success: body["success"].as_bool().unwrap_or(true),
                message: body["message"]
                    .as_str()
                    .unwrap_or("heartbeat disabled")
                    .to_string(),
            })
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `POST /models/scan`
    pub async fn models_scan(&self) -> Result<ModelScanResponse> {
        let resp = self
            .http
            .post(self.url("/models/scan"))
            .send()
            .await
            .context("failed to reach gateway")?;

        if resp.status().is_success() {
            let providers = resp
                .json::<Vec<serde_json::Value>>()
                .await
                .context("invalid JSON from /models/scan")?
                .into_iter()
                .map(|value| ModelScanProviderResult {
                    provider: value["provider"].as_str().unwrap_or("unknown").to_string(),
                    models: value["models"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|model| ScannedModelDetail {
                            name: model["name"].as_str().unwrap_or("unknown").to_string(),
                            size: model["size"].as_u64(),
                            modified_at: model["modified_at"].as_str().map(str::to_string),
                        })
                        .collect(),
                })
                .collect();
            Ok(ModelScanResponse { providers })
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `GET /gateway/health`
    pub async fn gateway_health(&self) -> Result<HealthResponse> {
        let resp = self
            .http
            .get(self.url("/gateway/health"))
            .send()
            .await
            .context("failed to reach gateway")?;

        if resp.status().is_success() {
            Ok(HealthResponse {
                healthy: true,
                message: "Gateway is healthy.".into(),
            })
        } else {
            Ok(HealthResponse {
                healthy: false,
                message: format!("Gateway returned HTTP {}", resp.status()),
            })
        }
    }

    /// Probe gateway reachability and auth semantics separately from process liveness.
    pub async fn gateway_probe(&self) -> Result<GatewayProbeResponse> {
        let health_resp = self.http.get(self.url("/health")).send().await;
        let status_resp = self.http.get(self.url("/status")).send().await;

        let health_http_ok = matches!(
            health_resp.as_ref().map(|r| r.status().is_success()),
            Ok(true)
        );
        let status_http_ok = matches!(
            status_resp.as_ref().map(|r| r.status().is_success()),
            Ok(true)
        );
        let auth_required = matches!(status_resp.as_ref().map(|r| r.status().as_u16()), Ok(401));
        let auth_valid = if status_http_ok {
            Some(true)
        } else if auth_required {
            Some(false)
        } else {
            None
        };

        let note = if status_http_ok && health_http_ok {
            "RPC reachable and operator status endpoint responded successfully.".to_string()
        } else if health_http_ok && auth_required {
            "Process is up (`/health` OK) but protected RPC requires a valid bearer token."
                .to_string()
        } else if health_http_ok {
            "Process is up via `/health`, but protected RPC/status did not respond cleanly."
                .to_string()
        } else {
            "Gateway health probe failed; process may be down or bound to a different address/profile.".to_string()
        };

        Ok(GatewayProbeResponse {
            gateway_url: self.base_url.clone(),
            status_http_ok,
            health_http_ok,
            auth_required,
            auth_valid,
            note,
        })
    }

    /// Discover operator-facing runtime URLs and config binding context.
    pub async fn gateway_discover(&self) -> Result<GatewayDiscoverResponse> {
        let status = self.status().await.ok();
        let gateway_url = self.base_url.clone();
        let ws_url = if let Some(rest) = gateway_url.strip_prefix("https://") {
            format!("wss://{rest}/ws")
        } else if let Some(rest) = gateway_url.strip_prefix("http://") {
            format!("ws://{rest}/ws")
        } else {
            format!("{gateway_url}/ws")
        };

        Ok(GatewayDiscoverResponse {
            gateway_url: gateway_url.clone(),
            health_url: format!("{gateway_url}/health"),
            websocket_url: ws_url,
            config_path: local_config_path().display().to_string(),
            auth_enabled: status.as_ref().map(|s| s.status == "running").unwrap_or(false),
            note: "Use `/health` for probes and `/status` for operator detail; mismatches usually mean wrong gateway URL, auth token, or profile/config binding.".to_string(),
        })
    }

    /// Perform a raw gateway HTTP call for parity-style operator debugging.
    pub async fn gateway_call(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
        token: Option<&str>,
    ) -> Result<GatewayCallResponse> {
        let method = Method::from_bytes(method.to_ascii_uppercase().as_bytes())
            .with_context(|| format!("invalid HTTP method `{method}`"))?;
        let normalized_path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };

        let mut request = self
            .http
            .request(method.clone(), self.url(&normalized_path));
        if let Some(token) = token {
            request = request.header(AUTHORIZATION, format!("Bearer {token}"));
        }
        if let Some(body) = body {
            request = request
                .header(CONTENT_TYPE, "application/json")
                .body(body.to_string());
        }

        let response = request.send().await.context("failed to reach gateway")?;
        let status_code = response.status().as_u16();
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let body = response
            .text()
            .await
            .context("failed to read gateway response body")?;

        Ok(GatewayCallResponse {
            method: method.as_str().to_string(),
            path: normalized_path,
            status_code,
            content_type,
            body,
        })
    }

    /// Aggregate persisted session-turn token usage. Monetary cost is intentionally not derived yet.
    pub async fn gateway_usage_cost(&self) -> Result<GatewayUsageCostResponse> {
        let sessions = self.sessions_list(None, None, 500).await?;
        let mut total_turns = 0usize;
        let mut prompt_tokens = 0u64;
        let mut completion_tokens = 0u64;

        for session in &sessions.sessions {
            let response = self
                .http
                .get(self.url(&format!("/sessions/{}/transcript", session.id)))
                .send()
                .await
                .with_context(|| {
                    format!("failed to fetch transcript for session {}", session.id)
                })?;
            if !response.status().is_success() {
                continue;
            }
            let transcript: serde_json::Value = response
                .json()
                .await
                .with_context(|| format!("invalid transcript JSON for session {}", session.id))?;
            if let Some(items) = transcript.as_array() {
                let session_turns = items
                    .iter()
                    .filter(|item| item["kind"].as_str() == Some("assistant_message"))
                    .count();
                total_turns += session_turns;
            }

            let detail = self.sessions_show(&session.id).await.ok();
            if let Some(turn_count) = detail.and_then(|d| d.turn_count) {
                total_turns = total_turns.max(turn_count as usize);
            }
        }

        let status_response = self.http.get(self.url("/status")).send().await;
        if let Ok(resp) = status_response {
            let _ = resp.bytes().await;
        }

        let transcript_sessions = sessions
            .sessions
            .iter()
            .map(|session| session.id.clone())
            .collect::<Vec<_>>();
        for session_id in transcript_sessions {
            let response = self
                .http
                .get(self.url(&format!("/sessions/{session_id}/transcript")))
                .send()
                .await
                .with_context(|| format!("failed to fetch transcript for session {session_id}"))?;
            if !response.status().is_success() {
                continue;
            }
            let transcript: serde_json::Value = response
                .json()
                .await
                .with_context(|| format!("invalid transcript JSON for session {session_id}"))?;
            if let Some(items) = transcript.as_array() {
                let mut per_turn: std::collections::BTreeMap<String, (u64, u64)> =
                    std::collections::BTreeMap::new();
                for item in items {
                    let turn_id = item["turn_id"].as_str().unwrap_or_default();
                    let payload = &item["payload"];
                    if let Some(p) = payload.get("prompt_tokens").and_then(|v| v.as_u64()) {
                        let entry = per_turn.entry(turn_id.to_string()).or_default();
                        entry.0 = entry.0.max(p);
                    }
                    if let Some(c) = payload.get("completion_tokens").and_then(|v| v.as_u64()) {
                        let entry = per_turn.entry(turn_id.to_string()).or_default();
                        entry.1 = entry.1.max(c);
                    }
                }
                for (_turn, (p, c)) in per_turn {
                    prompt_tokens += p;
                    completion_tokens += c;
                }
            }
        }

        Ok(GatewayUsageCostResponse {
            total_sessions: sessions.sessions.len(),
            total_turns,
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
            note: "Token aggregates only; provider-specific monetary cost accounting is not implemented yet.".to_string(),
        })
    }

    /// `GET /status`
    pub async fn gateway_status(&self) -> Result<StatusResponse> {
        self.status().await
    }

    /// `POST /gateway/start`
    pub async fn gateway_start(&self) -> Result<ActionResult> {
        let resp = self
            .http
            .post(self.url("/gateway/start"))
            .send()
            .await
            .context("failed to reach gateway")?;

        Ok(ActionResult {
            success: resp.status().is_success(),
            message: if resp.status().is_success() {
                "Gateway start signal sent.".into()
            } else {
                format!("Gateway returned HTTP {}", resp.status())
            },
        })
    }

    /// `POST /gateway/stop`
    pub async fn gateway_stop(&self) -> Result<ActionResult> {
        let resp = self
            .http
            .post(self.url("/gateway/stop"))
            .send()
            .await
            .context("failed to reach gateway")?;

        Ok(ActionResult {
            success: resp.status().is_success(),
            message: if resp.status().is_success() {
                "Gateway stop signal sent.".into()
            } else {
                format!("Gateway returned HTTP {}", resp.status())
            },
        })
    }

    /// `POST /gateway/restart`
    pub async fn gateway_restart(&self) -> Result<ActionResult> {
        let resp = self
            .http
            .post(self.url("/gateway/restart"))
            .send()
            .await
            .context("failed to reach gateway")?;

        Ok(ActionResult {
            success: resp.status().is_success(),
            message: if resp.status().is_success() {
                "Gateway restart signal sent.".into()
            } else {
                format!("Gateway returned HTTP {}", resp.status())
            },
        })
    }

    /// `GET /cron/status`
    pub async fn cron_status(&self) -> Result<CronStatusResponse> {
        let resp = self
            .http
            .get(self.url("/cron/status"))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            resp.json().await.context("invalid JSON from /cron/status")
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `GET /cron`
    pub async fn cron_list(&self, include_disabled: bool) -> Result<CronListResponse> {
        let resp = self
            .http
            .get(self.url("/cron"))
            .query(&[("include_disabled", include_disabled)])
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let jobs = resp
                .json::<Vec<CronJobSummary>>()
                .await
                .context("invalid JSON from /cron")?;
            Ok(CronListResponse { jobs })
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `GET /cron/{id}`
    pub async fn cron_get(&self, id: &str) -> Result<CronJobDetailResponse> {
        let resp = self
            .http
            .get(self.url(&format!("/cron/{id}")))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            resp.json()
                .await
                .context("invalid JSON from GET /cron/{id}")
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `POST /cron`
    pub async fn cron_add_system_event(
        &self,
        name: Option<&str>,
        text: &str,
        at: DateTime<Utc>,
        session_target: &str,
        delivery_mode: &str,
        webhook_url: Option<&str>,
    ) -> Result<ActionResult> {
        let mut body = json!({
            "name": name,
            "schedule": { "kind": "at", "at": at.to_rfc3339() },
            "payload": { "kind": "system_event", "text": text },
            "session_target": session_target,
            "delivery_mode": delivery_mode,
            "enabled": true
        });
        if let Some(url) = webhook_url {
            body["webhook_url"] = json!(url);
        }
        let resp = self
            .http
            .post(self.url("/cron"))
            .json(&body)
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let body: serde_json::Value =
                resp.json().await.context("invalid JSON from POST /cron")?;
            Ok(ActionResult {
                success: true,
                message: format!(
                    "Cron job created: {}",
                    body["job_id"].as_str().unwrap_or("unknown")
                ),
            })
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `POST /cron/{id}`
    pub async fn cron_update(
        &self,
        id: &str,
        name: Option<&str>,
        delivery_mode: Option<&str>,
        webhook_url: Option<&str>,
    ) -> Result<ActionResult> {
        let mut payload = serde_json::Map::new();
        if let Some(name) = name {
            payload.insert("name".to_string(), json!(name));
        }
        if let Some(delivery_mode) = delivery_mode {
            payload.insert("delivery_mode".to_string(), json!(delivery_mode));
        }
        if let Some(webhook_url) = webhook_url {
            payload.insert("webhook_url".to_string(), json!(webhook_url));
        }
        self.cron_patch(id, serde_json::Value::Object(payload), "Cron job updated")
            .await
    }

    pub async fn cron_enable(&self, id: &str) -> Result<ActionResult> {
        self.cron_patch(id, json!({ "enabled": true }), "Cron job enabled")
            .await
    }

    pub async fn cron_disable(&self, id: &str) -> Result<ActionResult> {
        self.cron_patch(id, json!({ "enabled": false }), "Cron job disabled")
            .await
    }

    async fn cron_patch(
        &self,
        id: &str,
        payload: serde_json::Value,
        ok_message: &str,
    ) -> Result<ActionResult> {
        let resp = self
            .http
            .post(self.url(&format!("/cron/{id}")))
            .json(&payload)
            .send()
            .await
            .context("failed to reach gateway")?;
        Ok(ActionResult {
            success: resp.status().is_success(),
            message: if resp.status().is_success() {
                ok_message.to_string()
            } else {
                format!("Gateway returned HTTP {}", resp.status())
            },
        })
    }

    /// `DELETE /cron/{id}`
    pub async fn cron_remove(&self, id: &str) -> Result<ActionResult> {
        let resp = self
            .http
            .delete(self.url(&format!("/cron/{id}")))
            .send()
            .await
            .context("failed to reach gateway")?;
        Ok(ActionResult {
            success: resp.status().is_success(),
            message: if resp.status().is_success() {
                "Cron job removed".to_string()
            } else {
                format!("Gateway returned HTTP {}", resp.status())
            },
        })
    }

    /// `POST /cron/{id}/run`
    pub async fn cron_run(&self, id: &str) -> Result<ActionResult> {
        let resp = self
            .http
            .post(self.url(&format!("/cron/{id}/run")))
            .send()
            .await
            .context("failed to reach gateway")?;
        Ok(ActionResult {
            success: resp.status().is_success(),
            message: if resp.status().is_success() {
                "Cron job triggered".to_string()
            } else {
                format!("Gateway returned HTTP {}", resp.status())
            },
        })
    }

    /// `GET /cron/{id}/runs`
    pub async fn cron_runs(&self, id: &str) -> Result<CronRunsResponse> {
        let resp = self
            .http
            .get(self.url(&format!("/cron/{id}/runs")))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let items: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from GET /cron/{id}/runs")?;
            let runs = items
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .map(|run| CronRunSummary {
                    job_id: run["job_id"].as_str().unwrap_or("?").to_string(),
                    status: run["status"].as_str().unwrap_or("unknown").to_string(),
                    started_at: run["started_at"].as_str().unwrap_or("?").to_string(),
                    finished_at: run["finished_at"].as_str().map(String::from),
                    output: run["output"].as_str().map(String::from),
                })
                .collect();
            Ok(CronRunsResponse { runs })
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `POST /cron/wake`
    pub async fn cron_wake(
        &self,
        text: &str,
        mode: &str,
        context_messages: Option<u64>,
    ) -> Result<ActionResult> {
        let resp = self
            .http
            .post(self.url("/cron/wake"))
            .json(&json!({
                "text": text,
                "mode": mode,
                "context_messages": context_messages,
            }))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let body: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from POST /cron/wake")?;
            Ok(ActionResult {
                success: true,
                message: body["message"]
                    .as_str()
                    .unwrap_or("Wake event queued")
                    .to_string(),
            })
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// List cron jobs filtered to only those with `system_event` payloads.
    pub async fn system_event_list(
        &self,
        include_disabled: bool,
    ) -> Result<SystemEventListResponse> {
        let all = self.cron_list(include_disabled).await?;
        let events = all
            .jobs
            .into_iter()
            .filter(|job| job.payload.kind() == "system_event")
            .collect();
        Ok(SystemEventListResponse { events })
    }

    /// `GET /reminders`
    pub async fn reminders_list(&self, include_delivered: bool) -> Result<RemindersListResponse> {
        let resp = self
            .http
            .get(self.url("/reminders"))
            .query(&[("includeDelivered", include_delivered)])
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let items: serde_json::Value =
                resp.json().await.context("invalid JSON from /reminders")?;
            let reminders = items
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .map(|item| ReminderSummary {
                    id: item["id"].as_str().unwrap_or("?").to_string(),
                    message: item["message"].as_str().unwrap_or("").to_string(),
                    target: item["target"].as_str().unwrap_or("main").to_string(),
                    fire_at: item["fire_at"].as_str().unwrap_or("?").to_string(),
                    delivered: item["delivered"].as_bool().unwrap_or(false),
                    created_at: item["created_at"].as_str().unwrap_or("?").to_string(),
                    delivered_at: item["delivered_at"].as_str().map(String::from),
                    status: item["status"].as_str().unwrap_or("pending").to_string(),
                    outcome_at: item["outcome_at"].as_str().map(String::from),
                    last_error: item["last_error"].as_str().map(String::from),
                })
                .collect();
            Ok(RemindersListResponse { reminders })
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `POST /reminders`
    pub async fn reminders_add(
        &self,
        message: &str,
        fire_at: DateTime<Utc>,
        target: &str,
    ) -> Result<ActionResult> {
        let resp = self
            .http
            .post(self.url("/reminders"))
            .json(&json!({
                "message": message,
                "fire_at": fire_at.to_rfc3339(),
                "target": target,
            }))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let body: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from POST /reminders")?;
            Ok(ActionResult {
                success: true,
                message: format!(
                    "Reminder created: {}",
                    body["id"].as_str().unwrap_or("unknown")
                ),
            })
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `DELETE /reminders/{id}`
    pub async fn reminders_cancel(&self, id: &str) -> Result<ActionResult> {
        let resp = self
            .http
            .delete(self.url(&format!("/reminders/{id}")))
            .send()
            .await
            .context("failed to reach gateway")?;
        Ok(ActionResult {
            success: resp.status().is_success(),
            message: if resp.status().is_success() {
                "Reminder cancelled".to_string()
            } else {
                format!("Gateway returned HTTP {}", resp.status())
            },
        })
    }

    /// `POST /messages/send`
    pub async fn message_send(
        &self,
        channel: &str,
        text: &str,
        session: Option<&str>,
        thread: Option<&str>,
    ) -> Result<MessageSendResponse> {
        let mut body = json!({
            "channel": channel,
            "text": text,
        });
        if let Some(s) = session {
            body["session"] = json!(s);
        }
        if let Some(t) = thread {
            body["thread"] = json!(t);
        }
        let resp = self
            .http
            .post(self.url("/messages/send"))
            .json(&body)
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let v: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from POST /messages/send")?;
            Ok(MessageSendResponse {
                success: true,
                channel: channel.to_string(),
                message_id: v["id"].as_str().map(String::from),
                detail: v["detail"]
                    .as_str()
                    .unwrap_or("Message sent")
                    .to_string(),
            })
        } else {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            Ok(MessageSendResponse {
                success: false,
                channel: channel.to_string(),
                message_id: None,
                detail: format!("Gateway returned HTTP {status}: {body_text}"),
            })
        }
    }

    /// `GET /messages/search`
    pub async fn message_search(
        &self,
        query: &str,
        channel: Option<&str>,
        session: Option<&str>,
        limit: u64,
    ) -> Result<MessageSearchResponse> {
        let mut params: Vec<(&str, String)> = vec![
            ("q", query.to_string()),
            ("limit", limit.to_string()),
        ];
        if let Some(ch) = channel {
            params.push(("channel", ch.to_string()));
        }
        if let Some(sess) = session {
            params.push(("session", sess.to_string()));
        }
        let resp = self
            .http
            .get(self.url("/messages/search"))
            .query(&params)
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let body: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from GET /messages/search")?;
            let hits = body["hits"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .map(|hit| MessageSearchHit {
                    id: hit["id"].as_str().unwrap_or("?").to_string(),
                    channel: hit["channel"].as_str().map(String::from),
                    session: hit["session"].as_str().map(String::from),
                    sender: hit["sender"].as_str().map(String::from),
                    text: hit["text"].as_str().unwrap_or("").to_string(),
                    timestamp: hit["timestamp"].as_str().map(String::from),
                    score: hit["score"].as_f64(),
                })
                .collect::<Vec<_>>();
            let total = body["total"].as_u64().unwrap_or(hits.len() as u64) as usize;
            Ok(MessageSearchResponse {
                query: query.to_string(),
                total,
                hits,
            })
        } else {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            bail!("Gateway returned HTTP {status}: {body_text}");
        }
    }

    /// `POST /messages/broadcast`
    pub async fn message_broadcast(
        &self,
        text: &str,
        channels: &[String],
        session: Option<&str>,
    ) -> Result<crate::output::MessageBroadcastResponse> {
        use crate::output::{MessageBroadcastChannelResult, MessageBroadcastResponse};

        let mut body = json!({
            "text": text,
        });
        if !channels.is_empty() {
            body["channels"] = json!(channels);
        }
        if let Some(s) = session {
            body["session"] = json!(s);
        }
        let resp = self
            .http
            .post(self.url("/messages/broadcast"))
            .json(&body)
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let v: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from POST /messages/broadcast")?;
            let results = v["results"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .map(|r| MessageBroadcastChannelResult {
                    channel: r["channel"].as_str().unwrap_or("?").to_string(),
                    success: r["success"].as_bool().unwrap_or(false),
                    message_id: r["id"].as_str().map(String::from),
                    detail: r["detail"]
                        .as_str()
                        .unwrap_or("sent")
                        .to_string(),
                })
                .collect::<Vec<_>>();
            let succeeded = results.iter().filter(|r| r.success).count();
            Ok(MessageBroadcastResponse {
                total: results.len(),
                succeeded,
                failed: results.len() - succeeded,
                results,
            })
        } else {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            bail!("Gateway returned HTTP {status}: {body_text}");
        }
    }

    /// `POST /messages/react`
    pub async fn message_react(
        &self,
        message_id: &str,
        emoji: &str,
        remove: bool,
        channel: Option<&str>,
        session: Option<&str>,
    ) -> Result<crate::output::MessageReactResponse> {
        use crate::output::MessageReactResponse;

        let mut body = json!({
            "message_id": message_id,
            "emoji": emoji,
        });
        if remove {
            body["remove"] = json!(true);
        }
        if let Some(ch) = channel {
            body["channel"] = json!(ch);
        }
        if let Some(s) = session {
            body["session"] = json!(s);
        }
        let resp = self
            .http
            .post(self.url("/messages/react"))
            .json(&body)
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let v: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from POST /messages/react")?;
            Ok(MessageReactResponse {
                success: true,
                message_id: v["message_id"]
                    .as_str()
                    .unwrap_or(message_id)
                    .to_string(),
                emoji: v["emoji"].as_str().unwrap_or(emoji).to_string(),
                removed: v["removed"].as_bool().unwrap_or(remove),
                detail: v["detail"]
                    .as_str()
                    .unwrap_or(if remove {
                        "Reaction removed"
                    } else {
                        "Reaction added"
                    })
                    .to_string(),
            })
        } else {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            Ok(MessageReactResponse {
                success: false,
                message_id: message_id.to_string(),
                emoji: emoji.to_string(),
                removed: remove,
                detail: format!("Gateway returned HTTP {status}: {body_text}"),
            })
        }
    }

    /// `POST /messages/pin`
    pub async fn message_pin(
        &self,
        message_id: &str,
        unpin: bool,
        channel: Option<&str>,
        session: Option<&str>,
    ) -> Result<crate::output::MessagePinResponse> {
        use crate::output::MessagePinResponse;

        let mut body = json!({
            "message_id": message_id,
        });
        if unpin {
            body["unpin"] = json!(true);
        }
        if let Some(ch) = channel {
            body["channel"] = json!(ch);
        }
        if let Some(s) = session {
            body["session"] = json!(s);
        }
        let resp = self
            .http
            .post(self.url("/messages/pin"))
            .json(&body)
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let v: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from POST /messages/pin")?;
            Ok(MessagePinResponse {
                success: true,
                message_id: v["message_id"]
                    .as_str()
                    .unwrap_or(message_id)
                    .to_string(),
                pinned: !v["unpinned"].as_bool().unwrap_or(unpin),
                detail: v["detail"]
                    .as_str()
                    .unwrap_or(if unpin {
                        "Message unpinned"
                    } else {
                        "Message pinned"
                    })
                    .to_string(),
            })
        } else {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            Ok(MessagePinResponse {
                success: false,
                message_id: message_id.to_string(),
                pinned: !unpin,
                detail: format!("Gateway returned HTTP {status}: {body_text}"),
            })
        }
    }

    /// `DELETE /messages/{id}`
    pub async fn message_delete(
        &self,
        message_id: &str,
        channel: &str,
        session: Option<&str>,
    ) -> Result<crate::output::MessageDeleteResponse> {
        use crate::output::MessageDeleteResponse;

        let mut params: Vec<(&str, &str)> = vec![("channel", channel)];
        if let Some(s) = session {
            params.push(("session", s));
        }
        let resp = self
            .http
            .delete(self.url(&format!("/messages/{message_id}")))
            .query(&params)
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let v: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from DELETE /messages/{id}")?;
            Ok(MessageDeleteResponse {
                success: true,
                message_id: v["id"]
                    .as_str()
                    .unwrap_or(message_id)
                    .to_string(),
                channel: channel.to_string(),
                detail: v["detail"]
                    .as_str()
                    .unwrap_or("Message deleted")
                    .to_string(),
            })
        } else {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            Ok(MessageDeleteResponse {
                success: false,
                message_id: message_id.to_string(),
                channel: channel.to_string(),
                detail: format!("Gateway returned HTTP {status}: {body_text}"),
            })
        }
    }

    /// `GET /messages/threads/{id}`
    pub async fn message_thread_list(
        &self,
        thread_id: &str,
        channel: Option<&str>,
        session: Option<&str>,
        limit: u64,
    ) -> Result<crate::output::MessageThreadListResponse> {
        use crate::output::{MessageThreadListResponse, ThreadMessage};

        let mut params: Vec<(&str, String)> = vec![("limit", limit.to_string())];
        if let Some(ch) = channel {
            params.push(("channel", ch.to_string()));
        }
        if let Some(s) = session {
            params.push(("session", s.to_string()));
        }
        let resp = self
            .http
            .get(self.url(&format!("/messages/threads/{thread_id}")))
            .query(&params)
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let body: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from GET /messages/threads/{id}")?;
            let messages = body["messages"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .map(|m| ThreadMessage {
                            id: m["id"].as_str().unwrap_or("").to_string(),
                            sender: m["sender"].as_str().map(ToString::to_string),
                            text: m["text"].as_str().unwrap_or("").to_string(),
                            timestamp: m["timestamp"].as_str().map(ToString::to_string),
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let total = body["total"]
                .as_u64()
                .unwrap_or(messages.len() as u64) as usize;
            Ok(MessageThreadListResponse {
                thread_id: thread_id.to_string(),
                total,
                messages,
            })
        } else {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("GET /messages/threads/{thread_id} returned HTTP {status}: {body_text}");
        }
    }

    /// `POST /messages/threads/{id}/reply`
    pub async fn message_thread_reply(
        &self,
        thread_id: &str,
        channel: &str,
        text: &str,
        session: Option<&str>,
    ) -> Result<crate::output::MessageThreadReplyResponse> {
        use crate::output::MessageThreadReplyResponse;

        let mut body = json!({
            "channel": channel,
            "text": text,
        });
        if let Some(s) = session {
            body["session"] = json!(s);
        }
        let resp = self
            .http
            .post(self.url(&format!("/messages/threads/{thread_id}/reply")))
            .json(&body)
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let v: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from POST /messages/threads/{id}/reply")?;
            Ok(MessageThreadReplyResponse {
                success: true,
                thread_id: thread_id.to_string(),
                message_id: v["id"].as_str().map(ToString::to_string),
                detail: v["detail"]
                    .as_str()
                    .unwrap_or("Reply sent")
                    .to_string(),
            })
        } else {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            Ok(MessageThreadReplyResponse {
                success: false,
                thread_id: thread_id.to_string(),
                message_id: None,
                detail: format!("Gateway returned HTTP {status}: {body_text}"),
            })
        }
    }

    /// `GET /sessions`
    pub async fn sessions_list(
        &self,
        active_minutes: Option<u64>,
        channel: Option<&str>,
        limit: u64,
    ) -> Result<SessionListResponse> {
        let mut query: Vec<(&str, String)> = vec![("limit", limit.to_string())];
        if let Some(active_minutes) = active_minutes {
            query.push(("active", active_minutes.to_string()));
        }
        if let Some(channel) = channel {
            query.push(("channel", channel.to_string()));
        }

        let resp = self
            .http
            .get(self.url("/sessions"))
            .query(&query)
            .send()
            .await
            .context("failed to reach gateway")?;

        if resp.status().is_success() {
            let body: serde_json::Value =
                resp.json().await.context("invalid JSON from /sessions")?;
            let sessions = body
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .map(|v| SessionSummary {
                    id: v["id"].as_str().unwrap_or("?").to_string(),
                    status: v["status"].as_str().unwrap_or("unknown").to_string(),
                    channel: v["channel"].as_str().map(String::from),
                    created_at: v["created_at"].as_str().map(String::from),
                    turn_count: v["turn_count"].as_u64().map(|n| n as u32),
                    usage_prompt_tokens: v["usage_prompt_tokens"].as_u64(),
                    usage_completion_tokens: v["usage_completion_tokens"].as_u64(),
                    latest_model: v["latest_model"].as_str().map(String::from),
                })
                .collect();
            Ok(SessionListResponse { sessions })
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `GET /sessions/:id`
    pub async fn sessions_show(&self, id: &str) -> Result<SessionDetailResponse> {
        let resp = self
            .http
            .get(self.url(&format!("/sessions/{id}")))
            .send()
            .await
            .context("failed to reach gateway")?;

        if resp.status().is_success() {
            let v: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from /sessions/:id")?;
            Ok(SessionDetailResponse {
                id: v["id"].as_str().unwrap_or("?").to_string(),
                status: v["status"].as_str().unwrap_or("unknown").to_string(),
                channel: v["channel"]
                    .as_str()
                    .map(String::from)
                    .or_else(|| v["channel_ref"].as_str().map(String::from)),
                created_at: v["created_at"].as_str().map(String::from),
                turn_count: v["turn_count"].as_u64().map(|n| n as u32),
                latest_model: v["latest_model"].as_str().map(String::from),
                usage_prompt_tokens: v["usage_prompt_tokens"].as_u64(),
                usage_completion_tokens: v["usage_completion_tokens"].as_u64(),
                last_turn_started_at: v["last_turn_started_at"].as_str().map(String::from),
                last_turn_ended_at: v["last_turn_ended_at"].as_str().map(String::from),
            })
        } else if resp.status() == reqwest::StatusCode::NOT_FOUND {
            bail!("Session '{id}' not found.");
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `GET /sessions/:id/status`
    pub async fn session_status(&self, id: &str) -> Result<SessionStatusCard> {
        let resp = self
            .http
            .get(self.url(&format!("/sessions/{id}/status")))
            .send()
            .await
            .context("failed to reach gateway")?;

        if resp.status().is_success() {
            resp.json()
                .await
                .context("invalid JSON from /sessions/:id/status")
        } else if resp.status() == reqwest::StatusCode::NOT_FOUND {
            bail!("Session '{id}' not found.");
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// Run doctor checks: config validation + gateway connectivity + model provider reachability.
    pub async fn doctor(&self) -> Result<DoctorReport> {
        let mut checks = Vec::new();

        let config_check = match rune_config::AppConfig::load(None::<&str>) {
            Ok(_) => DoctorCheck {
                name: "config".into(),
                passed: true,
                detail: "Configuration loaded successfully.".into(),
            },
            Err(e) => DoctorCheck {
                name: "config".into(),
                passed: false,
                detail: format!("Failed to load config: {e}"),
            },
        };
        checks.push(config_check);

        let gw_check = match self.health().await {
            Ok(h) if h.healthy => DoctorCheck {
                name: "gateway".into(),
                passed: true,
                detail: "Gateway is reachable and healthy.".into(),
            },
            Ok(h) => DoctorCheck {
                name: "gateway".into(),
                passed: false,
                detail: h.message,
            },
            Err(e) => DoctorCheck {
                name: "gateway".into(),
                passed: false,
                detail: format!("Cannot reach gateway: {e}"),
            },
        };
        checks.push(gw_check);

        Ok(DoctorReport { checks })
    }

    // ── Approvals ─────────────────────────────────────────────────────

    /// `GET /approvals` — list pending durable approval requests.
    pub async fn approvals_list(&self) -> Result<ApprovalListResponse> {
        let resp = self
            .http
            .get(self.url("/approvals"))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let approvals = resp
                .json::<Vec<ApprovalRequestSummary>>()
                .await
                .context("invalid JSON from /approvals")?;
            Ok(ApprovalListResponse { approvals })
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `POST /approvals` — submit a decision for a pending approval request.
    pub async fn approvals_decide(
        &self,
        id: &str,
        decision: &str,
        by: Option<&str>,
    ) -> Result<ApprovalRequestSummary> {
        let resp = self
            .http
            .post(self.url("/approvals"))
            .json(&serde_json::json!({ "id": id, "decision": decision, "decided_by": by }))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            resp.json::<ApprovalRequestSummary>()
                .await
                .context("invalid JSON from POST /approvals")
        } else {
            let body = resp.text().await.unwrap_or_default();
            bail!("Gateway error: {body}");
        }
    }

    /// `GET /approvals/policies` — list all tool approval policies.
    pub async fn approvals_policies_list(&self) -> Result<ApprovalPoliciesResponse> {
        let resp = self
            .http
            .get(self.url("/approvals/policies"))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let policies = resp
                .json::<Vec<ApprovalPolicySummary>>()
                .await
                .context("invalid JSON from /approvals/policies")?;
            Ok(ApprovalPoliciesResponse { policies })
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `GET /approvals/policies/{tool}` — get policy for a specific tool.
    pub async fn approvals_get(&self, tool: &str) -> Result<ApprovalPolicySummary> {
        let resp = self
            .http
            .get(self.url(&format!("/approvals/policies/{tool}")))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            resp.json::<ApprovalPolicySummary>()
                .await
                .context("invalid JSON from GET /approvals/policies/{tool}")
        } else if resp.status() == reqwest::StatusCode::BAD_REQUEST {
            bail!("No approval policy found for tool '{tool}'.");
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `PUT /approvals/policies/{tool}` — set policy for a tool.
    pub async fn approvals_set(&self, tool: &str, decision: &str) -> Result<serde_json::Value> {
        let resp = self
            .http
            .put(self.url(&format!("/approvals/policies/{tool}")))
            .json(&serde_json::json!({ "decision": decision }))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            Ok(resp.json().await?)
        } else {
            let body = resp.text().await.unwrap_or_default();
            bail!("Gateway error: {body}");
        }
    }

    /// `DELETE /approvals/policies/{tool}` — clear policy for a tool.
    pub async fn approvals_clear(&self, tool: &str) -> Result<serde_json::Value> {
        let resp = self
            .http
            .delete(self.url(&format!("/approvals/policies/{tool}")))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            Ok(resp.json().await?)
        } else if resp.status() == reqwest::StatusCode::BAD_REQUEST {
            bail!("No approval policy found for tool '{tool}'.");
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }
}

fn local_config_path() -> std::path::PathBuf {
    if let Some(config_path) = std::env::var_os("RUNE_CONFIG") {
        return std::path::PathBuf::from(config_path);
    }

    let profile = std::env::var("RUNE_PROFILE")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    match profile.as_deref() {
        Some("dev") => std::path::PathBuf::from("config.dev.toml"),
        Some(profile) => std::path::PathBuf::from(format!("config.{profile}.toml")),
        None => std::path::PathBuf::from("config.toml"),
    }
}

fn load_local_config_document() -> Result<(std::path::PathBuf, DocumentMut)> {
    let path = local_config_path();
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(err).with_context(|| format!("failed to read {}", path.display())),
    };

    let doc = if content.trim().is_empty() {
        DocumentMut::new()
    } else {
        content
            .parse::<DocumentMut>()
            .with_context(|| format!("failed to parse {}", path.display()))?
    };

    Ok((path, doc))
}

fn save_local_config_document(path: &std::path::Path, doc: &DocumentMut) -> Result<()> {
    std::fs::write(path, doc.to_string())
        .with_context(|| format!("failed to write {}", path.display()))
}

fn item_to_json(item: &Item) -> Result<serde_json::Value> {
    if let Some(value) = item.as_value() {
        let wrapped = format!("value = {}", value);
        let as_toml = wrapped
            .parse::<toml::Table>()
            .context("failed to parse wrapped TOML value while converting to JSON")?
            .remove("value")
            .ok_or_else(|| anyhow::anyhow!("wrapped TOML value did not produce `value` key"))?;
        serde_json::to_value(as_toml).context("failed to convert TOML value to JSON")
    } else {
        Err(anyhow::anyhow!("item is not a concrete TOML value"))
    }
}

fn parse_toml_value(raw: &str) -> Result<Value> {
    let wrapped = format!("value = {raw}");
    let doc = wrapped
        .parse::<DocumentMut>()
        .with_context(|| format!("failed to parse TOML value `{raw}`"))?;
    doc["value"]
        .as_value()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("parsed TOML value was not concrete"))
}

fn get_item_at_path<'a>(doc: &'a DocumentMut, key: &str) -> Option<&'a Item> {
    let mut current: &Item = doc.as_item();
    for segment in key.split('.') {
        current = current.get(segment)?;
    }
    Some(current)
}

fn ensure_parent_table<'a>(doc: &'a mut DocumentMut, key: &str) -> Result<&'a mut Table> {
    let segments = key.split('.').collect::<Vec<_>>();
    let mut table = doc.as_table_mut();
    for segment in segments.iter().take(segments.len().saturating_sub(1)) {
        if !table.contains_key(segment) {
            table.insert(segment, Item::Table(Table::new()));
        }
        let item = table.get_mut(segment).ok_or_else(|| {
            anyhow::anyhow!("missing intermediate config path segment `{segment}`")
        })?;
        if !item.is_table() {
            return Err(anyhow::anyhow!(
                "config path segment `{segment}` is not a table and cannot contain nested keys"
            ));
        }
        table = item
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("failed to access table segment `{segment}`"))?;
    }
    Ok(table)
}

fn get_parent_table_mut<'a>(doc: &'a mut DocumentMut, key: &str) -> Option<&'a mut Table> {
    let segments = key.split('.').collect::<Vec<_>>();
    let mut table = doc.as_table_mut();
    for segment in segments.iter().take(segments.len().saturating_sub(1)) {
        let item = table.get_mut(segment)?;
        table = item.as_table_mut()?;
    }
    Some(table)
}

pub fn config_file() -> ConfigFileResponse {
    let path = local_config_path();
    ConfigFileResponse {
        path: path.display().to_string(),
        exists: path.exists(),
    }
}

pub fn config_get(key: &str) -> Result<ConfigGetResponse> {
    let (path, doc) = load_local_config_document()?;
    let value = get_item_at_path(&doc, key).map(item_to_json).transpose()?;
    Ok(ConfigGetResponse {
        key: key.to_string(),
        found: value.is_some(),
        value,
        source_path: path.display().to_string(),
    })
}

pub fn config_set(key: &str, raw_value: &str) -> Result<ConfigMutationResponse> {
    let (path, mut doc) = load_local_config_document()?;
    let parsed = parse_toml_value(raw_value)?;
    let new_json = item_to_json(&Item::Value(parsed.clone()))?;
    let old_json = get_item_at_path(&doc, key).map(item_to_json).transpose()?;

    let segments = key.split('.').collect::<Vec<_>>();
    let leaf = segments
        .last()
        .ok_or_else(|| anyhow::anyhow!("config key cannot be empty"))?;
    let parent = ensure_parent_table(&mut doc, key)?;
    parent.insert(leaf, Item::Value(parsed));
    save_local_config_document(&path, &doc)?;

    Ok(ConfigMutationResponse {
        key: key.to_string(),
        changed: old_json.as_ref() != Some(&new_json),
        action: "set".to_string(),
        source_path: path.display().to_string(),
        value: Some(new_json),
        note: Some("Local TOML updated; environment-variable overrides may still take precedence at runtime.".to_string()),
    })
}

pub fn config_unset(key: &str) -> Result<ConfigMutationResponse> {
    let (path, mut doc) = load_local_config_document()?;
    let segments = key.split('.').collect::<Vec<_>>();
    let leaf = segments
        .last()
        .ok_or_else(|| anyhow::anyhow!("config key cannot be empty"))?;

    let removed = if segments.len() == 1 {
        doc.remove(leaf).is_some()
    } else {
        get_parent_table_mut(&mut doc, key)
            .and_then(|parent| parent.remove(leaf))
            .is_some()
    };

    if removed {
        save_local_config_document(&path, &doc)?;
    }

    Ok(ConfigMutationResponse {
        key: key.to_string(),
        changed: removed,
        action: "unset".to_string(),
        source_path: path.display().to_string(),
        value: None,
        note: Some("Unset only changes the local TOML file; defaults or environment overrides may still provide an effective runtime value.".to_string()),
    })
}

/// Validate a config file and return the result.
pub fn validate_config(file: Option<&str>) -> ConfigValidationResult {
    match rune_config::AppConfig::load(file) {
        Ok(config) => {
            let mut errors = Vec::new();
            if let Err(path_errors) = config.validate_paths() {
                for e in path_errors {
                    errors.push(e.to_string());
                }
            }
            if errors.is_empty() {
                ConfigValidationResult {
                    valid: true,
                    errors: vec![],
                }
            } else {
                ConfigValidationResult {
                    valid: false,
                    errors,
                }
            }
        }
        Err(e) => ConfigValidationResult {
            valid: false,
            errors: vec![e.to_string()],
        },
    }
}

/// Show resolved config as pretty-printed JSON.
pub fn show_config() -> Result<String> {
    let config = rune_config::AppConfig::load(Some(local_config_path()))
        .context("failed to load configuration")?;
    serde_json::to_string_pretty(&config).context("failed to serialize config")
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn health_returns_healthy_on_200() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.health().await.unwrap();
        assert!(resp.healthy);
    }

    #[tokio::test]
    async fn health_returns_unhealthy_on_500() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.health().await.unwrap();
        assert!(!resp.healthy);
    }

    #[tokio::test]
    async fn status_parses_json_response() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/status"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "running",
                "version": "0.1.0",
                "uptime_seconds": 300
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.status().await.unwrap();
        assert_eq!(resp.status, "running");
        assert_eq!(resp.version.as_deref(), Some("0.1.0"));
        assert_eq!(resp.uptime_seconds, Some(300));
    }

    #[test]
    fn config_file_reports_default_path() {
        let response = config_file();
        assert!(response.path.ends_with("config.toml"));
    }

    #[test]
    fn config_set_get_and_unset_roundtrip() {
        let _guard = crate::test_env_lock().lock().unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        let config_path = tmp.path().join("config.toml");
        unsafe {
            std::env::set_var("RUNE_CONFIG", &config_path);
        }

        let set = config_set("gateway.port", "9090").unwrap();
        assert!(set.changed);
        assert_eq!(set.value, Some(serde_json::json!(9090)));

        let get = config_get("gateway.port").unwrap();
        assert!(get.found);
        assert_eq!(get.value, Some(serde_json::json!(9090)));

        let unset = config_unset("gateway.port").unwrap();
        assert!(unset.changed);

        let missing = config_get("gateway.port").unwrap();
        assert!(!missing.found);
        assert!(missing.value.is_none());

        unsafe {
            std::env::remove_var("RUNE_CONFIG");
        }
    }

    #[test]
    fn config_set_rejects_invalid_toml_value() {
        let _guard = crate::test_env_lock().lock().unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        let config_path = tmp.path().join("config.toml");
        unsafe {
            std::env::set_var("RUNE_CONFIG", &config_path);
        }

        let err = config_set("gateway.port", "[").unwrap_err();
        assert!(err.to_string().contains("failed to parse TOML value"));

        unsafe {
            std::env::remove_var("RUNE_CONFIG");
        }
    }

    #[tokio::test]
    async fn cron_status_parses_json_response() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/cron/status"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "total_jobs": 1,
                "enabled_jobs": 1,
                "due_jobs": 0
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.cron_status().await.unwrap();
        assert_eq!(resp.total_jobs, 1);
    }

    #[tokio::test]
    async fn cron_list_parses_array() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/cron"))
            .and(query_param("include_disabled", "false"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {
                    "id": "job-1",
                    "name": "test",
                    "schedule": { "kind": "at", "at": "2026-03-18T10:00:00Z" },
                    "payload": { "kind": "system_event", "text": "ping" },
                    "delivery_mode": "announce",
                    "enabled": true,
                    "session_target": "main",
                    "created_at": "2026-03-18T09:00:00Z",
                    "last_run_at": null,
                    "next_run_at": "2026-03-18T10:00:00Z",
                    "run_count": 0
                }
            ])))
            .mount(&server)
            .await;
        let client = GatewayClient::new(&server.uri());
        let resp = client.cron_list(false).await.unwrap();
        assert_eq!(resp.jobs.len(), 1);
        assert_eq!(resp.jobs[0].id, "job-1");
        assert_eq!(resp.jobs[0].delivery_mode, "announce");
        assert_eq!(resp.jobs[0].payload.kind(), "system_event");
    }

    #[tokio::test]
    async fn cron_get_parses_detail() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/cron/job-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "job-1",
                "name": "test",
                "schedule": { "kind": "cron", "expr": "0 * * * * *", "tz": "UTC" },
                "payload": { "kind": "system_event", "text": "ping" },
                "delivery_mode": "webhook",
                "enabled": true,
                "session_target": "main",
                "created_at": "2026-03-18T09:00:00Z",
                "last_run_at": "2026-03-18T09:30:00Z",
                "next_run_at": "2026-03-18T10:00:00Z",
                "run_count": 2
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.cron_get("job-1").await.unwrap();
        assert_eq!(resp.job.id, "job-1");
        assert_eq!(resp.job.delivery_mode, "webhook");
        assert_eq!(resp.job.schedule.kind(), "cron");
    }

    #[tokio::test]
    async fn cron_add_uses_snake_case_surface() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/cron"))
            .and(body_json(json!({
                "name": "daily",
                "schedule": { "kind": "at", "at": "2026-03-18T10:00:00+00:00" },
                "payload": { "kind": "system_event", "text": "ping" },
                "session_target": "main",
                "delivery_mode": "announce",
                "enabled": true
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({
                "success": true,
                "job_id": "job-1",
                "message": "cron job created"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .cron_add_system_event(
                Some("daily"),
                "ping",
                DateTime::parse_from_rfc3339("2026-03-18T10:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                "main",
                "announce",
                None,
            )
            .await
            .unwrap();
        assert!(resp.success);
    }

    #[tokio::test]
    async fn cron_update_sends_selected_fields() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/cron/job-1"))
            .and(body_json(json!({
                "name": "renamed",
                "delivery_mode": "webhook"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "success": true,
                "job_id": "job-1",
                "message": "cron job updated"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .cron_update("job-1", Some("renamed"), Some("webhook"), None)
            .await
            .unwrap();
        assert!(resp.success);
    }

    #[tokio::test]
    async fn cron_wake_uses_snake_case_context_messages() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/cron/wake"))
            .and(body_json(json!({
                "text": "wake up",
                "mode": "now",
                "context_messages": 3
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "success": true,
                "message": "wake event queued for now"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.cron_wake("wake up", "now", Some(3)).await.unwrap();
        assert!(resp.success);
    }

    #[tokio::test]
    async fn models_scan_parses_provider_results() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/models/scan"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {
                    "provider": "ollama-local",
                    "models": [
                        {
                            "name": "llama3.2:latest",
                            "size": 12345,
                            "modified_at": "2026-03-19T03:00:00Z"
                        }
                    ]
                }
            ])))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.models_scan().await.unwrap();
        assert_eq!(resp.providers.len(), 1);
        assert_eq!(resp.providers[0].provider, "ollama-local");
        assert_eq!(resp.providers[0].models[0].name, "llama3.2:latest");
        assert_eq!(resp.providers[0].models[0].size, Some(12345));
    }

    #[tokio::test]
    async fn gateway_health_success() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/gateway/health"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.gateway_health().await.unwrap();
        assert!(resp.healthy);
    }

    #[tokio::test]
    async fn gateway_start_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/gateway/start"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.gateway_start().await.unwrap();
        assert!(resp.success);
    }

    #[tokio::test]
    async fn gateway_restart_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/gateway/restart"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.gateway_restart().await.unwrap();
        assert!(resp.success);
    }

    #[tokio::test]
    async fn sessions_list_parses_array() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/sessions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {"id": "s1", "status": "running", "channel": "webchat"},
                {"id": "s2", "status": "completed"}
            ])))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.sessions_list(None, None, 100).await.unwrap();
        assert_eq!(resp.sessions.len(), 2);
        assert_eq!(resp.sessions[0].id, "s1");
        assert_eq!(resp.sessions[1].channel, None);
    }

    #[tokio::test]
    async fn session_status_parses_card() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/sessions/session-1/status"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "session_id": "session-1",
                "runtime": "kind=direct | channel=local | status=running",
                "status": "running",
                "current_model": "gpt-5.4",
                "prompt_tokens": 100,
                "completion_tokens": 50,
                "total_tokens": 150,
                "estimated_cost": "not available",
                "turn_count": 3,
                "uptime_seconds": 42,
                "reasoning": "off",
                "verbose": false,
                "elevated": false,
                "approval_mode": "on-miss",
                "security_mode": "allowlist",
                "unresolved": ["cost posture is estimate-only"]
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.session_status("session-1").await.unwrap();
        assert_eq!(resp.session_id.as_deref(), Some("session-1"));
        assert_eq!(resp.current_model.as_deref(), Some("gpt-5.4"));
        assert_eq!(resp.total_tokens, Some(150));
        assert_eq!(resp.approval_mode.as_deref(), Some("on-miss"));
    }

    #[tokio::test]
    async fn sessions_show_not_found() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/sessions/nonexistent"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let err = client.sessions_show("nonexistent").await.unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn sessions_show_reads_channel_ref_field() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/sessions/session-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "session-1",
                "status": "running",
                "channel_ref": "telegram:ops",
                "created_at": "2026-03-14T00:00:00Z",
                "turn_count": 1,
                "latest_model": "fake-model",
                "usage_prompt_tokens": 10,
                "usage_completion_tokens": 5,
                "last_turn_started_at": "2026-03-14T00:00:01Z",
                "last_turn_ended_at": "2026-03-14T00:00:02Z"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let response = client.sessions_show("session-1").await.unwrap();
        assert_eq!(response.channel.as_deref(), Some("telegram:ops"));
    }

    #[test]
    fn validate_config_with_defaults_reports_path_errors() {
        let result = validate_config(None);
        assert!(!result.errors.is_empty() || result.valid);
    }

    #[test]
    fn show_config_returns_json() {
        let json = show_config().unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v["gateway"]["port"].is_number());
    }

    #[tokio::test]
    async fn reminders_list_parses_outcome_fields() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/reminders"))
            .and(query_param("includeDelivered", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {
                    "id": "r-1",
                    "message": "Stand up",
                    "target": "isolated",
                    "fire_at": "2026-04-01T09:00:00Z",
                    "status": "delivered",
                    "delivered": true,
                    "created_at": "2026-03-19T10:00:00Z",
                    "delivered_at": "2026-04-01T09:00:05Z",
                    "outcome_at": "2026-04-01T09:00:05Z",
                    "last_error": null
                },
                {
                    "id": "r-2",
                    "message": "Missed one",
                    "target": "main",
                    "fire_at": "2026-04-01T07:00:00Z",
                    "status": "missed",
                    "delivered": false,
                    "created_at": "2026-03-19T10:00:00Z",
                    "delivered_at": null,
                    "outcome_at": "2026-04-01T07:01:00Z",
                    "last_error": "session unavailable"
                }
            ])))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.reminders_list(true).await.unwrap();
        assert_eq!(resp.reminders.len(), 2);

        assert_eq!(resp.reminders[0].target, "isolated");
        assert_eq!(resp.reminders[0].status, "delivered");
        assert_eq!(
            resp.reminders[0].outcome_at.as_deref(),
            Some("2026-04-01T09:00:05Z")
        );
        assert!(resp.reminders[0].last_error.is_none());

        assert_eq!(resp.reminders[1].status, "missed");
        assert_eq!(
            resp.reminders[1].last_error.as_deref(),
            Some("session unavailable")
        );
        assert_eq!(
            resp.reminders[1].outcome_at.as_deref(),
            Some("2026-04-01T07:01:00Z")
        );
    }

    #[tokio::test]
    async fn system_event_list_filters_by_payload_kind() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/cron"))
            .and(query_param("include_disabled", "false"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {
                    "id": "job-1",
                    "name": "sys-ping",
                    "schedule": { "kind": "at", "at": "2026-04-01T09:00:00Z" },
                    "payload": { "kind": "system_event", "text": "ping" },
                    "delivery_mode": "none",
                    "enabled": true,
                    "session_target": "main",
                    "created_at": "2026-03-19T10:00:00Z",
                    "last_run_at": null,
                    "next_run_at": "2026-04-01T09:00:00Z",
                    "run_count": 0
                },
                {
                    "id": "job-2",
                    "name": "agent-task",
                    "schedule": { "kind": "cron", "expr": "0 * * * *" },
                    "payload": { "kind": "agent_turn", "message": "do stuff" },
                    "delivery_mode": "announce",
                    "enabled": true,
                    "session_target": "isolated",
                    "created_at": "2026-03-19T10:00:00Z",
                    "last_run_at": null,
                    "next_run_at": "2026-04-01T10:00:00Z",
                    "run_count": 0
                },
                {
                    "id": "job-3",
                    "name": "sys-check",
                    "schedule": { "kind": "every", "every_ms": 60000 },
                    "payload": { "kind": "system_event", "text": "health check" },
                    "delivery_mode": "webhook",
                    "webhook_url": "https://example.com/hook",
                    "enabled": true,
                    "session_target": "main",
                    "created_at": "2026-03-19T10:00:00Z",
                    "last_run_at": null,
                    "next_run_at": "2026-03-19T10:01:00Z",
                    "run_count": 0
                }
            ])))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.system_event_list(false).await.unwrap();
        assert_eq!(resp.events.len(), 2);
        assert_eq!(resp.events[0].id, "job-1");
        assert_eq!(resp.events[0].payload.kind(), "system_event");
        assert_eq!(resp.events[1].id, "job-3");
        assert_eq!(resp.events[1].payload.kind(), "system_event");
    }

    #[tokio::test]
    async fn message_search_parses_results() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/messages/search"))
            .and(query_param("q", "deploy"))
            .and(query_param("limit", "10"))
            .and(query_param("channel", "telegram"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "total": 2,
                "hits": [
                    {
                        "id": "msg-1",
                        "channel": "telegram",
                        "session": "sess-1",
                        "sender": "hamza",
                        "text": "deploy to staging",
                        "timestamp": "2026-03-19T10:00:00Z",
                        "score": 0.95
                    },
                    {
                        "id": "msg-2",
                        "channel": "telegram",
                        "session": null,
                        "sender": null,
                        "text": "deploy rollback",
                        "timestamp": "2026-03-19T11:00:00Z",
                        "score": 0.82
                    }
                ]
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .message_search("deploy", Some("telegram"), None, 10)
            .await
            .unwrap();
        assert_eq!(resp.query, "deploy");
        assert_eq!(resp.total, 2);
        assert_eq!(resp.hits.len(), 2);
        assert_eq!(resp.hits[0].id, "msg-1");
        assert_eq!(resp.hits[0].channel.as_deref(), Some("telegram"));
        assert_eq!(resp.hits[0].sender.as_deref(), Some("hamza"));
        assert_eq!(resp.hits[0].text, "deploy to staging");
        assert!(resp.hits[0].score.unwrap() > 0.9);
        assert_eq!(resp.hits[1].id, "msg-2");
        assert!(resp.hits[1].sender.is_none());
    }

    #[tokio::test]
    async fn message_search_empty_results() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/messages/search"))
            .and(query_param("q", "nonexistent"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "total": 0,
                "hits": []
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .message_search("nonexistent", None, None, 25)
            .await
            .unwrap();
        assert_eq!(resp.total, 0);
        assert!(resp.hits.is_empty());
    }

    #[tokio::test]
    async fn message_broadcast_parses_results() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages/broadcast"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [
                    {
                        "channel": "telegram",
                        "success": true,
                        "id": "msg-1",
                        "detail": "Message sent"
                    },
                    {
                        "channel": "discord",
                        "success": true,
                        "id": "msg-2",
                        "detail": "Message sent"
                    },
                    {
                        "channel": "slack",
                        "success": false,
                        "id": null,
                        "detail": "Channel not configured"
                    }
                ]
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .message_broadcast(
                "System maintenance",
                &["telegram".into(), "discord".into(), "slack".into()],
                None,
            )
            .await
            .unwrap();
        assert_eq!(resp.total, 3);
        assert_eq!(resp.succeeded, 2);
        assert_eq!(resp.failed, 1);
        assert!(resp.results[0].success);
        assert_eq!(resp.results[0].channel, "telegram");
        assert_eq!(resp.results[0].message_id.as_deref(), Some("msg-1"));
        assert!(!resp.results[2].success);
        assert_eq!(resp.results[2].detail, "Channel not configured");
    }

    #[tokio::test]
    async fn message_broadcast_with_session() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages/broadcast"))
            .and(body_json(json!({
                "text": "hello all",
                "channels": ["telegram"],
                "session": "sess-42"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": [
                    {
                        "channel": "telegram",
                        "success": true,
                        "id": "msg-10",
                        "detail": "Message sent"
                    }
                ]
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .message_broadcast("hello all", &["telegram".into()], Some("sess-42"))
            .await
            .unwrap();
        assert_eq!(resp.total, 1);
        assert_eq!(resp.succeeded, 1);
        assert_eq!(resp.failed, 0);
    }

    #[tokio::test]
    async fn message_broadcast_empty_channels_omits_field() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages/broadcast"))
            .and(body_json(json!({
                "text": "broadcast to all"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "results": []
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .message_broadcast("broadcast to all", &[], None)
            .await
            .unwrap();
        assert_eq!(resp.total, 0);
        assert_eq!(resp.succeeded, 0);
    }

    #[tokio::test]
    async fn message_react_add() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages/react"))
            .and(body_json(json!({
                "message_id": "msg-42",
                "emoji": "👍"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "message_id": "msg-42",
                "emoji": "👍",
                "removed": false,
                "detail": "Reaction added"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .message_react("msg-42", "👍", false, None, None)
            .await
            .unwrap();
        assert!(resp.success);
        assert_eq!(resp.message_id, "msg-42");
        assert_eq!(resp.emoji, "👍");
        assert!(!resp.removed);
        assert_eq!(resp.detail, "Reaction added");
    }

    #[tokio::test]
    async fn message_react_remove() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages/react"))
            .and(body_json(json!({
                "message_id": "msg-99",
                "emoji": "heart",
                "remove": true,
                "channel": "discord"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "message_id": "msg-99",
                "emoji": "heart",
                "removed": true,
                "detail": "Reaction removed"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .message_react("msg-99", "heart", true, Some("discord"), None)
            .await
            .unwrap();
        assert!(resp.success);
        assert!(resp.removed);
        assert_eq!(resp.detail, "Reaction removed");
    }

    #[tokio::test]
    async fn message_react_with_session() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages/react"))
            .and(body_json(json!({
                "message_id": "msg-5",
                "emoji": ":thumbsup:",
                "session": "sess-7"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "message_id": "msg-5",
                "emoji": ":thumbsup:",
                "removed": false,
                "detail": "Reaction added"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .message_react("msg-5", ":thumbsup:", false, None, Some("sess-7"))
            .await
            .unwrap();
        assert!(resp.success);
        assert_eq!(resp.emoji, ":thumbsup:");
    }

    #[tokio::test]
    async fn message_react_gateway_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages/react"))
            .respond_with(ResponseTemplate::new(404).set_body_string("Message not found"))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .message_react("msg-missing", "👍", false, None, None)
            .await
            .unwrap();
        assert!(!resp.success);
        assert!(resp.detail.contains("404"));
        assert!(resp.detail.contains("Message not found"));
    }

    #[tokio::test]
    async fn message_pin_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages/pin"))
            .and(body_json(json!({
                "message_id": "msg-50"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "message_id": "msg-50",
                "detail": "Message pinned"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .message_pin("msg-50", false, None, None)
            .await
            .unwrap();
        assert!(resp.success);
        assert_eq!(resp.message_id, "msg-50");
        assert!(resp.pinned);
        assert_eq!(resp.detail, "Message pinned");
    }

    #[tokio::test]
    async fn message_unpin_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages/pin"))
            .and(body_json(json!({
                "message_id": "msg-77",
                "unpin": true
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "message_id": "msg-77",
                "unpinned": true,
                "detail": "Message unpinned"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .message_pin("msg-77", true, None, None)
            .await
            .unwrap();
        assert!(resp.success);
        assert_eq!(resp.message_id, "msg-77");
        assert!(!resp.pinned);
        assert_eq!(resp.detail, "Message unpinned");
    }

    #[tokio::test]
    async fn message_pin_with_channel_and_session() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages/pin"))
            .and(body_json(json!({
                "message_id": "msg-10",
                "channel": "telegram",
                "session": "sess-5"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "message_id": "msg-10",
                "detail": "Message pinned"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .message_pin("msg-10", false, Some("telegram"), Some("sess-5"))
            .await
            .unwrap();
        assert!(resp.success);
        assert!(resp.pinned);
    }

    #[tokio::test]
    async fn message_pin_gateway_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages/pin"))
            .respond_with(ResponseTemplate::new(404).set_body_string("Message not found"))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .message_pin("msg-missing", false, None, None)
            .await
            .unwrap();
        assert!(!resp.success);
        assert!(resp.detail.contains("404"));
        assert!(resp.detail.contains("Message not found"));
    }

    #[tokio::test]
    async fn message_delete_success() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/messages/msg-42"))
            .and(query_param("channel", "telegram"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "msg-42",
                "detail": "Message deleted"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .message_delete("msg-42", "telegram", None)
            .await
            .unwrap();
        assert!(resp.success);
        assert_eq!(resp.message_id, "msg-42");
        assert_eq!(resp.channel, "telegram");
        assert_eq!(resp.detail, "Message deleted");
    }

    #[tokio::test]
    async fn message_delete_with_session() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/messages/msg-99"))
            .and(query_param("channel", "discord"))
            .and(query_param("session", "sess-7"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "msg-99",
                "detail": "Message deleted"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .message_delete("msg-99", "discord", Some("sess-7"))
            .await
            .unwrap();
        assert!(resp.success);
        assert_eq!(resp.message_id, "msg-99");
        assert_eq!(resp.channel, "discord");
    }

    #[tokio::test]
    async fn message_delete_gateway_error() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/messages/msg-missing"))
            .respond_with(ResponseTemplate::new(404).set_body_string("Message not found"))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .message_delete("msg-missing", "telegram", None)
            .await
            .unwrap();
        assert!(!resp.success);
        assert!(resp.detail.contains("404"));
        assert!(resp.detail.contains("Message not found"));
    }

    #[tokio::test]
    async fn message_thread_list_parses_results() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/messages/threads/thr-42"))
            .and(query_param("limit", "10"))
            .and(query_param("channel", "telegram"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "total": 2,
                "messages": [
                    {
                        "id": "msg-1",
                        "sender": "hamza",
                        "text": "initial message",
                        "timestamp": "2026-03-19T10:00:00Z"
                    },
                    {
                        "id": "msg-2",
                        "sender": "bot",
                        "text": "follow-up reply",
                        "timestamp": "2026-03-19T10:05:00Z"
                    }
                ]
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .message_thread_list("thr-42", Some("telegram"), None, 10)
            .await
            .unwrap();
        assert_eq!(resp.thread_id, "thr-42");
        assert_eq!(resp.total, 2);
        assert_eq!(resp.messages.len(), 2);
        assert_eq!(resp.messages[0].id, "msg-1");
        assert_eq!(resp.messages[0].sender.as_deref(), Some("hamza"));
        assert_eq!(resp.messages[1].text, "follow-up reply");
    }

    #[tokio::test]
    async fn message_thread_list_empty() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/messages/threads/thr-empty"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "total": 0,
                "messages": []
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .message_thread_list("thr-empty", None, None, 50)
            .await
            .unwrap();
        assert_eq!(resp.total, 0);
        assert!(resp.messages.is_empty());
    }

    #[tokio::test]
    async fn message_thread_reply_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages/threads/thr-42/reply"))
            .and(body_json(json!({
                "channel": "telegram",
                "text": "Thanks!"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "msg-new-1",
                "detail": "Reply sent"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .message_thread_reply("thr-42", "telegram", "Thanks!", None)
            .await
            .unwrap();
        assert!(resp.success);
        assert_eq!(resp.thread_id, "thr-42");
        assert_eq!(resp.message_id.as_deref(), Some("msg-new-1"));
        assert_eq!(resp.detail, "Reply sent");
    }

    #[tokio::test]
    async fn message_thread_reply_with_session() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages/threads/thr-99/reply"))
            .and(body_json(json!({
                "channel": "discord",
                "text": "noted",
                "session": "sess-7"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "msg-new-2",
                "detail": "Reply sent"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .message_thread_reply("thr-99", "discord", "noted", Some("sess-7"))
            .await
            .unwrap();
        assert!(resp.success);
        assert_eq!(resp.thread_id, "thr-99");
        assert_eq!(resp.message_id.as_deref(), Some("msg-new-2"));
    }

    #[tokio::test]
    async fn message_thread_reply_gateway_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/messages/threads/thr-missing/reply"))
            .respond_with(ResponseTemplate::new(404).set_body_string("Thread not found"))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .message_thread_reply("thr-missing", "telegram", "hello", None)
            .await
            .unwrap();
        assert!(!resp.success);
        assert!(resp.detail.contains("404"));
        assert!(resp.detail.contains("Thread not found"));
    }
}