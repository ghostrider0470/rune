//! Real implementation of the `execute_command` tool.

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use tokio::process::Command;
use tracing::instrument;

use crate::definition::{ToolCall, ToolResult};
use crate::error::ToolError;
use crate::executor::ToolExecutor;

/// Executor for shell command tools: `execute_command`.
///
/// Commands are run in a configurable working directory with an optional timeout.
pub struct ExecToolExecutor {
    working_dir: PathBuf,
    default_timeout: Duration,
}

impl ExecToolExecutor {
    /// Create a new exec tool executor.
    pub fn new(working_dir: impl Into<PathBuf>, default_timeout: Duration) -> Self {
        Self {
            working_dir: working_dir.into(),
            default_timeout,
        }
    }

    #[instrument(skip(self, call), fields(tool = %call.tool_name))]
    async fn execute_command(&self, call: &ToolCall) -> Result<ToolResult, ToolError> {
        let command_str = call
            .arguments
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolError::InvalidArgument("missing required parameter: command".into())
            })?;

        let workdir = call
            .arguments
            .get("workdir")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .unwrap_or_else(|| self.working_dir.clone());

        let timeout_secs = call
            .arguments
            .get("timeout")
            .and_then(|v| v.as_u64())
            .map(Duration::from_secs)
            .unwrap_or(self.default_timeout);

        let result = tokio::time::timeout(
            timeout_secs,
            Command::new("bash")
                .arg("-c")
                .arg(command_str)
                .current_dir(&workdir)
                .output(),
        )
        .await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                let mut combined = String::new();
                if !stdout.is_empty() {
                    combined.push_str(&stdout);
                }
                if !stderr.is_empty() {
                    if !combined.is_empty() {
                        combined.push('\n');
                    }
                    combined.push_str(&stderr);
                }

                // Truncate if very large (>50KB)
                if combined.len() > 50_000 {
                    combined.truncate(50_000);
                    combined.push_str("\n... (output truncated at 50KB)");
                }

                let is_error = !output.status.success();
                if is_error {
                    combined = format!(
                        "(exit code {})\n{combined}",
                        output.status.code().unwrap_or(-1)
                    );
                }

                Ok(ToolResult {
                    tool_call_id: call.tool_call_id,
                    output: combined,
                    is_error,
                })
            }
            Ok(Err(e)) => Err(ToolError::ExecutionFailed(format!(
                "failed to spawn command: {e}"
            ))),
            Err(_) => Ok(ToolResult {
                tool_call_id: call.tool_call_id,
                output: format!("command timed out after {}s", timeout_secs.as_secs()),
                is_error: true,
            }),
        }
    }
}

#[async_trait]
impl ToolExecutor for ExecToolExecutor {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        match call.tool_name.as_str() {
            "execute_command" | "exec" => self.execute_command(&call).await,
            other => Err(ToolError::NotFound(other.into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune_core::ToolCallId;
    use tempfile::TempDir;

    fn make_call(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: name.into(),
            arguments: args,
        }
    }

    #[tokio::test]
    async fn simple_echo() {
        let tmp = TempDir::new().unwrap();
        let exec = ExecToolExecutor::new(tmp.path(), Duration::from_secs(30));

        let call = make_call(
            "execute_command",
            serde_json::json!({"command": "echo hello"}),
        );
        let result = exec.execute(call).await.unwrap();
        assert!(!result.is_error);
        assert_eq!(result.output.trim(), "hello");
    }

    #[tokio::test]
    async fn captures_stderr() {
        let tmp = TempDir::new().unwrap();
        let exec = ExecToolExecutor::new(tmp.path(), Duration::from_secs(30));

        let call = make_call(
            "execute_command",
            serde_json::json!({"command": "echo err >&2"}),
        );
        let result = exec.execute(call).await.unwrap();
        assert!(!result.is_error);
        assert!(result.output.contains("err"));
    }

    #[tokio::test]
    async fn nonzero_exit_is_error() {
        let tmp = TempDir::new().unwrap();
        let exec = ExecToolExecutor::new(tmp.path(), Duration::from_secs(30));

        let call = make_call("execute_command", serde_json::json!({"command": "exit 42"}));
        let result = exec.execute(call).await.unwrap();
        assert!(result.is_error);
        assert!(result.output.contains("42"));
    }

    #[tokio::test]
    async fn timeout_produces_error_result() {
        let tmp = TempDir::new().unwrap();
        let exec = ExecToolExecutor::new(tmp.path(), Duration::from_secs(30));

        let call = make_call(
            "execute_command",
            serde_json::json!({"command": "sleep 60", "timeout": 1}),
        );
        let result = exec.execute(call).await.unwrap();
        assert!(result.is_error);
        assert!(result.output.contains("timed out"));
    }

    #[tokio::test]
    async fn custom_workdir() {
        let tmp = TempDir::new().unwrap();
        let exec = ExecToolExecutor::new(tmp.path(), Duration::from_secs(30));

        let call = make_call(
            "execute_command",
            serde_json::json!({"command": "pwd", "workdir": tmp.path().to_str().unwrap()}),
        );
        let result = exec.execute(call).await.unwrap();
        assert!(!result.is_error);
        assert!(result.output.trim().contains(tmp.path().to_str().unwrap()));
    }

    #[tokio::test]
    async fn missing_command_rejected() {
        let tmp = TempDir::new().unwrap();
        let exec = ExecToolExecutor::new(tmp.path(), Duration::from_secs(30));

        let call = make_call("execute_command", serde_json::json!({}));
        let err = exec.execute(call).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }
}
