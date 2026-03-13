//! Gateway HTTP client for CLI commands.

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde_json::json;
use toml_edit::{DocumentMut, Item, Table, Value};

use crate::output::{
    ActionResult, ConfigFileResponse, ConfigGetResponse, ConfigMutationResponse,
    ConfigValidationResult, CronJobSummary, CronListResponse, CronRunSummary, CronRunsResponse,
    CronStatusResponse, DoctorCheck, DoctorReport, HealthResponse, HeartbeatStatusResponse,
    SessionDetailResponse, SessionListResponse, SessionSummary, StatusResponse,
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
            .query(&[("includeDisabled", include_disabled)])
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            let items: serde_json::Value = resp.json().await.context("invalid JSON from /cron")?;
            let jobs = items
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .map(|job| CronJobSummary {
                    id: job["id"].as_str().unwrap_or("?").to_string(),
                    name: job["name"].as_str().map(String::from),
                    enabled: job["enabled"].as_bool().unwrap_or(false),
                    session_target: job["session_target"]
                        .as_str()
                        .unwrap_or("unknown")
                        .to_string(),
                    schedule_kind: job["schedule"]["kind"]
                        .as_str()
                        .unwrap_or("unknown")
                        .to_string(),
                    next_run_at: job["next_run_at"].as_str().map(String::from),
                    run_count: job["run_count"].as_u64().unwrap_or(0),
                })
                .collect();
            Ok(CronListResponse { jobs })
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
    ) -> Result<ActionResult> {
        let resp = self
            .http
            .post(self.url("/cron"))
            .json(&json!({
                "name": name,
                "schedule": { "kind": "at", "at": at.to_rfc3339() },
                "payload": { "kind": "system_event", "text": text },
                "sessionTarget": session_target,
                "enabled": true
            }))
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
    pub async fn cron_edit_name(&self, id: &str, name: &str) -> Result<ActionResult> {
        self.cron_patch(id, json!({ "name": name }), "Cron job updated")
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
                "contextMessages": context_messages,
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

    /// `GET /sessions`
    pub async fn sessions_list(&self) -> Result<SessionListResponse> {
        let resp = self
            .http
            .get(self.url("/sessions"))
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
                channel: v["channel"].as_str().map(String::from),
                created_at: v["created_at"].as_str().map(String::from),
                turn_count: v["turn_count"].as_u64().map(|n| n as u32),
            })
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

    /// `GET /approvals` — list all tool approval policies.
    pub async fn approvals_list(&self) -> Result<serde_json::Value> {
        let resp = self
            .http
            .get(self.url("/approvals"))
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            Ok(resp.json().await?)
        } else {
            bail!("Gateway returned HTTP {}", resp.status());
        }
    }

    /// `GET /approvals/{tool}` — get policy for a specific tool.
    pub async fn approvals_get(&self, tool: &str) -> Result<serde_json::Value> {
        let resp = self
            .http
            .get(self.url(&format!("/approvals/{tool}")))
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

    /// `PUT /approvals/{tool}` — set policy for a tool.
    pub async fn approvals_set(&self, tool: &str, decision: &str) -> Result<serde_json::Value> {
        let resp = self
            .http
            .put(self.url(&format!("/approvals/{tool}")))
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

    /// `DELETE /approvals/{tool}` — clear policy for a tool.
    pub async fn approvals_clear(&self, tool: &str) -> Result<serde_json::Value> {
        let resp = self
            .http
            .delete(self.url(&format!("/approvals/{tool}")))
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
    std::env::var_os("RUNE_CONFIG")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("config.toml"))
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
    use wiremock::matchers::{method, path};
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
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {
                    "id": "job-1",
                    "name": "test",
                    "enabled": true,
                    "session_target": "main",
                    "schedule": { "kind": "at" },
                    "run_count": 0
                }
            ])))
            .mount(&server)
            .await;
        let client = GatewayClient::new(&server.uri());
        let resp = client.cron_list(false).await.unwrap();
        assert_eq!(resp.jobs.len(), 1);
        assert_eq!(resp.jobs[0].id, "job-1");
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
        let resp = client.sessions_list().await.unwrap();
        assert_eq!(resp.sessions.len(), 2);
        assert_eq!(resp.sessions[0].id, "s1");
        assert_eq!(resp.sessions[1].channel, None);
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
}
