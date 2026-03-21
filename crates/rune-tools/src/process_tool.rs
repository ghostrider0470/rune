//! Background process management tool.
//!
//! Implements the `process` tool: list, poll, log, write, submit, paste, send-keys, kill running
//! background processes. Works in tandem with `exec_tool` when `background: true` is set.
//!
//! PTY-backed sessions are supported through the `exec` tool's `pty: true` mode. On Unix,
//! Rune currently realizes this by launching the command under `script(1)`, which gives the
//! child a real pseudo-terminal while preserving the existing process-manager control surface.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin};
use tokio::sync::Mutex;
use tokio::time::{Duration, Instant, sleep};
use tracing::instrument;

use crate::definition::{ToolCall, ToolResult};
use crate::error::ToolError;
use crate::executor::ToolExecutor;
use crate::process_audit::{CompletedProcessAudit, ProcessAuditStore};

/// Shared process manager that tracks background processes.
#[derive(Clone)]
pub struct ProcessManager {
    entries: Arc<Mutex<HashMap<String, Arc<ProcessEntry>>>>,
    audit_store: Option<Arc<dyn ProcessAuditStore>>,
}

impl ProcessManager {
    /// Create a new empty process manager.
    pub fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
            audit_store: None,
        }
    }

    async fn has_persisted_record(&self, process_id: &str) -> Result<bool, ToolError> {
        let Some(audit_store) = &self.audit_store else {
            return Ok(false);
        };
        Ok(audit_store
            .find(process_id)
            .await
            .map_err(ToolError::ExecutionFailed)?
            .is_some())
    }

    fn detached_process_error(process_id: &str) -> ToolError {
        ToolError::ExecutionFailed(format!(
            "process {process_id} has persisted metadata but no live handle is attached in this gateway process; restart-safe stdin/control reattachment is not implemented yet"
        ))
    }

    /// Attach a durable audit store used for restart-visible process metadata.
    #[must_use]
    pub fn with_audit_store(mut self, audit_store: Arc<dyn ProcessAuditStore>) -> Self {
        self.audit_store = Some(audit_store);
        self
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
            audit_store: self.audit_store.clone(),
        });

        if let Some(reader) = stdout_handle {
            spawn_reader(reader, entry.stdout.clone());
        }
        if let Some(reader) = stderr_handle {
            spawn_reader(reader, entry.stderr.clone());
        }

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
                live: true,
                durable_status: None,
                persisted: None,
                note: None,
            });
        }
        drop(entries);

        if let Some(audit_store) = &self.audit_store {
            let recent = audit_store.list_recent(200).await.unwrap_or_default();
            for record in recent {
                if infos
                    .iter()
                    .any(|info| info.process_id == record.process_id)
                {
                    continue;
                }
                infos.push(ProcessInfo {
                    process_id: record.process_id,
                    running: record.ended_at.is_none() && record.status == "running",
                    exit_code: None,
                    live: false,
                    durable_status: Some(record.status.clone()),
                    persisted: Some(PersistedProcessInfo {
                        tool_call_id: record.tool_call_id.to_string(),
                        tool_execution_id: record.tool_execution_id.to_string(),
                        command: record.command,
                        workdir: record.workdir,
                        started_at: record.started_at.to_rfc3339(),
                        ended_at: record.ended_at.map(|t| t.to_rfc3339()),
                    }),
                    note: Some(
                        "persisted process metadata is available, but the live handle is not attached in this gateway process".to_string(),
                    ),
                });
            }
        }

        infos.sort_by(|a, b| a.process_id.cmp(&b.process_id));
        infos
    }

    /// Poll a specific process.
    pub async fn poll(&self, process_id: &str) -> Result<ProcessInfo, ToolError> {
        match self.get(process_id).await {
            Ok(entry) => {
                let state = entry.state.lock().await;
                Ok(ProcessInfo {
                    process_id: process_id.to_string(),
                    running: state.running,
                    exit_code: state.exit_code,
                    live: true,
                    durable_status: None,
                    persisted: None,
                    note: None,
                })
            }
            Err(_) => {
                if let Some(audit_store) = &self.audit_store {
                    if let Some(record) = audit_store
                        .find(process_id)
                        .await
                        .map_err(ToolError::ExecutionFailed)?
                    {
                        return Ok(ProcessInfo {
                            process_id: process_id.to_string(),
                            running: record.ended_at.is_none() && record.status == "running",
                            exit_code: None,
                            live: false,
                            durable_status: Some(record.status.clone()),
                            persisted: Some(PersistedProcessInfo {
                                tool_call_id: record.tool_call_id.to_string(),
                                tool_execution_id: record.tool_execution_id.to_string(),
                                command: record.command,
                                workdir: record.workdir,
                                started_at: record.started_at.to_rfc3339(),
                                ended_at: record.ended_at.map(|t| t.to_rfc3339()),
                            }),
                            note: Some(
                                "persisted process metadata is available, but the live handle is not attached in this gateway process".to_string(),
                            ),
                        });
                    }
                }
                Err(ToolError::ExecutionFailed(format!(
                    "process not found: {process_id}"
                )))
            }
        }
    }

    /// Poll a process, waiting up to `timeout` for it to change from running to finished.
    pub async fn poll_wait(
        &self,
        process_id: &str,
        timeout: Duration,
    ) -> Result<ProcessInfo, ToolError> {
        let deadline = Instant::now() + timeout;
        loop {
            let info = self.poll(process_id).await?;
            if !info.running || Instant::now() >= deadline {
                return Ok(info);
            }
            sleep(Duration::from_millis(100)).await;
        }
    }

    /// Get combined stdout+stderr log with optional offset/limit.
    pub async fn log(
        &self,
        process_id: &str,
        offset: Option<usize>,
        limit: Option<usize>,
    ) -> Result<String, ToolError> {
        if let Ok(entry) = self.get(process_id).await {
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
            return Ok(output);
        }

        if let Some(audit_store) = &self.audit_store {
            if let Some(record) = audit_store
                .find(process_id)
                .await
                .map_err(ToolError::ExecutionFailed)?
            {
                let payload = serde_json::json!({
                    "processId": record.process_id,
                    "live": false,
                    "status": record.status,
                    "startedAt": record.started_at.to_rfc3339(),
                    "endedAt": record.ended_at.map(|t| t.to_rfc3339()),
                    "command": record.command,
                    "workdir": record.workdir,
                    "resultSummary": record.result_summary,
                    "errorSummary": record.error_summary,
                    "note": "Only persisted metadata is available after restart; live stdout/stderr capture is not reattachable yet"
                })
                .to_string();
                let start = offset.unwrap_or(0);
                let output: String = payload
                    .chars()
                    .skip(start)
                    .take(limit.unwrap_or(usize::MAX))
                    .collect();
                return Ok(output);
            }
        }

        Err(ToolError::ExecutionFailed(format!(
            "process not found: {process_id}"
        )))
    }

    /// Write data to a process's stdin.
    pub async fn write_stdin(
        &self,
        process_id: &str,
        data: &str,
        eof: bool,
    ) -> Result<usize, ToolError> {
        let entry = match self.get(process_id).await {
            Ok(entry) => entry,
            Err(err) => {
                if self.has_persisted_record(process_id).await? {
                    return Err(Self::detached_process_error(process_id));
                }
                return Err(err);
            }
        };
        let mut stdin_guard = entry.stdin.lock().await;
        let stdin = stdin_guard.as_mut().ok_or_else(|| {
            ToolError::ExecutionFailed(format!("stdin not available for process {process_id}"))
        })?;
        stdin
            .write_all(data.as_bytes())
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to write to stdin: {e}")))?;
        stdin
            .flush()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to flush stdin: {e}")))?;
        if eof {
            *stdin_guard = None;
        }
        Ok(data.len())
    }

    /// Close stdin for a process without writing more data.
    pub async fn close_stdin(&self, process_id: &str) -> Result<(), ToolError> {
        let entry = match self.get(process_id).await {
            Ok(entry) => entry,
            Err(err) => {
                if self.has_persisted_record(process_id).await? {
                    return Err(Self::detached_process_error(process_id));
                }
                return Err(err);
            }
        };
        let mut stdin_guard = entry.stdin.lock().await;
        if stdin_guard.is_none() {
            return Err(ToolError::ExecutionFailed(format!(
                "stdin not available for process {process_id}"
            )));
        }
        *stdin_guard = None;
        Ok(())
    }

    /// Kill a process.
    pub async fn kill(&self, process_id: &str) -> Result<(), ToolError> {
        let entry = match self.get(process_id).await {
            Ok(entry) => entry,
            Err(err) => {
                if self.has_persisted_record(process_id).await? {
                    return Err(Self::detached_process_error(process_id));
                }
                return Err(err);
            }
        };
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
    /// Whether the current gateway process still has a live handle attached.
    pub live: bool,
    /// Durable status recovered from persisted metadata when available.
    pub durable_status: Option<String>,
    /// Persisted metadata recovered from durable process audit storage.
    pub persisted: Option<PersistedProcessInfo>,
    /// Human-readable note about degraded restart visibility.
    pub note: Option<String>,
}

/// Restart-visible process metadata recovered from durable process audit storage.
#[derive(Debug, Clone)]
pub struct PersistedProcessInfo {
    /// Tool-call ID correlated to this process launch.
    pub tool_call_id: String,
    /// Durable tool-execution ID correlated to this process launch.
    pub tool_execution_id: String,
    /// Original launched command.
    pub command: String,
    /// Working directory used for launch.
    pub workdir: String,
    /// Durable start timestamp.
    pub started_at: String,
    /// Durable completion timestamp, when available.
    pub ended_at: Option<String>,
}

struct ProcessEntry {
    #[allow(dead_code)]
    id: String,
    child: Mutex<Child>,
    stdin: Mutex<Option<ChildStdin>>,
    stdout: Arc<Mutex<String>>,
    stderr: Arc<Mutex<String>>,
    state: Arc<Mutex<ProcessState>>,
    audit_store: Option<Arc<dyn ProcessAuditStore>>,
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
        let exit_code = status.and_then(|s| s.code());
        {
            let mut state = entry.state.lock().await;
            state.running = false;
            state.exit_code = exit_code;
        }

        if let Some(audit_store) = &entry.audit_store {
            let stdout = entry.stdout.lock().await.clone();
            let stderr = entry.stderr.lock().await.clone();
            let summary = if stderr.is_empty() {
                stdout
            } else if stdout.is_empty() {
                stderr
            } else {
                format!("{stdout}\n{stderr}")
            };
            let summary = truncate_summary(&summary);
            let status_text = match exit_code {
                Some(0) => "completed",
                Some(_) => "failed",
                None => "killed",
            };
            let _ = audit_store
                .record_completion(CompletedProcessAudit {
                    process_id: entry.id.clone(),
                    status: status_text.to_string(),
                    result_summary: if status_text == "completed" {
                        Some(summary.clone())
                    } else {
                        None
                    },
                    error_summary: if matches!(status_text, "failed" | "killed") {
                        Some(summary)
                    } else {
                        None
                    },
                    ended_at: Utc::now(),
                })
                .await;
        }
    });
}

fn truncate_summary(value: &str) -> String {
    const MAX: usize = 4_000;
    if value.len() <= MAX {
        return value.to_string();
    }
    let mut out = value[..MAX].to_string();
    out.push_str("\n... (truncated)");
    out
}

fn key_token_to_bytes(token: &str) -> Option<&'static [u8]> {
    match token {
        "enter" => Some(b"\n"),
        "return" => Some(b"\n"),
        "tab" => Some(b"\t"),
        "ctrl-c" => Some(&[0x03]),
        "ctrl-d" => Some(&[0x04]),
        "esc" => Some(&[0x1b]),
        _ => None,
    }
}

fn parse_hex_byte(token: &str) -> Result<u8, ToolError> {
    let trimmed = token.trim();
    let normalized = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    u8::from_str_radix(normalized, 16)
        .map_err(|_| ToolError::InvalidArgument(format!("invalid send-keys hex byte: {token}")))
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
                            "live": i.live,
                            "durableStatus": i.durable_status,
                            "persisted": i.persisted.as_ref().map(|p| serde_json::json!({
                                "toolCallId": p.tool_call_id,
                                "toolExecutionId": p.tool_execution_id,
                                "command": p.command,
                                "workdir": p.workdir,
                                "startedAt": p.started_at,
                                "endedAt": p.ended_at,
                            })),
                            "note": i.note,
                        })
                    })
                    .collect();
                Ok(ToolResult {
                    tool_call_id: call.tool_call_id.clone(),
                    output: serde_json::to_string_pretty(&json).unwrap_or_default(),
                    is_error: false,
                    tool_execution_id: None,
                })
            }
            "poll" => {
                let pid = process_id
                    .ok_or_else(|| ToolError::InvalidArgument("poll requires sessionId".into()))?;
                let timeout = call
                    .arguments
                    .get("timeout")
                    .and_then(|v| v.as_u64())
                    .map(Duration::from_millis)
                    .unwrap_or_else(|| Duration::from_millis(0));
                let info = if timeout.is_zero() {
                    self.manager.poll(pid).await?
                } else {
                    self.manager.poll_wait(pid, timeout).await?
                };
                Ok(ToolResult {
                    tool_call_id: call.tool_call_id.clone(),
                    output: serde_json::json!({
                        "processId": info.process_id,
                        "running": info.running,
                        "exitCode": info.exit_code,
                        "live": info.live,
                        "durableStatus": info.durable_status,
                        "persisted": info.persisted.as_ref().map(|p| serde_json::json!({
                            "toolCallId": p.tool_call_id,
                            "toolExecutionId": p.tool_execution_id,
                            "command": p.command,
                            "workdir": p.workdir,
                            "startedAt": p.started_at,
                            "endedAt": p.ended_at,
                        })),
                        "note": info.note,
                    })
                    .to_string(),
                    is_error: false,
                    tool_execution_id: None,
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
                    tool_call_id: call.tool_call_id.clone(),
                    output,
                    is_error: false,
                    tool_execution_id: None,
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
                let eof = call
                    .arguments
                    .get("eof")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let written = self.manager.write_stdin(pid, data, eof).await?;
                Ok(ToolResult {
                    tool_call_id: call.tool_call_id.clone(),
                    output: format!("wrote {written} bytes"),
                    is_error: false,
                    tool_execution_id: None,
                })
            }
            "submit" => {
                let pid = process_id.ok_or_else(|| {
                    ToolError::InvalidArgument("submit requires sessionId".into())
                })?;
                let data = call
                    .arguments
                    .get("data")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let payload = format!("{data}\n");
                let written = self.manager.write_stdin(pid, &payload, false).await?;
                Ok(ToolResult {
                    tool_call_id: call.tool_call_id.clone(),
                    output: format!("submitted {written} bytes"),
                    is_error: false,
                    tool_execution_id: None,
                })
            }
            "paste" => {
                let pid = process_id
                    .ok_or_else(|| ToolError::InvalidArgument("paste requires sessionId".into()))?;
                let text = call
                    .arguments
                    .get("text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ToolError::InvalidArgument("paste requires text parameter".into())
                    })?;
                let bracketed = call
                    .arguments
                    .get("bracketed")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let payload = if bracketed {
                    format!("\u{1b}[200~{text}\u{1b}[201~")
                } else {
                    text.to_string()
                };
                let written = self.manager.write_stdin(pid, &payload, false).await?;
                Ok(ToolResult {
                    tool_call_id: call.tool_call_id.clone(),
                    output: format!("pasted {written} bytes"),
                    is_error: false,
                    tool_execution_id: None,
                })
            }
            "send-keys" => {
                let pid = process_id.ok_or_else(|| {
                    ToolError::InvalidArgument("send-keys requires sessionId".into())
                })?;
                let mut payload = Vec::<u8>::new();
                if let Some(literal) = call.arguments.get("literal").and_then(|v| v.as_str()) {
                    payload.extend_from_slice(literal.as_bytes());
                }
                if let Some(keys) = call.arguments.get("keys").and_then(|v| v.as_array()) {
                    for key in keys {
                        let token = key.as_str().ok_or_else(|| {
                            ToolError::InvalidArgument(
                                "send-keys keys entries must be strings".into(),
                            )
                        })?;
                        let bytes = key_token_to_bytes(token).ok_or_else(|| {
                            ToolError::InvalidArgument(format!(
                                "unsupported send-keys token: {token}"
                            ))
                        })?;
                        payload.extend_from_slice(bytes);
                    }
                }
                if let Some(hex_values) = call.arguments.get("hex").and_then(|v| v.as_array()) {
                    for value in hex_values {
                        let token = value.as_str().ok_or_else(|| {
                            ToolError::InvalidArgument(
                                "send-keys hex entries must be strings".into(),
                            )
                        })?;
                        payload.push(parse_hex_byte(token)?);
                    }
                }
                if payload.is_empty() {
                    return Err(ToolError::InvalidArgument(
                        "send-keys requires keys, hex, or literal".into(),
                    ));
                }
                let text = String::from_utf8_lossy(&payload).to_string();
                let written = self.manager.write_stdin(pid, &text, false).await?;
                if call
                    .arguments
                    .get("eof")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    self.manager.close_stdin(pid).await?;
                }
                Ok(ToolResult {
                    tool_call_id: call.tool_call_id.clone(),
                    output: format!("sent {written} bytes"),
                    is_error: false,
                    tool_execution_id: None,
                })
            }
            "kill" => {
                let pid = process_id
                    .ok_or_else(|| ToolError::InvalidArgument("kill requires sessionId".into()))?;
                self.manager.kill(pid).await?;
                Ok(ToolResult {
                    tool_call_id: call.tool_call_id.clone(),
                    output: format!("killed process {pid}"),
                    is_error: false,
                    tool_execution_id: None,
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
    use crate::process_audit::{NewProcessAudit, ProcessAuditRecord};
    use async_trait::async_trait;
    use rune_core::ToolCallId;
    use std::process::Stdio;
    use tokio::process::Command;
    use tokio::sync::Mutex;
    use uuid::Uuid;

    fn make_call(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: name.into(),
            arguments: args,
        }
    }

    #[derive(Default)]
    struct MemProcessAuditStore {
        records: Mutex<Vec<ProcessAuditRecord>>,
    }

    #[async_trait]
    impl ProcessAuditStore for MemProcessAuditStore {
        async fn record_spawn(&self, spawn: NewProcessAudit) -> Result<ProcessAuditRecord, String> {
            let record = ProcessAuditRecord {
                process_id: spawn.process_id,
                tool_call_id: spawn.tool_call_id,
                tool_execution_id: Uuid::now_v7(),
                session_id: spawn.session_id,
                turn_id: spawn.turn_id,
                tool_name: spawn.tool_name,
                command: spawn.command,
                workdir: spawn.workdir,
                arguments: spawn.arguments,
                status: "running".to_string(),
                result_summary: None,
                error_summary: None,
                started_at: spawn.started_at,
                ended_at: None,
            };
            self.records.lock().await.push(record.clone());
            Ok(record)
        }

        async fn record_completion(
            &self,
            completion: CompletedProcessAudit,
        ) -> Result<ProcessAuditRecord, String> {
            let mut records = self.records.lock().await;
            let record = records
                .iter_mut()
                .find(|record| record.process_id == completion.process_id)
                .ok_or_else(|| format!("missing process {}", completion.process_id))?;
            record.status = completion.status;
            record.result_summary = completion.result_summary;
            record.error_summary = completion.error_summary;
            record.ended_at = Some(completion.ended_at);
            Ok(record.clone())
        }

        async fn find(&self, process_id: &str) -> Result<Option<ProcessAuditRecord>, String> {
            Ok(self
                .records
                .lock()
                .await
                .iter()
                .find(|record| record.process_id == process_id)
                .cloned())
        }

        async fn list_recent(&self, limit: usize) -> Result<Vec<ProcessAuditRecord>, String> {
            Ok(self
                .records
                .lock()
                .await
                .iter()
                .rev()
                .take(limit)
                .cloned()
                .collect())
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
    async fn poll_wait_returns_after_process_exits() {
        let mgr = ProcessManager::new();

        let mut child = Command::new("bash")
            .arg("-c")
            .arg("sleep 0.2")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();

        let stdin = child.stdin.take();
        mgr.register("test-poll-wait".into(), child, stdin).await;

        let started = Instant::now();
        let info = mgr
            .poll_wait("test-poll-wait", Duration::from_millis(1000))
            .await
            .unwrap();
        let elapsed = started.elapsed();

        assert!(!info.running);
        assert!(elapsed >= Duration::from_millis(150));
    }

    #[tokio::test]
    async fn process_poll_action_honors_timeout() {
        let mgr = ProcessManager::new();
        let executor = ProcessToolExecutor::new(mgr.clone());

        let mut child = Command::new("bash")
            .arg("-c")
            .arg("sleep 0.2")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();

        let stdin = child.stdin.take();
        mgr.register("test-process-poll".into(), child, stdin).await;

        let started = Instant::now();
        let result = executor
            .execute(make_call(
                "process",
                serde_json::json!({
                    "action": "poll",
                    "sessionId": "test-process-poll",
                    "timeout": 1000
                }),
            ))
            .await
            .unwrap();
        let elapsed = started.elapsed();
        let value: serde_json::Value = serde_json::from_str(&result.output).unwrap();

        assert_eq!(value["processId"], "test-process-poll");
        assert_eq!(value["running"], false);
        assert!(elapsed >= Duration::from_millis(150));
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

    #[tokio::test]
    async fn kill_marks_process_non_running() {
        let mgr = ProcessManager::new();

        let mut child = Command::new("sleep")
            .arg("10")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();

        let stdin = child.stdin.take();
        mgr.register("kill-test".into(), child, stdin).await;
        mgr.kill("kill-test").await.unwrap();

        let info = mgr
            .poll_wait("kill-test", Duration::from_millis(1000))
            .await
            .unwrap();
        assert!(!info.running);
    }

    #[tokio::test]
    async fn submit_and_paste_write_to_stdin() {
        let mgr = ProcessManager::new();
        let exec = ProcessToolExecutor::new(mgr.clone());

        let mut child = Command::new("bash")
            .args(["-c", "cat"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();

        let stdin = child.stdin.take();
        mgr.register("cat-1".into(), child, stdin).await;

        exec.execute(make_call(
            "process",
            serde_json::json!({"action": "submit", "sessionId": "cat-1", "data": "hello"}),
        ))
        .await
        .unwrap();

        exec.execute(make_call(
            "process",
            serde_json::json!({"action": "paste", "sessionId": "cat-1", "text": "world", "bracketed": false}),
        ))
        .await
        .unwrap();

        exec.execute(make_call(
            "process",
            serde_json::json!({"action": "write", "sessionId": "cat-1", "data": "!", "eof": true}),
        ))
        .await
        .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let log = mgr.log("cat-1", None, None).await.unwrap();
        assert!(log.contains("hello\nworld!"));
    }

    #[tokio::test]
    async fn send_keys_supports_literal_and_enter() {
        let mgr = ProcessManager::new();
        let exec = ProcessToolExecutor::new(mgr.clone());

        let mut child = Command::new("bash")
            .args(["-c", "cat"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();

        let stdin = child.stdin.take();
        mgr.register("cat-keys".into(), child, stdin).await;

        exec.execute(make_call(
            "process",
            serde_json::json!({
                "action": "send-keys",
                "sessionId": "cat-keys",
                "literal": "abc",
                "keys": ["enter"],
                "eof": true
            }),
        ))
        .await
        .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let log = mgr.log("cat-keys", None, None).await.unwrap();
        assert!(log.contains("abc\n"));
    }

    #[tokio::test]
    async fn send_keys_supports_hex_bytes() {
        let mgr = ProcessManager::new();
        let exec = ProcessToolExecutor::new(mgr.clone());

        let mut child = Command::new("bash")
            .args(["-c", "cat"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();

        let stdin = child.stdin.take();
        mgr.register("cat-hex".into(), child, stdin).await;

        exec.execute(make_call(
            "process",
            serde_json::json!({
                "action": "send-keys",
                "sessionId": "cat-hex",
                "hex": ["41", "42", "0x43", "0a"],
                "eof": true
            }),
        ))
        .await
        .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let log = mgr.log("cat-hex", None, None).await.unwrap();
        assert!(log.contains("ABC\n"));
    }

    #[tokio::test]
    async fn write_to_detached_persisted_process_is_explicit() {
        let audit_store = Arc::new(MemProcessAuditStore::default());
        let mgr = ProcessManager::new().with_audit_store(audit_store.clone());

        audit_store
            .record_spawn(NewProcessAudit {
                process_id: "persisted-only".into(),
                tool_call_id: Uuid::now_v7(),
                session_id: None,
                turn_id: None,
                tool_name: "execute_command".into(),
                command: "sleep 5".into(),
                workdir: "/tmp".into(),
                arguments: serde_json::json!({"background": true}),
                started_at: Utc::now(),
            })
            .await
            .unwrap();

        let err = mgr
            .write_stdin("persisted-only", "hello", false)
            .await
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("persisted metadata but no live handle is attached")
        );
    }

    #[tokio::test]
    async fn kill_detached_persisted_process_is_explicit() {
        let audit_store = Arc::new(MemProcessAuditStore::default());
        let mgr = ProcessManager::new().with_audit_store(audit_store.clone());

        audit_store
            .record_spawn(NewProcessAudit {
                process_id: "persisted-kill".into(),
                tool_call_id: Uuid::now_v7(),
                session_id: None,
                turn_id: None,
                tool_name: "execute_command".into(),
                command: "sleep 5".into(),
                workdir: "/tmp".into(),
                arguments: serde_json::json!({"background": true}),
                started_at: Utc::now(),
            })
            .await
            .unwrap();

        let err = mgr.kill("persisted-kill").await.unwrap_err();
        assert!(
            err.to_string()
                .contains("persisted metadata but no live handle is attached")
        );
    }

    #[tokio::test]
    async fn poll_detached_process_includes_persisted_metadata() {
        let audit_store = Arc::new(MemProcessAuditStore::default());
        let mgr = ProcessManager::new().with_audit_store(audit_store.clone());
        let exec = ProcessToolExecutor::new(mgr);

        audit_store
            .record_spawn(NewProcessAudit {
                process_id: "persisted-poll".into(),
                tool_call_id: Uuid::now_v7(),
                session_id: None,
                turn_id: None,
                tool_name: "execute_command".into(),
                command: "sleep 5".into(),
                workdir: "/tmp".into(),
                arguments: serde_json::json!({"background": true}),
                started_at: Utc::now(),
            })
            .await
            .unwrap();

        let result = exec
            .execute(make_call(
                "process",
                serde_json::json!({"action": "poll", "sessionId": "persisted-poll"}),
            ))
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&result.output).unwrap();

        assert_eq!(value["live"], false);
        assert_eq!(value["durableStatus"], "running");
        assert_eq!(value["persisted"]["command"], "sleep 5");
        assert_eq!(value["persisted"]["workdir"], "/tmp");
        assert!(value["persisted"]["toolCallId"].as_str().is_some());
        assert!(value["persisted"]["toolExecutionId"].as_str().is_some());
        assert!(value["persisted"]["startedAt"].as_str().is_some());
    }

    #[tokio::test]
    async fn list_includes_persisted_metadata_for_detached_processes() {
        let audit_store = Arc::new(MemProcessAuditStore::default());
        let mgr = ProcessManager::new().with_audit_store(audit_store.clone());
        let exec = ProcessToolExecutor::new(mgr);

        audit_store
            .record_spawn(NewProcessAudit {
                process_id: "persisted-list".into(),
                tool_call_id: Uuid::now_v7(),
                session_id: None,
                turn_id: None,
                tool_name: "execute_command".into(),
                command: "echo hi".into(),
                workdir: "/tmp".into(),
                arguments: serde_json::json!({"background": true}),
                started_at: Utc::now(),
            })
            .await
            .unwrap();

        let result = exec
            .execute(make_call("process", serde_json::json!({"action": "list"})))
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        let item = value
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["processId"] == "persisted-list")
            .unwrap();

        assert_eq!(item["live"], false);
        assert_eq!(item["persisted"]["command"], "echo hi");
        assert_eq!(item["persisted"]["workdir"], "/tmp");
        assert!(item["persisted"]["toolCallId"].as_str().is_some());
        assert!(item["persisted"]["toolExecutionId"].as_str().is_some());
    }
}
