use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use regex::Regex;
use rune_tools::{ToolCall, ToolError, ToolExecutor, ToolResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditFinding {
    pub check: String,
    pub severity: Severity,
    pub status: String,
    pub summary: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditReport {
    pub timestamp: DateTime<Utc>,
    pub target: String,
    pub findings: Vec<AuditFinding>,
    pub passed: bool,
    pub summary: AuditSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuditSummary {
    pub info: usize,
    pub warning: usize,
    pub critical: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum CheckType {
    OpenPorts,
    Permissions,
    Ssh,
    Firewall,
    Secrets,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditConfig {
    #[serde(default)]
    pub exclude_patterns: Vec<String>,
    #[serde(default = "default_severity_threshold")]
    pub severity_threshold: Severity,
    #[serde(default)]
    pub checks: Vec<CheckType>,
}

fn default_severity_threshold() -> Severity {
    Severity::Info
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            exclude_patterns: Vec::new(),
            severity_threshold: Severity::Info,
            checks: vec![CheckType::All],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretMatch {
    pub pattern_name: String,
    pub severity: Severity,
    pub line: usize,
    pub column: usize,
    pub snippet: String,
}

pub fn security_audit_tool_definition() -> rune_tools::ToolDefinition {
    rune_tools::ToolDefinition {
        name: "security_audit".into(),
        description: "Run a native security audit over a workspace or host path, including permissions, SSH/firewall checks, and secret scanning.".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "target": {
                    "type": "string",
                    "description": "Optional workspace-relative directory path to inspect"
                },
                "checks": {
                    "type": "array",
                    "items": {
                        "type": "string",
                        "enum": ["open_ports", "permissions", "ssh", "firewall", "secrets", "all"]
                    },
                    "description": "Subset of checks to run; defaults to all"
                },
                "exclude_patterns": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Path substrings to exclude from recursive scanning"
                },
                "severity_threshold": {
                    "type": "string",
                    "enum": ["info", "warning", "critical"],
                    "description": "Only include findings at or above this severity"
                }
            }
        }),
        category: rune_core::ToolCategory::ProcessExec,
        requires_approval: false,
    }
}

pub struct SecurityAuditToolExecutor {
    workspace_root: PathBuf,
}

impl SecurityAuditToolExecutor {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
        }
    }

    fn resolve_target(&self, raw: Option<&str>) -> Result<PathBuf, ToolError> {
        let root = self
            .workspace_root
            .canonicalize()
            .map_err(|e| ToolError::ExecutionFailed(format!("workspace root invalid: {e}")))?;
        let Some(raw) = raw else {
            return Ok(root);
        };
        let candidate = Path::new(raw);
        if candidate.is_absolute() {
            return Err(ToolError::InvalidArguments {
                tool: "security_audit".into(),
                reason: "absolute paths are not allowed".into(),
            });
        }
        let joined = root.join(candidate);
        let canonical = joined
            .canonicalize()
            .map_err(|e| ToolError::ExecutionFailed(format!("path resolution failed: {e}")))?;
        if !canonical.starts_with(&root) {
            return Err(ToolError::InvalidArguments {
                tool: "security_audit".into(),
                reason: "path escapes workspace boundary".into(),
            });
        }
        Ok(canonical)
    }
}

#[async_trait]
impl ToolExecutor for SecurityAuditToolExecutor {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        #[derive(Deserialize)]
        struct RawArgs {
            target: Option<String>,
            checks: Option<Vec<CheckType>>,
            exclude_patterns: Option<Vec<String>>,
            severity_threshold: Option<String>,
        }

        let args: RawArgs = serde_json::from_value(call.arguments).map_err(|e| {
            ToolError::InvalidArguments {
                tool: call.tool_name.clone(),
                reason: format!("invalid security_audit arguments: {e}"),
            }
        })?;

        let target = self.resolve_target(args.target.as_deref())?;
        let threshold = match args.severity_threshold.as_deref() {
            Some("critical") => Severity::Critical,
            Some("warning") => Severity::Warning,
            Some("info") | None => Severity::Info,
            Some(other) => {
                return Err(ToolError::InvalidArguments {
                    tool: call.tool_name.clone(),
                    reason: format!("unsupported severity_threshold: {other}"),
                })
            }
        };

        let config = AuditConfig {
            exclude_patterns: args.exclude_patterns.unwrap_or_default(),
            severity_threshold: threshold,
            checks: args.checks.unwrap_or_else(|| vec![CheckType::All]),
        };

        let report = run_security_audit_with_config(&target, &config);
        let output = serde_json::to_string_pretty(&report).map_err(|e| {
            ToolError::ExecutionFailed(format!("failed to serialize security audit report: {e}"))
        })?;

        Ok(ToolResult {
            tool_call_id: call.tool_call_id,
            output,
            is_error: false,
            tool_execution_id: None,
        })
    }
}

pub fn run_security_audit(target: Option<&Path>) -> AuditReport {
    run_security_audit_with_config(
        target.unwrap_or_else(|| Path::new(".")),
        &AuditConfig::default(),
    )
}

pub fn run_security_audit_with_config(target: &Path, config: &AuditConfig) -> AuditReport {
    let mut findings = Vec::new();
    let checks = normalize_checks(&config.checks);

    if checks.contains(&CheckType::OpenPorts) {
        findings.extend(scan_open_ports());
    }
    if checks.contains(&CheckType::Permissions) {
        findings.extend(scan_file_permissions(target, &config.exclude_patterns));
    }
    if checks.contains(&CheckType::Ssh) {
        findings.extend(scan_ssh_config());
    }
    if checks.contains(&CheckType::Firewall) {
        findings.extend(scan_firewall_status());
    }
    if checks.contains(&CheckType::Secrets) {
        findings.extend(scan_secrets(target, &config.exclude_patterns));
    }

    findings.retain(|finding| finding.severity >= config.severity_threshold);

    let mut summary = AuditSummary::default();
    for finding in &findings {
        match finding.severity {
            Severity::Info => summary.info += 1,
            Severity::Warning => summary.warning += 1,
            Severity::Critical => summary.critical += 1,
        }
    }

    AuditReport {
        timestamp: Utc::now(),
        target: target.display().to_string(),
        passed: summary.critical == 0,
        findings,
        summary,
    }
}

fn normalize_checks(checks: &[CheckType]) -> Vec<CheckType> {
    if checks.is_empty() || checks.contains(&CheckType::All) {
        return vec![
            CheckType::OpenPorts,
            CheckType::Permissions,
            CheckType::Ssh,
            CheckType::Firewall,
            CheckType::Secrets,
        ];
    }

    let mut out = checks.to_vec();
    out.sort();
    out.dedup();
    out
}

fn scan_open_ports() -> Vec<AuditFinding> {
    let output = run_first_available(&[&["ss", "-tuln"], &["netstat", "-tuln"]]);

    match output {
        Some((cmd, stdout)) => {
            let ports: Vec<String> = stdout
                .lines()
                .filter(|line| {
                    line.contains("LISTEN") || line.starts_with("tcp") || line.starts_with("udp")
                })
                .take(20)
                .map(|s| s.trim().to_string())
                .collect();

            let severity = if ports
                .iter()
                .any(|line| line.contains(":22 ") || line.ends_with(":22"))
            {
                Severity::Warning
            } else {
                Severity::Info
            };

            vec![AuditFinding {
                check: "open_ports".into(),
                severity,
                status: "observed".into(),
                summary: format!("Detected {} listening socket entries via {cmd}", ports.len()),
                detail: if ports.is_empty() {
                    format!("{cmd} returned no listening sockets")
                } else {
                    ports.join("\n")
                },
            }]
        }
        None => vec![AuditFinding {
            check: "open_ports".into(),
            severity: Severity::Warning,
            status: "unknown".into(),
            summary: "Could not inspect listening ports".into(),
            detail: "Neither `ss` nor `netstat` was available in PATH".into(),
        }],
    }
}

fn scan_file_permissions(target: &Path, exclude_patterns: &[String]) -> Vec<AuditFinding> {
    let candidates = collect_sensitive_files(target, exclude_patterns);
    if candidates.is_empty() {
        return vec![AuditFinding {
            check: "file_permissions".into(),
            severity: Severity::Info,
            status: "pass".into(),
            summary: "No sensitive files found in target".into(),
            detail: format!(
                "Scanned {} for .env, key, cert, and SSH material",
                target.display()
            ),
        }];
    }

    let checked_count = candidates.len();
    let mut risky = Vec::new();
    for path in candidates {
        if let Ok(meta) = fs::metadata(&path) {
            let mode = meta.permissions().mode() & 0o777;
            let sensitive = is_private_material(&path);
            if sensitive && mode & 0o077 != 0 {
                risky.push(format!(
                    "{} mode {:o} should not be group/world accessible",
                    path.display(),
                    mode
                ));
            } else if mode & 0o002 != 0 {
                risky.push(format!(
                    "{} mode {:o} is world-writable",
                    path.display(),
                    mode
                ));
            }
        }
    }

    let (severity, status, summary, detail) = if risky.is_empty() {
        (
            Severity::Info,
            "pass",
            "Sensitive file permissions look reasonable".to_string(),
            format!(
                "Checked {} sensitive files under {}",
                checked_count,
                target.display()
            ),
        )
    } else {
        (
            Severity::Critical,
            "fail",
            format!("Found {} risky file permission entries", risky.len()),
            risky.join("\n"),
        )
    };

    vec![AuditFinding {
        check: "file_permissions".into(),
        severity,
        status: status.into(),
        summary,
        detail,
    }]
}

fn scan_ssh_config() -> Vec<AuditFinding> {
    let home = std::env::var("HOME").ok().map(PathBuf::from);
    let Some(home) = home else {
        return vec![AuditFinding {
            check: "ssh_config".into(),
            severity: Severity::Warning,
            status: "unknown".into(),
            summary: "HOME not set; skipping SSH config audit".into(),
            detail: "Could not resolve ~/.ssh/config".into(),
        }];
    };

    let ssh_dir = home.join(".ssh");
    let config = ssh_dir.join("config");
    if !config.exists() {
        return vec![AuditFinding {
            check: "ssh_config".into(),
            severity: Severity::Info,
            status: "pass".into(),
            summary: "No ~/.ssh/config present".into(),
            detail: format!(
                "Skipped SSH config checks because {} does not exist",
                config.display()
            ),
        }];
    }

    let mut issues = Vec::new();
    if let Ok(meta) = fs::metadata(&ssh_dir) {
        let mode = meta.permissions().mode() & 0o777;
        if mode != 0o700 {
            issues.push(format!("{} mode {:o}; expected 700", ssh_dir.display(), mode));
        }
    }
    if let Ok(meta) = fs::metadata(&config) {
        let mode = meta.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            issues.push(format!("{} mode {:o}; expected 600", config.display(), mode));
        }
    }
    if let Ok(contents) = fs::read_to_string(&config) {
        let lower = contents.to_lowercase();
        if lower.contains("stricthostkeychecking no") {
            issues.push("StrictHostKeyChecking disabled".into());
        }
        if lower.contains("passwordauthentication yes") {
            issues.push("PasswordAuthentication enabled".into());
        }
        if lower.contains("permitrootlogin yes") {
            issues.push("PermitRootLogin enabled".into());
        }
    }

    let severity = if issues.is_empty() {
        Severity::Info
    } else {
        Severity::Warning
    };
    let status = if issues.is_empty() { "pass" } else { "warn" };
    let summary = if issues.is_empty() {
        "SSH config check passed".to_string()
    } else {
        format!("SSH config has {} hardening issue(s)", issues.len())
    };

    vec![AuditFinding {
        check: "ssh_config".into(),
        severity,
        status: status.into(),
        summary,
        detail: if issues.is_empty() {
            format!("Checked {}", config.display())
        } else {
            issues.join("\n")
        },
    }]
}

fn scan_firewall_status() -> Vec<AuditFinding> {
    let checks = [
        ("ufw", vec!["ufw", "status"]),
        ("firewalld", vec!["firewall-cmd", "--state"]),
        ("iptables", vec!["iptables", "-L"]),
    ];

    for (name, cmd) in checks {
        if let Ok(output) = Command::new(cmd[0]).args(&cmd[1..]).output() {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let text = if stdout.is_empty() { stderr } else { stdout };
            let lower = text.to_lowercase();
            let (severity, status, summary) =
                if lower.contains("inactive") || lower.contains("not running") {
                    (
                        Severity::Critical,
                        "fail",
                        format!("{name} reports firewall inactive"),
                    )
                } else {
                    (Severity::Info, "pass", format!("{name} responded"))
                };
            return vec![AuditFinding {
                check: "firewall_status".into(),
                severity,
                status: status.into(),
                summary,
                detail: text,
            }];
        }
    }

    vec![AuditFinding {
        check: "firewall_status".into(),
        severity: Severity::Warning,
        status: "unknown".into(),
        summary: "Could not determine firewall status".into(),
        detail: "No supported firewall CLI (ufw, firewall-cmd, iptables) was available".into(),
    }]
}

fn scan_secrets(target: &Path, exclude_patterns: &[String]) -> Vec<AuditFinding> {
    let mut findings = Vec::new();
    let files = collect_scannable_files(target, exclude_patterns);
    let patterns = secret_patterns();

    for path in files {
        let Ok(contents) = fs::read_to_string(&path) else {
            continue;
        };
        let mut matches = Vec::new();
        for (idx, line) in contents.lines().enumerate() {
            for pattern in &patterns {
                for mat in pattern.regex.find_iter(line) {
                    matches.push(SecretMatch {
                        pattern_name: pattern.name.to_string(),
                        severity: pattern.severity.clone(),
                        line: idx + 1,
                        column: mat.start() + 1,
                        snippet: line.trim().to_string(),
                    });
                }
            }
        }

        for matched in matches {
            findings.push(AuditFinding {
                check: "secret_scan".into(),
                severity: matched.severity,
                status: "fail".into(),
                summary: format!(
                    "{}:{}:{} matched {}",
                    path.display(),
                    matched.line,
                    matched.column,
                    matched.pattern_name
                ),
                detail: matched.snippet,
            });
        }
    }

    if findings.is_empty() {
        findings.push(AuditFinding {
            check: "secret_scan".into(),
            severity: Severity::Info,
            status: "pass".into(),
            summary: "No secret-like patterns detected".into(),
            detail: format!("Scanned {} recursively", target.display()),
        });
    }

    findings
}

struct SecretPattern {
    name: &'static str,
    severity: Severity,
    regex: Regex,
}

fn secret_patterns() -> Vec<SecretPattern> {
    vec![
        SecretPattern {
            name: "aws_access_key",
            severity: Severity::Critical,
            regex: Regex::new(r"AKIA[0-9A-Z]{16}").expect("valid aws regex"),
        },
        SecretPattern {
            name: "github_pat",
            severity: Severity::Critical,
            regex: Regex::new(r"ghp_[A-Za-z0-9]{36}").expect("valid github regex"),
        },
        SecretPattern {
            name: "slack_token",
            severity: Severity::Critical,
            regex: Regex::new(r"xox[bpoas]-[A-Za-z0-9-]+")
                .expect("valid slack regex"),
        },
        SecretPattern {
            name: "openai_key",
            severity: Severity::Critical,
            regex: Regex::new(r"sk-[A-Za-z0-9]{20,}").expect("valid openai regex"),
        },
        SecretPattern {
            name: "jwt_token",
            severity: Severity::Warning,
            regex: Regex::new(r"eyJ[A-Za-z0-9_-]*\.eyJ[A-Za-z0-9_-]*\.")
                .expect("valid jwt regex"),
        },
        SecretPattern {
            name: "private_key",
            severity: Severity::Critical,
            regex: Regex::new(r"BEGIN[ A-Z]*PRIVATE KEY").expect("valid key regex"),
        },
        SecretPattern {
            name: "password_assignment",
            severity: Severity::Warning,
            regex: Regex::new(r"(?i)password\s*[:=]").expect("valid password regex"),
        },
        SecretPattern {
            name: "connection_string",
            severity: Severity::Warning,
            regex: Regex::new(r"(?i)(mongodb|mysql|postgres|redis)://[^\s]*@")
                .expect("valid connection string regex"),
        },
        SecretPattern {
            name: "generic_token",
            severity: Severity::Warning,
            regex: Regex::new(
                r"(?i)(api[_-]?key|api[_-]?secret|access[_-]?token|auth[_-]?token|bearer)",
            )
            .expect("valid generic token regex"),
        },
    ]
}

fn collect_sensitive_files(target: &Path, exclude_patterns: &[String]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(target, exclude_patterns, &mut out, true);
    out
}

fn collect_scannable_files(target: &Path, exclude_patterns: &[String]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(target, exclude_patterns, &mut out, false);
    out
}

fn walk(path: &Path, exclude_patterns: &[String], out: &mut Vec<PathBuf>, sensitive_only: bool) {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return;
    };
    if should_skip(path, exclude_patterns) {
        return;
    }
    if meta.is_file() {
        if !sensitive_only || is_sensitive_path(path) {
            out.push(path.to_path_buf());
        }
        return;
    }
    if !meta.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if [".git", "target", "node_modules"].contains(&name) {
            continue;
        }
        walk(&p, exclude_patterns, out, sensitive_only);
    }
}

fn should_skip(path: &Path, exclude_patterns: &[String]) -> bool {
    let rendered = path.to_string_lossy();
    exclude_patterns.iter().any(|pattern| rendered.contains(pattern))
}

fn is_sensitive_path(path: &Path) -> bool {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    name.starts_with(".env")
        || name.ends_with(".pem")
        || name.ends_with(".key")
        || name.ends_with(".p12")
        || name == "id_rsa"
        || name == "id_ed25519"
        || name == "config"
}

fn is_private_material(path: &Path) -> bool {
    is_sensitive_path(path)
}

fn run_first_available(commands: &[&[&str]]) -> Option<(String, String)> {
    for cmd in commands {
        if let Ok(output) = Command::new(cmd[0]).args(&cmd[1..]).output() {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            return Some((cmd.join(" "), stdout));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune_core::ToolCallId;
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    fn tool_call(arguments: serde_json::Value) -> ToolCall {
        ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: "security_audit".to_string(),
            arguments,
        }
    }

    #[test]
    fn tool_definition_is_registered_as_security_audit() {
        let def = security_audit_tool_definition();
        assert_eq!(def.name, "security_audit");
    }

    #[test]
    fn audit_report_contains_expected_checks() {
        let dir = tempfile::tempdir().unwrap();
        let env_file = dir.path().join(".env");
        fs::write(&env_file, "SECRET=test").unwrap();
        fs::set_permissions(&env_file, fs::Permissions::from_mode(0o644)).unwrap();

        let report = run_security_audit(Some(dir.path()));
        let checks: Vec<_> = report.findings.iter().map(|f| f.check.as_str()).collect();
        assert!(checks.contains(&"open_ports"));
        assert!(checks.contains(&"file_permissions"));
        assert!(checks.contains(&"ssh_config"));
        assert!(checks.contains(&"firewall_status"));
        assert!(checks.contains(&"secret_scan"));
    }

    #[test]
    fn sensitive_permission_violation_is_critical() {
        let dir = tempfile::tempdir().unwrap();
        let key = dir.path().join("server.key");
        fs::write(&key, "private").unwrap();
        fs::set_permissions(&key, fs::Permissions::from_mode(0o644)).unwrap();

        let finding = scan_file_permissions(dir.path(), &[])
            .into_iter()
            .find(|f| f.check == "file_permissions")
            .unwrap();
        assert_eq!(finding.severity, Severity::Critical);
        assert_eq!(finding.status, "fail");
    }

    #[test]
    fn secret_scanner_detects_multiple_supported_patterns() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("config.txt");
        fs::write(
            &file,
            "aws=AKIA1234567890ABCDEF\nopenai=sk-abcdefghijklmnopqrstuv\n",
        )
        .unwrap();

        let findings = scan_secrets(dir.path(), &[]);
        assert!(findings.iter().any(|f| f.summary.contains("aws_access_key")));
        assert!(findings.iter().any(|f| f.summary.contains("openai_key")));
    }

    #[test]
    fn severity_threshold_filters_info_findings() {
        let dir = tempdir().unwrap();
        let config = AuditConfig {
            severity_threshold: Severity::Warning,
            checks: vec![CheckType::Secrets],
            ..AuditConfig::default()
        };

        let report = run_security_audit_with_config(dir.path(), &config);
        assert!(report.findings.is_empty());
    }

    #[tokio::test]
    async fn tool_executor_rejects_absolute_paths() {
        let dir = tempdir().unwrap();
        let executor = SecurityAuditToolExecutor::new(dir.path());

        let error = executor
            .execute(tool_call(json!({ "target": dir.path() })))
            .await
            .expect_err("absolute path should fail");

        match error {
            ToolError::InvalidArguments { tool, .. } => assert_eq!(tool, "security_audit"),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn tool_executor_runs_workspace_relative_scan() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("secrets.env"), "token=ghp_123456789012345678901234567890123456").unwrap();
        let executor = SecurityAuditToolExecutor::new(dir.path());

        let result = executor
            .execute(tool_call(json!({ "target": ".", "checks": ["secrets"] })))
            .await
            .expect("tool execution should succeed");

        assert!(!result.is_error);
        assert!(result.output.contains("github_pat"));
    }
}
