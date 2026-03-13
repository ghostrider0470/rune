//! Output formatting: JSON and human-readable modes.

use serde::Serialize;
use std::fmt;

/// Output format selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Human,
    Json,
}

impl OutputFormat {
    /// Create from the `--json` flag value.
    #[must_use]
    pub fn from_json_flag(json: bool) -> Self {
        if json { Self::Json } else { Self::Human }
    }
}

/// Render a serializable value according to the chosen format.
///
/// For JSON mode, outputs compact JSON to stdout.
/// For human mode, uses the `Display` implementation.
pub fn render<T: Serialize + fmt::Display>(value: &T, format: OutputFormat) -> String {
    match format {
        OutputFormat::Json => serde_json::to_string_pretty(value).unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}")),
        OutputFormat::Human => value.to_string(),
    }
}

/// A simple status response from the gateway.
#[derive(Debug, Clone, Serialize)]
pub struct StatusResponse {
    pub status: String,
    pub version: Option<String>,
    pub uptime_seconds: Option<u64>,
}

impl fmt::Display for StatusResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Status: {}", self.status)?;
        if let Some(ref v) = self.version {
            write!(f, "\nVersion: {v}")?;
        }
        if let Some(u) = self.uptime_seconds {
            write!(f, "\nUptime: {u}s")?;
        }
        Ok(())
    }
}

/// Health check response.
#[derive(Debug, Clone, Serialize)]
pub struct HealthResponse {
    pub healthy: bool,
    pub message: String,
}

impl fmt::Display for HealthResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let icon = if self.healthy { "✓" } else { "✗" };
        write!(f, "{icon} {}", self.message)
    }
}

/// A single diagnostic check result used by `rune doctor`.
#[derive(Debug, Clone, Serialize)]
pub struct DoctorCheck {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

impl fmt::Display for DoctorCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let icon = if self.passed { "✓" } else { "✗" };
        write!(f, "  {icon} {}: {}", self.name, self.detail)
    }
}

/// Full doctor report.
#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    pub checks: Vec<DoctorCheck>,
}

impl fmt::Display for DoctorReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Doctor Report")?;
        writeln!(f, "─────────────")?;
        for check in &self.checks {
            writeln!(f, "{check}")?;
        }
        let passed = self.checks.iter().filter(|c| c.passed).count();
        let total = self.checks.len();
        write!(f, "\n{passed}/{total} checks passed")
    }
}

/// Session summary for list output.
#[derive(Debug, Clone, Serialize)]
pub struct SessionSummary {
    pub id: String,
    pub status: String,
    pub channel: Option<String>,
    pub created_at: Option<String>,
}

impl fmt::Display for SessionSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} [{}]", self.id, self.status)?;
        if let Some(ref ch) = self.channel {
            write!(f, " ({ch})")?;
        }
        Ok(())
    }
}

/// Session list response.
#[derive(Debug, Clone, Serialize)]
pub struct SessionListResponse {
    pub sessions: Vec<SessionSummary>,
}

impl fmt::Display for SessionListResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.sessions.is_empty() {
            return write!(f, "No active sessions.");
        }
        for s in &self.sessions {
            writeln!(f, "  {s}")?;
        }
        Ok(())
    }
}

/// Detailed session view.
#[derive(Debug, Clone, Serialize)]
pub struct SessionDetailResponse {
    pub id: String,
    pub status: String,
    pub channel: Option<String>,
    pub created_at: Option<String>,
    pub turn_count: Option<u32>,
}

impl fmt::Display for SessionDetailResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Session: {}", self.id)?;
        writeln!(f, "  Status:  {}", self.status)?;
        if let Some(ref ch) = self.channel {
            writeln!(f, "  Channel: {ch}")?;
        }
        if let Some(ref t) = self.created_at {
            writeln!(f, "  Created: {t}")?;
        }
        if let Some(n) = self.turn_count {
            write!(f, "  Turns:   {n}")?;
        }
        Ok(())
    }
}

/// Config validation result.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
}

impl fmt::Display for ConfigValidationResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.valid {
            write!(f, "✓ Configuration is valid.")
        } else {
            writeln!(f, "✗ Configuration errors:")?;
            for e in &self.errors {
                writeln!(f, "  - {e}")?;
            }
            Ok(())
        }
    }
}

/// Simple action acknowledgment (gateway start/stop).
#[derive(Debug, Clone, Serialize)]
pub struct ActionResult {
    pub success: bool,
    pub message: String,
}

impl fmt::Display for ActionResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let icon = if self.success { "✓" } else { "✗" };
        write!(f, "{icon} {}", self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_format_from_flag() {
        assert_eq!(OutputFormat::from_json_flag(true), OutputFormat::Json);
        assert_eq!(OutputFormat::from_json_flag(false), OutputFormat::Human);
    }

    #[test]
    fn render_status_human() {
        let s = StatusResponse {
            status: "running".into(),
            version: Some("0.1.0".into()),
            uptime_seconds: Some(120),
        };
        let out = render(&s, OutputFormat::Human);
        assert!(out.contains("Status: running"));
        assert!(out.contains("Version: 0.1.0"));
        assert!(out.contains("Uptime: 120s"));
    }

    #[test]
    fn render_status_json() {
        let s = StatusResponse {
            status: "running".into(),
            version: None,
            uptime_seconds: None,
        };
        let out = render(&s, OutputFormat::Json);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["status"], "running");
    }

    #[test]
    fn render_health_human() {
        let h = HealthResponse {
            healthy: true,
            message: "All systems go".into(),
        };
        assert_eq!(render(&h, OutputFormat::Human), "✓ All systems go");
    }

    #[test]
    fn render_health_unhealthy() {
        let h = HealthResponse {
            healthy: false,
            message: "DB unreachable".into(),
        };
        let out = render(&h, OutputFormat::Human);
        assert!(out.starts_with('✗'));
    }

    #[test]
    fn render_doctor_report() {
        let r = DoctorReport {
            checks: vec![
                DoctorCheck {
                    name: "config".into(),
                    passed: true,
                    detail: "valid".into(),
                },
                DoctorCheck {
                    name: "db".into(),
                    passed: false,
                    detail: "unreachable".into(),
                },
            ],
        };
        let out = render(&r, OutputFormat::Human);
        assert!(out.contains("1/2 checks passed"));
    }

    #[test]
    fn render_config_validation_valid() {
        let v = ConfigValidationResult {
            valid: true,
            errors: vec![],
        };
        let out = render(&v, OutputFormat::Human);
        assert!(out.contains("✓"));
    }

    #[test]
    fn render_config_validation_invalid() {
        let v = ConfigValidationResult {
            valid: false,
            errors: vec!["bad port".into()],
        };
        let out = render(&v, OutputFormat::Human);
        assert!(out.contains("bad port"));
    }

    #[test]
    fn render_session_list_empty() {
        let l = SessionListResponse { sessions: vec![] };
        assert_eq!(render(&l, OutputFormat::Human), "No active sessions.");
    }

    #[test]
    fn render_action_result_json() {
        let a = ActionResult {
            success: true,
            message: "started".into(),
        };
        let out = render(&a, OutputFormat::Json);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["success"], true);
    }
}
