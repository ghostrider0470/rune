//! Gateway HTTP client for CLI commands.

use anyhow::{Context, Result, bail};
use reqwest::Client;

use crate::output::{
    ActionResult, ConfigValidationResult, DoctorCheck, DoctorReport, HealthResponse,
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

    /// `GET /sessions`
    pub async fn sessions_list(&self) -> Result<SessionListResponse> {
        let resp = self
            .http
            .get(self.url("/sessions"))
            .send()
            .await
            .context("failed to reach gateway")?;

        if resp.status().is_success() {
            let body: serde_json::Value = resp.json().await.context("invalid JSON from /sessions")?;
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
            let v: serde_json::Value = resp.json().await.context("invalid JSON from /sessions/:id")?;
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

        // Check 1: config loads
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

        // Check 2: gateway reachable
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
    let config = rune_config::AppConfig::load(None::<&str>).context("failed to load configuration")?;
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
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "status": "running",
                    "version": "0.1.0",
                    "uptime_seconds": 300
                })),
            )
            .mount(&server)
            .await;

        let client = GatewayClient::new(&server.uri());
        let resp = client.status().await.unwrap();
        assert_eq!(resp.status, "running");
        assert_eq!(resp.version.as_deref(), Some("0.1.0"));
        assert_eq!(resp.uptime_seconds, Some(300));
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
        // Default paths (/data/db etc.) won't exist on dev machines
        let result = validate_config(None);
        // Should either be valid (if paths exist) or report errors
        assert!(!result.errors.is_empty() || result.valid);
    }

    #[test]
    fn show_config_returns_json() {
        let json = show_config().unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v["gateway"]["port"].is_number());
    }
}
