//! Git tool for agents to perform version control operations via subprocess.

use std::path::PathBuf;
use std::process::Stdio;

use async_trait::async_trait;
use tracing::instrument;

use crate::definition::{ToolCall, ToolDefinition, ToolResult};
use crate::error::ToolError;
use crate::executor::ToolExecutor;

/// Allowed git subcommands. Anything outside this list is rejected.
const ALLOWED_OPERATIONS: &[&str] = &[
    "status", "diff", "add", "commit", "push", "pull", "log", "branch", "checkout", "merge",
];

/// Maximum output bytes returned from a git command (50 KB).
const MAX_OUTPUT_BYTES: usize = 50 * 1024;

/// Default timeout for git operations (60 seconds).
const DEFAULT_TIMEOUT_SECS: u64 = 60;

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
                tool_call_id: call.tool_call_id,
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
                } else if let Some(s) = v.as_str() {
                    // Split a single string on whitespace for convenience
                    Some(s.split_whitespace().map(String::from).collect())
                } else {
                    None
                }
            })
            .unwrap_or_default();

        // Validate workspace root exists
        let workspace_root = self
            .workspace_root
            .canonicalize()
            .map_err(|e| ToolError::ExecutionFailed(format!("workspace root invalid: {e}")))?;

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
            tool_call_id: call.tool_call_id,
            output: result_text,
            is_error,
            tool_execution_id: None,
        })
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
        description: "Execute git operations in the workspace. Supports: status, diff, add, commit, push, pull, log, branch, checkout, merge.".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "description": "Git subcommand to execute",
                    "enum": ["status", "diff", "add", "commit", "push", "pull", "log", "branch", "checkout", "merge"]
                },
                "args": {
                    "description": "Additional arguments for the git command. Can be a JSON array of strings or a single string (split on whitespace).",
                    "oneOf": [
                        { "type": "array", "items": { "type": "string" } },
                        { "type": "string" }
                    ]
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
        assert!(!result.is_error, "git status should succeed: {}", result.output);
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
        assert!(!result.is_error, "git log should succeed: {}", result.output);
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
}
