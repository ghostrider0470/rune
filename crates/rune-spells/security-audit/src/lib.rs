use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

pub fn security_audit_tool_definition() -> rune_tools::ToolDefinition {
    rune_tools::ToolDefinition {
        name: "security_audit".into(),
        description: "Run a baseline host security audit covering listening ports, sensitive file permissions, SSH hardening, and firewall status.".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "target": {
                    "type": "string",
                    "description": "Directory path to inspect for sensitive file permissions"
                }
            }
        }),
        category: rune_core::ToolCategory::ProcessExec,
        requires_approval: false,
    }
}

pub fn run_security_audit(target: Option<&Path>) -> AuditReport {
    let target = target.unwrap_or_else(|| Path::new("."));
    let mut findings = Vec::new();
    findings.extend(scan_open_ports());
    findings.extend(scan_file_permissions(target));
    findings.extend(scan_ssh_config());
    findings.extend(scan_firewall_status());

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

            let severity = if ports.iter().any(|line| line.contains(":22 ") || line.ends_with(":22")) {
                Severity::Warning
            } else {
                Severity::Info
            };

            vec![AuditFinding {
                check: "open_ports".into(),
                severity,
                status: "observed".into(),
                summary: format!(
                    "Detected {} listening socket entries via {cmd}",
                    ports.len()
                ),
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

fn scan_file_permissions(target: &Path) -> Vec<AuditFinding> {
    let candidates = collect_sensitive_files(target);
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
            issues.push(format!(
                "{} mode {:o}; expected 700",
                ssh_dir.display(),
                mode
            ));
        }
    }
    if let Ok(meta) = fs::metadata(&config) {
        let mode = meta.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            issues.push(format!(
                "{} mode {:o}; expected 600",
                config.display(),
                mode
            ));
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

fn collect_sensitive_files(target: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(target, &mut out);
    out
}

fn walk(path: &Path, out: &mut Vec<PathBuf>) {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return;
    };
    if meta.is_file() {
        if is_sensitive_path(path) {
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
        walk(&p, out);
    }
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
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    name.starts_with(".env")
        || name.ends_with(".pem")
        || name.ends_with(".key")
        || name.ends_with(".p12")
        || name == "id_rsa"
        || name == "id_ed25519"
        || name == "config"
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
    use std::os::unix::fs::PermissionsExt;

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
    }

    #[test]
    fn sensitive_permission_violation_is_critical() {
        let dir = tempfile::tempdir().unwrap();
        let key = dir.path().join("server.key");
        fs::write(&key, "private").unwrap();
        fs::set_permissions(&key, fs::Permissions::from_mode(0o644)).unwrap();

        let finding = scan_file_permissions(dir.path())
            .into_iter()
            .find(|f| f.check == "file_permissions")
            .unwrap();
        assert_eq!(finding.severity, Severity::Critical);
        assert_eq!(finding.status, "fail");
    }
}
