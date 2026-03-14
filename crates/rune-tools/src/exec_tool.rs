//! Real implementation of the `execute_command` tool.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use uuid::Uuid;

#[cfg(unix)]
const SCRIPT_PATH: &str = "/usr/bin/script";

use serde_json::json;

use async_trait::async_trait;
use tokio::process::Command;
use tracing::instrument;

use crate::definition::{ToolCall, ToolResult};
use crate::error::ToolError;
use crate::executor::ToolExecutor;
use crate::process_audit::{NewProcessAudit, ProcessAuditStore};
use crate::process_tool::ProcessManager;

/// Executor for shell command tools: `execute_command` / `exec`.
///
/// Commands are run in a configurable working directory with an optional timeout.
/// When `background: true` is set, the command is registered with the shared
/// [`ProcessManager`] and a durable `sessionId` handle is returned for use with
/// the `process` tool.
pub struct ExecToolExecutor {
    working_dir: PathBuf,
    default_timeout: Duration,
    process_manager: Option<ProcessManager>,
    audit_store: Option<Arc<dyn ProcessAuditStore>>,
}

fn build_shell_command(command_str: &str, pty: bool) -> Command {
    #[cfg(unix)]
    {
        if pty {
            let mut command = Command::new(SCRIPT_PATH);
            command.arg("-qec").arg(command_str).arg("/dev/null");
            return command;
        }
    }

    let mut command = Command::new("bash");
    command.arg("-c").arg(command_str);
    command
}

fn extract_uuid_argument(arguments: &serde_json::Value, key: &str) -> Option<Uuid> {
    arguments
        .get(key)
        .and_then(|value| value.as_str())
        .and_then(|value| Uuid::parse_str(value).ok())
}

impl ExecToolExecutor {
    /// Create a new exec tool executor.
    pub fn new(working_dir: impl Into<PathBuf>, default_timeout: Duration) -> Self {
        Self {
            working_dir: working_dir.into(),
            default_timeout,
            process_manager: None,
            audit_store: None,
        }
    }

    /// Attach a background process manager for `background: true` runs.
    #[must_use]
    pub fn with_process_manager(mut self, process_manager: ProcessManager) -> Self {
        self.process_manager = Some(process_manager);
        self
    }

    /// Attach a durable process audit store for restart-visible metadata.
    #[must_use]
    pub fn with_audit_store(mut self, audit_store: Arc<dyn ProcessAuditStore>) -> Self {
        self.audit_store = Some(audit_store);
        self
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

        let background = call
            .arguments
            .get("background")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let pty = call
            .arguments
            .get("pty")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let elevated = call
            .arguments
            .get("elevated")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let ask_mode = call
            .arguments
            .get("ask")
            .and_then(|v| v.as_str())
            .unwrap_or("on-miss");
        let security_mode = call
            .arguments
            .get("security")
            .and_then(|v| v.as_str())
            .unwrap_or("allowlist");
        let host = call
            .arguments
            .get("host")
            .and_then(|v| v.as_str())
            .unwrap_or("sandbox");

        if background {
            return self
                .spawn_background(
                    call,
                    command_str,
                    workdir,
                    pty,
                    elevated,
                    ask_mode,
                    security_mode,
                    host,
                )
                .await;
        }

        let timeout_secs = call
            .arguments
            .get("timeout")
            .and_then(|v| v.as_u64())
            .map(Duration::from_secs)
            .unwrap_or(self.default_timeout);

        let mut command = build_shell_command(command_str, pty);
        command.current_dir(&workdir);

        let result = tokio::time::timeout(timeout_secs, command.output()).await;

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

                let output = if is_error {
                    json!({
                        "stdout_stderr": combined,
                        "exitCode": output.status.code(),
                        "pty": pty,
                        "elevated": elevated,
                        "ask": ask_mode,
                        "security": security_mode,
                        "host": host,
                        "background": false,
                    })
                    .to_string()
                } else {
                    combined
                };

                Ok(ToolResult {
                    tool_call_id: call.tool_call_id,
                    output,
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

    #[allow(clippy::too_many_arguments)]
    async fn spawn_background(
        &self,
        call: &ToolCall,
        command_str: &str,
        workdir: PathBuf,
        pty: bool,
        elevated: bool,
        ask_mode: &str,
        security_mode: &str,
        host: &str,
    ) -> Result<ToolResult, ToolError> {
        let process_manager = self.process_manager.as_ref().ok_or_else(|| {
            ToolError::ExecutionFailed(
                "background execution requested but no process manager is configured".into(),
            )
        })?;

        let mut command = build_shell_command(command_str, pty);
        let mut child = command
            .current_dir(&workdir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to spawn command: {e}")))?;

        let stdin = child.stdin.take();
        let process_id = call.tool_call_id.to_string();
        process_manager
            .register(process_id.clone(), child, stdin)
            .await;

        if let Some(audit_store) = &self.audit_store {
            audit_store
                .record_spawn(NewProcessAudit {
                    process_id: process_id.clone(),
                    tool_call_id: call.tool_call_id.into_uuid(),
                    session_id: extract_uuid_argument(&call.arguments, "__session_id"),
                    turn_id: extract_uuid_argument(&call.arguments, "__turn_id"),
                    tool_name: call.tool_name.clone(),
                    command: command_str.to_string(),
                    workdir: workdir.display().to_string(),
                    arguments: call.arguments.clone(),
                    started_at: Utc::now(),
                })
                .await
                .map_err(ToolError::ExecutionFailed)?;
        }

        let output = json!({
            "sessionId": process_id,
            "background": true,
            "running": true,
            "pty": pty,
            "elevated": elevated,
            "ask": ask_mode,
            "security": security_mode,
            "host": host,
            "workdir": workdir,
            "command": command_str,
            "durable": self.audit_store.is_some(),
        })
        .to_string();

        Ok(ToolResult {
            tool_call_id: call.tool_call_id,
            output,
            is_error: false,
        })
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

    #[cfg(unix)]
    #[tokio::test]
    async fn pty_true_exposes_a_terminal_to_the_child() {
        let tmp = TempDir::new().unwrap();
        let exec = ExecToolExecutor::new(tmp.path(), Duration::from_secs(30));

        let without_pty = make_call(
            "exec",
            serde_json::json!({"command": "if [ -t 0 ]; then echo tty; else echo notty; fi"}),
        );
        let without_pty_result = exec.execute(without_pty).await.unwrap();
        assert_eq!(without_pty_result.output.trim(), "notty");

        let with_pty = make_call(
            "exec",
            serde_json::json!({"command": "if [ -t 0 ]; then echo tty; else echo notty; fi", "pty": true}),
        );
        let with_pty_result = exec.execute(with_pty).await.unwrap();
        assert!(with_pty_result.output.contains("tty"));
    }

    #[tokio::test]
    async fn background_returns_session_handle_and_process_is_visible() {
        let tmp = TempDir::new().unwrap();
        let manager = ProcessManager::new();
        let exec = ExecToolExecutor::new(tmp.path(), Duration::from_secs(30))
            .with_process_manager(manager.clone());

        let call = make_call(
            "exec",
            serde_json::json!({"command": "echo hi && sleep 1", "background": true}),
        );
        let result = exec.execute(call).await.unwrap();
        assert!(!result.is_error);

        let value: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        let session_id = value["sessionId"].as_str().unwrap();
        assert_eq!(value["background"], true);
        assert_eq!(value["running"], true);

        let infos = manager.list().await;
        assert!(infos.iter().any(|info| info.process_id == session_id));
    }

    #[tokio::test]
    async fn background_without_manager_is_rejected() {
        let tmp = TempDir::new().unwrap();
        let exec = ExecToolExecutor::new(tmp.path(), Duration::from_secs(30));

        let call = make_call(
            "exec",
            serde_json::json!({"command": "echo hi", "background": true}),
        );
        let err = exec.execute(call).await.unwrap_err();
        assert!(matches!(err, ToolError::ExecutionFailed(_)));
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
