//! Background process management tool.
//!
//! Implements the `process` tool: list, poll, log, write, kill running background processes.
//! Works in tandem with `exec_tool` when `background: true` is set.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin};
use tokio::sync::Mutex;
use tracing::instrument;

use crate::definition::{ToolCall, ToolResult};
use crate::error::ToolError;
use crate::executor::ToolExecutor;

/// Shared process manager that tracks background processes.
#[derive(Clone)]
pub struct ProcessManager {
    entries: Arc<Mutex<HashMap<String, Arc<ProcessEntry>>>>,
}

impl ProcessManager {
    /// Create a new empty process manager.
    pub fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register a new background process.
    pub async fn register(&self, process_id: String, mut child: Child, stdin: Option<ChildStdin>) {
        let stdout_handle = child.stdout.take();
        let stderr_handle = child.stderr.take();

        let entry = Arc::new(ProcessEntry {
            id: process_id.clone(),
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            stdout: Arc::new(Mutex::new(String::new())),
            stderr: Arc::new(Mutex::new(String::new())),
            state: Arc::new(Mutex::new(ProcessState {
                running: true,
                exit_code: None,
            })),
        });

        // Spawn readers for stdout/stderr
        if let Some(reader) = stdout_handle {
            spawn_reader(reader, entry.stdout.clone());
        }
        if let Some(reader) = stderr_handle {
            spawn_reader(reader, entry.stderr.clone());
        }

        // Spawn waiter for exit status
        spawn_waiter(entry.clone());

        self.entries.lock().await.insert(process_id, entry);
    }

    /// List all tracked processes.
    pub async fn list(&self) -> Vec<ProcessInfo> {
        let entries = self.entries.lock().await;
        let mut infos = Vec::new();
        for (id, entry) in &*entries {
            let state = entry.state.lock().await;
            infos.push(ProcessInfo {
                process_id: id.clone(),
                running: state.running,
                exit_code: state.exit_code,
            });
        }
        infos.sort_by(|a, b| a.process_id.cmp(&b.process_id));
        infos
    }

    /// Poll a specific process.
    pub async fn poll(&self, process_id: &str) -> Result<ProcessInfo, ToolError> {
        let entry = self.get(process_id).await?;
        let state = entry.state.lock().await;
        Ok(ProcessInfo {
            process_id: process_id.to_string(),
            running: state.running,
            exit_code: state.exit_code,
        })
    }

    /// Get combined stdout+stderr log with optional offset/limit.
    pub async fn log(
        &self,
        process_id: &str,
        offset: Option<usize>,
        limit: Option<usize>,
    ) -> Result<String, ToolError> {
        let entry = self.get(process_id).await?;
        let stdout = entry.stdout.lock().await.clone();
        let stderr = entry.stderr.lock().await.clone();

        let combined = if stderr.is_empty() {
            stdout
        } else if stdout.is_empty() {
            stderr
        } else {
            format!("{stdout}\n{stderr}")
        };

        let start = offset.unwrap_or(0);
        let output: String = combined
            .chars()
            .skip(start)
            .take(limit.unwrap_or(usize::MAX))
            .collect();
        Ok(output)
    }

    /// Write data to a process's stdin.
    pub async fn write_stdin(&self, process_id: &str, data: &str) -> Result<usize, ToolError> {
        let entry = self.get(process_id).await?;
        let mut stdin_guard = entry.stdin.lock().await;
        let stdin = stdin_guard.as_mut().ok_or_else(|| {
            ToolError::ExecutionFailed(format!("stdin not available for process {process_id}"))
        })?;
        stdin
            .write_all(data.as_bytes())
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to write to stdin: {e}")))?;
        Ok(data.len())
    }

    /// Kill a process.
    pub async fn kill(&self, process_id: &str) -> Result<(), ToolError> {
        let entry = self.get(process_id).await?;
        let mut child = entry.child.lock().await;
        child.kill().await.map_err(|e| {
            ToolError::ExecutionFailed(format!("failed to kill process {process_id}: {e}"))
        })?;
        let mut state = entry.state.lock().await;
        state.running = false;
        Ok(())
    }

    async fn get(&self, process_id: &str) -> Result<Arc<ProcessEntry>, ToolError> {
        self.entries
            .lock()
            .await
            .get(process_id)
            .cloned()
            .ok_or_else(|| ToolError::ExecutionFailed(format!("process not found: {process_id}")))
    }
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Summary info about a tracked process.
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    /// Process identifier.
    pub process_id: String,
    /// Whether the process is still running.
    pub running: bool,
    /// Exit code (None if still running or killed).
    pub exit_code: Option<i32>,
}

struct ProcessEntry {
    #[allow(dead_code)]
    id: String,
    child: Mutex<Child>,
    stdin: Mutex<Option<ChildStdin>>,
    stdout: Arc<Mutex<String>>,
    stderr: Arc<Mutex<String>>,
    state: Arc<Mutex<ProcessState>>,
}

struct ProcessState {
    running: bool,
    exit_code: Option<i32>,
}

fn spawn_reader<R>(mut reader: R, buffer: Arc<Mutex<String>>)
where
    R: AsyncReadExt + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut bytes = Vec::new();
        if reader.read_to_end(&mut bytes).await.is_ok() {
            let mut target = buffer.lock().await;
            target.push_str(&String::from_utf8_lossy(&bytes));
        }
    });
}

fn spawn_waiter(entry: Arc<ProcessEntry>) {
    tokio::spawn(async move {
        let status = {
            let mut child = entry.child.lock().await;
            child.wait().await.ok()
        };
        let mut state = entry.state.lock().await;
        state.running = false;
        state.exit_code = status.and_then(|s| s.code());
    });
}

/// Tool executor that handles `process` tool calls.
pub struct ProcessToolExecutor {
    manager: ProcessManager,
}

impl ProcessToolExecutor {
    /// Create a new process tool executor with the given manager.
    pub fn new(manager: ProcessManager) -> Self {
        Self { manager }
    }

    #[instrument(skip(self, call), fields(tool = "process"))]
    async fn handle_process(&self, call: &ToolCall) -> Result<ToolResult, ToolError> {
        let action = call
            .arguments
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolError::InvalidArgument("missing required parameter: action".into())
            })?;

        let process_id = call
            .arguments
            .get("sessionId")
            .or_else(|| call.arguments.get("processId"))
            .and_then(|v| v.as_str());

        match action {
            "list" => {
                let infos = self.manager.list().await;
                let json: Vec<serde_json::Value> = infos
                    .iter()
                    .map(|i| {
                        serde_json::json!({
                            "processId": i.process_id,
                            "running": i.running,
                            "exitCode": i.exit_code,
                        })
                    })
                    .collect();
                Ok(ToolResult {
                    tool_call_id: call.tool_call_id,
                    output: serde_json::to_string_pretty(&json).unwrap_or_default(),
                    is_error: false,
                })
            }
            "poll" => {
                let pid = process_id
                    .ok_or_else(|| ToolError::InvalidArgument("poll requires sessionId".into()))?;
                let info = self.manager.poll(pid).await?;
                Ok(ToolResult {
                    tool_call_id: call.tool_call_id,
                    output: serde_json::json!({
                        "processId": info.process_id,
                        "running": info.running,
                        "exitCode": info.exit_code,
                    })
                    .to_string(),
                    is_error: false,
                })
            }
            "log" => {
                let pid = process_id
                    .ok_or_else(|| ToolError::InvalidArgument("log requires sessionId".into()))?;
                let offset = call
                    .arguments
                    .get("offset")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize);
                let limit = call
                    .arguments
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize);
                let output = self.manager.log(pid, offset, limit).await?;
                Ok(ToolResult {
                    tool_call_id: call.tool_call_id,
                    output,
                    is_error: false,
                })
            }
            "write" => {
                let pid = process_id
                    .ok_or_else(|| ToolError::InvalidArgument("write requires sessionId".into()))?;
                let data = call
                    .arguments
                    .get("data")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ToolError::InvalidArgument("write requires data parameter".into())
                    })?;
                let written = self.manager.write_stdin(pid, data).await?;
                Ok(ToolResult {
                    tool_call_id: call.tool_call_id,
                    output: format!("wrote {written} bytes"),
                    is_error: false,
                })
            }
            "kill" => {
                let pid = process_id
                    .ok_or_else(|| ToolError::InvalidArgument("kill requires sessionId".into()))?;
                self.manager.kill(pid).await?;
                Ok(ToolResult {
                    tool_call_id: call.tool_call_id,
                    output: format!("killed process {pid}"),
                    is_error: false,
                })
            }
            other => Err(ToolError::InvalidArgument(format!(
                "unknown process action: {other}"
            ))),
        }
    }
}

#[async_trait]
impl ToolExecutor for ProcessToolExecutor {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        match call.tool_name.as_str() {
            "process" => self.handle_process(&call).await,
            other => Err(ToolError::NotFound(other.into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune_core::ToolCallId;
    use std::process::Stdio;
    use tokio::process::Command;

    fn make_call(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: name.into(),
            arguments: args,
        }
    }

    #[tokio::test]
    async fn register_and_list() {
        let mgr = ProcessManager::new();

        let mut child = Command::new("sleep")
            .arg("10")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();

        let stdin = child.stdin.take();
        mgr.register("test-1".into(), child, stdin).await;

        let infos = mgr.list().await;
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].process_id, "test-1");
        assert!(infos[0].running);

        mgr.kill("test-1").await.unwrap();
    }

    #[tokio::test]
    async fn spawn_echo_and_read_log() {
        let mgr = ProcessManager::new();

        let mut child = Command::new("bash")
            .args(["-c", "echo hello-from-process"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();

        let stdin = child.stdin.take();
        mgr.register("echo-1".into(), child, stdin).await;

        // Wait for process to finish
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let info = mgr.poll("echo-1").await.unwrap();
        assert!(!info.running);
        assert_eq!(info.exit_code, Some(0));

        let log = mgr.log("echo-1", None, None).await.unwrap();
        assert!(log.contains("hello-from-process"));
    }

    #[tokio::test]
    async fn process_tool_list_action() {
        let mgr = ProcessManager::new();
        let exec = ProcessToolExecutor::new(mgr);

        let call = make_call("process", serde_json::json!({"action": "list"}));
        let result = exec.execute(call).await.unwrap();
        assert!(!result.is_error);
        assert!(result.output.contains("[]"));
    }

    #[tokio::test]
    async fn process_tool_unknown_action_rejected() {
        let mgr = ProcessManager::new();
        let exec = ProcessToolExecutor::new(mgr);

        let call = make_call("process", serde_json::json!({"action": "dance"}));
        let err = exec.execute(call).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }
}
