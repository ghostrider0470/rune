//! ACP (Agent Communication Protocol) dispatch tool.
//!
//! Dispatches coding tasks to external CLI agents (Claude Code, Codex) as
//! async subprocesses and streams their output back as tool results.

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use tokio::process::Command;
use tracing::{info, warn};

use crate::definition::{ToolCall, ToolDefinition, ToolResult};
use crate::error::ToolError;
use crate::executor::ToolExecutor;

/// Maximum output bytes returned to the LLM context (100 KB).
const MAX_OUTPUT_BYTES: usize = 100 * 1024;

/// Default timeout for ACP tasks (10 minutes).
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

/// Known external agent backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpAgent {
    ClaudeCode,
    Codex,
}

impl AcpAgent {
    fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "claude" | "claude-code" | "claude_code" => Some(Self::ClaudeCode),
            "codex" | "openai-codex" => Some(Self::Codex),
            _ => None,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
        }
    }

    fn binary_name(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude",
            Self::Codex => "codex",
        }
    }
}

/// Configuration for an ACP agent backend.
#[derive(Debug, Clone)]
pub struct AcpAgentConfig {
    pub agent: AcpAgent,
    pub binary_path: PathBuf,
}

/// Executor for the `acp_dispatch` tool.
pub struct AcpToolExecutor {
    agents: Vec<AcpAgentConfig>,
    workspace: PathBuf,
    timeout: Duration,
}

impl AcpToolExecutor {
    /// Create a new ACP executor. Agents are resolved from PATH at dispatch time
    /// unless explicit configs are added via `with_agent`.
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            agents: Vec::new(),
            workspace,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_agent(mut self, config: AcpAgentConfig) -> Self {
        self.agents.retain(|a| a.agent != config.agent);
        self.agents.push(config);
        self
    }

    fn resolve_agent(&self, name: &str) -> Option<AcpAgentConfig> {
        let target = AcpAgent::from_str(name)?;
        // Check explicit configs first
        if let Some(cfg) = self.agents.iter().find(|a| a.agent == target) {
            return Some(cfg.clone());
        }
        // Fall back to PATH resolution
        Some(AcpAgentConfig {
            agent: target,
            binary_path: PathBuf::from(target.binary_name()),
        })
    }

    async fn dispatch(&self, call: &ToolCall) -> Result<ToolResult, ToolError> {
        let agent_name = call
            .arguments
            .get("agent")
            .and_then(|v| v.as_str())
            .unwrap_or("claude-code");

        let task = call
            .arguments
            .get("task")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments {
                tool: "acp_dispatch".into(),
                reason: "missing required field: task".into(),
            })?;

        let workdir = call
            .arguments
            .get("workdir")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .unwrap_or_else(|| self.workspace.clone());

        let timeout_secs = call
            .arguments
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .map(Duration::from_secs)
            .unwrap_or(self.timeout);

        let agent_config = match self.resolve_agent(agent_name) {
            Some(config) => config,
            None => {
                return Ok(ToolResult {
                    tool_call_id: call.tool_call_id.clone(),
                    output: format!(
                        "Unknown agent '{agent_name}'. Supported: claude-code, codex"
                    ),
                    is_error: true,
                    tool_execution_id: None,
                });
            }
        };

        info!(
            agent = agent_config.agent.label(),
            task_len = task.len(),
            workdir = %workdir.display(),
            "dispatching ACP task"
        );

        let label = agent_config.agent.label().to_string();
        let mut cmd = Self::build_command(&agent_config, task, &workdir);

        let result = match tokio::time::timeout(timeout_secs, cmd.output()).await {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let exit_code = output.status.code().unwrap_or(-1);

                let mut result_text = String::new();
                result_text.push_str(&format!(
                    "Agent: {label}\nExit code: {exit_code}\n"
                ));

                if !stdout.is_empty() {
                    result_text.push_str("\n--- stdout ---\n");
                    result_text.push_str(&stdout);
                }
                if !stderr.is_empty() {
                    result_text.push_str("\n--- stderr ---\n");
                    result_text.push_str(&stderr);
                }

                // Truncate for LLM context
                if result_text.len() > MAX_OUTPUT_BYTES {
                    result_text.truncate(MAX_OUTPUT_BYTES);
                    result_text.push_str("\n\n[output truncated]");
                }

                ToolResult {
                    tool_call_id: call.tool_call_id.clone(),
                    output: result_text,
                    is_error: !output.status.success(),
                    tool_execution_id: None,
                }
            }
            Ok(Err(e)) => {
                warn!(agent = %label, error = %e, "ACP spawn failed");
                ToolResult {
                    tool_call_id: call.tool_call_id.clone(),
                    output: format!("Failed to spawn {label}: {e}"),
                    is_error: true,
                    tool_execution_id: None,
                }
            }
            Err(_) => ToolResult {
                tool_call_id: call.tool_call_id.clone(),
                output: format!(
                    "ACP task timed out after {}s",
                    timeout_secs.as_secs()
                ),
                is_error: true,
                tool_execution_id: None,
            },
        };

        Ok(result)
    }

    fn build_command(
        config: &AcpAgentConfig,
        task: &str,
        workdir: &PathBuf,
    ) -> Command {
        let mut cmd = Command::new(&config.binary_path);

        match config.agent {
            AcpAgent::ClaudeCode => {
                cmd.arg("-p")
                    .arg(task)
                    .arg("--output-format")
                    .arg("text")
                    .arg("--dangerously-skip-permissions")
                    .arg("--no-session-persistence");
            }
            AcpAgent::Codex => {
                cmd.arg("exec")
                    .arg(task)
                    .arg("--model")
                    .arg("o3");
            }
        }

        cmd.current_dir(workdir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        cmd
    }
}

#[async_trait]
impl ToolExecutor for AcpToolExecutor {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        match call.tool_name.as_str() {
            "acp_dispatch" => self.dispatch(&call).await,
            other => Err(ToolError::NotFound(other.into())),
        }
    }
}

/// Return the `ToolDefinition` for registration in the tool registry.
pub fn acp_dispatch_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: "acp_dispatch".into(),
        description: "Dispatch a coding task to an external AI agent (Claude Code CLI or Codex CLI). The agent runs as a subprocess with full tool access and returns its output. Use this for complex coding tasks that benefit from a dedicated agent session.".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "agent": {
                    "type": "string",
                    "description": "Which agent to use: 'claude-code' or 'codex' (default: claude-code)",
                    "enum": ["claude-code", "codex"]
                },
                "task": {
                    "type": "string",
                    "description": "The coding task to dispatch to the agent. Be specific and include context."
                },
                "workdir": {
                    "type": "string",
                    "description": "Working directory for the agent (defaults to workspace root)"
                },
                "timeout_secs": {
                    "type": "integer",
                    "description": "Timeout in seconds (default: 600 = 10 minutes)"
                }
            },
            "required": ["task"]
        }),
        category: rune_core::ToolCategory::External,
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
            tool_name: "acp_dispatch".into(),
            arguments: args,
        }
    }

    #[test]
    fn agent_from_str_variants() {
        assert_eq!(AcpAgent::from_str("claude"), Some(AcpAgent::ClaudeCode));
        assert_eq!(AcpAgent::from_str("claude-code"), Some(AcpAgent::ClaudeCode));
        assert_eq!(AcpAgent::from_str("Claude-Code"), Some(AcpAgent::ClaudeCode));
        assert_eq!(AcpAgent::from_str("codex"), Some(AcpAgent::Codex));
        assert_eq!(AcpAgent::from_str("openai-codex"), Some(AcpAgent::Codex));
        assert_eq!(AcpAgent::from_str("unknown"), None);
    }

    #[test]
    fn definition_schema_requires_task() {
        let def = acp_dispatch_tool_definition();
        assert_eq!(def.name, "acp_dispatch");
        let required = def.parameters["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v.as_str() == Some("task")));
    }

    #[tokio::test]
    async fn missing_task_returns_error() {
        let exec = AcpToolExecutor::new(PathBuf::from("/tmp"));
        let call = make_call(serde_json::json!({"agent": "claude-code"}));
        let err = exec.execute(call).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments { .. }));
    }

    #[tokio::test]
    async fn unknown_agent_returns_error_result() {
        let exec = AcpToolExecutor::new(PathBuf::from("/tmp"));
        let call = make_call(serde_json::json!({"agent": "gpt-agent", "task": "hello"}));
        let result = exec.execute(call).await.unwrap();
        assert!(result.is_error);
        assert!(result.output.contains("Unknown agent"));
    }

    #[tokio::test]
    async fn unknown_tool_name_rejected() {
        let exec = AcpToolExecutor::new(PathBuf::from("/tmp"));
        let call = ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: "not_acp".into(),
            arguments: serde_json::json!({}),
        };
        let err = exec.execute(call).await.unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
    }
}
