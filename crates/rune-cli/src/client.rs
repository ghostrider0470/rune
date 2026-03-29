//! Gateway HTTP client for CLI commands.

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use reqwest::Client;
use reqwest::Method;
use reqwest::StatusCode;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde_json::json;
use toml_edit::{DocumentMut, Item, Table, Value};

use crate::output::{
    AcpAckResponse, AcpInboxMessage, AcpInboxResponse, AcpSendResponse, ActionResult,
    AgentDetailResponse, AgentKillResponse, AgentListResponse, AgentResultResponse,
    AgentRunResponse, AgentSpawnResponse, AgentSteerResponse, AgentSummary, ApprovalListResponse,
    ApprovalPoliciesResponse, ApprovalPolicySummary, ApprovalRequestSummary, ConfigFileResponse,
    ConfigGetResponse, ConfigMutationResponse, ConfigValidationResult, ConfigureResponse,
    CronJobDetailResponse, CronJobSummary, CronListResponse, CronRunSummary, CronRunsResponse,
    CronStatusResponse, DoctorReport, GatewayCallResponse, GatewayConfigResponse,
    GatewayDiscoverResponse, GatewayProbeResponse, GatewayUsageCostResponse, HealthResponse,
    HeartbeatStatusResponse, LogsQueryResponse, MessageSearchHit, MessageSearchResponse,
    MessageSendResponse, ModelScanProviderResult, ModelScanResponse, ReminderSummary,
    RemindersListResponse, SandboxExplainResponse, SandboxListResponse, SandboxRecreateResponse,
    ScannedModelDetail, SecretsApplyResponse, SecretsAuditResponse, SecretsConfigureResponse,
    SecretsReloadResponse, SecurityAuditResponse, SessionDetailResponse, SessionListResponse,
    SessionStatusCard, SessionSummary, SessionTreeNode, SessionTreeResponse, SkillCheckResponse,
    SkillInfoResponse, SkillListResponse, SkillSummary, SpellSearchResponse, StatusResponse,
    SystemEventListResponse,
};

/// HTTP client that talks to the Rune gateway API.
#[derive(Debug, Clone)]
pub struct GatewayClient {
    pub(crate) base_url: String,
    pub(crate) http: Client,
}

#[derive(Debug, Clone, Default)]
pub struct Ms365TodoTaskUpdateInput {
    pub title: Option<String>,
    pub status: Option<String>,
    pub importance: Option<String>,
    pub due_date: Option<String>,
    pub body: Option<String>,
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

    pub(crate) fn url(&self, path: &str) -> String {
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
                instance_id: body["capabilities"]["identity"]["id"].as_str().map(String::from).or_else(|| body["capabilities"]["instance_id"].as_str().map(String::from)),
                instance_name: body["capabilities"]["identity"]["name"].as_str().map(String::from).or_else(|| body["capabilities"]["instance_name"].as_str().map(String::from)),
                instance_roles: body["capabilities"]["identity"]["roles"].as_array().map(|roles| roles.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default(),
                capabilities_version: body["capabilities"]["identity"]["capabilities_version"].as_u64().and_then(|v| u32::try_from(v).ok()),
                advertised_addr: body["capabilities"]["identity"]["advertised_addr"].as_str().map(String::from),
            })
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `GET /config`
    pub async fn gateway_config(&self) -> Result<GatewayConfigResponse> {
        let resp = self
            .http
            .get(self.url("/config"))
            .send()
            .await
            .context("failed to reach gateway")?;

        if resp.status().is_success() {
            let config = resp
                .json::<serde_json::Value>()
                .await
                .context("invalid JSON from /config")?;
            Ok(GatewayConfigResponse {
                action: "current".to_string(),
                config,
                note: Some("Returned from the live gateway; secrets are redacted.".to_string()),
            })
        } else {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            bail!("Gateway returned HTTP {status}: {body_text}");
        }
    }

    /// `PUT /config`
    pub async fn gateway_config_apply(
        &self,
        config: serde_json::Value,
    ) -> Result<GatewayConfigResponse> {
        let resp = self
            .http
            .put(self.url("/config"))
            .json(&config)
            .send()
            .await
            .context("failed to reach gateway")?;

        if resp.status().is_success() {
            let config = resp
                .json::<serde_json::Value>()
                .await
                .context("invalid JSON from PUT /config")?;
            Ok(GatewayConfigResponse {
                action: "applied".to_string(),
                config,
                note: Some(
                    "Live gateway config replaced; returned config view is redacted.".to_string(),
                ),
            })
        } else {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            bail!("Gateway returned HTTP {status}: {body_text}");
        }
    }

    /// `GET /api/logs`
    pub async fn logs_query(
        &self,
        level: Option<&str>,
        source: Option<&str>,
        limit: Option<usize>,
        since: Option<&str>,
    ) -> Result<LogsQueryResponse> {
        let mut query: Vec<(&str, String)> = Vec::new();
        if let Some(level) = level {
            query.push(("level", level.to_string()));
        }
        if let Some(source) = source {
            query.push(("source", source.to_string()));
        }
        if let Some(limit) = limit {
            query.push(("limit", limit.to_string()));
        }
        if let Some(since) = since {
            query.push(("since", since.to_string()));
        }

        let resp = self
            .http
            .get(self.url("/api/logs"))
            .query(&query)
            .send()
            .await
            .context("failed to reach gateway")?;

        if resp.status().is_success() {
            resp.json::<LogsQueryResponse>()
                .await
                .context("invalid JSON from /api/logs")
        } else {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            bail!("Gateway returned HTTP {status}: {body_text}");
        }
    }

    /// `POST /api/doctor/run`
    pub async fn doctor_run(&self) -> Result<DoctorReport> {
        let resp = self
            .http
            .post(self.url("/api/doctor/run"))
            .send()
            .await
            .context("failed to reach gateway")?;

        if resp.status().is_success() {
            resp.json::<DoctorReport>()
                .await
                .context("invalid JSON from /api/doctor/run")
        } else {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            bail!("Gateway returned HTTP {status}: {body_text}");
        }
    }

    /// `GET /api/doctor/results`
    pub async fn doctor_results(&self) -> Result<DoctorReport> {
        let resp = self
            .http
            .get(self.url("/api/doctor/results"))
            .send()
            .await
            .context("failed to reach gateway")?;

        if resp.status().is_success() {
            resp.json::<DoctorReport>()
                .await
                .context("invalid JSON from /api/doctor/results")
        } else {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            bail!("Gateway returned HTTP {status}: {body_text}");
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

    /// Search installed spells using the existing `/skills` inventory.
    pub async fn spells_search(&self, query: &str) -> Result<SpellSearchResponse> {
        let skills = self.skills_list().await?;
        let query_lower = query.to_lowercase();
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();

        let mut spells: Vec<SkillSummary> = skills
            .skills
            .into_iter()
            .filter(|skill| {
                let namespace = skill.namespace.as_deref().unwrap_or_default();
                let haystack = [
                    skill.name.as_str(),
                    skill.description.as_str(),
                    namespace,
                    skill.kind.as_str(),
                    &skill.tags.join(" "),
                    &skill.triggers.join(" "),
                ]
                .join(" ")
                .to_lowercase();

                query_words.iter().all(|word| haystack.contains(*word))
            })
            .collect();

        spells.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(SpellSearchResponse {
            query: query.to_string(),
            total: spells.len(),
            spells,
        })
    }

    /// `GET /skills`
    pub async fn skills_list(&self) -> Result<SkillListResponse> {
        let resp = self
            .http
            .get(self.url("/skills"))
            .send()
            .await
            .context("failed to reach gateway")?;

        if resp.status().is_success() {
            let skills = resp
                .json::<Vec<SkillSummary>>()
                .await
                .context("invalid JSON from /skills")?;
            Ok(SkillListResponse { skills })
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `GET /skills/{name}` with fallback-to-list if detail is unavailable.
    pub async fn skills_info(&self, name: &str) -> Result<SkillInfoResponse> {
        let resp = self
            .http
            .get(self.url(&format!("/skills/{name}")))
            .send()
            .await
            .context("failed to reach gateway")?;

        if resp.status().is_success() {
            return resp
                .json::<SkillInfoResponse>()
                .await
                .context("invalid JSON from /skills/{name}");
        }

        if !matches!(
            resp.status(),
            StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED
        ) {
            bail!("Gateway returned HTTP {}", resp.status());
        }

        let skills = self.skills_list().await?;
        let skill = skills
            .skills
            .into_iter()
            .find(|skill| skill.name == name)
            .ok_or_else(|| anyhow::anyhow!("skill not found: {name}"))?;
        Ok(SkillInfoResponse { skill })
    }

    /// `POST /skills/reload`
    pub async fn skills_check(&self) -> Result<SkillCheckResponse> {
        let resp = self
            .http
            .post(self.url("/skills/reload"))
            .send()
            .await
            .context("failed to reach gateway")?;

        if resp.status().is_success() {
            resp.json::<SkillCheckResponse>()
                .await
                .context("invalid JSON from /skills/reload")
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `POST /skills/{name}/enable`
    pub async fn skills_enable(&self, name: &str) -> Result<ActionResult> {
        let resp = self
            .http
            .post(self.url(&format!("/skills/{name}/enable")))
            .send()
            .await
            .context("failed to reach gateway")?;

        if resp.status().is_success() {
            let body: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from /skills/{name}/enable")?;
            Ok(ActionResult {
                success: body["success"].as_bool().unwrap_or(true),
                message: body["message"]
                    .as_str()
                    .unwrap_or("skill enabled")
                    .to_string(),
            })
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `POST /skills/{name}/disable`
    pub async fn skills_disable(&self, name: &str) -> Result<ActionResult> {
        let resp = self
            .http
            .post(self.url(&format!("/skills/{name}/disable")))
            .send()
            .await
            .context("failed to reach gateway")?;

        if resp.status().is_success() {
            let body: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from /skills/{name}/disable")?;
            Ok(ActionResult {
                success: body["success"].as_bool().unwrap_or(true),
                message: body["message"]
                    .as_str()
                    .unwrap_or("skill disabled")
                    .to_string(),
            })
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
        let sessions = self
            .sessions_list(None, None, None, None, None, 500)
            .await?;
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
                detail: v["detail"].as_str().unwrap_or("Message sent").to_string(),
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
        let mut params: Vec<(&str, String)> =
            vec![("q", query.to_string()), ("limit", limit.to_string())];
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
                    detail: r["detail"].as_str().unwrap_or("sent").to_string(),
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
                message_id: v["message_id"].as_str().unwrap_or(message_id).to_string(),
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

    /// `PATCH /messages/{id}` — edit the text content of an existing message.
    pub async fn message_edit(
        &self,
        message_id: &str,
        channel: &str,
        text: &str,
        session: Option<&str>,
    ) -> Result<crate::output::MessageEditResponse> {
        use crate::output::MessageEditResponse;

        let mut body = json!({
            "channel": channel,
            "text": text,
        });
        if let Some(s) = session {
            body["session"] = json!(s);
        }
        let resp = self
            .http
            .patch(self.url(&format!("/messages/{message_id}")))
            .json(&body)
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let v: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from PATCH /messages/{id}")?;
            Ok(MessageEditResponse {
                success: true,
                message_id: v["id"].as_str().unwrap_or(message_id).to_string(),
                channel: channel.to_string(),
                detail: v["detail"].as_str().unwrap_or("Message edited").to_string(),
            })
        } else {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            Ok(MessageEditResponse {
                success: false,
                message_id: message_id.to_string(),
                channel: channel.to_string(),
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
                message_id: v["message_id"].as_str().unwrap_or(message_id).to_string(),
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
                message_id: v["id"].as_str().unwrap_or(message_id).to_string(),
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

    /// `GET /messages/{id}` — read/fetch a single message by ID.
    pub async fn message_read(
        &self,
        message_id: &str,
        channel: &str,
        session: Option<&str>,
    ) -> Result<crate::output::MessageReadResponse> {
        use crate::output::MessageReadResponse;

        let mut params: Vec<(&str, &str)> = vec![("channel", channel)];
        if let Some(s) = session {
            params.push(("session", s));
        }
        let resp = self
            .http
            .get(self.url(&format!("/messages/{message_id}")))
            .query(&params)
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let v: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from GET /messages/{id}")?;
            Ok(MessageReadResponse {
                success: true,
                message_id: v["id"].as_str().unwrap_or(message_id).to_string(),
                channel: v["channel"].as_str().unwrap_or(channel).to_string(),
                sender: v["sender"].as_str().map(ToString::to_string),
                text: v["text"].as_str().map(ToString::to_string),
                timestamp: v["timestamp"].as_str().map(ToString::to_string),
                thread_id: v["thread_id"].as_str().map(ToString::to_string),
                detail: v["detail"]
                    .as_str()
                    .unwrap_or("Message retrieved")
                    .to_string(),
            })
        } else {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            Ok(MessageReadResponse {
                success: false,
                message_id: message_id.to_string(),
                channel: channel.to_string(),
                sender: None,
                text: None,
                timestamp: None,
                thread_id: None,
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
            let total = body["total"].as_u64().unwrap_or(messages.len() as u64) as usize;
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
                detail: v["detail"].as_str().unwrap_or("Reply sent").to_string(),
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

    /// `POST /messages/{id}/tags` — add a tag to a message.
    pub async fn message_tag_add(
        &self,
        message_id: &str,
        tag: &str,
        channel: Option<&str>,
        session: Option<&str>,
    ) -> Result<crate::output::MessageTagResponse> {
        use crate::output::MessageTagResponse;

        let mut body = json!({
            "tag": tag,
        });
        if let Some(ch) = channel {
            body["channel"] = json!(ch);
        }
        if let Some(s) = session {
            body["session"] = json!(s);
        }
        let resp = self
            .http
            .post(self.url(&format!("/messages/{message_id}/tags")))
            .json(&body)
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let v: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from POST /messages/{id}/tags")?;
            Ok(MessageTagResponse {
                success: true,
                message_id: v["message_id"].as_str().unwrap_or(message_id).to_string(),
                tag: v["tag"].as_str().unwrap_or(tag).to_string(),
                added: true,
                detail: v["detail"].as_str().unwrap_or("Tag added").to_string(),
            })
        } else {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            Ok(MessageTagResponse {
                success: false,
                message_id: message_id.to_string(),
                tag: tag.to_string(),
                added: true,
                detail: format!("Gateway returned HTTP {status}: {body_text}"),
            })
        }
    }

    /// `DELETE /messages/{id}/tags/{tag}` — remove a tag from a message.
    pub async fn message_tag_remove(
        &self,
        message_id: &str,
        tag: &str,
        channel: Option<&str>,
        session: Option<&str>,
    ) -> Result<crate::output::MessageTagResponse> {
        use crate::output::MessageTagResponse;

        let mut params: Vec<(&str, &str)> = vec![];
        if let Some(ch) = channel {
            params.push(("channel", ch));
        }
        if let Some(s) = session {
            params.push(("session", s));
        }
        let resp = self
            .http
            .delete(self.url(&format!("/messages/{message_id}/tags/{tag}")))
            .query(&params)
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let v: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from DELETE /messages/{id}/tags/{tag}")?;
            Ok(MessageTagResponse {
                success: true,
                message_id: v["message_id"].as_str().unwrap_or(message_id).to_string(),
                tag: v["tag"].as_str().unwrap_or(tag).to_string(),
                added: false,
                detail: v["detail"].as_str().unwrap_or("Tag removed").to_string(),
            })
        } else {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            Ok(MessageTagResponse {
                success: false,
                message_id: message_id.to_string(),
                tag: tag.to_string(),
                added: false,
                detail: format!("Gateway returned HTTP {status}: {body_text}"),
            })
        }
    }

    /// `GET /messages/{id}/tags` — list all tags on a message.
    pub async fn message_tag_list(
        &self,
        message_id: &str,
        channel: Option<&str>,
        session: Option<&str>,
    ) -> Result<crate::output::MessageTagListResponse> {
        use crate::output::MessageTagListResponse;

        let mut params: Vec<(&str, &str)> = vec![];
        if let Some(ch) = channel {
            params.push(("channel", ch));
        }
        if let Some(s) = session {
            params.push(("session", s));
        }
        let resp = self
            .http
            .get(self.url(&format!("/messages/{message_id}/tags")))
            .query(&params)
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let v: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from GET /messages/{id}/tags")?;
            let tags = v["tags"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|t| t.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            Ok(MessageTagListResponse {
                message_id: v["message_id"].as_str().unwrap_or(message_id).to_string(),
                tags,
            })
        } else {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("GET /messages/{message_id}/tags returned HTTP {status}: {body_text}");
        }
    }

    /// `GET /messages/{id}/reactions` — list emoji reactions on a message.
    pub async fn message_list_reactions(
        &self,
        message_id: &str,
        channel: Option<&str>,
        session: Option<&str>,
    ) -> Result<crate::output::MessageReactionListResponse> {
        use crate::output::{MessageReactionListResponse, ReactionDetail};

        let mut params: Vec<(&str, &str)> = vec![];
        if let Some(ch) = channel {
            params.push(("channel", ch));
        }
        if let Some(s) = session {
            params.push(("session", s));
        }
        let resp = self
            .http
            .get(self.url(&format!("/messages/{message_id}/reactions")))
            .query(&params)
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let v: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from GET /messages/{id}/reactions")?;
            let reactions = v["reactions"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .map(|r| ReactionDetail {
                            emoji: r["emoji"].as_str().unwrap_or("?").to_string(),
                            count: r["count"].as_u64().unwrap_or(1),
                            users: r["users"]
                                .as_array()
                                .map(|u| {
                                    u.iter()
                                        .filter_map(|v| v.as_str().map(String::from))
                                        .collect()
                                })
                                .unwrap_or_default(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            Ok(MessageReactionListResponse {
                message_id: v["message_id"].as_str().unwrap_or(message_id).to_string(),
                reactions,
            })
        } else {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "GET /messages/{message_id}/reactions returned HTTP {status}: {body_text}"
            );
        }
    }

    /// `POST /messages/{id}/ack` — acknowledge (mark as read/received) a message.
    pub async fn message_ack(
        &self,
        message_id: &str,
        channel: &str,
        session: Option<&str>,
    ) -> Result<crate::output::MessageAckResponse> {
        use crate::output::MessageAckResponse;

        let mut body = json!({
            "channel": channel,
        });
        if let Some(s) = session {
            body["session"] = json!(s);
        }
        let resp = self
            .http
            .post(self.url(&format!("/messages/{message_id}/ack")))
            .json(&body)
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let v: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from POST /messages/{id}/ack")?;
            Ok(MessageAckResponse {
                success: true,
                message_id: v["message_id"].as_str().unwrap_or(message_id).to_string(),
                channel: v["channel"].as_str().unwrap_or(channel).to_string(),
                detail: v["detail"]
                    .as_str()
                    .unwrap_or("Message acknowledged")
                    .to_string(),
            })
        } else {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            Ok(MessageAckResponse {
                success: false,
                message_id: message_id.to_string(),
                channel: channel.to_string(),
                detail: format!("Gateway returned HTTP {status}: {body_text}"),
            })
        }
    }

    /// `GET /tts/status`
    pub async fn message_voice_status(&self) -> Result<crate::output::MessageVoiceStatusResponse> {
        use crate::output::{MessageVoiceStatusResponse, TtsVoiceDetail};

        let resp = self
            .http
            .get(self.url("/tts/status"))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let v: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from GET /tts/status")?;
            let voices = v["voices"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .map(|e| TtsVoiceDetail {
                    id: e["id"].as_str().unwrap_or("?").to_string(),
                    name: e["name"].as_str().unwrap_or("").to_string(),
                    language: e["language"].as_str().unwrap_or("").to_string(),
                })
                .collect();
            Ok(MessageVoiceStatusResponse {
                enabled: v["enabled"].as_bool().unwrap_or(false),
                provider: v["provider"].as_str().unwrap_or("").to_string(),
                voice: v["voice"].as_str().unwrap_or("").to_string(),
                model: v["model"].as_str().unwrap_or("").to_string(),
                auto_mode: v["auto_mode"].as_str().unwrap_or("off").to_string(),
                voices,
            })
        } else {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            bail!("Gateway returned HTTP {status}: {body_text}");
        }
    }

    /// `POST /tts/synthesize` — returns raw audio bytes.
    pub async fn tts_synthesize(
        &self,
        text: &str,
        voice: Option<&str>,
        model: Option<&str>,
    ) -> Result<Vec<u8>> {
        let mut body = json!({ "text": text });
        if let Some(v) = voice {
            body["voice"] = json!(v);
        }
        if let Some(m) = model {
            body["model"] = json!(m);
        }
        let resp = self
            .http
            .post(self.url("/tts/synthesize"))
            .json(&body)
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let bytes = resp
                .bytes()
                .await
                .context("failed to read audio from POST /tts/synthesize")?;
            Ok(bytes.to_vec())
        } else {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            bail!("TTS synthesis failed: HTTP {status}: {body_text}");
        }
    }

    /// `GET /sessions`
    pub async fn sessions_list(
        &self,
        active_minutes: Option<u64>,
        channel: Option<&str>,
        kind: Option<&str>,
        parent: Option<&str>,
        project: Option<&str>,
        limit: u64,
    ) -> Result<SessionListResponse> {
        let mut query: Vec<(&str, String)> = vec![("limit", limit.to_string())];
        if let Some(active_minutes) = active_minutes {
            query.push(("active", active_minutes.to_string()));
        }
        if let Some(channel) = channel {
            query.push(("channel", channel.to_string()));
        }
        if let Some(kind) = kind {
            query.push(("kind", kind.to_string()));
        }
        if let Some(parent) = parent {
            query.push(("parent", parent.to_string()));
        }
        if let Some(project) = project {
            query.push(("project", project.to_string()));
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
                    kind: v["kind"].as_str().unwrap_or("direct").to_string(),
                    status: v["status"].as_str().unwrap_or("unknown").to_string(),
                    channel: v["channel"].as_str().map(String::from),
                    project_id: v["project_id"].as_str().map(String::from),
                    requester_session_id: v["requester_session_id"].as_str().map(String::from),
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

    /// `POST /sessions` — create a new session via the gateway.
    pub async fn sessions_create(&self, kind: &str) -> Result<SessionDetailResponse> {
        let resp = self
            .http
            .post(self.url("/sessions"))
            .json(&json!({ "kind": kind }))
            .send()
            .await
            .context("failed to reach gateway")?;

        if resp.status().is_success() {
            let v: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from POST /sessions")?;
            Ok(SessionDetailResponse {
                id: v["id"].as_str().unwrap_or("?").to_string(),
                kind: v["kind"].as_str().unwrap_or("direct").to_string(),
                status: v["status"].as_str().unwrap_or("unknown").to_string(),
                channel: v["channel_ref"].as_str().map(String::from),
                project_id: v["project_id"].as_str().map(String::from),
                requester_session_id: v["requester_session_id"].as_str().map(String::from),
                created_at: v["created_at"].as_str().map(String::from),
                turn_count: v["turn_count"].as_u64().map(|n| n as u32),
                latest_model: v["latest_model"].as_str().map(String::from),
                usage_prompt_tokens: v["usage_prompt_tokens"].as_u64(),
                usage_completion_tokens: v["usage_completion_tokens"].as_u64(),
                last_turn_started_at: v["last_turn_started_at"].as_str().map(String::from),
                last_turn_ended_at: v["last_turn_ended_at"].as_str().map(String::from),
            })
        } else {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            bail!("Gateway returned HTTP {status}: {body_text}");
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
                kind: v["kind"].as_str().unwrap_or("direct").to_string(),
                status: v["status"].as_str().unwrap_or("unknown").to_string(),
                channel: v["channel"]
                    .as_str()
                    .map(String::from)
                    .or_else(|| v["channel_ref"].as_str().map(String::from)),
                project_id: v["project_id"].as_str().map(String::from),
                requester_session_id: v["requester_session_id"].as_str().map(String::from),
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

    /// `GET /sessions/:id/transcript` — fetch full session transcript.
    pub async fn sessions_transcript(
        &self,
        id: &str,
    ) -> Result<Vec<crate::output::TranscriptEntry>> {
        let resp = self
            .http
            .get(self.url(&format!("/sessions/{id}/transcript")))
            .send()
            .await
            .context("failed to reach gateway")?;

        if resp.status().is_success() {
            let entries: Vec<crate::output::TranscriptEntry> = resp
                .json()
                .await
                .context("invalid JSON from /sessions/:id/transcript")?;
            Ok(entries)
        } else if resp.status() == reqwest::StatusCode::NOT_FOUND {
            bail!("Session '{id}' not found.");
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `GET /sessions?kind=subagent` — list subagent sessions.
    pub async fn agents_list(
        &self,
        active_minutes: Option<u64>,
        limit: u64,
    ) -> Result<AgentListResponse> {
        let mut query: Vec<(&str, String)> = vec![
            ("kind", "subagent".to_string()),
            ("limit", limit.to_string()),
        ];
        if let Some(active_minutes) = active_minutes {
            query.push(("active", active_minutes.to_string()));
        }

        let resp = self
            .http
            .get(self.url("/sessions"))
            .query(&query)
            .send()
            .await
            .context("failed to reach gateway")?;

        if resp.status().is_success() {
            let body: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from /sessions?kind=subagent")?;
            let agents = body
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .map(|v| AgentSummary {
                    id: v["id"].as_str().unwrap_or("?").to_string(),
                    status: v["status"].as_str().unwrap_or("unknown").to_string(),
                    parent_session_id: v["requester_session_id"].as_str().map(String::from),
                    created_at: v["created_at"].as_str().map(String::from),
                    turn_count: v["turn_count"].as_u64().map(|n| n as u32),
                    usage_prompt_tokens: v["usage_prompt_tokens"].as_u64(),
                    usage_completion_tokens: v["usage_completion_tokens"].as_u64(),
                    latest_model: v["latest_model"].as_str().map(String::from),
                })
                .collect();
            Ok(AgentListResponse { agents })
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `GET /sessions/:id` — show subagent session detail (re-uses session endpoint).
    pub async fn agents_show(&self, id: &str) -> Result<AgentDetailResponse> {
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
            Ok(AgentDetailResponse {
                id: v["id"].as_str().unwrap_or("?").to_string(),
                status: v["status"].as_str().unwrap_or("unknown").to_string(),
                parent_session_id: v["requester_session_id"].as_str().map(String::from),
                created_at: v["created_at"].as_str().map(String::from),
                turn_count: v["turn_count"].as_u64().map(|n| n as u32),
                latest_model: v["latest_model"].as_str().map(String::from),
                usage_prompt_tokens: v["usage_prompt_tokens"].as_u64(),
                usage_completion_tokens: v["usage_completion_tokens"].as_u64(),
                last_turn_started_at: v["last_turn_started_at"].as_str().map(String::from),
                last_turn_ended_at: v["last_turn_ended_at"].as_str().map(String::from),
            })
        } else if resp.status() == reqwest::StatusCode::NOT_FOUND {
            bail!("Agent session '{id}' not found.");
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `DELETE /sessions/:id` — delete a session and its transcript history.
    pub async fn session_delete(&self, id: &str) -> Result<ActionResult> {
        let resp = self
            .http
            .delete(self.url(&format!("/sessions/{id}")))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            bail!("Session '{id}' not found.");
        }
        Ok(ActionResult {
            success: resp.status().is_success(),
            message: if resp.status().is_success() {
                format!("Session {id} deleted")
            } else {
                format!("Gateway returned HTTP {}", resp.status())
            },
        })
    }

    /// `GET /sessions/:id/tree`
    pub async fn sessions_tree(&self, id: &str) -> Result<SessionTreeResponse> {
        let resp = self
            .http
            .get(self.url(&format!("/sessions/{id}/tree")))
            .send()
            .await
            .context("failed to reach gateway")?;

        if resp.status().is_success() {
            let root: SessionTreeNode = resp
                .json()
                .await
                .context("invalid JSON from /sessions/:id/tree")?;
            Ok(SessionTreeResponse { root })
        } else if resp.status() == reqwest::StatusCode::NOT_FOUND {
            bail!("Session '{id}' not found.");
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// Execute a fresh doctor run via the gateway.
    pub async fn doctor(&self) -> Result<DoctorReport> {
        self.doctor_run().await
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

    // ── Security ──────────────────────────────────────────────────

    /// `POST /security/audit` — run a security audit.
    pub async fn security_audit(&self) -> Result<SecurityAuditResponse> {
        let resp = self
            .http
            .post(self.url("/security/audit"))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            Ok(resp
                .json()
                .await
                .context("invalid JSON from /security/audit")?)
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    // ── Sandbox ───────────────────────────────────────────────────

    /// `GET /sandbox` — list sandbox boundaries.
    pub async fn sandbox_list(&self) -> Result<SandboxListResponse> {
        let resp = self
            .http
            .get(self.url("/sandbox"))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            Ok(resp.json().await.context("invalid JSON from /sandbox")?)
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `POST /sandbox/recreate` — recreate sandbox boundaries.
    pub async fn sandbox_recreate(&self) -> Result<SandboxRecreateResponse> {
        let resp = self
            .http
            .post(self.url("/sandbox/recreate"))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            Ok(resp
                .json()
                .await
                .context("invalid JSON from /sandbox/recreate")?)
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `GET /sandbox/explain` — explain current sandbox policy.
    pub async fn sandbox_explain(&self) -> Result<SandboxExplainResponse> {
        let resp = self
            .http
            .get(self.url("/sandbox/explain"))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            Ok(resp
                .json()
                .await
                .context("invalid JSON from /sandbox/explain")?)
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    // ── Secrets ───────────────────────────────────────────────────

    /// `POST /secrets/reload` — reload secrets from the secret store.
    pub async fn secrets_reload(&self) -> Result<SecretsReloadResponse> {
        let resp = self
            .http
            .post(self.url("/secrets/reload"))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            Ok(resp
                .json()
                .await
                .context("invalid JSON from /secrets/reload")?)
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `POST /secrets/audit` — audit secret usage.
    pub async fn secrets_audit(&self) -> Result<SecretsAuditResponse> {
        let resp = self
            .http
            .post(self.url("/secrets/audit"))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            Ok(resp
                .json()
                .await
                .context("invalid JSON from /secrets/audit")?)
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `GET /secrets/configure` — show secret store configuration.
    pub async fn secrets_configure(&self) -> Result<SecretsConfigureResponse> {
        let resp = self
            .http
            .get(self.url("/secrets/configure"))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            Ok(resp
                .json()
                .await
                .context("invalid JSON from /secrets/configure")?)
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `POST /secrets/apply` — apply a secrets manifest.
    pub async fn secrets_apply(&self, manifest: serde_json::Value) -> Result<SecretsApplyResponse> {
        let resp = self
            .http
            .post(self.url("/secrets/apply"))
            .json(&manifest)
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            Ok(resp
                .json()
                .await
                .context("invalid JSON from /secrets/apply")?)
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    // ── Configure ─────────────────────────────────────────────────

    /// `POST /configure` — run the setup wizard via the gateway.
    pub async fn configure(&self) -> Result<ConfigureResponse> {
        let resp = self
            .http
            .post(self.url("/configure"))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            Ok(resp.json().await.context("invalid JSON from /configure")?)
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    // ── Subagent lifecycle ───────────────────────────────────────────

    /// `POST /agents/spawn` — spawn a child subagent session.
    pub async fn agent_spawn(
        &self,
        parent: &str,
        mode: &str,
        policy: &str,
        task: &str,
        provider: Option<&str>,
    ) -> Result<AgentSpawnResponse> {
        let mut body = json!({
            "parent_session_id": parent,
            "mode": mode,
            "policy": policy,
            "task": task,
        });
        if let Some(p) = provider {
            body["provider"] = json!(p);
        }
        let resp = self
            .http
            .post(self.url("/agents/spawn"))
            .json(&body)
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let v: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from POST /agents/spawn")?;
            Ok(AgentSpawnResponse {
                session_id: v["session_id"].as_str().unwrap_or("?").to_string(),
                parent_session_id: v["parent_session_id"]
                    .as_str()
                    .unwrap_or(parent)
                    .to_string(),
                mode: v["mode"].as_str().unwrap_or(mode).to_string(),
                policy: v["policy"].as_str().unwrap_or(policy).to_string(),
                status: v["status"].as_str().unwrap_or("spawned").to_string(),
            })
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `POST /agents/:id/steer` — send follow-up instruction to a running subagent.
    pub async fn agent_steer(&self, id: &str, message: &str) -> Result<AgentSteerResponse> {
        let resp = self
            .http
            .post(self.url(&format!("/agents/{id}/steer")))
            .json(&json!({ "message": message }))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let v: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from POST /agents/:id/steer")?;
            Ok(AgentSteerResponse {
                session_id: id.to_string(),
                accepted: v["accepted"].as_bool().unwrap_or(true),
                detail: v["detail"]
                    .as_str()
                    .unwrap_or("instruction delivered")
                    .to_string(),
            })
        } else if resp.status() == reqwest::StatusCode::NOT_FOUND {
            bail!("Agent session '{id}' not found.");
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `POST /agents/:id/kill` — terminate a running subagent session.
    pub async fn agent_kill(&self, id: &str, reason: Option<&str>) -> Result<AgentKillResponse> {
        let mut body = json!({});
        if let Some(r) = reason {
            body["reason"] = json!(r);
        }
        let resp = self
            .http
            .post(self.url(&format!("/agents/{id}/kill")))
            .json(&body)
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let v: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from POST /agents/:id/kill")?;
            Ok(AgentKillResponse {
                session_id: id.to_string(),
                killed: v["killed"].as_bool().unwrap_or(true),
                detail: v["detail"]
                    .as_str()
                    .unwrap_or("session terminated")
                    .to_string(),
            })
        } else if resp.status() == reqwest::StatusCode::NOT_FOUND {
            bail!("Agent session '{id}' not found.");
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    // ── Agent turn invocation ────────────────────────────────────────

    /// `POST /agents/:id/run` — send a single instruction to a session.
    pub async fn agent_run(
        &self,
        session: &str,
        message: &str,
        max_turns: u32,
        wait: bool,
    ) -> Result<AgentRunResponse> {
        let resp = self
            .http
            .post(self.url(&format!("/agents/{session}/run")))
            .json(&json!({
                "message": message,
                "max_turns": max_turns,
                "wait": wait,
            }))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let v: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from POST /agents/:id/run")?;
            Ok(AgentRunResponse {
                session_id: session.to_string(),
                turn_id: v["turn_id"].as_str().unwrap_or("?").to_string(),
                status: v["status"].as_str().unwrap_or("completed").to_string(),
                output: v["output"].as_str().map(String::from),
            })
        } else if resp.status() == reqwest::StatusCode::NOT_FOUND {
            bail!("Session '{session}' not found.");
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `GET /agents/:id/turns/:turn_id` — retrieve a turn result.
    pub async fn agent_result(&self, session: &str, turn: &str) -> Result<AgentResultResponse> {
        let resp = self
            .http
            .get(self.url(&format!("/agents/{session}/turns/{turn}")))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let v: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from GET /agents/:id/turns/:turn_id")?;
            Ok(AgentResultResponse {
                session_id: session.to_string(),
                turn_id: turn.to_string(),
                status: v["status"].as_str().unwrap_or("unknown").to_string(),
                output: v["output"].as_str().map(String::from),
            })
        } else if resp.status() == reqwest::StatusCode::NOT_FOUND {
            bail!("Turn '{turn}' not found in session '{session}'.");
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    // ── ACP bridge ───────────────────────────────────────────────────

    /// `POST /acp/send` — send an ACP message between sessions.
    pub async fn acp_send(&self, from: &str, to: &str, payload: &str) -> Result<AcpSendResponse> {
        let payload_value: serde_json::Value =
            serde_json::from_str(payload).context("invalid JSON payload for ACP send")?;
        let resp = self
            .http
            .post(self.url("/acp/send"))
            .json(&json!({
                "from": from,
                "to": to,
                "payload": payload_value,
            }))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let v: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from POST /acp/send")?;
            Ok(AcpSendResponse {
                message_id: v["message_id"].as_str().unwrap_or("?").to_string(),
                from: from.to_string(),
                to: to.to_string(),
                delivered: v["delivered"].as_bool().unwrap_or(false),
            })
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `GET /acp/inbox?session=:id` — list pending ACP messages.
    pub async fn acp_inbox(&self, session: &str) -> Result<AcpInboxResponse> {
        let resp = self
            .http
            .get(self.url("/acp/inbox"))
            .query(&[("session", session)])
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let v: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from GET /acp/inbox")?;
            let messages = v
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .map(|m| AcpInboxMessage {
                            message_id: m["message_id"].as_str().unwrap_or("?").to_string(),
                            from: m["from"].as_str().unwrap_or("?").to_string(),
                            received_at: m["received_at"].as_str().unwrap_or("?").to_string(),
                            payload: m["payload"].clone(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            Ok(AcpInboxResponse {
                session_id: session.to_string(),
                messages,
            })
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `POST /acp/ack` — acknowledge an ACP message.
    pub async fn acp_ack(&self, message_id: &str, session: &str) -> Result<AcpAckResponse> {
        let resp = self
            .http
            .post(self.url("/acp/ack"))
            .json(&json!({
                "message_id": message_id,
                "session": session,
            }))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let v: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from POST /acp/ack")?;
            Ok(AcpAckResponse {
                message_id: message_id.to_string(),
                acknowledged: v["acknowledged"].as_bool().unwrap_or(true),
            })
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    // ── Config admin (#30) ──────────────────────────────────────
    pub async fn config_reload(&self) -> Result<crate::output::ConfigReloadResponse> {
        let r = self
            .http
            .post(self.url("/config/reload"))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn config_diff(&self) -> Result<crate::output::ConfigDiffResponse> {
        let r = self
            .http
            .get(self.url("/config/diff"))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn config_env(&self) -> Result<crate::output::ConfigEnvResponse> {
        let r = self
            .http
            .get(self.url("/config/env"))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn config_export(&self) -> Result<crate::output::ConfigExportResponse> {
        let r = self
            .http
            .get(self.url("/config/export"))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }

    // ── Processes (#39) ────────────────────────────────────────
    pub async fn process_list(&self) -> Result<serde_json::Value> {
        let r = self
            .http
            .get(self.url("/processes"))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn process_get(&self, id: &str) -> Result<serde_json::Value> {
        let r = self
            .http
            .get(self.url(&format!("/processes/{id}")))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn process_log(&self, id: &str) -> Result<String> {
        let r = self
            .http
            .get(self.url(&format!("/processes/{id}/log")))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.text().await.context("text")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn process_kill(&self, id: &str) -> Result<crate::output::ActionResult> {
        let r = self
            .http
            .post(self.url(&format!("/processes/{id}/kill")))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }

    // ── Microsoft 365 (#206) ────────────────────────────────────
    pub async fn ms365_mail_unread(
        &self,
        limit: u32,
        folder: &str,
    ) -> Result<crate::output::Ms365MailUnreadResponse> {
        let r = self
            .http
            .get(self.url("/ms365/mail/unread"))
            .query(&[("limit", limit.to_string()), ("folder", folder.to_string())])
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_calendar_upcoming(
        &self,
        limit: u32,
        hours: u32,
    ) -> Result<crate::output::Ms365CalendarUpcomingResponse> {
        let r = self
            .http
            .get(self.url("/ms365/calendar/upcoming"))
            .query(&[("limit", limit.to_string()), ("hours", hours.to_string())])
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_mail_read(&self, id: &str) -> Result<crate::output::Ms365MailReadResponse> {
        let r = self
            .http
            .get(self.url(&format!("/ms365/mail/messages/{id}")))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_calendar_read(
        &self,
        id: &str,
    ) -> Result<crate::output::Ms365CalendarReadResponse> {
        let r = self
            .http
            .get(self.url(&format!("/ms365/calendar/events/{id}")))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_calendar_create(
        &self,
        subject: &str,
        start: &str,
        end: &str,
        attendees: &[String],
        location: Option<&str>,
        body: Option<&str>,
    ) -> Result<crate::output::Ms365CalendarCreateResponse> {
        let mut payload = serde_json::json!({
            "subject": subject,
            "start": start,
            "end": end,
            "attendees": attendees,
        });
        if let Some(loc) = location {
            payload["location"] = serde_json::json!(loc);
        }
        if let Some(b) = body {
            payload["body"] = serde_json::json!(b);
        }
        let r = self
            .http
            .post(self.url("/ms365/calendar/events"))
            .json(&payload)
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn ms365_calendar_update(
        &self,
        id: &str,
        subject: Option<&str>,
        start: Option<&str>,
        end: Option<&str>,
        attendees: Option<&[String]>,
        location: Option<&str>,
        body: Option<&str>,
    ) -> Result<crate::output::Ms365CalendarUpdateResponse> {
        let mut payload = serde_json::Map::new();
        if let Some(value) = subject {
            payload.insert("subject".to_string(), serde_json::json!(value));
        }
        if let Some(value) = start {
            payload.insert("start".to_string(), serde_json::json!(value));
        }
        if let Some(value) = end {
            payload.insert("end".to_string(), serde_json::json!(value));
        }
        if let Some(value) = attendees {
            payload.insert("attendees".to_string(), serde_json::json!(value));
        }
        if let Some(value) = location {
            payload.insert("location".to_string(), serde_json::json!(value));
        }
        if let Some(value) = body {
            payload.insert("body".to_string(), serde_json::json!(value));
        }
        let r = self
            .http
            .post(self.url(&format!("/ms365/calendar/events/{id}")))
            .json(&payload)
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }

    pub async fn ms365_calendar_delete(
        &self,
        id: &str,
    ) -> Result<crate::output::Ms365CalendarDeleteResponse> {
        let r = self
            .http
            .post(self.url(&format!("/ms365/calendar/events/{id}/delete")))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_calendar_respond(
        &self,
        id: &str,
        response: &str,
        comment: Option<&str>,
    ) -> Result<crate::output::Ms365CalendarRespondResponse> {
        let mut payload = serde_json::json!({
            "response": response,
        });
        if let Some(c) = comment {
            payload["comment"] = serde_json::json!(c);
        }
        let r = self
            .http
            .post(self.url(&format!("/ms365/calendar/events/{id}/respond")))
            .json(&payload)
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_auth_status(&self) -> Result<crate::output::Ms365AuthStatusResponse> {
        let r = self
            .http
            .get(self.url("/ms365/auth/status"))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_mail_folders(&self) -> Result<crate::output::Ms365MailFoldersResponse> {
        let r = self
            .http
            .get(self.url("/ms365/mail/folders"))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_mail_send(
        &self,
        to: &[String],
        subject: &str,
        body: &str,
        cc: &[String],
    ) -> Result<crate::output::Ms365MailSendResponse> {
        let payload = serde_json::json!({
            "to": to,
            "subject": subject,
            "body": body,
            "cc": cc,
        });
        let r = self
            .http
            .post(self.url("/ms365/mail/send"))
            .json(&payload)
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_mail_reply(
        &self,
        id: &str,
        body: &str,
        reply_all: bool,
    ) -> Result<crate::output::Ms365MailReplyResponse> {
        let payload = serde_json::json!({
            "body": body,
            "reply_all": reply_all,
        });
        let r = self
            .http
            .post(self.url(&format!("/ms365/mail/messages/{id}/reply")))
            .json(&payload)
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_mail_forward(
        &self,
        id: &str,
        to: &[String],
        comment: Option<&str>,
    ) -> Result<crate::output::Ms365MailForwardResponse> {
        let mut payload = serde_json::json!({
            "to": to,
        });
        if let Some(c) = comment {
            payload["comment"] = serde_json::json!(c);
        }
        let r = self
            .http
            .post(self.url(&format!("/ms365/mail/messages/{id}/forward")))
            .json(&payload)
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_mail_attachments(
        &self,
        message_id: &str,
    ) -> Result<crate::output::Ms365MailAttachmentsResponse> {
        let r = self
            .http
            .get(self.url(&format!("/ms365/mail/messages/{message_id}/attachments")))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_mail_attachment_read(
        &self,
        message_id: &str,
        attachment_id: &str,
    ) -> Result<crate::output::Ms365MailAttachmentReadResponse> {
        let r = self
            .http
            .get(self.url(&format!(
                "/ms365/mail/messages/{message_id}/attachments/{attachment_id}"
            )))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    /// Download raw attachment content. Returns (filename, bytes).
    pub async fn ms365_mail_attachment_download(
        &self,
        message_id: &str,
        attachment_id: &str,
    ) -> Result<(String, Vec<u8>)> {
        let meta: crate::output::Ms365MailAttachmentReadResponse = {
            let r = self
                .http
                .get(self.url(&format!(
                    "/ms365/mail/messages/{message_id}/attachments/{attachment_id}"
                )))
                .send()
                .await
                .context("gateway")?;
            if r.status().is_success() {
                r.json().await.context("json")?
            } else {
                bail!("HTTP {}", r.status());
            }
        };
        let r = self
            .http
            .get(self.url(&format!(
                "/ms365/mail/messages/{message_id}/attachments/{attachment_id}/content"
            )))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok((meta.name, r.bytes().await.context("bytes")?.to_vec()))
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_files_list(
        &self,
        path: &str,
        limit: u32,
    ) -> Result<crate::output::Ms365FilesListResponse> {
        let r = self
            .http
            .get(self.url("/ms365/files"))
            .query(&[("path", path.to_string()), ("limit", limit.to_string())])
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_files_read(&self, id: &str) -> Result<crate::output::Ms365FileReadResponse> {
        let r = self
            .http
            .get(self.url(&format!("/ms365/files/{id}")))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_files_search(
        &self,
        query: &str,
        limit: u32,
    ) -> Result<crate::output::Ms365FilesSearchResponse> {
        let r = self
            .http
            .get(self.url("/ms365/files/search"))
            .query(&[("query", query.to_string()), ("limit", limit.to_string())])
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    /// Download a OneDrive file's content. Returns (filename, bytes).
    pub async fn ms365_files_download(&self, id: &str) -> Result<(String, Vec<u8>)> {
        let meta: crate::output::Ms365FileReadResponse = {
            let r = self
                .http
                .get(self.url(&format!("/ms365/files/{id}")))
                .send()
                .await
                .context("gateway")?;
            if r.status().is_success() {
                r.json().await.context("json")?
            } else {
                bail!("HTTP {}", r.status());
            }
        };
        let r = self
            .http
            .get(self.url(&format!("/ms365/files/{id}/content")))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok((meta.name, r.bytes().await.context("bytes")?.to_vec()))
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_users_me(&self) -> Result<crate::output::Ms365UserProfileResponse> {
        let r = self
            .http
            .get(self.url("/ms365/users/me"))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_users_list(
        &self,
        limit: u32,
    ) -> Result<crate::output::Ms365UsersListResponse> {
        let r = self
            .http
            .get(self.url("/ms365/users"))
            .query(&[("limit", limit.to_string())])
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_users_read(
        &self,
        id: &str,
    ) -> Result<crate::output::Ms365UserProfileResponse> {
        let r = self
            .http
            .get(self.url(&format!("/ms365/users/{id}")))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_planner_plans(
        &self,
        limit: u32,
    ) -> Result<crate::output::Ms365PlannerPlansResponse> {
        let r = self
            .http
            .get(self.url("/ms365/planner/plans"))
            .query(&[("limit", limit.to_string())])
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_planner_tasks(
        &self,
        plan_id: &str,
        limit: u32,
    ) -> Result<crate::output::Ms365PlannerTasksResponse> {
        let r = self
            .http
            .get(self.url(&format!("/ms365/planner/plans/{plan_id}/tasks")))
            .query(&[("limit", limit.to_string())])
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_planner_task_read(
        &self,
        id: &str,
    ) -> Result<crate::output::Ms365PlannerTaskReadResponse> {
        let r = self
            .http
            .get(self.url(&format!("/ms365/planner/tasks/{id}")))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_planner_task_create(
        &self,
        plan_id: &str,
        title: &str,
        bucket_id: Option<&str>,
        due_date: Option<&str>,
        description: Option<&str>,
    ) -> Result<crate::output::Ms365PlannerTaskCreateResponse> {
        let mut payload = serde_json::json!({
            "plan_id": plan_id,
            "title": title,
        });
        if let Some(bucket_id) = bucket_id {
            payload["bucket_id"] = serde_json::json!(bucket_id);
        }
        if let Some(due_date) = due_date {
            payload["due_date"] = serde_json::json!(due_date);
        }
        if let Some(description) = description {
            payload["description"] = serde_json::json!(description);
        }
        let r = self
            .http
            .post(self.url("/ms365/planner/tasks"))
            .json(&payload)
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_planner_task_update(
        &self,
        id: &str,
        title: Option<&str>,
        bucket_id: Option<&str>,
        due_date: Option<&str>,
        description: Option<&str>,
        percent_complete: Option<u8>,
    ) -> Result<crate::output::Ms365PlannerTaskUpdateResponse> {
        let mut payload = serde_json::json!({});
        if let Some(title) = title {
            payload["title"] = serde_json::json!(title);
        }
        if let Some(bucket_id) = bucket_id {
            payload["bucket_id"] = serde_json::json!(bucket_id);
        }
        if let Some(due_date) = due_date {
            payload["due_date"] = serde_json::json!(due_date);
        }
        if let Some(description) = description {
            payload["description"] = serde_json::json!(description);
        }
        if let Some(percent_complete) = percent_complete {
            payload["percent_complete"] = serde_json::json!(percent_complete);
        }
        let r = self
            .http
            .post(self.url(&format!("/ms365/planner/tasks/{id}")))
            .json(&payload)
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_planner_task_complete(
        &self,
        id: &str,
    ) -> Result<crate::output::Ms365PlannerTaskCompleteResponse> {
        let r = self
            .http
            .post(self.url(&format!("/ms365/planner/tasks/{id}/complete")))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_todo_lists(
        &self,
        limit: u32,
    ) -> Result<crate::output::Ms365TodoListsResponse> {
        let r = self
            .http
            .get(self.url("/ms365/todo/lists"))
            .query(&[("limit", limit.to_string())])
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_todo_tasks(
        &self,
        list_id: &str,
        limit: u32,
    ) -> Result<crate::output::Ms365TodoTasksResponse> {
        let r = self
            .http
            .get(self.url(&format!("/ms365/todo/lists/{list_id}/tasks")))
            .query(&[("limit", limit.to_string())])
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_todo_task_read(
        &self,
        list_id: &str,
        id: &str,
    ) -> Result<crate::output::Ms365TodoTaskReadResponse> {
        let r = self
            .http
            .get(self.url(&format!("/ms365/todo/lists/{list_id}/tasks/{id}")))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_todo_task_create(
        &self,
        list_id: &str,
        title: &str,
        due_date: Option<&str>,
        importance: Option<&str>,
        body: Option<&str>,
    ) -> Result<crate::output::Ms365TodoTaskCreateResponse> {
        let mut payload = serde_json::json!({
            "title": title,
        });
        if let Some(due_date) = due_date {
            payload["due_date"] = serde_json::json!(due_date);
        }
        if let Some(importance) = importance {
            payload["importance"] = serde_json::json!(importance);
        }
        if let Some(body) = body {
            payload["body"] = serde_json::json!(body);
        }
        let r = self
            .http
            .post(self.url(&format!("/ms365/todo/lists/{list_id}/tasks")))
            .json(&payload)
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_todo_task_update(
        &self,
        list_id: &str,
        id: &str,
        update: Ms365TodoTaskUpdateInput,
    ) -> Result<crate::output::Ms365TodoTaskUpdateResponse> {
        let mut payload = serde_json::json!({});
        if let Some(title) = update.title.as_deref() {
            payload["title"] = serde_json::json!(title);
        }
        if let Some(status) = update.status.as_deref() {
            payload["status"] = serde_json::json!(status);
        }
        if let Some(importance) = update.importance.as_deref() {
            payload["importance"] = serde_json::json!(importance);
        }
        if let Some(due_date) = update.due_date.as_deref() {
            payload["due_date"] = serde_json::json!(due_date);
        }
        if let Some(body) = update.body.as_deref() {
            payload["body"] = serde_json::json!(body);
        }
        let r = self
            .http
            .post(self.url(&format!("/ms365/todo/lists/{list_id}/tasks/{id}")))
            .json(&payload)
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_todo_task_complete(
        &self,
        list_id: &str,
        id: &str,
    ) -> Result<crate::output::Ms365TodoTaskCompleteResponse> {
        let r = self
            .http
            .post(self.url(&format!("/ms365/todo/lists/{list_id}/tasks/{id}/complete")))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }

    pub async fn ms365_sites_list(
        &self,
        limit: u32,
    ) -> Result<crate::output::Ms365SitesListResponse> {
        let r = self
            .http
            .get(self.url("/ms365/sites"))
            .query(&[("limit", limit.to_string())])
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_sites_read(&self, id: &str) -> Result<crate::output::Ms365SiteReadResponse> {
        let r = self
            .http
            .get(self.url(&format!("/ms365/sites/{id}")))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_sites_lists(
        &self,
        site_id: &str,
        limit: u32,
    ) -> Result<crate::output::Ms365SiteListsResponse> {
        let r = self
            .http
            .get(self.url(&format!("/ms365/sites/{site_id}/lists")))
            .query(&[("limit", limit.to_string())])
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_sites_list_items(
        &self,
        site_id: &str,
        list_id: &str,
        limit: u32,
    ) -> Result<crate::output::Ms365SiteListItemsResponse> {
        let r = self
            .http
            .get(self.url(&format!("/ms365/sites/{site_id}/lists/{list_id}/items")))
            .query(&[("limit", limit.to_string())])
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }

    pub async fn ms365_teams_list(
        &self,
        limit: u32,
    ) -> Result<crate::output::Ms365TeamsListResponse> {
        let r = self
            .http
            .get(self.url("/ms365/teams"))
            .query(&[("limit", limit.to_string())])
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_teams_channels(
        &self,
        team_id: &str,
        limit: u32,
    ) -> Result<crate::output::Ms365TeamsChannelsResponse> {
        let r = self
            .http
            .get(self.url(&format!("/ms365/teams/{team_id}/channels")))
            .query(&[("limit", limit.to_string())])
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_teams_channel_read(
        &self,
        team_id: &str,
        id: &str,
    ) -> Result<crate::output::Ms365TeamsChannelReadResponse> {
        let r = self
            .http
            .get(self.url(&format!("/ms365/teams/{team_id}/channels/{id}")))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn ms365_teams_messages(
        &self,
        team_id: &str,
        channel_id: &str,
        limit: u32,
    ) -> Result<crate::output::Ms365TeamsMessagesResponse> {
        let r = self
            .http
            .get(self.url(&format!(
                "/ms365/teams/{team_id}/channels/{channel_id}/messages"
            )))
            .query(&[("limit", limit.to_string())])
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }

    // ── Lifecycle (#74, #70) ────────────────────────────────────
    pub async fn setup(&self) -> Result<crate::output::SetupResponse> {
        let r = self
            .http
            .post(self.url("/setup"))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn backup_create(
        &self,
        label: Option<&str>,
    ) -> Result<crate::output::BackupCreateResponse> {
        let mut b = json!({});
        if let Some(l) = label {
            b["label"] = json!(l);
        }
        let r = self
            .http
            .post(self.url("/backups"))
            .json(&b)
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn backup_list(&self) -> Result<crate::output::BackupListResponse> {
        let r = self
            .http
            .get(self.url("/backups"))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn backup_restore(&self, id: &str) -> Result<crate::output::BackupRestoreResponse> {
        let r = self
            .http
            .post(self.url(&format!("/backups/{id}/restore")))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn update_check(&self) -> Result<crate::output::UpdateCheckResponse> {
        let r = self
            .http
            .get(self.url("/update/check"))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn update_apply(&self) -> Result<crate::output::UpdateApplyResponse> {
        let r = self
            .http
            .post(self.url("/update/apply"))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn update_status(&self) -> Result<crate::output::UpdateStatusResponse> {
        let r = self
            .http
            .get(self.url("/update/status"))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
        }
    }
    pub async fn reset(&self) -> Result<crate::output::ResetResponse> {
        let r = self
            .http
            .post(self.url("/reset"))
            .send()
            .await
            .context("gateway")?;
        if r.status().is_success() {
            Ok(r.json().await.context("json")?)
        } else {
            bail!("HTTP {}", r.status());
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

/// Show resolved config as pretty-printed JSON with secrets redacted.
pub fn show_config() -> Result<String> {
    let config = rune_config::AppConfig::load(Some(local_config_path()))
        .context("failed to load configuration")?;
    serde_json::to_string_pretty(&config.redacted()).context("failed to serialize config")
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

    #[tokio::test]
    async fn gateway_config_gets_redacted_json() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/config"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "gateway": {
                    "host": "127.0.0.1",
                    "auth_token": "***redacted***"
                }
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.gateway_config().await.unwrap();
        assert_eq!(resp.action, "current");
        assert_eq!(resp.config["gateway"]["host"], "127.0.0.1");
        assert_eq!(resp.config["gateway"]["auth_token"], "***redacted***");
    }

    #[tokio::test]
    async fn gateway_config_apply_puts_json_and_parses_response() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/config"))
            .and(body_json(serde_json::json!({
                "gateway": {
                    "host": "0.0.0.0",
                    "port": 8787
                }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "gateway": {
                    "host": "0.0.0.0",
                    "port": 8787,
                    "auth_token": "***redacted***"
                }
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .gateway_config_apply(serde_json::json!({
                "gateway": {
                    "host": "0.0.0.0",
                    "port": 8787
                }
            }))
            .await
            .unwrap();
        assert_eq!(resp.action, "applied");
        assert_eq!(resp.config["gateway"]["port"], 8787);
        assert_eq!(resp.config["gateway"]["auth_token"], "***redacted***");
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
    async fn logs_query_passes_filters_and_parses_response() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/logs"))
            .and(query_param("level", "warn"))
            .and(query_param("source", "gateway"))
            .and(query_param("limit", "25"))
            .and(query_param("since", "2026-03-20T09:00:00Z"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "entries": [
                    {
                        "timestamp": "2026-03-20T09:30:00Z",
                        "level": "warn",
                        "message": "gateway restart pending"
                    }
                ],
                "message": "1 log entry returned"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .logs_query(
                Some("warn"),
                Some("gateway"),
                Some(25),
                Some("2026-03-20T09:00:00Z"),
            )
            .await
            .unwrap();
        assert_eq!(resp.entries.len(), 1);
        assert_eq!(resp.entries[0]["level"], "warn");
        assert_eq!(resp.message, "1 log entry returned");
    }

    #[tokio::test]
    async fn doctor_run_posts_and_parses_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/doctor/run"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "overall": "degraded",
                "checks": [
                    {
                        "name": "auth",
                        "status": "warn",
                        "message": "no auth token configured"
                    }
                ],
                "run_at": "2026-03-20T09:35:00Z"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.doctor_run().await.unwrap();
        assert_eq!(resp.overall, "degraded");
        assert_eq!(resp.checks.len(), 1);
        assert_eq!(resp.checks[0].status, "warn");
        assert_eq!(resp.run_at, "2026-03-20T09:35:00Z");
    }

    #[tokio::test]
    async fn doctor_results_gets_and_parses_response() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/doctor/results"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "overall": "healthy",
                "checks": [
                    {
                        "name": "session_store",
                        "status": "pass",
                        "message": "session store reachable"
                    }
                ],
                "run_at": "2026-03-20T09:36:00Z"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.doctor_results().await.unwrap();
        assert_eq!(resp.overall, "healthy");
        assert_eq!(resp.checks[0].name, "session_store");
        assert_eq!(resp.checks[0].status, "pass");
        assert_eq!(resp.run_at, "2026-03-20T09:36:00Z");
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
    async fn skills_list_parses_entries() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/skills"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {
                    "name": "alpha",
                    "description": "First skill",
                    "enabled": true,
                    "source_dir": "/data/skills/alpha",
                    "binary_path": "/data/skills/alpha/run.sh"
                },
                {
                    "name": "beta",
                    "description": "Second skill",
                    "enabled": false,
                    "source_dir": "/data/skills/beta",
                    "binary_path": null
                }
            ])))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.skills_list().await.unwrap();
        assert_eq!(resp.skills.len(), 2);
        assert_eq!(resp.skills[0].name, "alpha");
        assert!(resp.skills[0].enabled);
        assert_eq!(
            resp.skills[0].binary_path.as_deref(),
            Some("/data/skills/alpha/run.sh")
        );
        assert_eq!(resp.skills[1].name, "beta");
        assert!(!resp.skills[1].enabled);
        assert!(resp.skills[1].binary_path.is_none());
    }

    #[tokio::test]
    async fn skills_check_parses_reload_summary() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/skills/reload"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "success": true,
                "discovered": 4,
                "loaded": 3,
                "removed": 1
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.skills_check().await.unwrap();
        assert!(resp.success);
        assert_eq!(resp.discovered, 4);
        assert_eq!(resp.loaded, 3);
        assert_eq!(resp.removed, 1);
    }

    #[tokio::test]
    async fn skills_info_falls_back_to_list_when_detail_route_is_absent() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/skills/alpha"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/skills"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {
                    "name": "alpha",
                    "description": "First skill",
                    "enabled": true,
                    "source_dir": "/data/skills/alpha",
                    "binary_path": "/data/skills/alpha/run.sh"
                }
            ])))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.skills_info("alpha").await.unwrap();
        assert_eq!(resp.skill.name, "alpha");
        assert!(resp.skill.enabled);
        assert_eq!(
            resp.skill.binary_path.as_deref(),
            Some("/data/skills/alpha/run.sh")
        );
    }

    #[tokio::test]
    async fn skills_info_errors_when_skill_is_missing() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/skills/missing"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/skills"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {
                    "name": "alpha",
                    "description": "First skill",
                    "enabled": true,
                    "source_dir": "/data/skills/alpha",
                    "binary_path": "/data/skills/alpha/run.sh"
                }
            ])))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let err = client.skills_info("missing").await.unwrap_err();
        assert!(err.to_string().contains("skill not found: missing"));
    }

    #[tokio::test]
    async fn skills_enable_parses_action_result() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/skills/alpha/enable"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "success": true,
                "message": "skill 'alpha' enabled"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.skills_enable("alpha").await.unwrap();
        assert!(resp.success);
        assert_eq!(resp.message, "skill 'alpha' enabled");
    }

    #[tokio::test]
    async fn skills_disable_parses_action_result() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/skills/alpha/disable"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "success": true,
                "message": "skill 'alpha' disabled"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.skills_disable("alpha").await.unwrap();
        assert!(resp.success);
        assert_eq!(resp.message, "skill 'alpha' disabled");
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
    async fn gateway_status_parses_instance_identity_manifest() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/status"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "status": "running",
                "version": "0.1.0",
                "uptime_seconds": 12,
                "capabilities": {
                    "identity": {
                        "id": "node-a",
                        "name": "Node A",
                        "advertised_addr": "http://10.0.0.5:8787",
                        "roles": ["gateway", "scheduler"],
                        "capabilities_version": 1
                    }
                }
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.gateway_status().await.unwrap();
        assert_eq!(resp.instance_id.as_deref(), Some("node-a"));
        assert_eq!(resp.instance_name.as_deref(), Some("Node A"));
        assert_eq!(resp.advertised_addr.as_deref(), Some("http://10.0.0.5:8787"));
        assert_eq!(resp.instance_roles, vec!["gateway", "scheduler"]);
        assert_eq!(resp.capabilities_version, Some(1));
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
        let resp = client
            .sessions_list(None, None, None, None, None, 100)
            .await
            .unwrap();
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

    #[tokio::test]
    async fn agents_list_parses_subagent_sessions() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/sessions"))
            .and(query_param("kind", "subagent"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "id": "sub-1",
                    "kind": "subagent",
                    "status": "running",
                    "requester_session_id": "parent-abc",
                    "turn_count": 2
                }
            ])))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.agents_list(None, 100).await.unwrap();
        assert_eq!(resp.agents.len(), 1);
        assert_eq!(resp.agents[0].id, "sub-1");
        assert_eq!(
            resp.agents[0].parent_session_id.as_deref(),
            Some("parent-abc")
        );
    }

    #[tokio::test]
    async fn agents_show_parses_detail() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/sessions/sub-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "sub-1",
                "kind": "subagent",
                "status": "running",
                "requester_session_id": "parent-abc",
                "created_at": "2026-03-19T00:00:00Z",
                "turn_count": 2,
                "latest_model": "gpt-5",
                "usage_prompt_tokens": 100,
                "usage_completion_tokens": 50
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.agents_show("sub-1").await.unwrap();
        assert_eq!(resp.id, "sub-1");
        assert_eq!(resp.parent_session_id.as_deref(), Some("parent-abc"));
        assert_eq!(resp.latest_model.as_deref(), Some("gpt-5"));
    }

    #[tokio::test]
    async fn agents_show_not_found() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/sessions/nonexistent"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let err = client.agents_show("nonexistent").await.unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn sessions_tree_builds_hierarchy() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/sessions/root-1/tree"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "root-1",
                "kind": "direct",
                "status": "running",
                "channel": "local",
                "children": [
                    {
                        "id": "child-a",
                        "kind": "subagent",
                        "status": "running",
                        "channel": "local",
                        "children": [
                            {
                                "id": "grandchild-1",
                                "kind": "subagent",
                                "status": "idle",
                                "channel": "local",
                                "children": []
                            }
                        ]
                    },
                    {
                        "id": "child-b",
                        "kind": "subagent",
                        "status": "idle",
                        "channel": "local",
                        "children": []
                    }
                ]
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.sessions_tree("root-1").await.unwrap();
        assert_eq!(resp.root.id, "root-1");
        assert_eq!(resp.root.children.len(), 2);
        assert_eq!(resp.root.children[0].id, "child-a");
        assert_eq!(resp.root.children[0].children.len(), 1);
        assert_eq!(resp.root.children[0].children[0].id, "grandchild-1");
        assert_eq!(resp.root.children[1].id, "child-b");
        assert!(resp.root.children[1].children.is_empty());
    }

    #[tokio::test]
    async fn sessions_tree_not_found() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/sessions/missing/tree"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let err = client.sessions_tree("missing").await.unwrap_err();
        assert!(err.to_string().contains("not found"));
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
    async fn message_edit_success() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/messages/msg-42"))
            .and(body_json(json!({
                "channel": "telegram",
                "text": "Updated text"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "msg-42",
                "detail": "Message edited"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .message_edit("msg-42", "telegram", "Updated text", None)
            .await
            .unwrap();
        assert!(resp.success);
        assert_eq!(resp.message_id, "msg-42");
        assert_eq!(resp.channel, "telegram");
        assert_eq!(resp.detail, "Message edited");
    }

    #[tokio::test]
    async fn message_edit_with_session() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/messages/msg-99"))
            .and(body_json(json!({
                "channel": "discord",
                "text": "Fixed typo",
                "session": "sess-7"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "msg-99",
                "detail": "Message edited"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .message_edit("msg-99", "discord", "Fixed typo", Some("sess-7"))
            .await
            .unwrap();
        assert!(resp.success);
        assert_eq!(resp.message_id, "msg-99");
        assert_eq!(resp.channel, "discord");
    }

    #[tokio::test]
    async fn message_edit_gateway_error() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/messages/msg-missing"))
            .respond_with(ResponseTemplate::new(404).set_body_string("Message not found"))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .message_edit("msg-missing", "telegram", "new text", None)
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
    async fn message_read_success() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/messages/msg-42"))
            .and(query_param("channel", "telegram"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "msg-42",
                "channel": "telegram",
                "sender": "alice",
                "text": "Hello world",
                "timestamp": "2026-03-19T12:00:00Z",
                "detail": "Message retrieved"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .message_read("msg-42", "telegram", None)
            .await
            .unwrap();
        assert!(resp.success);
        assert_eq!(resp.message_id, "msg-42");
        assert_eq!(resp.channel, "telegram");
        assert_eq!(resp.sender.as_deref(), Some("alice"));
        assert_eq!(resp.text.as_deref(), Some("Hello world"));
        assert_eq!(resp.timestamp.as_deref(), Some("2026-03-19T12:00:00Z"));
        assert!(resp.thread_id.is_none());
    }

    #[tokio::test]
    async fn message_read_with_session_and_thread() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/messages/msg-77"))
            .and(query_param("channel", "discord"))
            .and(query_param("session", "sess-5"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "msg-77",
                "channel": "discord",
                "sender": "bob",
                "text": "threaded message",
                "timestamp": "2026-03-19T14:00:00Z",
                "thread_id": "thr-10"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .message_read("msg-77", "discord", Some("sess-5"))
            .await
            .unwrap();
        assert!(resp.success);
        assert_eq!(resp.message_id, "msg-77");
        assert_eq!(resp.thread_id.as_deref(), Some("thr-10"));
    }

    #[tokio::test]
    async fn message_read_gateway_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/messages/msg-missing"))
            .respond_with(ResponseTemplate::new(404).set_body_string("Message not found"))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .message_read("msg-missing", "telegram", None)
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

    #[tokio::test]
    async fn ms365_planner_task_create_posts_expected_payload() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/ms365/planner/tasks"))
            .and(body_json(json!({
                "plan_id": "plan-1",
                "title": "Draft follow-up",
                "bucket_id": "bucket-1",
                "due_date": "2026-03-25T12:00:00Z",
                "description": "Prepare operator summary"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "success": true,
                "message": "Planner task created",
                "task_id": "task-1"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .ms365_planner_task_create(
                "plan-1",
                "Draft follow-up",
                Some("bucket-1"),
                Some("2026-03-25T12:00:00Z"),
                Some("Prepare operator summary"),
            )
            .await
            .unwrap();
        assert!(resp.success);
        assert_eq!(resp.task_id.as_deref(), Some("task-1"));
    }

    #[tokio::test]
    async fn ms365_planner_task_update_posts_only_changed_fields() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/ms365/planner/tasks/task-1"))
            .and(body_json(json!({
                "title": "Updated summary",
                "percent_complete": 60
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "success": true,
                "message": "Planner task updated",
                "task_id": "task-1"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .ms365_planner_task_update(
                "task-1",
                Some("Updated summary"),
                None,
                None,
                None,
                Some(60),
            )
            .await
            .unwrap();
        assert!(resp.success);
        assert_eq!(resp.task_id.as_deref(), Some("task-1"));
    }

    #[tokio::test]
    async fn ms365_planner_task_complete_posts_expected_route() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/ms365/planner/tasks/task-1/complete"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "success": true,
                "message": "Planner task completed",
                "task_id": "task-1"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.ms365_planner_task_complete("task-1").await.unwrap();
        assert!(resp.success);
        assert_eq!(resp.task_id.as_deref(), Some("task-1"));
    }

    #[tokio::test]
    async fn ms365_todo_task_create_posts_expected_payload() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/ms365/todo/lists/list-1/tasks"))
            .and(body_json(json!({
                "title": "Draft follow-up",
                "due_date": "2026-03-25T12:00:00Z",
                "importance": "high",
                "body": "Prepare operator summary"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "success": true,
                "message": "To-Do task created",
                "task_id": "task-1"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .ms365_todo_task_create(
                "list-1",
                "Draft follow-up",
                Some("2026-03-25T12:00:00Z"),
                Some("high"),
                Some("Prepare operator summary"),
            )
            .await
            .unwrap();
        assert!(resp.success);
        assert_eq!(resp.task_id.as_deref(), Some("task-1"));
    }

    #[tokio::test]
    async fn ms365_todo_task_update_posts_only_changed_fields() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/ms365/todo/lists/list-1/tasks/task-1"))
            .and(body_json(json!({
                "status": "inProgress",
                "importance": "high"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "success": true,
                "message": "To-Do task updated",
                "task_id": "task-1"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .ms365_todo_task_update(
                "list-1",
                "task-1",
                Ms365TodoTaskUpdateInput {
                    status: Some("inProgress".to_string()),
                    importance: Some("high".to_string()),
                    ..Ms365TodoTaskUpdateInput::default()
                },
            )
            .await
            .unwrap();
        assert!(resp.success);
        assert_eq!(resp.task_id.as_deref(), Some("task-1"));
    }

    #[tokio::test]
    async fn ms365_todo_task_complete_posts_expected_route() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/ms365/todo/lists/list-1/tasks/task-1/complete"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "success": true,
                "message": "To-Do task completed",
                "task_id": "task-1"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client
            .ms365_todo_task_complete("list-1", "task-1")
            .await
            .unwrap();
        assert!(resp.success);
        assert_eq!(resp.task_id.as_deref(), Some("task-1"));
    }

    // ── Security ──────────────────────────────────────────────────

    #[tokio::test]
    async fn security_audit_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/security/audit"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "passed": true,
                "checks": [{"name": "sandbox", "status": "pass", "detail": "enabled"}],
                "summary": "all checks passed"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.security_audit().await.unwrap();
        assert!(resp.passed);
        assert_eq!(resp.checks.len(), 1);
        assert_eq!(resp.checks[0].name, "sandbox");
    }

    #[tokio::test]
    async fn security_audit_gateway_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/security/audit"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        assert!(client.security_audit().await.is_err());
    }

    // ── Sandbox ───────────────────────────────────────────────────

    #[tokio::test]
    async fn sandbox_list_success() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/sandbox"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "boundaries": [
                    {"path": "/workspace", "mode": "read-write", "active": true}
                ]
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.sandbox_list().await.unwrap();
        assert_eq!(resp.boundaries.len(), 1);
        assert!(resp.boundaries[0].active);
    }

    #[tokio::test]
    async fn sandbox_recreate_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/sandbox/recreate"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "success": true,
                "detail": "Sandbox boundaries recreated."
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.sandbox_recreate().await.unwrap();
        assert!(resp.success);
    }

    #[tokio::test]
    async fn sandbox_explain_success() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/sandbox/explain"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "explanation": "Agent is confined to /workspace."
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.sandbox_explain().await.unwrap();
        assert!(resp.explanation.contains("/workspace"));
    }

    // ── Secrets ───────────────────────────────────────────────────

    #[tokio::test]
    async fn secrets_reload_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/secrets/reload"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "success": true,
                "reloaded": 3,
                "detail": "Secrets reloaded."
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.secrets_reload().await.unwrap();
        assert!(resp.success);
        assert_eq!(resp.reloaded, 3);
    }

    #[tokio::test]
    async fn secrets_audit_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/secrets/audit"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "total": 5,
                "stale": 1,
                "unused": 2,
                "entries": [
                    {"key": "OPENAI_API_KEY", "status": "active", "last_used": "2026-03-19"}
                ]
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.secrets_audit().await.unwrap();
        assert_eq!(resp.total, 5);
        assert_eq!(resp.entries.len(), 1);
    }

    #[tokio::test]
    async fn secrets_configure_success() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/secrets/configure"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "store_kind": "env",
                "detail": "Environment variable secret store"
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.secrets_configure().await.unwrap();
        assert_eq!(resp.store_kind, "env");
    }

    #[tokio::test]
    async fn secrets_apply_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/secrets/apply"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "success": true,
                "applied": 2,
                "detail": "Manifest applied."
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.secrets_apply(json!({"secrets": []})).await.unwrap();
        assert!(resp.success);
        assert_eq!(resp.applied, 2);
    }

    #[tokio::test]
    async fn secrets_apply_gateway_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/secrets/apply"))
            .respond_with(ResponseTemplate::new(400).set_body_string("Invalid manifest"))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        assert!(client.secrets_apply(json!({})).await.is_err());
    }

    // ── Configure ─────────────────────────────────────────────────

    #[tokio::test]
    async fn configure_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/configure"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "success": true,
                "detail": "Setup wizard completed."
            })))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.configure().await.unwrap();
        assert!(resp.success);
    }

    #[tokio::test]
    async fn configure_gateway_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/configure"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        assert!(client.configure().await.is_err());
    }
}

#[cfg(test)]
mod subagent_tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn agent_spawn_ok() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/agents/spawn"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"session_id":"child-1","parent_session_id":"sess-1","mode":"coding","policy":"inherit","status":"spawned"})))
            .mount(&server).await;
        let c = GatewayClient::new(&server.uri());
        let r = c
            .agent_spawn("sess-1", "coding", "inherit", "test", None)
            .await
            .unwrap();
        assert_eq!(r.session_id, "child-1");
        assert_eq!(r.parent_session_id, "sess-1");
    }

    #[tokio::test]
    async fn agent_steer_ok() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/agents/child-1/steer"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"accepted":true,"detail":"ok"})),
            )
            .mount(&server)
            .await;
        let c = GatewayClient::new(&server.uri());
        let r = c.agent_steer("child-1", "focus").await.unwrap();
        assert!(r.accepted);
    }

    #[tokio::test]
    async fn agent_steer_404() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/agents/x/steer"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let c = GatewayClient::new(&server.uri());
        assert!(c.agent_steer("x", "hi").await.is_err());
    }

    #[tokio::test]
    async fn agent_kill_ok() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/agents/child-1/kill"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"killed":true,"detail":"done"})),
            )
            .mount(&server)
            .await;
        let c = GatewayClient::new(&server.uri());
        let r = c.agent_kill("child-1", Some("timeout")).await.unwrap();
        assert!(r.killed);
    }

    #[tokio::test]
    async fn agent_run_ok() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/agents/s1/run"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"turn_id":"t1","status":"completed","output":"ok"})),
            )
            .mount(&server)
            .await;
        let c = GatewayClient::new(&server.uri());
        let r = c.agent_run("s1", "go", 1, true).await.unwrap();
        assert_eq!(r.turn_id, "t1");
    }

    #[tokio::test]
    async fn agent_result_ok() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/agents/s1/turns/t1"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"status":"completed","output":"res"})),
            )
            .mount(&server)
            .await;
        let c = GatewayClient::new(&server.uri());
        let r = c.agent_result("s1", "t1").await.unwrap();
        assert_eq!(r.output.as_deref(), Some("res"));
    }

    #[tokio::test]
    async fn acp_send_ok() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/acp/send"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"message_id":"m1","delivered":true})),
            )
            .mount(&server)
            .await;
        let c = GatewayClient::new(&server.uri());
        let r = c.acp_send("a", "b", r#"{"x":1}"#).await.unwrap();
        assert!(r.delivered);
    }

    #[tokio::test]
    async fn acp_inbox_ok() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/acp/inbox"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([{"message_id":"m1","from":"b","received_at":"2026-03-20T10:00:00Z","payload":{}}])))
            .mount(&server).await;
        let c = GatewayClient::new(&server.uri());
        let r = c.acp_inbox("a").await.unwrap();
        assert_eq!(r.messages.len(), 1);
    }

    #[tokio::test]
    async fn acp_ack_ok() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/acp/ack"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"acknowledged":true})))
            .mount(&server)
            .await;
        let c = GatewayClient::new(&server.uri());
        let r = c.acp_ack("m1", "a").await.unwrap();
        assert!(r.acknowledged);
    }
}

// ── Plugins lifecycle ───────────────────────────────────────────

impl GatewayClient {
    /// `GET /plugins` — list installed plugins.
    pub async fn plugins_list(&self) -> Result<crate::output::PluginListResponse> {
        let resp = self
            .http
            .get(self.url("/plugins"))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let v: serde_json::Value = resp.json().await.context("invalid JSON from /plugins")?;
            let plugins = v["plugins"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .map(|p| crate::output::PluginSummary {
                    name: p["name"].as_str().unwrap_or("?").to_string(),
                    version: p["version"].as_str().unwrap_or("0.0.0").to_string(),
                    enabled: p["enabled"].as_bool().unwrap_or(false),
                    description: p["description"].as_str().unwrap_or("").to_string(),
                })
                .collect();
            Ok(crate::output::PluginListResponse { plugins })
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `GET /plugins/:name` — show plugin details.
    pub async fn plugins_info(&self, name: &str) -> Result<crate::output::PluginInfoResponse> {
        let resp = self
            .http
            .get(self.url(&format!("/plugins/{name}")))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let v: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from /plugins/:name")?;
            Ok(crate::output::PluginInfoResponse {
                name: v["name"].as_str().unwrap_or("?").to_string(),
                version: v["version"].as_str().unwrap_or("0.0.0").to_string(),
                enabled: v["enabled"].as_bool().unwrap_or(false),
                description: v["description"].as_str().unwrap_or("").to_string(),
                source: v["source"].as_str().unwrap_or("unknown").to_string(),
                manifest_valid: v["manifest_valid"].as_bool().unwrap_or(true),
            })
        } else if resp.status() == reqwest::StatusCode::NOT_FOUND {
            bail!("Plugin '{name}' not found.");
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `POST /plugins/:action` — plugin lifecycle mutation.
    pub async fn plugins_mutate(
        &self,
        action: &str,
        name_or_source: &str,
    ) -> Result<crate::output::PluginMutationResponse> {
        let resp = self
            .http
            .post(self.url(&format!("/plugins/{action}")))
            .json(&serde_json::json!({"name": name_or_source}))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let v: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from POST /plugins/:action")?;
            Ok(crate::output::PluginMutationResponse {
                success: v["success"].as_bool().unwrap_or(true),
                plugin: v["plugin"].as_str().unwrap_or(name_or_source).to_string(),
                action: action.to_string(),
                detail: v["detail"].as_str().unwrap_or("done").to_string(),
            })
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `GET /hooks` — list configured hooks.
    pub async fn hooks_list(&self) -> Result<crate::output::HookListResponse> {
        let resp = self
            .http
            .get(self.url("/hooks"))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let v: serde_json::Value = resp.json().await.context("invalid JSON from /hooks")?;
            let hooks = v["hooks"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .map(|h| crate::output::HookSummary {
                    name: h["name"].as_str().unwrap_or("?").to_string(),
                    event: h["event"].as_str().unwrap_or("?").to_string(),
                    enabled: h["enabled"].as_bool().unwrap_or(false),
                    description: h["description"].as_str().unwrap_or("").to_string(),
                })
                .collect();
            Ok(crate::output::HookListResponse { hooks })
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `GET /hooks/:name` — show hook details.
    pub async fn hooks_info(&self, name: &str) -> Result<crate::output::HookInfoResponse> {
        let resp = self
            .http
            .get(self.url(&format!("/hooks/{name}")))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let v: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from /hooks/:name")?;
            Ok(crate::output::HookInfoResponse {
                name: v["name"].as_str().unwrap_or("?").to_string(),
                event: v["event"].as_str().unwrap_or("?").to_string(),
                enabled: v["enabled"].as_bool().unwrap_or(false),
                description: v["description"].as_str().unwrap_or("").to_string(),
                source: v["source"].as_str().unwrap_or("unknown").to_string(),
            })
        } else if resp.status() == reqwest::StatusCode::NOT_FOUND {
            bail!("Hook '{name}' not found.");
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `GET /hooks/check` — validate hook configuration.
    pub async fn hooks_check(&self) -> Result<crate::output::HookCheckResponse> {
        let resp = self
            .http
            .get(self.url("/hooks/check"))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let v: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from /hooks/check")?;
            Ok(crate::output::HookCheckResponse {
                total: v["total"].as_u64().unwrap_or(0) as usize,
                valid: v["valid"].as_u64().unwrap_or(0) as usize,
                invalid: v["invalid"].as_u64().unwrap_or(0) as usize,
                detail: v["detail"].as_str().unwrap_or("check complete").to_string(),
            })
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `POST /hooks/:action` — hook lifecycle mutation.
    pub async fn hooks_mutate(
        &self,
        action: &str,
        name_or_source: &str,
    ) -> Result<crate::output::HookMutationResponse> {
        let resp = self
            .http
            .post(self.url(&format!("/hooks/{action}")))
            .json(&serde_json::json!({"name": name_or_source}))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let v: serde_json::Value = resp
                .json()
                .await
                .context("invalid JSON from POST /hooks/:action")?;
            Ok(crate::output::HookMutationResponse {
                success: v["success"].as_bool().unwrap_or(true),
                hook: v["hook"].as_str().unwrap_or(name_or_source).to_string(),
                action: action.to_string(),
                detail: v["detail"].as_str().unwrap_or("done").to_string(),
            })
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }
}
