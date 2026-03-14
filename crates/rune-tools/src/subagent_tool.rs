//! Sub-agent management tool: list, steer, and kill spawned sub-agents.

use async_trait::async_trait;
use tracing::instrument;

use crate::definition::{ToolCall, ToolResult};
use crate::error::ToolError;
use crate::executor::ToolExecutor;

/// Trait for sub-agent management, implemented by the runtime layer.
#[async_trait]
pub trait SubagentManager: Send + Sync {
    /// List spawned sub-agents. Returns JSON array.
    async fn list(&self, recent_minutes: Option<u64>) -> Result<String, String>;
    /// Send a steering message to a sub-agent.
    async fn steer(&self, target: &str, message: &str) -> Result<String, String>;
    /// Kill a sub-agent.
    async fn kill(&self, target: &str) -> Result<String, String>;
}

/// Tool executor for sub-agent management.
pub struct SubagentToolExecutor<M: SubagentManager> {
    manager: M,
}

impl<M: SubagentManager> SubagentToolExecutor<M> {
    /// Create a new sub-agent tool executor.
    pub fn new(manager: M) -> Self {
        Self { manager }
    }

    #[instrument(skip(self, call), fields(tool = "subagents"))]
    async fn handle(&self, call: &ToolCall) -> Result<ToolResult, ToolError> {
        let action = call
            .arguments
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("list");

        let result = match action {
            "list" => {
                let recent = call.arguments.get("recentMinutes").and_then(|v| v.as_u64());
                self.manager.list(recent).await
            }
            "steer" => {
                let target = call
                    .arguments
                    .get("target")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ToolError::InvalidArgument("steer requires 'target' parameter".into())
                    })?;
                let message = call
                    .arguments
                    .get("message")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ToolError::InvalidArgument("steer requires 'message' parameter".into())
                    })?;
                self.manager.steer(target, message).await
            }
            "kill" => {
                let target = call
                    .arguments
                    .get("target")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ToolError::InvalidArgument("kill requires 'target' parameter".into())
                    })?;
                self.manager.kill(target).await
            }
            other => {
                return Err(ToolError::InvalidArgument(format!(
                    "unknown subagents action: {other}"
                )));
            }
        };

        match result {
            Ok(output) => Ok(ToolResult {
                tool_call_id: call.tool_call_id,
                output,
                is_error: false,
                tool_execution_id: None,
            }),
            Err(e) => Ok(ToolResult {
                tool_call_id: call.tool_call_id,
                output: e,
                is_error: true,
                tool_execution_id: None,
            }),
        }
    }
}

#[async_trait]
impl<M: SubagentManager> ToolExecutor for SubagentToolExecutor<M> {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        match call.tool_name.as_str() {
            "subagents" => self.handle(&call).await,
            other => Err(ToolError::NotFound(other.into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune_core::ToolCallId;

    struct MockManager;

    #[async_trait]
    impl SubagentManager for MockManager {
        async fn list(&self, _recent: Option<u64>) -> Result<String, String> {
            Ok(
                "[{\"id\": \"sub-1\", \"status\": \"running\", \"task\": \"build feature\"}]"
                    .into(),
            )
        }
        async fn steer(&self, target: &str, message: &str) -> Result<String, String> {
            Ok(format!(
                "{{\"target\": \"{target}\", \"steered\": true, \"message\": \"{message}\"}}"
            ))
        }
        async fn kill(&self, target: &str) -> Result<String, String> {
            Ok(format!("{{\"target\": \"{target}\", \"killed\": true}}"))
        }
    }

    fn make_call(args: serde_json::Value) -> ToolCall {
        ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: "subagents".into(),
            arguments: args,
        }
    }

    #[tokio::test]
    async fn list_returns_agents() {
        let exec = SubagentToolExecutor::new(MockManager);
        let call = make_call(serde_json::json!({"action": "list"}));
        let result = exec.execute(call).await.unwrap();
        assert!(!result.is_error);
        assert!(result.output.contains("sub-1"));
    }

    #[tokio::test]
    async fn steer_requires_target_and_message() {
        let exec = SubagentToolExecutor::new(MockManager);
        let call = make_call(serde_json::json!({"action": "steer"}));
        let err = exec.execute(call).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }

    #[tokio::test]
    async fn kill_works() {
        let exec = SubagentToolExecutor::new(MockManager);
        let call = make_call(serde_json::json!({"action": "kill", "target": "sub-1"}));
        let result = exec.execute(call).await.unwrap();
        assert!(result.output.contains("killed"));
    }

    #[tokio::test]
    async fn missing_action_defaults_to_list() {
        let exec = SubagentToolExecutor::new(MockManager);
        let call = make_call(serde_json::json!({}));
        let result = exec.execute(call).await.unwrap();
        assert!(!result.is_error);
        assert!(result.output.contains("sub-1"));
    }
}
