use std::path::{Path, PathBuf};

use async_trait::async_trait;
use rune_core::ToolCategory;
use rune_tools::{ToolCall, ToolDefinition, ToolError, ToolExecutor, ToolResult};
use serde::{Deserialize, Serialize};
use serde_json::json;
use syn::parse_file;
use thiserror::Error;
use tracing::info;

#[derive(Debug, Error)]
pub enum CodeReviewError {
    #[error("invalid target: {0}")]
    InvalidTarget(String),
    #[error("parse error: {0}")]
    ParseError(String),
    #[error("mechanical check failed: {0}")]
    MechanicalError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReviewTarget {
    File(PathBuf),
    Diff(String),
    PullRequest { owner: String, repo: String, number: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewConfig {
    pub dimensions: Vec<String>,
    pub enable_mechanical: bool,
    pub enable_semantic: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub dimension: String,
    pub severity: Severity,
    pub file: Option<String>,
    pub line: Option<usize>,
    pub title: String,
    pub explanation: String,
    pub suggestion: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Severity {
    Critical,
    Major,
    Minor,
    Nit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewReport {
    pub target: String,
    pub findings: Vec<Finding>,
    pub summary: ReviewSummary,
    pub blocks_merge: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReviewSummary {
    pub critical: usize,
    pub major: usize,
    pub minor: usize,
    pub nit: usize,
}

pub fn code_review_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: "code_review".into(),
        description: "Perform structured code review on a file, diff, or PR.".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "target": {
                    "type": "string",
                    "description": "File path, diff string, or PR identifier (owner/repo#number)"
                },
                "dimensions": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Review dimensions to focus on (default: all)"
                },
                "enable_mechanical": {
                    "type": "boolean",
                    "description": "Enable AST-based mechanical checks (default: true)"
                },
                "enable_semantic": {
                    "type": "boolean",
                    "description": "Enable LLM-powered semantic review (default: true)"
                }
            },
            "required": ["target"]
        }),
        category: ToolCategory::FileRead,
        requires_approval: false,
    }
}

pub struct CodeReviewToolExecutor {
    workspace_root: PathBuf,
}

impl CodeReviewToolExecutor {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
        }
    }
}

#[async_trait]
impl ToolExecutor for CodeReviewToolExecutor {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let target_str = call.arguments.get("target")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments {
                tool: call.tool_name.clone(),
                reason: "missing required target".into(),
            })?;

        let target = parse_review_target(target_str, &self.workspace_root)?;

        let dimensions = call.arguments.get("dimensions")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|s| s.as_str()).map(String::from).collect())
            .unwrap_or_else(|| vec![
                "security".to_string(),
                "performance".to_string(),
                "correctness".to_string(),
                "maintainability".to_string(),
                "testing".to_string(),
                "accessibility".to_string(),
                "documentation".to_string(),
            ]);

        let config = ReviewConfig {
            dimensions,
            enable_mechanical: call.arguments.get("enable_mechanical").and_then(|v| v.as_bool()).unwrap_or(true),
            enable_semantic: call.arguments.get("enable_semantic").and_then(|v| v.as_bool()).unwrap_or(true),
        };

        let report = code_review(&target, &config).await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let output = serde_json::to_string_pretty(&report)
            .map_err(|e| ToolError::ExecutionFailed(format!("serialization failed: {e}")))?;

        Ok(ToolResult {
            tool_call_id: call.tool_call_id,
            output,
            is_error: false,
            tool_execution_id: None,
        })
    }
}

fn parse_review_target(target_str: &str, workspace_root: &Path) -> Result<ReviewTarget, ToolError> {
    if let Some((owner, repo, number)) = parse_pr_target(target_str) {
        return Ok(ReviewTarget::PullRequest { owner, repo, number });
    }

    if target_str.ends_with(".rs") {
        let candidate = Path::new(target_str);
        if candidate.is_absolute() {
            return Err(ToolError::InvalidArguments {
                tool: "code_review".to_string(),
                reason: "absolute paths are not allowed".into(),
            });
        }
        let joined = workspace_root.join(candidate);
        let resolved = joined.canonicalize().map_err(|e| ToolError::ExecutionFailed(format!("path resolution failed: {e}")))?;
        return Ok(ReviewTarget::File(resolved));
    }

    Ok(ReviewTarget::Diff(target_str.to_string()))
}

fn parse_pr_target(raw: &str) -> Option<(String, String, u64)> {
    let (repo_path, number_str) = raw.rsplit_once('#')?;
    if repo_path.is_empty() || number_str.is_empty() {
        return None;
    }

    let number = number_str.parse().ok()?;
    let (owner, repo) = repo_path.split_once('/')?;
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        return None;
    }

    Some((owner.to_string(), repo.to_string(), number))
}

async fn code_review(target: &ReviewTarget, config: &ReviewConfig) -> Result<ReviewReport, CodeReviewError> {
    let mut findings = Vec::new();
    let mut summary = ReviewSummary::default();

    match target {
        ReviewTarget::File(path) => {
            let content = tokio::fs::read_to_string(path).await
                .map_err(|e| CodeReviewError::ParseError(e.to_string()))?;
            if config.enable_mechanical {
                mechanical_review(&content, &config.dimensions, &mut findings, &mut summary);
            }
            if config.enable_semantic {
                info!("Semantic review stub: would call LLM on file {}", path.display());
                findings.push(Finding {
                    dimension: "semantic".to_string(),
                    severity: Severity::Minor,
                    file: Some(path.to_string_lossy().to_string()),
                    line: None,
                    title: "Stub semantic finding".to_string(),
                    explanation: "Placeholder for LLM-powered review.".to_string(),
                    suggestion: Some("Integrate with model provider.".to_string()),
                });
                summary.minor += 1;
            }
        }
        ReviewTarget::Diff(diff) => {
            findings.push(Finding {
                dimension: "diff".to_string(),
                severity: Severity::Nit,
                file: None,
                line: None,
                title: "Diff review stub".to_string(),
                explanation: format!("Reviewed diff of length {}", diff.len()),
                suggestion: None,
            });
            summary.nit += 1;
        }
        ReviewTarget::PullRequest { owner, repo, number } => {
            findings.push(Finding {
                dimension: "pr".to_string(),
                severity: Severity::Nit,
                file: None,
                line: None,
                title: "PR review stub".to_string(),
                explanation: format!("Reviewed PR #{number} in {owner}/{repo}"),
                suggestion: None,
            });
            summary.nit += 1;
        }
    }

    let blocks_merge = summary.critical > 0 || summary.major > 0;

    Ok(ReviewReport {
        target: format!("{:?}", target),
        findings,
        summary,
        blocks_merge,
    })
}

fn mechanical_review(content: &str, dimensions: &[String], findings: &mut Vec<Finding>, summary: &mut ReviewSummary) {
    if parse_file(content).is_ok() {
        let unsafe_count = content.matches("unsafe").count();
        if unsafe_count > 0 && dimensions.contains(&"security".to_string()) {
            findings.push(Finding {
                dimension: "security".to_string(),
                severity: Severity::Major,
                file: None,
                line: None,
                title: "Unsafe blocks detected".to_string(),
                explanation: format!("Found {unsafe_count} unsafe blocks; review for necessity and safety."),
                suggestion: Some("Minimize unsafe usage or add justifications and tests.".to_string()),
            });
            summary.major += 1;
        }

        if !content.contains("mod tests") && content.contains(".unwrap()") && dimensions.contains(&"correctness".to_string()) {
            let lines = content.lines().enumerate()
                .filter(|(_, line)| line.contains(".unwrap()"))
                .map(|(i, _)| i + 1)
                .collect::<Vec<usize>>();
            let count = lines.len();
            for line in lines {
                findings.push(Finding {
                    dimension: "correctness".to_string(),
                    severity: Severity::Major,
                    file: None,
                    line: Some(line),
                    title: "unwrap() used in production code".to_string(),
                    explanation: "Unwrap can cause panics; prefer error handling.".to_string(),
                    suggestion: Some("Use ? operator or match for proper error propagation.".to_string()),
                });
            }
            summary.major += count;
        }
    } else {
        findings.push(Finding {
            dimension: "maintainability".to_string(),
            severity: Severity::Minor,
            file: None,
            line: None,
            title: "Syntax parse error".to_string(),
            explanation: "Code could not be parsed as valid Rust.".to_string(),
            suggestion: Some("Fix syntax errors before review.".to_string()),
        });
        summary.minor += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parses_pr_target_with_owner_repo_number() {
        let target = parse_review_target("ghostrider0470/rune#123", Path::new("."))
            .expect("should parse");

        match target {
            ReviewTarget::PullRequest { owner, repo, number } => {
                assert_eq!(owner, "ghostrider0470");
                assert_eq!(repo, "rune");
                assert_eq!(number, 123);
            }
            other => panic!("expected PR target, got {other:?}"),
        }
    }

    #[test]
    fn rejects_malformed_pr_target_as_pr() {
        assert!(parse_pr_target("org/team/repo#12").is_none());
        assert!(parse_pr_target("repo-only#12").is_none());
        assert!(parse_pr_target("owner/repo#not-a-number").is_none());
    }

    #[tokio::test]
    async fn test_basic_file_review() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.rs");
        std::fs::write(&file_path, "fn main() { let x = Some(1).unwrap(); unsafe { } }").unwrap();

        let target = ReviewTarget::File(file_path);
        let config = ReviewConfig {
            dimensions: vec!["security".to_string(), "correctness".to_string()],
            enable_mechanical: true,
            enable_semantic: false,
        };

        let report = code_review(&target, &config).await.unwrap();
        assert!(report.summary.major >= 2);
        assert!(report.blocks_merge);
    }
}
