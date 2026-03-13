use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use rune_core::ToolCategory;
use serde::Deserialize;
use serde_json::json;
use thiserror::Error;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{ChildStdin, Command};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::definition::{ToolCall, ToolDefinition, ToolResult};
use crate::error::ToolError;
use crate::executor::{AlwaysAllow, ApprovalCheck, ToolExecutor};
use crate::registry::ToolRegistry;

const DEFAULT_READ_LIMIT: usize = 200;
const BINARY_SCAN_BYTES: usize = 1024;

/// A concrete executor for Rune's built-in tools.
pub struct BuiltinToolExecutor {
    approvals: Arc<dyn ApprovalCheck>,
    processes: Arc<ProcessManager>,
}

impl BuiltinToolExecutor {
    /// Create a built-in executor with a permissive approval checker.
    #[must_use]
    pub fn new() -> Self {
        Self::with_approval_checker(Arc::new(AlwaysAllow))
    }

    /// Create a built-in executor with a custom approval checker.
    #[must_use]
    pub fn with_approval_checker(approvals: Arc<dyn ApprovalCheck>) -> Self {
        Self {
            approvals,
            processes: Arc::new(ProcessManager::new()),
        }
    }
}

impl Default for BuiltinToolExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolExecutor for BuiltinToolExecutor {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        match call.tool_name.as_str() {
            "read" => ReadFileTool.execute(call).await,
            "write" => WriteFileTool.execute(call).await,
            "edit" => EditFileTool.execute(call).await,
            "list_files" => ListFilesTool.execute(call).await,
            "exec" => {
                self.approvals.check(&call, true).await?;
                ExecTool::new(self.processes.clone()).execute(call).await
            }
            "process" => ProcessTool::new(self.processes.clone()).execute(call).await,
            name => Err(ToolError::UnknownTool {
                name: name.to_string(),
            }),
        }
    }
}

/// Register the real built-in tools into the given registry.
pub fn register_builtin_tools(registry: &mut ToolRegistry) {
    let builtins = [
        ToolDefinition {
            name: "read".into(),
            description: "Read the contents of a file.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file to read" },
                    "offset": { "type": "integer", "description": "Line number to start from (1-indexed)" },
                    "limit": { "type": "integer", "description": "Maximum number of lines to read" }
                },
                "required": ["path"]
            }),
            category: ToolCategory::FileRead,
            requires_approval: false,
        },
        ToolDefinition {
            name: "write".into(),
            description: "Write content to a file, creating it if needed.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file to write" },
                    "content": { "type": "string", "description": "Content to write" }
                },
                "required": ["path", "content"]
            }),
            category: ToolCategory::FileWrite,
            requires_approval: false,
        },
        ToolDefinition {
            name: "edit".into(),
            description: "Edit a file by replacing exact text.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file to edit" },
                    "oldText": { "type": "string", "description": "Exact text to find" },
                    "newText": { "type": "string", "description": "Replacement text" }
                },
                "required": ["path", "oldText", "newText"]
            }),
            category: ToolCategory::FileWrite,
            requires_approval: false,
        },
        ToolDefinition {
            name: "exec".into(),
            description: "Execute a shell command.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Shell command to execute" },
                    "workdir": { "type": "string", "description": "Working directory" },
                    "timeout": { "type": "integer", "description": "Timeout in seconds" },
                    "env": {
                        "type": "object",
                        "description": "Environment variables",
                        "additionalProperties": { "type": "string" }
                    },
                    "background": { "type": "boolean", "description": "Run in background and return a process id" }
                },
                "required": ["command"]
            }),
            category: ToolCategory::ProcessExec,
            requires_approval: true,
        },
        ToolDefinition {
            name: "process".into(),
            description: "Inspect and manage background processes.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["list", "poll", "log", "write", "kill"] },
                    "processId": { "type": "string" },
                    "input": { "type": "string" },
                    "offset": { "type": "integer" },
                    "limit": { "type": "integer" }
                },
                "required": ["action"]
            }),
            category: ToolCategory::ProcessBackground,
            requires_approval: false,
        },
        ToolDefinition {
            name: "list_files".into(),
            description: "List files in a directory with optional pattern filtering.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory path" },
                    "pattern": { "type": "string", "description": "Optional glob pattern, e.g. *.rs" }
                },
                "required": ["path"]
            }),
            category: ToolCategory::FileRead,
            requires_approval: false,
        },
    ];

    for tool in builtins {
        registry.register(tool);
    }
}

/// Backwards-compatible alias while the rest of the workspace still uses the old symbol.
pub fn register_builtin_stubs(registry: &mut ToolRegistry) {
    register_builtin_tools(registry);
}

/// Validate that a tool call's arguments satisfy the `required` fields in the tool schema.
pub fn validate_arguments(def: &ToolDefinition, args: &serde_json::Value) -> Result<(), ToolError> {
    let required = def.parameters.get("required").and_then(|v| v.as_array());

    if let Some(required_fields) = required {
        let obj = args.as_object();
        for field in required_fields {
            if let Some(field_name) = field.as_str() {
                let present = obj.map(|o| o.contains_key(field_name)).unwrap_or(false);
                if !present {
                    return Err(ToolError::InvalidArguments {
                        tool: def.name.clone(),
                        reason: format!("missing required field: {field_name}"),
                    });
                }
            }
        }
    }

    Ok(())
}

struct ReadFileTool;
struct WriteFileTool;
struct EditFileTool;
struct ListFilesTool;

struct ExecTool {
    processes: Arc<ProcessManager>,
}

impl ExecTool {
    fn new(processes: Arc<ProcessManager>) -> Self {
        Self { processes }
    }
}

struct ProcessTool {
    processes: Arc<ProcessManager>,
}

impl ProcessTool {
    fn new(processes: Arc<ProcessManager>) -> Self {
        Self { processes }
    }
}

#[async_trait]
impl ToolExecutor for ReadFileTool {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let args: ReadArgs = parse_args(&call)?;
        let bytes = fs::read(&args.path)
            .await
            .map_err(|err| execution_failed(format!("failed to read {}: {err}", args.path.display())))?;

        if is_binary(&bytes) {
            return Ok(ToolResult {
                tool_call_id: call.tool_call_id,
                output: format!("binary file: {}", args.path.display()),
                is_error: false,
            });
        }

        let text = String::from_utf8(bytes)
            .map_err(|err| execution_failed(format!("{} is not valid UTF-8: {err}", args.path.display())))?;
        let lines: Vec<&str> = text.lines().collect();
        let start = args.offset.unwrap_or(1).max(1) - 1;
        let limit = args.limit.unwrap_or(DEFAULT_READ_LIMIT);
        let end = start.saturating_add(limit).min(lines.len());
        let slice = if start >= lines.len() {
            String::new()
        } else {
            lines[start..end].join("\n")
        };

        Ok(ToolResult {
            tool_call_id: call.tool_call_id,
            output: slice,
            is_error: false,
        })
    }
}

#[async_trait]
impl ToolExecutor for WriteFileTool {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let args: WriteArgs = parse_args(&call)?;
        create_parent_dir(&args.path).await?;
        fs::write(&args.path, args.content.as_bytes())
            .await
            .map_err(|err| execution_failed(format!("failed to write {}: {err}", args.path.display())))?;

        Ok(ToolResult {
            tool_call_id: call.tool_call_id,
            output: format!("wrote {}", args.path.display()),
            is_error: false,
        })
    }
}

#[async_trait]
impl ToolExecutor for EditFileTool {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let args: EditArgs = parse_args(&call)?;
        let content = fs::read_to_string(&args.path)
            .await
            .map_err(|err| execution_failed(format!("failed to read {}: {err}", args.path.display())))?;

        if !content.contains(&args.old_text) {
            return Err(execution_failed(format!(
                "exact text not found in {}",
                args.path.display()
            )));
        }

        let updated = content.replacen(&args.old_text, &args.new_text, 1);
        fs::write(&args.path, updated.as_bytes())
            .await
            .map_err(|err| execution_failed(format!("failed to write {}: {err}", args.path.display())))?;

        Ok(ToolResult {
            tool_call_id: call.tool_call_id,
            output: format!("edited {}", args.path.display()),
            is_error: false,
        })
    }
}

#[async_trait]
impl ToolExecutor for ListFilesTool {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let args: ListFilesArgs = parse_args(&call)?;
        let mut entries = fs::read_dir(&args.path)
            .await
            .map_err(|err| execution_failed(format!("failed to list {}: {err}", args.path.display())))?;
        let mut paths = Vec::new();

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|err| execution_failed(format!("failed reading dir entry: {err}")))?
        {
            let path = entry.path();
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            if let Some(pattern) = &args.pattern
                && !glob_match(pattern, &file_name)
            {
                continue;
            }
            paths.push(path.display().to_string());
        }

        paths.sort();

        Ok(ToolResult {
            tool_call_id: call.tool_call_id,
            output: paths.join("\n"),
            is_error: false,
        })
    }
}

#[async_trait]
impl ToolExecutor for ExecTool {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let args: ExecArgs = parse_args(&call)?;
        if args.background.unwrap_or(false) {
            let process_id = self.processes.spawn(args).await?;
            return Ok(ToolResult {
                tool_call_id: call.tool_call_id,
                output: json!({ "processId": process_id }).to_string(),
                is_error: false,
            });
        }

        let mut command = shell_command(&args.command);
        if let Some(workdir) = &args.workdir {
            command.current_dir(workdir);
        }
        if let Some(env) = &args.env {
            command.envs(env);
        }

        let output = if let Some(timeout_secs) = args.timeout {
            tokio::time::timeout(Duration::from_secs(timeout_secs), command.output())
                .await
                .map_err(|_| execution_failed(format!("command timed out after {timeout_secs}s")))?
                .map_err(|err| execution_failed(format!("failed to execute command: {err}")))?
        } else {
            command
                .output()
                .await
                .map_err(|err| execution_failed(format!("failed to execute command: {err}")))?
        };

        Ok(ToolResult {
            tool_call_id: call.tool_call_id,
            output: json!({
                "status": output.status.code(),
                "stdout": String::from_utf8_lossy(&output.stdout),
                "stderr": String::from_utf8_lossy(&output.stderr)
            })
            .to_string(),
            is_error: !output.status.success(),
        })
    }
}

#[async_trait]
impl ToolExecutor for ProcessTool {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let args: ProcessArgs = parse_args(&call)?;
        let output = match args.action.as_str() {
            "list" => self.processes.list().await?,
            "poll" => {
                let process_id = required_process_id(&args)?;
                self.processes.poll(&process_id).await?
            }
            "log" => {
                let process_id = required_process_id(&args)?;
                self.processes
                    .log(&process_id, args.offset.unwrap_or(0), args.limit)
                    .await?
            }
            "write" => {
                let process_id = required_process_id(&args)?;
                let input = args.input.unwrap_or_default();
                self.processes.write(&process_id, input).await?
            }
            "kill" => {
                let process_id = required_process_id(&args)?;
                self.processes.kill(&process_id).await?
            }
            other => {
                return Err(ToolError::InvalidArguments {
                    tool: call.tool_name,
                    reason: format!("unsupported process action: {other}"),
                });
            }
        };

        Ok(ToolResult {
            tool_call_id: call.tool_call_id,
            output,
            is_error: false,
        })
    }
}

#[derive(Debug, Deserialize)]
struct ReadArgs {
    path: PathBuf,
    offset: Option<usize>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct WriteArgs {
    path: PathBuf,
    content: String,
}

#[derive(Debug, Deserialize)]
struct EditArgs {
    path: PathBuf,
    #[serde(rename = "oldText")]
    old_text: String,
    #[serde(rename = "newText")]
    new_text: String,
}

#[derive(Debug, Deserialize)]
struct ExecArgs {
    command: String,
    workdir: Option<PathBuf>,
    timeout: Option<u64>,
    env: Option<HashMap<String, String>>,
    background: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ProcessArgs {
    action: String,
    #[serde(rename = "processId")]
    process_id: Option<String>,
    input: Option<String>,
    offset: Option<usize>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct ListFilesArgs {
    path: PathBuf,
    pattern: Option<String>,
}

#[derive(Debug, Error)]
enum ProcessError {
    #[error("process not found: {0}")]
    NotFound(String),
    #[error("process stdin is unavailable for {0}")]
    MissingStdin(String),
    #[error("process I/O failed: {0}")]
    Io(String),
}

impl From<ProcessError> for ToolError {
    fn from(value: ProcessError) -> Self {
        execution_failed(value.to_string())
    }
}

struct ProcessManager {
    entries: Mutex<HashMap<String, Arc<ProcessEntry>>>,
}

impl ProcessManager {
    fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }

    async fn spawn(&self, args: ExecArgs) -> Result<String, ToolError> {
        let mut command = shell_command(&args.command);
        if let Some(workdir) = &args.workdir {
            command.current_dir(workdir);
        }
        if let Some(env) = &args.env {
            command.envs(env);
        }
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        command.stdin(Stdio::piped());

        let mut child = command
            .spawn()
            .map_err(|err| execution_failed(format!("failed to spawn command: {err}")))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| execution_failed("child stdout unavailable".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| execution_failed("child stderr unavailable".to_string()))?;
        let stdin = child.stdin.take();

        let process_id = rune_core::ToolCallId::new().to_string();
        let entry = Arc::new(ProcessEntry::new(process_id.clone(), child, stdin));

        spawn_reader(stdout, entry.stdout.clone());
        spawn_reader(stderr, entry.stderr.clone());
        spawn_waiter(entry.clone());

        self.entries.lock().await.insert(process_id.clone(), entry);
        Ok(process_id)
    }

    async fn list(&self) -> Result<String, ToolError> {
        let entries = self.entries.lock().await;
        let mut items = Vec::new();
        for (id, entry) in &*entries {
            let state = entry.state.lock().await;
            items.push(json!({
                "processId": id,
                "running": state.running,
                "exitCode": state.exit_code
            }));
        }
        items.sort_by(|a, b| a["processId"].as_str().cmp(&b["processId"].as_str()));
        Ok(serde_json::Value::Array(items).to_string())
    }

    async fn poll(&self, process_id: &str) -> Result<String, ToolError> {
        let entry = self.get(process_id).await?;
        let state = entry.state.lock().await;
        Ok(json!({
            "processId": process_id,
            "running": state.running,
            "exitCode": state.exit_code
        })
        .to_string())
    }

    async fn log(
        &self,
        process_id: &str,
        offset: usize,
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
            format!("{stdout}{stderr}")
        };

        let output: String = combined
            .chars()
            .skip(offset)
            .take(limit.unwrap_or(usize::MAX))
            .collect();
        Ok(output)
    }

    async fn write(&self, process_id: &str, input: String) -> Result<String, ToolError> {
        let entry = self.get(process_id).await?;
        let mut stdin = entry.stdin.lock().await;
        let stdin = stdin
            .as_mut()
            .ok_or_else(|| ProcessError::MissingStdin(process_id.to_string()))?;
        stdin
            .write_all(input.as_bytes())
            .await
            .map_err(|err| ProcessError::Io(err.to_string()))?;
        Ok(json!({ "processId": process_id, "written": input.len() }).to_string())
    }

    async fn kill(&self, process_id: &str) -> Result<String, ToolError> {
        let entry = self.get(process_id).await?;
        let mut child = entry.child.lock().await;
        child
            .kill()
            .await
            .map_err(|err| execution_failed(format!("failed to kill process {process_id}: {err}")))?;
        let mut state = entry.state.lock().await;
        state.running = false;
        state.exit_code = None;
        Ok(json!({ "processId": process_id, "killed": true }).to_string())
    }

    async fn get(&self, process_id: &str) -> Result<Arc<ProcessEntry>, ToolError> {
        self.entries
            .lock()
            .await
            .get(process_id)
            .cloned()
            .ok_or_else(|| ProcessError::NotFound(process_id.to_string()).into())
    }
}

struct ProcessEntry {
    #[allow(dead_code)]
    id: String,
    child: Mutex<tokio::process::Child>,
    stdin: Mutex<Option<ChildStdin>>,
    stdout: Arc<Mutex<String>>,
    stderr: Arc<Mutex<String>>,
    state: Arc<Mutex<ProcessState>>,
}

impl ProcessEntry {
    fn new(id: String, child: tokio::process::Child, stdin: Option<ChildStdin>) -> Self {
        Self {
            id,
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            stdout: Arc::new(Mutex::new(String::new())),
            stderr: Arc::new(Mutex::new(String::new())),
            state: Arc::new(Mutex::new(ProcessState {
                running: true,
                exit_code: None,
            })),
        }
    }
}

struct ProcessState {
    running: bool,
    exit_code: Option<i32>,
}

fn spawn_reader<R>(mut reader: R, buffer: Arc<Mutex<String>>) -> JoinHandle<()>
where
    R: AsyncReadExt + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut bytes = Vec::new();
        if reader.read_to_end(&mut bytes).await.is_ok() {
            let mut target = buffer.lock().await;
            target.push_str(&String::from_utf8_lossy(&bytes));
        }
    })
}

fn spawn_waiter(entry: Arc<ProcessEntry>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let status = {
            let mut child = entry.child.lock().await;
            child.wait().await.ok()
        };
        let mut state = entry.state.lock().await;
        state.running = false;
        state.exit_code = status.and_then(|s| s.code());
    })
}

fn parse_args<T: for<'de> Deserialize<'de>>(call: &ToolCall) -> Result<T, ToolError> {
    serde_json::from_value(call.arguments.clone()).map_err(|err| ToolError::InvalidArguments {
        tool: call.tool_name.clone(),
        reason: err.to_string(),
    })
}

async fn create_parent_dir(path: &Path) -> Result<(), ToolError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).await.map_err(|err| {
            execution_failed(format!("failed to create parent directory {}: {err}", parent.display()))
        })?;
    }
    Ok(())
}

fn shell_command(command: &str) -> Command {
    let mut cmd = Command::new("sh");
    cmd.arg("-lc").arg(command);
    cmd
}

fn required_process_id(args: &ProcessArgs) -> Result<String, ToolError> {
    args.process_id.clone().ok_or_else(|| ToolError::InvalidArguments {
        tool: "process".to_string(),
        reason: "missing required field: processId".to_string(),
    })
}

fn execution_failed(message: String) -> ToolError {
    ToolError::ExecutionFailed { message }
}

fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(BINARY_SCAN_BYTES).any(|byte| *byte == 0)
}

fn glob_match(pattern: &str, text: &str) -> bool {
    glob_match_bytes(pattern.as_bytes(), text.as_bytes())
}

fn glob_match_bytes(pattern: &[u8], text: &[u8]) -> bool {
    if pattern.is_empty() {
        return text.is_empty();
    }

    match pattern[0] {
        b'*' => {
            if glob_match_bytes(&pattern[1..], text) {
                return true;
            }
            if !text.is_empty() {
                return glob_match_bytes(pattern, &text[1..]);
            }
            false
        }
        b'?' => !text.is_empty() && glob_match_bytes(&pattern[1..], &text[1..]),
        ch => !text.is_empty() && ch == text[0] && glob_match_bytes(&pattern[1..], &text[1..]),
    }
}
