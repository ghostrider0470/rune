//! Git tool for agents to perform version control operations via subprocess.

use std::path::PathBuf;
use std::process::Stdio;

use serde_json::json;

use async_trait::async_trait;
use tracing::instrument;

use crate::definition::{ToolCall, ToolDefinition, ToolResult};
use crate::error::ToolError;
use crate::executor::ToolExecutor;

/// Allowed git subcommands. Anything outside this list is rejected.
const ALLOWED_OPERATIONS: &[&str] = &[
    "status",
    "diff",
    "add",
    "commit",
    "push",
    "pull",
    "log",
    "branch",
    "checkout",
    "merge",
    "pr_status",
    "pr_open",
    "pr_merge",
    "repo_state",
    "staged",
    "suggest_branch",
];

/// Maximum output bytes returned from a git command (50 KB).
const MAX_OUTPUT_BYTES: usize = 50 * 1024;

/// Default timeout for git operations (60 seconds).
const DEFAULT_TIMEOUT_SECS: u64 = 60;
const DEFAULT_PROTECTED_BRANCHES: &[&str] = &["main", "master"];

/// Executor for the `git` tool.
///
/// Wraps the git CLI, running subcommands as child processes within a
/// workspace boundary. Only an allow-listed set of operations are permitted.
pub struct GitToolExecutor {
    workspace_root: PathBuf,
}

impl GitToolExecutor {
    /// Create a new git tool executor rooted at the given workspace directory.
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
        }
    }

    #[instrument(skip(self, call), fields(tool = "git"))]
    async fn handle(&self, call: &ToolCall) -> Result<ToolResult, ToolError> {
        let operation = call
            .arguments
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments {
                tool: "git".into(),
                reason: "missing required field: operation".into(),
            })?;

        // Validate against allow-list
        if !ALLOWED_OPERATIONS.contains(&operation) {
            return Ok(ToolResult {
                tool_call_id: call.tool_call_id.clone(),
                output: format!(
                    "Unsupported git operation: {operation}. Allowed: {}",
                    ALLOWED_OPERATIONS.join(", ")
                ),
                is_error: true,
                tool_execution_id: None,
            });
        }

        // Collect additional arguments
        let args: Vec<String> = call
            .arguments
            .get("args")
            .and_then(|v| {
                // Accept either a JSON array of strings or a single string
                if let Some(arr) = v.as_array() {
                    Some(
                        arr.iter()
                            .filter_map(|item| item.as_str().map(String::from))
                            .collect(),
                    )
                } else {
                    // Split a single string on whitespace for convenience
                    v.as_str()
                        .map(|s| s.split_whitespace().map(String::from).collect())
                }
            })
            .unwrap_or_default();

        let mode = call
            .arguments
            .get("mode")
            .and_then(|v| v.as_str())
            .map(str::to_owned);

        // Validate workspace root exists
        let workspace_root = self
            .workspace_root
            .canonicalize()
            .map_err(|e| ToolError::ExecutionFailed(format!("workspace root invalid: {e}")))?;

        if let Some(result) = self
            .preflight(operation, &args, call, &workspace_root)
            .await?
        {
            return Ok(result);
        }

        if let Some(result) = self
            .synthetic_operation(operation, &args, mode.as_deref(), call, &workspace_root)
            .await?
        {
            return Ok(result);
        }

        // Build the command: git <operation> [args...]
        let mut cmd = tokio::process::Command::new("git");
        cmd.arg(operation);
        cmd.args(&args);
        cmd.current_dir(&workspace_root);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        // Spawn and wait with timeout
        let child = cmd
            .spawn()
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to spawn git: {e}")))?;

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            child.wait_with_output(),
        )
        .await
        .map_err(|_| {
            ToolError::ExecutionFailed(format!(
                "git {operation} timed out after {DEFAULT_TIMEOUT_SECS}s"
            ))
        })?
        .map_err(|e| ToolError::ExecutionFailed(format!("git process error: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let exit_code = output.status.code().unwrap_or(-1);

        // Format output
        let mut result_text = String::new();

        if !stdout.is_empty() {
            result_text.push_str(&stdout);
        }

        if !stderr.is_empty() {
            if !result_text.is_empty() {
                result_text.push('\n');
            }
            result_text.push_str(&stderr);
        }

        if result_text.is_empty() {
            result_text = format!("git {operation} completed (exit code {exit_code})");
        }

        // Truncate if too large
        if result_text.len() > MAX_OUTPUT_BYTES {
            let truncated = truncate_utf8(&result_text, MAX_OUTPUT_BYTES);
            result_text = format!(
                "{truncated}\n\n[truncated: showing {MAX_OUTPUT_BYTES} of {} bytes]",
                result_text.len()
            );
        }

        let is_error = !output.status.success();
        if is_error {
            result_text = format!("git {operation} failed (exit code {exit_code}):\n{result_text}");
        }

        Ok(ToolResult {
            tool_call_id: call.tool_call_id.clone(),
            output: result_text,
            is_error,
            tool_execution_id: None,
        })
    }
}

impl GitToolExecutor {
    async fn preflight(
        &self,
        operation: &str,
        args: &[String],
        call: &ToolCall,
        workspace_root: &std::path::Path,
    ) -> Result<Option<ToolResult>, ToolError> {
        if matches!(operation, "pr_status" | "pr_open" | "pr_merge" | "repo_state" | "staged" | "suggest_branch") {
            return Ok(None);
        }

        let safety = call.arguments.get("safety").and_then(|v| v.as_object());

        let allow_dirty = safety
            .and_then(|obj| obj.get("allow_dirty"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let require_clean = safety
            .and_then(|obj| obj.get("require_clean"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let require_base_in_sync = safety
            .and_then(|obj| obj.get("require_base_in_sync"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let protected_branches = safety
            .and_then(|obj| obj.get("protected_branches"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect::<Vec<_>>()
            })
            .filter(|arr| !arr.is_empty())
            .unwrap_or_else(|| {
                DEFAULT_PROTECTED_BRANCHES
                    .iter()
                    .map(|branch| (*branch).to_owned())
                    .collect()
            });

        let check_dirty = require_clean || !allow_dirty;
        let dirty = if check_dirty {
            let output = self
                .run_git_capture(workspace_root, ["status", "--porcelain"])
                .await?;
            if !output.status.success() {
                return Err(ToolError::ExecutionFailed(format!(
                    "git status --porcelain failed during safety preflight: {}",
                    format_output("status", &output)
                )));
            }
            !String::from_utf8_lossy(&output.stdout).trim().is_empty()
        } else {
            false
        };

        if dirty && !allow_dirty {
            return Ok(Some(self.blocked_result(
                call,
                operation,
                json!({
                    "kind": "dirty_tree",
                    "message": "git working tree is dirty; pass safety.allow_dirty=true to override",
                }),
            )));
        }

        let current_branch = self.current_branch(workspace_root).await?;
        let on_protected_branch = current_branch
            .as_deref()
            .map(|branch| {
                protected_branches
                    .iter()
                    .any(|candidate| candidate == branch)
            })
            .unwrap_or(false);

        if on_protected_branch && is_mutating_operation(operation) {
            return Ok(Some(self.blocked_result(
                call,
                operation,
                json!({
                    "kind": "protected_branch",
                    "message": format!(
                        "refusing to run mutating git operation on protected branch {}",
                        current_branch.as_deref().unwrap_or("<detached>")
                    ),
                    "branch": current_branch,
                    "protected_branches": protected_branches,
                }),
            )));
        }

        if require_base_in_sync {
            let base_branch = safety
                .and_then(|obj| obj.get("base_branch"))
                .and_then(|v| v.as_str())
                .unwrap_or("main");
            let remote_name = safety
                .and_then(|obj| obj.get("remote"))
                .and_then(|v| v.as_str())
                .unwrap_or("origin");
            let fetch_output = self
                .run_git_capture(workspace_root, ["fetch", remote_name, base_branch])
                .await?;
            if !fetch_output.status.success() {
                return Ok(Some(self.blocked_result(
                    call,
                    operation,
                    json!({
                        "kind": "base_sync_fetch_failed",
                        "message": format!("failed to fetch {remote_name}/{base_branch} during safety preflight"),
                        "details": format_output("fetch", &fetch_output),
                    }),
                )));
            }

            let merge_base = self
                .run_git_capture(
                    workspace_root,
                    [
                        "merge-base",
                        "HEAD",
                        &format!("{remote_name}/{base_branch}"),
                    ],
                )
                .await?;
            let head_rev = self
                .run_git_capture(workspace_root, ["rev-parse", "HEAD"])
                .await?;
            let base_rev = self
                .run_git_capture(
                    workspace_root,
                    ["rev-parse", &format!("{remote_name}/{base_branch}")],
                )
                .await?;

            if !(merge_base.status.success()
                && head_rev.status.success()
                && base_rev.status.success())
            {
                return Ok(Some(self.blocked_result(
                    call,
                    operation,
                    json!({
                        "kind": "base_sync_probe_failed",
                        "message": format!("failed to compare HEAD against {remote_name}/{base_branch}"),
                        "details": {
                            "merge_base": format_output("merge-base", &merge_base),
                            "head": format_output("rev-parse HEAD", &head_rev),
                            "base": format_output(&format!("rev-parse {remote_name}/{base_branch}"), &base_rev),
                        },
                    }),
                )));
            }

            let merge_base_sha = String::from_utf8_lossy(&merge_base.stdout)
                .trim()
                .to_owned();
            let head_sha = String::from_utf8_lossy(&head_rev.stdout).trim().to_owned();
            let base_sha = String::from_utf8_lossy(&base_rev.stdout).trim().to_owned();
            if merge_base_sha != base_sha {
                return Ok(Some(self.blocked_result(
                    call,
                    operation,
                    json!({
                        "kind": "base_out_of_sync",
                        "message": format!("HEAD is not based on the latest {remote_name}/{base_branch}"),
                        "branch": current_branch,
                        "head": head_sha,
                        "base": base_sha,
                    }),
                )));
            }
            if require_clean && dirty {
                return Ok(Some(self.blocked_result(
                    call,
                    operation,
                    json!({
                        "kind": "dirty_tree",
                        "message": "git working tree is dirty and safety.require_clean=true",
                    }),
                )));
            }
        }

        if require_clean && dirty {
            return Ok(Some(self.blocked_result(
                call,
                operation,
                json!({
                    "kind": "dirty_tree",
                    "message": "git working tree is dirty and safety.require_clean=true",
                }),
            )));
        }

        let _ = args;
        Ok(None)
    }

    async fn current_branch(
        &self,
        workspace_root: &std::path::Path,
    ) -> Result<Option<String>, ToolError> {
        let output = self
            .run_git_capture(workspace_root, ["rev-parse", "--abbrev-ref", "HEAD"])
            .await?;
        if !output.status.success() {
            return Ok(None);
        }
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if branch.is_empty() || branch == "HEAD" {
            Ok(None)
        } else {
            Ok(Some(branch))
        }
    }

    async fn run_git_capture<I, S>(
        &self,
        workspace_root: &std::path::Path,
        args: I,
    ) -> Result<std::process::Output, ToolError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let mut cmd = tokio::process::Command::new("git");
        cmd.args(args);
        cmd.current_dir(workspace_root);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let child = cmd
            .spawn()
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to spawn git: {e}")))?;

        tokio::time::timeout(
            std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            child.wait_with_output(),
        )
        .await
        .map_err(|_| {
            ToolError::ExecutionFailed(format!(
                "git helper timed out after {DEFAULT_TIMEOUT_SECS}s"
            ))
        })?
        .map_err(|e| ToolError::ExecutionFailed(format!("git process error: {e}")))
    }

    async fn synthetic_operation(
        &self,
        operation: &str,
        args: &[String],
        mode: Option<&str>,
        call: &ToolCall,
        workspace_root: &std::path::Path,
    ) -> Result<Option<ToolResult>, ToolError> {
        match operation {
            "pr_status" => Ok(Some(self.pr_status(call, workspace_root).await?)),
            "pr_open" => Ok(Some(self.pr_open(call, workspace_root).await?)),
            "pr_merge" => Ok(Some(self.pr_merge(call, workspace_root).await?)),
            "repo_state" => Ok(Some(self.repo_state(call, workspace_root).await?)),
            "staged" => Ok(Some(self.staged_status(call, mode, workspace_root).await?)),
            "suggest_branch" => Ok(Some(
                self.suggest_branch_name(call, args, workspace_root).await?,
            )),
            _ => Ok(None),
        }
    }

    async fn pr_status(
        &self,
        call: &ToolCall,
        workspace_root: &std::path::Path,
    ) -> Result<ToolResult, ToolError> {
        let current_branch = self.current_branch(workspace_root).await?;
        let branch = call
            .arguments
            .get("branch")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .or(current_branch.clone());

        let Some(branch) = branch else {
            return Ok(self.structured_result(
                call,
                false,
                json!({
                    "kind": "no_branch",
                    "message": "cannot inspect PR status from detached HEAD without an explicit branch"
                }),
            ));
        };

        let mut cmd = tokio::process::Command::new("gh");
        cmd.args([
            "pr",
            "view",
            &branch,
            "--json",
            "number,title,url,state,isDraft,mergeStateStatus,headRefName,baseRefName,statusCheckRollup",
        ]);
        cmd.current_dir(workspace_root);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        let child = cmd
            .spawn()
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to spawn gh: {e}")))?;
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            child.wait_with_output(),
        )
        .await
        .map_err(|_| {
            ToolError::ExecutionFailed(format!(
                "gh pr view timed out after {DEFAULT_TIMEOUT_SECS}s"
            ))
        })?
        .map_err(|e| ToolError::ExecutionFailed(format!("gh process error: {e}")))?;

        if !output.status.success() {
            let details = format_output("gh pr view", &output);
            let missing = details.contains("no pull requests found")
                || details.contains("Could not resolve to a PullRequest");
            return Ok(self.structured_result(
                call,
                missing,
                json!({
                    "branch": branch,
                    "current_branch": current_branch,
                    "has_pr": false,
                    "message": if missing { "no pull request found for branch" } else { "failed to inspect pull request status" },
                    "details": details,
                }),
            ));
        }

        let value: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|e| {
            ToolError::ExecutionFailed(format!("failed to parse gh pr view JSON: {e}"))
        })?;
        let checks = value
            .get("statusCheckRollup")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .map(|item| {
                        json!({
                            "name": item.get("name").and_then(|v| v.as_str()),
                            "status": item.get("status").and_then(|v| v.as_str()),
                            "conclusion": item.get("conclusion").and_then(|v| v.as_str()),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(self.structured_result(
            call,
            true,
            json!({
                "branch": branch,
                "current_branch": current_branch,
                "has_pr": true,
                "number": value.get("number").cloned(),
                "title": value.get("title").cloned(),
                "url": value.get("url").cloned(),
                "state": value.get("state").cloned(),
                "is_draft": value.get("isDraft").cloned(),
                "merge_state_status": value.get("mergeStateStatus").cloned(),
                "head_ref": value.get("headRefName").cloned(),
                "base_ref": value.get("baseRefName").cloned(),
                "checks": checks,
            }),
        ))
    }

    async fn repo_state(
        &self,
        call: &ToolCall,
        workspace_root: &std::path::Path,
    ) -> Result<ToolResult, ToolError> {
        let current_branch = self.current_branch(workspace_root).await?;
        let protected_branches = call
            .arguments
            .get("protected_branches")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect::<Vec<_>>()
            })
            .filter(|arr| !arr.is_empty())
            .unwrap_or_else(|| {
                DEFAULT_PROTECTED_BRANCHES
                    .iter()
                    .map(|branch| (*branch).to_owned())
                    .collect()
            });
        let base_branch = call
            .arguments
            .get("base_branch")
            .and_then(|v| v.as_str())
            .unwrap_or("main");
        let remote = call
            .arguments
            .get("remote")
            .and_then(|v| v.as_str())
            .unwrap_or("origin");

        let status_output = self
            .run_git_capture(workspace_root, ["status", "--porcelain", "--branch"])
            .await?;
        if !status_output.status.success() {
            return Err(ToolError::ExecutionFailed(format!(
                "git status --porcelain --branch failed: {}",
                format_output("status --porcelain --branch", &status_output)
            )));
        }
        let status_text = String::from_utf8_lossy(&status_output.stdout);
        let mut lines = status_text.lines();
        let branch_line = lines.next().unwrap_or_default().trim().to_owned();
        let worktree_entries = lines
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.to_owned())
            .collect::<Vec<_>>();
        let dirty = !worktree_entries.is_empty();
        let branch_inferred = branch_line
            .strip_prefix("## ")
            .and_then(|raw| {
                let raw = raw
                    .strip_prefix("No commits yet on ")
                    .or_else(|| raw.strip_prefix("Initial commit on "))
                    .unwrap_or(raw);
                let head = raw
                    .split_whitespace()
                    .next()
                    .unwrap_or(raw)
                    .split("...")
                    .next()
                    .unwrap_or(raw);
                (!head.is_empty() && head != "HEAD").then_some(head.to_owned())
            })
            .or(current_branch.clone());
        let on_protected_branch = branch_inferred
            .as_deref()
            .map(|branch| protected_branches.iter().any(|candidate| candidate == branch))
            .unwrap_or(false);

        let head_rev = self.run_git_capture(workspace_root, ["rev-parse", "HEAD"]).await?;
        let head_sha = if head_rev.status.success() {
            Some(String::from_utf8_lossy(&head_rev.stdout).trim().to_owned())
        } else {
            None
        };

        let mut base_sync = serde_json::json!({
            "ok": false,
            "checked": false,
            "base_branch": base_branch,
            "remote": remote,
        });

        let fetch_output = self
            .run_git_capture(workspace_root, ["fetch", remote, base_branch])
            .await?;
        if fetch_output.status.success() {
            let remote_ref = format!("{remote}/{base_branch}");
            let merge_base = self
                .run_git_capture(workspace_root, ["merge-base", "HEAD", &remote_ref])
                .await?;
            let base_rev = self
                .run_git_capture(workspace_root, ["rev-parse", &remote_ref])
                .await?;
            if merge_base.status.success() && base_rev.status.success() {
                let merge_base_sha = String::from_utf8_lossy(&merge_base.stdout).trim().to_owned();
                let base_sha = String::from_utf8_lossy(&base_rev.stdout).trim().to_owned();
                let in_sync = merge_base_sha == base_sha;
                base_sync = serde_json::json!({
                    "ok": in_sync,
                    "checked": true,
                    "base_branch": base_branch,
                    "remote": remote,
                    "base_sha": base_sha,
                    "merge_base_sha": merge_base_sha,
                    "head_sha": head_sha,
                    "reason": if in_sync { "head includes latest remote base" } else { "head is behind remote base" },
                });
            } else {
                base_sync = serde_json::json!({
                    "ok": false,
                    "checked": true,
                    "base_branch": base_branch,
                    "remote": remote,
                    "reason": "failed to compare HEAD against remote base",
                    "details": {
                        "merge_base": format_output("merge-base", &merge_base),
                        "base_rev": format_output("rev-parse base", &base_rev),
                    }
                });
            }
        } else {
            base_sync = serde_json::json!({
                "ok": false,
                "checked": true,
                "base_branch": base_branch,
                "remote": remote,
                "reason": "failed to fetch remote base",
                "details": format_output("fetch", &fetch_output),
            });
        }

        Ok(self.structured_result(
            call,
            true,
            serde_json::json!({
                "current_branch": branch_inferred,
                "branch_status": branch_line,
                "dirty": dirty,
                "worktree_entries": worktree_entries,
                "protected_branches": protected_branches,
                "on_protected_branch": on_protected_branch,
                "head_sha": head_sha,
                "base_sync": base_sync,
            }),
        ))
    }

    async fn pr_open(
        &self,
        call: &ToolCall,
        workspace_root: &std::path::Path,
    ) -> Result<ToolResult, ToolError> {
        let title = call
            .arguments
            .get("title")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments {
                tool: "git".into(),
                reason: "pr_open requires title".into(),
            })?;
        let body = call
            .arguments
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let base = call
            .arguments
            .get("base")
            .and_then(|v| v.as_str())
            .unwrap_or("main");
        let head = call
            .arguments
            .get("head")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .or(self.current_branch(workspace_root).await?);
        let Some(head) = head else {
            return Ok(self.structured_result(
                call,
                false,
                serde_json::json!({"message": "cannot open PR from detached HEAD without explicit head branch"}),
            ));
        };

        let mut cmd = tokio::process::Command::new("gh");
        cmd.args([
            "pr",
            "create",
            "--base",
            base,
            "--head",
            &head,
            "--title",
            title,
            "--body",
            body,
        ]);
        cmd.current_dir(workspace_root);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        let child = cmd
            .spawn()
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to spawn gh: {e}")))?;
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            child.wait_with_output(),
        )
        .await
        .map_err(|_| {
            ToolError::ExecutionFailed(format!(
                "gh pr create timed out after {DEFAULT_TIMEOUT_SECS}s"
            ))
        })?
        .map_err(|e| ToolError::ExecutionFailed(format!("gh process error: {e}")))?;

        if !output.status.success() {
            return Ok(self.structured_result(
                call,
                false,
                serde_json::json!({
                    "message": "failed to open pull request",
                    "head": head,
                    "base": base,
                    "details": format_output("gh pr create", &output),
                }),
            ));
        }

        let url = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        Ok(self.structured_result(
            call,
            true,
            serde_json::json!({
                "message": "pull request opened",
                "head": head,
                "base": base,
                "url": url,
            }),
        ))
    }

    async fn pr_merge(
        &self,
        call: &ToolCall,
        workspace_root: &std::path::Path,
    ) -> Result<ToolResult, ToolError> {
        let target = call
            .arguments
            .get("target")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .or(self
                .current_branch(workspace_root)
                .await?
                .map(|branch| branch.to_owned()));
        let Some(target) = target else {
            return Ok(self.structured_result(
                call,
                false,
                serde_json::json!({"message": "cannot merge PR from detached HEAD without explicit target"}),
            ));
        };
        let strategy = call
            .arguments
            .get("strategy")
            .and_then(|v| v.as_str())
            .unwrap_or("squash");
        let delete_branch = call
            .arguments
            .get("delete_branch")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let mut args = vec!["pr", "merge", &target];
        match strategy {
            "merge" => args.push("--merge"),
            "rebase" => args.push("--rebase"),
            _ => args.push("--squash"),
        }
        if delete_branch {
            args.push("--delete-branch");
        }

        let mut cmd = tokio::process::Command::new("gh");
        cmd.args(&args);
        cmd.current_dir(workspace_root);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        let child = cmd
            .spawn()
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to spawn gh: {e}")))?;
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            child.wait_with_output(),
        )
        .await
        .map_err(|_| {
            ToolError::ExecutionFailed(format!(
                "gh pr merge timed out after {DEFAULT_TIMEOUT_SECS}s"
            ))
        })?
        .map_err(|e| ToolError::ExecutionFailed(format!("gh process error: {e}")))?;

        if !output.status.success() {
            return Ok(self.structured_result(
                call,
                false,
                serde_json::json!({
                    "message": "failed to merge pull request",
                    "target": target,
                    "strategy": strategy,
                    "details": format_output("gh pr merge", &output),
                }),
            ));
        }

        Ok(self.structured_result(
            call,
            true,
            serde_json::json!({
                "message": "pull request merged",
                "target": target,
                "strategy": strategy,
                "delete_branch": delete_branch,
                "details": format_output("gh pr merge", &output),
            }),
        ))
    }

    async fn staged_status(
        &self,
        call: &ToolCall,
        mode: Option<&str>,
        workspace_root: &std::path::Path,
    ) -> Result<ToolResult, ToolError> {
        let output = self
            .run_git_capture(workspace_root, ["diff", "--cached", "--name-status"])
            .await?;
        if !output.status.success() {
            return Err(ToolError::ExecutionFailed(format!(
                "git diff --cached --name-status failed: {}",
                format_output("diff --cached --name-status", &output)
            )));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let files = stdout
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let mut parts = line.split_whitespace();
                let status = parts.next().unwrap_or_default().to_owned();
                let path = parts.next().unwrap_or_default().to_owned();
                json!({"status": status, "path": path})
            })
            .collect::<Vec<_>>();

        let mut ok = true;
        let mut message = if files.is_empty() {
            "no files staged".to_owned()
        } else {
            format!("{} staged file(s)", files.len())
        };

        if mode == Some("single_commit") && files.is_empty() {
            ok = false;
            message = "single_commit validation failed: no files staged".to_owned();
        }

        Ok(self.structured_result(
            call,
            ok,
            json!({
                "mode": mode,
                "message": message,
                "count": files.len(),
                "files": files,
            }),
        ))
    }

    async fn suggest_branch_name(
        &self,
        call: &ToolCall,
        args: &[String],
        workspace_root: &std::path::Path,
    ) -> Result<ToolResult, ToolError> {
        let prefix = call
            .arguments
            .get("prefix")
            .and_then(|v| v.as_str())
            .unwrap_or("agent/rune");
        let slug_source = if args.is_empty() {
            call.arguments
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("work-item")
                .to_owned()
        } else {
            args.join("-")
        };
        let issue = call.arguments.get("issue").and_then(|v| v.as_i64());
        let slug = slugify(&slug_source);
        let candidate = if let Some(issue) = issue {
            format!("{prefix}/issue-{issue}-{slug}")
        } else {
            format!("{prefix}/{slug}")
        };
        let branch_exists = self
            .run_git_capture(
                workspace_root,
                [
                    "show-ref",
                    "--verify",
                    "--quiet",
                    &format!("refs/heads/{candidate}"),
                ],
            )
            .await?
            .status
            .success();

        Ok(self.structured_result(
            call,
            true,
            json!({
                "suggested": candidate,
                "available_locally": !branch_exists,
            }),
        ))
    }

    fn structured_result(
        &self,
        call: &ToolCall,
        ok: bool,
        payload: serde_json::Value,
    ) -> ToolResult {
        ToolResult {
            tool_call_id: call.tool_call_id.clone(),
            output: json!({"ok": ok, "result": payload}).to_string(),
            is_error: !ok,
            tool_execution_id: None,
        }
    }

    fn blocked_result(
        &self,
        call: &ToolCall,
        operation: &str,
        payload: serde_json::Value,
    ) -> ToolResult {
        ToolResult {
            tool_call_id: call.tool_call_id.clone(),
            output: json!({
                "ok": false,
                "operation": operation,
                "blocked": payload,
            })
            .to_string(),
            is_error: true,
            tool_execution_id: None,
        }
    }
}

fn is_mutating_operation(operation: &str) -> bool {
    matches!(
        operation,
        "add" | "commit" | "push" | "pull" | "checkout" | "merge"
    )
}

fn slugify(input: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

fn format_output(label: &str, output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => format!(
            "git {label} exited with {}",
            output.status.code().unwrap_or(-1)
        ),
        (false, true) => stdout,
        (true, false) => stderr,
        (false, false) => format!("{stdout}\n{stderr}"),
    }
}

/// Truncate a string to at most `max_bytes` without splitting a UTF-8 codepoint.
fn truncate_utf8(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[async_trait]
impl ToolExecutor for GitToolExecutor {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        match call.tool_name.as_str() {
            "git" => self.handle(&call).await,
            other => Err(ToolError::NotFound(other.into())),
        }
    }
}

/// Return the `ToolDefinition` for registration in the tool registry.
pub fn git_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: "git".into(),
        description: "Execute git operations in the workspace. Supports: status, diff, add, commit, push, pull, log, branch, checkout, merge, pr_status, pr_open, pr_merge, repo_state, staged, suggest_branch.".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "description": "Git subcommand to execute",
                    "enum": ["status", "diff", "add", "commit", "push", "pull", "log", "branch", "checkout", "merge", "pr_status", "pr_open", "pr_merge", "repo_state", "staged", "suggest_branch"]
                },
                "args": {
                    "description": "Additional arguments for the git command. Can be a JSON array of strings or a single string (split on whitespace).",
                    "oneOf": [
                        { "type": "array", "items": { "type": "string" } },
                        { "type": "string" }
                    ]
                },
                "mode": {
                    "type": "string",
                    "description": "Optional validation mode for synthetic operations such as staged-file checks."
                },
                "branch": {
                    "type": "string",
                    "description": "Optional branch name for pr_status lookups."
                },
                "issue": {
                    "type": "integer",
                    "description": "Optional issue number used when suggesting branch names."
                },
                "prefix": {
                    "type": "string",
                    "description": "Optional branch prefix used by suggest_branch."
                },
                "name": {
                    "type": "string",
                    "description": "Optional slug source used by suggest_branch when args are omitted."
                },
                "safety": {
                    "type": "object",
                    "description": "Optional safety rails for dirty-tree, base-sync, and protected-branch checks before executing mutating git operations.",
                    "properties": {
                        "allow_dirty": { "type": "boolean" },
                        "require_clean": { "type": "boolean" },
                        "require_base_in_sync": { "type": "boolean" },
                        "base_branch": { "type": "string" },
                        "remote": { "type": "string" },
                        "protected_branches": {
                            "type": "array",
                            "items": { "type": "string" }
                        }
                    }
                }
            },
            "required": ["operation"]
        }),
        category: rune_core::ToolCategory::ProcessExec,
        requires_approval: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune_core::ToolCallId;

    fn make_call(args: serde_json::Value) -> ToolCall {
        ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: "git".into(),
            arguments: args,
        }
    }

    #[test]
    fn definition_schema_has_required_operation() {
        let def = git_tool_definition();
        assert_eq!(def.name, "git");
        let required = def.parameters["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v.as_str() == Some("operation")));
    }

    #[tokio::test]
    async fn missing_operation_returns_error() {
        let exec = GitToolExecutor::new("/tmp");
        let call = make_call(serde_json::json!({}));
        let err = exec.execute(call).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments { .. }));
    }

    #[tokio::test]
    async fn unsupported_operation_returns_error_result() {
        let exec = GitToolExecutor::new("/tmp");
        let call = make_call(serde_json::json!({"operation": "rebase"}));
        let result = exec.execute(call).await.unwrap();
        assert!(result.is_error);
        assert!(result.output.contains("Unsupported git operation"));
    }

    #[tokio::test]
    async fn unknown_tool_name_rejected() {
        let exec = GitToolExecutor::new("/tmp");
        let call = ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: "not_git".into(),
            arguments: serde_json::json!({}),
        };
        let err = exec.execute(call).await.unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
    }

    #[tokio::test]
    async fn git_status_in_temp_dir() {
        // Create a temp directory with a git repo
        let dir = tempfile::tempdir().unwrap();
        let status = std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output();

        // Skip test if git is not available
        let Ok(init_output) = status else { return };
        if !init_output.status.success() {
            return;
        }

        let exec = GitToolExecutor::new(dir.path());
        let call = make_call(serde_json::json!({"operation": "status"}));
        let result = exec.execute(call).await.unwrap();
        assert!(
            !result.is_error,
            "git status should succeed: {}",
            result.output
        );
    }

    #[tokio::test]
    async fn git_log_in_temp_dir() {
        let dir = tempfile::tempdir().unwrap();

        // Init repo + create initial commit
        let init = std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output();
        let Ok(init_out) = init else { return };
        if !init_out.status.success() {
            return;
        }

        // Configure git user for the test repo
        let _ = std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir.path())
            .output();
        let _ = std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir.path())
            .output();

        // Create a file and commit
        std::fs::write(dir.path().join("test.txt"), "hello").unwrap();
        let _ = std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output();
        let _ = std::process::Command::new("git")
            .args(["commit", "-m", "initial commit"])
            .current_dir(dir.path())
            .output();

        let exec = GitToolExecutor::new(dir.path());

        // Test log with args as array
        let call = make_call(serde_json::json!({"operation": "log", "args": ["--oneline", "-1"]}));
        let result = exec.execute(call).await.unwrap();
        assert!(
            !result.is_error,
            "git log should succeed: {}",
            result.output
        );
        assert!(result.output.contains("initial commit"));
    }

    #[tokio::test]
    async fn args_as_string_splits_on_whitespace() {
        let dir = tempfile::tempdir().unwrap();
        let init = std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output();
        let Ok(init_out) = init else { return };
        if !init_out.status.success() {
            return;
        }

        let exec = GitToolExecutor::new(dir.path());
        let call = make_call(serde_json::json!({"operation": "branch", "args": "-a"}));
        let result = exec.execute(call).await.unwrap();
        // In a fresh repo with no commits, branch -a may return empty or error
        // We just check it doesn't fail with our own error
        assert!(!result.output.contains("Unsupported git operation"));
    }

    #[tokio::test]
    async fn blocks_mutating_operation_on_dirty_tree_by_default() {
        let dir = tempfile::tempdir().unwrap();
        let _ = std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::fs::write(dir.path().join("dirty.txt"), "dirty").unwrap();

        let exec = GitToolExecutor::new(dir.path());
        let call = make_call(serde_json::json!({"operation": "add", "args": ["dirty.txt"]}));
        let result = exec.execute(call).await.unwrap();
        assert!(result.is_error);
        assert!(result.output.contains("dirty_tree"));
    }

    #[tokio::test]
    async fn blocks_mutating_operation_on_protected_branch() {
        let dir = tempfile::tempdir().unwrap();
        let _ = std::process::Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(dir.path())
            .output();
        let _ = std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir.path())
            .output();
        let _ = std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir.path())
            .output();
        std::fs::write(dir.path().join("test.txt"), "hello").unwrap();
        let _ = std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output();
        let _ = std::process::Command::new("git")
            .args(["commit", "-m", "initial commit"])
            .current_dir(dir.path())
            .output();

        let exec = GitToolExecutor::new(dir.path());
        let call = make_call(
            serde_json::json!({"operation": "commit", "args": ["--allow-empty", "-m", "x"], "safety": {"allow_dirty": true}}),
        );
        let result = exec.execute(call).await.unwrap();
        assert!(result.is_error);
        assert!(result.output.contains("protected_branch"));
    }

    #[tokio::test]
    async fn blocks_when_base_branch_is_out_of_sync() {
        let root = tempfile::tempdir().unwrap();
        let remote = root.path().join("remote.git");
        let repo = root.path().join("repo");
        let other = root.path().join("other");

        let _ = std::process::Command::new("git")
            .args(["init", "--bare", remote.to_str().unwrap()])
            .output()
            .unwrap();
        let _ = std::process::Command::new("git")
            .args(["clone", remote.to_str().unwrap(), repo.to_str().unwrap()])
            .output()
            .unwrap();
        let _ = std::process::Command::new("git")
            .args(["clone", remote.to_str().unwrap(), other.to_str().unwrap()])
            .output()
            .unwrap();

        for path in [&repo, &other] {
            let _ = std::process::Command::new("git")
                .args(["config", "user.email", "test@test.com"])
                .current_dir(path)
                .output();
            let _ = std::process::Command::new("git")
                .args(["config", "user.name", "Test"])
                .current_dir(path)
                .output();
        }

        std::fs::write(repo.join("base.txt"), "one").unwrap();
        let _ = std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&repo)
            .output();
        let _ = std::process::Command::new("git")
            .args(["commit", "-m", "base"])
            .current_dir(&repo)
            .output();
        let _ = std::process::Command::new("git")
            .args(["push", "origin", "HEAD:main"])
            .current_dir(&repo)
            .output();

        let _ = std::process::Command::new("git")
            .args(["checkout", "-b", "feature"])
            .current_dir(&repo)
            .output();

        let _ = std::process::Command::new("git")
            .args(["pull", "origin", "main"])
            .current_dir(&other)
            .output();
        std::fs::write(other.join("base.txt"), "two").unwrap();
        let _ = std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&other)
            .output();
        let _ = std::process::Command::new("git")
            .args(["commit", "-m", "remote update"])
            .current_dir(&other)
            .output();
        let _ = std::process::Command::new("git")
            .args(["push", "origin", "HEAD:main"])
            .current_dir(&other)
            .output();

        let exec = GitToolExecutor::new(&repo);
        let call = make_call(serde_json::json!({
            "operation": "status",
            "safety": {
                "allow_dirty": true,
                "require_base_in_sync": true,
                "base_branch": "main",
                "remote": "origin",
                "protected_branches": ["main"]
            }
        }));
        let result = exec.execute(call).await.unwrap();
        assert!(result.is_error);
        assert!(result.output.contains("base_out_of_sync"));
    }

    #[tokio::test]
    async fn repo_state_reports_dirty_and_protected_branch() {
        let dir = tempfile::tempdir().unwrap();
        let _ = std::process::Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::fs::write(dir.path().join("dirty.txt"), "dirty").unwrap();

        let exec = GitToolExecutor::new(dir.path());
        let call = make_call(serde_json::json!({"operation": "repo_state"}));
        let result = exec.execute(call).await.unwrap();
        assert!(!result.is_error, "repo_state should succeed: {}", result.output);
        assert!(result.output.contains("\"dirty\":true"));
        assert!(result.output.contains("\"current_branch\":\"main\""));
        assert!(result.output.contains("\"on_protected_branch\":true"));
    }

    #[tokio::test]
    async fn pr_open_requires_title() {
        let dir = tempfile::tempdir().unwrap();
        let exec = GitToolExecutor::new(dir.path());
        let call = make_call(serde_json::json!({"operation": "pr_open"}));
        let err = exec.execute(call).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments { .. }));
    }

}

#[cfg(test)]
mod synthetic_git_tool_tests {
    use super::*;
    use rune_core::ToolCallId;

    fn make_call(args: serde_json::Value) -> ToolCall {
        ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: "git".into(),
            arguments: args,
        }
    }

    #[tokio::test]
    async fn suggest_branch_includes_issue_slug() {
        let dir = tempfile::tempdir().unwrap();
        let exec = GitToolExecutor::new(dir.path());
        let call = make_call(serde_json::json!({
            "operation": "suggest_branch",
            "safety": {"allow_dirty": true},
            "issue": 773,
            "name": "PR status + staged validation"
        }));
        let result = exec.execute(call).await.unwrap();
        assert!(
            !result.is_error,
            "suggest_branch should succeed: {}",
            result.output
        );
        assert!(
            result
                .output
                .contains("issue-773-pr-status-staged-validation")
        );
    }


    #[tokio::test]
    async fn pr_status_reports_missing_pr_for_branch_without_pull_request() {
        let dir = tempfile::tempdir().unwrap();
        let _ = std::process::Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        let _ = std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir.path())
            .output();
        let _ = std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir.path())
            .output();
        std::fs::write(dir.path().join("test.txt"), "hello").unwrap();
        let _ = std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output();
        let _ = std::process::Command::new("git")
            .args(["commit", "-m", "initial commit"])
            .current_dir(dir.path())
            .output();
        let _ = std::process::Command::new("git")
            .args(["checkout", "-b", "feature/no-pr"])
            .current_dir(dir.path())
            .output();

        let exec = GitToolExecutor::new(dir.path());
        let call = make_call(serde_json::json!({"operation": "pr_status"}));
        let result = exec.execute(call).await.unwrap();
        assert!(result.is_error);
        assert!(result.output.contains("\"has_pr\":false"));
        assert!(result.output.contains("no pull request found for branch") || result.output.contains("failed to inspect pull request status"));
    }

    #[tokio::test]
    async fn staged_mode_single_commit_requires_files() {
        let dir = tempfile::tempdir().unwrap();
        let _ = std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let exec = GitToolExecutor::new(dir.path());
        let call = make_call(serde_json::json!({
            "operation": "staged",
            "mode": "single_commit"
        }));
        let result = exec.execute(call).await.unwrap();
        assert!(result.is_error);
        assert!(result.output.contains("single_commit validation failed"));
    }

    #[tokio::test]
    async fn staged_lists_indexed_files() {
        let dir = tempfile::tempdir().unwrap();
        let _ = std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::fs::write(dir.path().join("staged.txt"), "hello").unwrap();
        let _ = std::process::Command::new("git")
            .args(["add", "staged.txt"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let exec = GitToolExecutor::new(dir.path());
        let call =
            make_call(serde_json::json!({"operation": "staged", "safety": {"allow_dirty": true}}));
        let result = exec.execute(call).await.unwrap();
        assert!(!result.is_error, "staged should succeed: {}", result.output);
        assert!(result.output.contains("staged.txt"));
    }
}
