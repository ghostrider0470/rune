//! Logs breadth extensions — tail, search, export.
//!
//! Extends `GatewayClient` with additional log query surfaces
//! beyond the base `logs_query` method.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::client::GatewayClient;
use crate::output::format_log_entry;

// ── Response types ──────────────────────────────────────────────

/// Response for `logs tail`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogsTailResponse {
    pub entries: Vec<serde_json::Value>,
    pub source: String,
}

impl fmt::Display for LogsTailResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Log tail ({}):", self.source)?;
        for e in &self.entries {
            writeln!(f, "  {}", format_log_entry(e))?;
        }
        Ok(())
    }
}

/// Response for `logs search`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogsSearchResponse {
    pub query: String,
    pub entries: Vec<serde_json::Value>,
    pub total: usize,
}

impl fmt::Display for LogsSearchResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Search \"{}\": {} result(s)", self.query, self.total)?;
        for e in &self.entries {
            writeln!(f, "  {}", format_log_entry(e))?;
        }
        Ok(())
    }
}

/// Response for `logs export`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogsExportResponse {
    pub success: bool,
    pub path: String,
    pub message: String,
}

impl fmt::Display for LogsExportResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let icon = if self.success { "\u{2713}" } else { "\u{2717}" };
        writeln!(f, "{icon} Export: {}", self.message)?;
        write!(f, "  Output: {}", self.path)
    }
}

// ── Client extension methods ────────────────────────────────────

impl GatewayClient {
    /// `GET /api/logs/tail`
    pub async fn logs_tail(
        &self,
        level: Option<&str>,
        source: Option<&str>,
        follow: bool,
        lines: usize,
    ) -> Result<LogsTailResponse> {
        let mut query: Vec<(&str, String)> = vec![("lines", lines.to_string())];
        if follow {
            query.push(("follow", "true".to_string()));
        }
        if let Some(l) = level {
            query.push(("level", l.to_string()));
        }
        if let Some(s) = source {
            query.push(("source", s.to_string()));
        }
        let resp = self
            .http()
            .get(self.url("/api/logs/tail"))
            .query(&query)
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            resp.json::<LogsTailResponse>()
                .await
                .context("invalid JSON from /api/logs/tail")
        } else {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("Gateway returned HTTP {status}: {body}")
        }
    }

    /// `GET /api/logs/search`
    pub async fn logs_search(
        &self,
        query_text: &str,
        level: Option<&str>,
        source: Option<&str>,
        limit: usize,
    ) -> Result<LogsSearchResponse> {
        let mut query: Vec<(&str, String)> =
            vec![("q", query_text.to_string()), ("limit", limit.to_string())];
        if let Some(l) = level {
            query.push(("level", l.to_string()));
        }
        if let Some(s) = source {
            query.push(("source", s.to_string()));
        }
        let resp = self
            .http()
            .get(self.url("/api/logs/search"))
            .query(&query)
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            resp.json::<LogsSearchResponse>()
                .await
                .context("invalid JSON from /api/logs/search")
        } else {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("Gateway returned HTTP {status}: {body}")
        }
    }

    /// `POST /api/logs/export`
    pub async fn logs_export(
        &self,
        format: &str,
        level: Option<&str>,
        source: Option<&str>,
        since: Option<&str>,
        until: Option<&str>,
        limit: Option<usize>,
        output: Option<&str>,
    ) -> Result<LogsExportResponse> {
        let mut body = serde_json::json!({ "format": format });
        if let Some(l) = level {
            body["level"] = serde_json::json!(l);
        }
        if let Some(s) = source {
            body["source"] = serde_json::json!(s);
        }
        if let Some(s) = since {
            body["since"] = serde_json::json!(s);
        }
        if let Some(u) = until {
            body["until"] = serde_json::json!(u);
        }
        if let Some(n) = limit {
            body["limit"] = serde_json::json!(n);
        }
        if let Some(o) = output {
            body["output"] = serde_json::json!(o);
        }
        let resp = self
            .http()
            .post(self.url("/api/logs/export"))
            .json(&body)
            .send()
            .await
            .context("failed to reach gateway")?;
        if resp.status().is_success() {
            resp.json::<LogsExportResponse>()
                .await
                .context("invalid JSON from /api/logs/export")
        } else {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            bail!("Gateway returned HTTP {status}: {text}")
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::{render, OutputFormat};
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn render_logs_tail() {
        let r = LogsTailResponse {
            entries: vec![json!({"timestamp":"T1","level":"INFO","message":"hello"})],
            source: "gateway".into(),
        };
        let h = render(&r, OutputFormat::Human);
        assert!(h.contains("hello"));
        assert!(h.contains("gateway"));
    }

    #[test]
    fn render_logs_search() {
        let r = LogsSearchResponse {
            query: "err".into(),
            entries: vec![json!({"timestamp":"T1","level":"ERROR","message":"err found"})],
            total: 1,
        };
        let h = render(&r, OutputFormat::Human);
        assert!(h.contains("err"));
    }

    #[test]
    fn render_logs_export() {
        let r = LogsExportResponse {
            success: true,
            path: "/tmp/out.json".into(),
            message: "Exported 10 entries".into(),
        };
        let h = render(&r, OutputFormat::Human);
        assert!(h.contains("Exported"));
    }

    #[tokio::test]
    async fn logs_tail_parses_response() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/logs/tail"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "entries": [{"timestamp":"T1","level":"INFO","message":"hello"}],
                "source": "gateway"
            })))
            .mount(&s)
            .await;
        let r = GatewayClient::new(&s.uri())
            .logs_tail(None, None, false, 50)
            .await
            .unwrap();
        assert_eq!(r.source, "gateway");
        assert_eq!(r.entries.len(), 1);
    }

    #[tokio::test]
    async fn logs_search_parses_response() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/logs/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "query": "err",
                "entries": [{"timestamp":"T1","level":"ERROR","message":"err found"}],
                "total": 1
            })))
            .mount(&s)
            .await;
        let r = GatewayClient::new(&s.uri())
            .logs_search("err", None, None, 50)
            .await
            .unwrap();
        assert_eq!(r.total, 1);
    }

    #[tokio::test]
    async fn logs_export_parses_response() {
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/logs/export"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "success": true,
                "path": "/tmp/out.json",
                "message": "Exported 10 entries"
            })))
            .mount(&s)
            .await;
        let r = GatewayClient::new(&s.uri())
            .logs_export("json", None, None, None, None, None, None)
            .await
            .unwrap();
        assert!(r.success);
    }
}
