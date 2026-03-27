use std::path::{Path, PathBuf};

use async_trait::async_trait;
use rune_core::ToolCategory;
use rune_tools::{ToolCall, ToolDefinition, ToolError, ToolExecutor, ToolResult};
use serde::{Deserialize, Serialize};
use serde_json::json;
use syn::visit::Visit;
use thiserror::Error;
use tracing::info;

const ALL_DIMENSIONS: &[&str] = &[
    "security",
    "performance",
    "correctness",
    "maintainability",
    "testing",
    "accessibility",
    "documentation",
];

#[derive(Debug, Error)]
pub enum CodeReviewError {
    #[error("invalid target: {0}")]
    InvalidTarget(String),
    #[error("parse error: {0}")]
    ParseError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReviewTarget {
    File(PathBuf),
    Diff(String),
    PullRequest { owner: String, repo: String, number: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Dimension {
    Security,
    Performance,
    Correctness,
    Maintainability,
    Testing,
    Accessibility,
    Documentation,
}

impl Dimension {
    fn all() -> Vec<Self> {
        vec![
            Self::Security,
            Self::Performance,
            Self::Correctness,
            Self::Maintainability,
            Self::Testing,
            Self::Accessibility,
            Self::Documentation,
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewConfig {
    pub dimensions: Vec<Dimension>,
    pub severity_threshold: Severity,
    pub max_lines_per_pass: usize,
    pub enable_mechanical: bool,
    pub enable_semantic: bool,
}

impl Default for ReviewConfig {
    fn default() -> Self {
        Self {
            dimensions: Dimension::all(),
            severity_threshold: Severity::Nit,
            max_lines_per_pass: 400,
            enable_mechanical: true,
            enable_semantic: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Severity {
    Critical,
    Major,
    Minor,
    Nit,
}

impl Severity {
    fn rank(&self) -> u8 {
        match self {
            Self::Nit => 0,
            Self::Minor => 1,
            Self::Major => 2,
            Self::Critical => 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub dimension: Dimension,
    pub severity: Severity,
    pub file: String,
    pub line: Option<usize>,
    pub title: String,
    pub explanation: String,
    pub suggestion: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewReport {
    pub target: String,
    pub pass_results: Vec<PassResult>,
    pub findings: Vec<Finding>,
    pub summary: ReviewSummary,
    pub blocks_merge: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PassResult {
    pub pass: ReviewPass,
    pub summary: String,
    pub findings: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewPass {
    Structure,
    Detail,
    Hardening,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReviewSummary {
    pub critical: usize,
    pub major: usize,
    pub minor: usize,
    pub nit: usize,
}

impl ReviewSummary {
    fn add(&mut self, severity: &Severity) {
        match severity {
            Severity::Critical => self.critical += 1,
            Severity::Major => self.major += 1,
            Severity::Minor => self.minor += 1,
            Severity::Nit => self.nit += 1,
        }
    }
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
                    "items": { "type": "string", "enum": ALL_DIMENSIONS },
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
        let target_str = call
            .arguments
            .get("target")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments {
                tool: call.tool_name.clone(),
                reason: "missing required target".into(),
            })?;

        let target = parse_review_target(target_str, &self.workspace_root)?;
        let config = parse_review_config(&call)?;

        let report = code_review(&target, &config)
            .await
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

fn parse_review_config(call: &ToolCall) -> Result<ReviewConfig, ToolError> {
    let dimensions = match call.arguments.get("dimensions").and_then(|v| v.as_array()) {
        Some(values) => values
            .iter()
            .map(|value| {
                let raw = value.as_str().ok_or_else(|| ToolError::InvalidArguments {
                    tool: call.tool_name.clone(),
                    reason: "dimensions must contain only strings".into(),
                })?;
                parse_dimension(raw).ok_or_else(|| ToolError::InvalidArguments {
                    tool: call.tool_name.clone(),
                    reason: format!("unknown dimension: {raw}"),
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
        None => Dimension::all(),
    };

    Ok(ReviewConfig {
        dimensions,
        enable_mechanical: call
            .arguments
            .get("enable_mechanical")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        enable_semantic: call
            .arguments
            .get("enable_semantic")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        ..ReviewConfig::default()
    })
}

fn parse_dimension(raw: &str) -> Option<Dimension> {
    match raw {
        "security" => Some(Dimension::Security),
        "performance" => Some(Dimension::Performance),
        "correctness" => Some(Dimension::Correctness),
        "maintainability" => Some(Dimension::Maintainability),
        "testing" => Some(Dimension::Testing),
        "accessibility" => Some(Dimension::Accessibility),
        "documentation" => Some(Dimension::Documentation),
        _ => None,
    }
}

fn parse_review_target(target_str: &str, workspace_root: &Path) -> Result<ReviewTarget, ToolError> {
    if let Some((owner, repo, number)) = parse_pr_target(target_str) {
        return Ok(ReviewTarget::PullRequest { owner, repo, number });
    }

    let candidate = Path::new(target_str);
    if candidate.components().count() > 1 || target_str.ends_with(".rs") {
        if candidate.is_absolute() {
            return Err(ToolError::InvalidArguments {
                tool: "code_review".to_string(),
                reason: "absolute paths are not allowed".into(),
            });
        }

        let workspace_root = workspace_root.canonicalize().map_err(|e| {
            ToolError::ExecutionFailed(format!("workspace root invalid: {e}"))
        })?;
        let joined = workspace_root.join(candidate);
        if !joined.exists() {
            return Err(ToolError::InvalidArguments {
                tool: "code_review".to_string(),
                reason: "path does not exist".into(),
            });
        }
        let resolved = joined.canonicalize().map_err(|e| {
            ToolError::ExecutionFailed(format!("path resolution failed: {e}"))
        })?;
        if !resolved.starts_with(&workspace_root) {
            return Err(ToolError::InvalidArguments {
                tool: "code_review".to_string(),
                reason: "path escapes workspace boundary".into(),
            });
        }
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

pub async fn code_review(target: &ReviewTarget, config: &ReviewConfig) -> Result<ReviewReport, CodeReviewError> {
    let mut findings = Vec::new();
    let mut pass_results = Vec::new();

    match target {
        ReviewTarget::File(path) => {
            let content = tokio::fs::read_to_string(path)
                .await
                .map_err(|e| CodeReviewError::ParseError(e.to_string()))?;

            if config.enable_mechanical {
                let mechanical = mechanical_review(&content, path, &config.dimensions)?;
                pass_results.push(PassResult {
                    pass: ReviewPass::Detail,
                    summary: format!("Mechanical analysis produced {} findings", mechanical.len()),
                    findings: mechanical.len(),
                });
                findings.extend(mechanical);
            }

            if config.enable_semantic {
                info!("Semantic review stub: would call LLM on file {}", path.display());
                pass_results.push(PassResult {
                    pass: ReviewPass::Structure,
                    summary: "Semantic structure review placeholder".to_string(),
                    findings: 0,
                });
                pass_results.push(PassResult {
                    pass: ReviewPass::Hardening,
                    summary: "Semantic hardening review placeholder".to_string(),
                    findings: 0,
                });
            }
        }
        ReviewTarget::Diff(diff) => {
            pass_results.push(PassResult {
                pass: ReviewPass::Structure,
                summary: format!("Diff target received ({} bytes)", diff.len()),
                findings: 0,
            });
            findings.push(Finding {
                dimension: Dimension::Maintainability,
                severity: Severity::Nit,
                file: "<diff>".to_string(),
                line: None,
                title: "Diff review is currently semantic-only".to_string(),
                explanation: "Mechanical AST checks require a file target in this initial implementation.".to_string(),
                suggestion: Some("Review a workspace-relative file path for AST-backed checks.".to_string()),
            });
        }
        ReviewTarget::PullRequest { owner, repo, number } => {
            pass_results.push(PassResult {
                pass: ReviewPass::Structure,
                summary: format!("PR target {owner}/{repo}#{number} queued for external diff fetch"),
                findings: 0,
            });
            findings.push(Finding {
                dimension: Dimension::Documentation,
                severity: Severity::Nit,
                file: format!("{owner}/{repo}#{number}"),
                line: None,
                title: "PR integration scaffold only".to_string(),
                explanation: "This spell accepts PR targets, but inline GitHub review posting is not implemented yet.".to_string(),
                suggestion: Some("Fetch the diff and re-run review against file or diff content.".to_string()),
            });
        }
    }

    let filtered_findings = findings
        .into_iter()
        .filter(|finding| finding.severity.rank() >= config.severity_threshold.rank())
        .collect::<Vec<_>>();

    let mut summary = ReviewSummary::default();
    for finding in &filtered_findings {
        summary.add(&finding.severity);
    }

    let blocks_merge = summary.critical > 0 || summary.major > 0;

    Ok(ReviewReport {
        target: format!("{:?}", target),
        pass_results,
        findings: filtered_findings,
        summary,
        blocks_merge,
    })
}

fn mechanical_review(
    content: &str,
    path: &Path,
    dimensions: &[Dimension],
) -> Result<Vec<Finding>, CodeReviewError> {
    let syntax = syn::parse_file(content).map_err(|e| CodeReviewError::ParseError(e.to_string()))?;
    let review_file = path.display().to_string();
    let mut visitor = MechanicalVisitor::default();
    visitor.visit_file(&syntax);

    let mut findings = Vec::new();

    if dimensions.contains(&Dimension::Security) {
        for line in visitor.unsafe_lines {
            findings.push(Finding {
                dimension: Dimension::Security,
                severity: Severity::Major,
                file: review_file.clone(),
                line: Some(line),
                title: "unsafe block requires manual justification".to_string(),
                explanation: "Unsafe Rust bypasses compiler guarantees and needs an explicit safety review.".to_string(),
                suggestion: Some("Document the safety invariant and add focused tests around the unsafe block.".to_string()),
            });
        }
    }

    if dimensions.contains(&Dimension::Correctness) {
        for line in &visitor.unwrap_lines {
            findings.push(Finding {
                dimension: Dimension::Correctness,
                severity: Severity::Major,
                file: review_file.clone(),
                line: Some(*line),
                title: "unwrap() used in non-test code".to_string(),
                explanation: "unwrap() can panic in production paths and bypass normal error handling.".to_string(),
                suggestion: Some("Use ? for propagation, match for recovery, or expect() with a precise invariant message.".to_string()),
            });
        }

        for line in &visitor.lock_unwrap_lines {
            findings.push(Finding {
                dimension: Dimension::Correctness,
                severity: Severity::Major,
                file: review_file.clone(),
                line: Some(*line),
                title: "lock().unwrap() can panic on poisoned mutex".to_string(),
                explanation: "Mutex poisoning turns lock acquisition into a recoverable error; unwrap() crashes instead.".to_string(),
                suggestion: Some("Handle PoisonError explicitly and decide whether to recover or abort with context.".to_string()),
            });
        }
    }

    if dimensions.contains(&Dimension::Performance) {
        for line in &visitor.blocking_async_lines {
            findings.push(Finding {
                dimension: Dimension::Performance,
                severity: Severity::Major,
                file: review_file.clone(),
                line: Some(*line),
                title: "blocking call inside async function".to_string(),
                explanation: "Blocking file or thread APIs stall the async executor and can starve unrelated tasks.".to_string(),
                suggestion: Some("Use tokio::fs or tokio::time::sleep, or move the blocking work onto spawn_blocking.".to_string()),
            });
        }

        for line in &visitor.clone_lines {
            findings.push(Finding {
                dimension: Dimension::Performance,
                severity: Severity::Minor,
                file: review_file.clone(),
                line: Some(*line),
                title: "clone() detected; verify cost on hot paths".to_string(),
                explanation: "Repeated cloning can hide avoidable allocations or refcount churn in performance-sensitive code.".to_string(),
                suggestion: Some("Prefer borrowing, Arc reuse, or moving ownership where practical.".to_string()),
            });
        }
    }

    if dimensions.contains(&Dimension::Maintainability) && visitor.file_has_no_tests {
        findings.push(Finding {
            dimension: Dimension::Maintainability,
            severity: Severity::Minor,
            file: review_file,
            line: None,
            title: "file has no inline test module".to_string(),
            explanation: "The review engine did not detect a #[cfg(test)] mod tests block in this file.".to_string(),
            suggestion: Some("Add focused unit tests for new logic or ensure coverage exists elsewhere.".to_string()),
        });
    }

    Ok(findings)
}

#[derive(Default)]
struct MechanicalVisitor {
    unsafe_lines: Vec<usize>,
    unwrap_lines: Vec<usize>,
    lock_unwrap_lines: Vec<usize>,
    blocking_async_lines: Vec<usize>,
    clone_lines: Vec<usize>,
    in_async_fn: bool,
    file_has_no_tests: bool,
}

impl<'ast> Visit<'ast> for MechanicalVisitor {
    fn visit_file(&mut self, node: &'ast syn::File) {
        self.file_has_no_tests = !node.items.iter().any(item_has_test_cfg);
        syn::visit::visit_file(self, node);
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let previous = self.in_async_fn;
        self.in_async_fn = node.sig.asyncness.is_some();
        syn::visit::visit_item_fn(self, node);
        self.in_async_fn = previous;
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        let previous = self.in_async_fn;
        self.in_async_fn = node.sig.asyncness.is_some();
        syn::visit::visit_impl_item_fn(self, node);
        self.in_async_fn = previous;
    }

    fn visit_expr_unsafe(&mut self, node: &'ast syn::ExprUnsafe) {
        self.unsafe_lines.push(0);
        syn::visit::visit_expr_unsafe(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let line = 0;
        match node.method.to_string().as_str() {
            "unwrap" => {
                self.unwrap_lines.push(line);
                if let syn::Expr::MethodCall(inner) = &*node.receiver {
                    if inner.method == "lock" {
                        self.lock_unwrap_lines.push(line);
                    }
                }
            }
            "clone" => self.clone_lines.push(line),
            _ => {}
        }
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if self.in_async_fn {
            if let syn::Expr::Path(path) = &*node.func {
                let segments = path
                    .path
                    .segments
                    .iter()
                    .map(|segment| segment.ident.to_string())
                    .collect::<Vec<_>>();
                if segments == ["std", "thread", "sleep"] || segments == ["std", "fs", "read_to_string"] || segments == ["std", "fs", "read"] {
                    self.blocking_async_lines.push(0);
                }
            }
        }
        syn::visit::visit_expr_call(self, node);
    }
}

fn item_has_test_cfg(item: &syn::Item) -> bool {
    match item {
        syn::Item::Mod(module) => attrs_have_test_cfg(&module.attrs),
        syn::Item::Fn(function) => attrs_have_test_cfg(&function.attrs),
        _ => false,
    }
}

fn attrs_have_test_cfg(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| attr.path().is_ident("test") || attr.path().is_ident("cfg"))
}


#[cfg(test)]
mod tests {
    use rune_core::ToolCallId;
    use super::*;
    use tempfile::tempdir;

    fn tool_call(name: &str, arguments: serde_json::Value) -> ToolCall {
        ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: name.to_string(),
            arguments,
        }
    }

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
    async fn executor_rejects_absolute_paths() {
        let tmp = tempdir().expect("tempdir");
        let executor = CodeReviewToolExecutor::new(tmp.path());
        let error = executor
            .execute(tool_call(
                "code_review",
                serde_json::json!({ "target": tmp.path().display().to_string() }),
            ))
            .await
            .expect_err("absolute paths should be rejected");

        match error {
            ToolError::InvalidArguments { tool, .. } => assert_eq!(tool, "code_review"),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn mechanical_review_flags_core_rust_risks() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.rs");
        std::fs::write(
            &file_path,
            r#"
async fn run(m: std::sync::Mutex<String>) {
    let _guard = m.lock().unwrap();
    let _x = std::fs::read_to_string("x");
    let _y = String::new().clone();
    unsafe { core::ptr::read_volatile(&1); }
}
"#,
        )
        .unwrap();

        let target = ReviewTarget::File(file_path.clone());
        let config = ReviewConfig {
            dimensions: vec![Dimension::Security, Dimension::Performance, Dimension::Correctness, Dimension::Maintainability],
            enable_mechanical: true,
            enable_semantic: false,
            ..ReviewConfig::default()
        };

        let report = code_review(&target, &config).await.unwrap();
        assert!(report.blocks_merge);
        assert!(report.findings.iter().any(|f| f.title.contains("unsafe block")));
        assert!(report.findings.iter().any(|f| f.title.contains("unwrap() used")));
        assert!(report.findings.iter().any(|f| f.title.contains("lock().unwrap()")));
        assert!(report.findings.iter().any(|f| f.title.contains("blocking call inside async")));
        assert!(report.findings.iter().any(|f| f.title.contains("clone() detected")));
        assert!(report.pass_results.iter().any(|p| matches!(p.pass, ReviewPass::Detail)));
    }

    #[tokio::test]
    async fn file_with_tests_avoids_missing_test_finding() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.rs");
        std::fs::write(
            &file_path,
            r#"
fn add(a: i32, b: i32) -> i32 { a + b }

#[cfg(test)]
mod tests {
    #[test]
    fn adds() { assert_eq!(super::add(1, 2), 3); }
}
"#,
        )
        .unwrap();

        let target = ReviewTarget::File(file_path);
        let config = ReviewConfig {
            enable_mechanical: true,
            enable_semantic: false,
            ..ReviewConfig::default()
        };
        let report = code_review(&target, &config).await.unwrap();
        assert!(!report.findings.iter().any(|f| f.title.contains("no inline test module")));
    }
}
