//! Implementation of session spawning and messaging tools.
//!
//! These tools allow the agent to spawn sub-agent sessions and send
//! messages across sessions.

use async_trait::async_trait;
use tracing::instrument;

use crate::definition::{ToolCall, ToolResult};
use crate::error::ToolError;
use crate::executor::ToolExecutor;

/// Trait for session spawning operations.
#[async_trait]
pub trait SessionSpawner: Send + Sync {
    /// Spawn a new isolated session. Returns JSON with session details.
    async fn spawn_session(
        &self,
        task: &str,
        model: Option<&str>,
        mode: Option<&str>,
        timeout_seconds: Option<u64>,
    ) -> Result<String, String>;

    /// Send a message to another session. Returns JSON with delivery info.
    async fn send_message(
        &self,
        session_key: Option<&str>,
        label: Option<&str>,
        message: &str,
    ) -> Result<String, String>;
}

/// Tool executor for session spawning and cross-session messaging.
pub struct SpawnToolExecutor<S: SessionSpawner> {
    spawner: S,
}

impl<S: SessionSpawner> SpawnToolExecutor<S> {
    /// Create a new spawn tool executor.
    pub fn new(spawner: S) -> Self {
        Self { spawner }
    }

    #[instrument(skip(self, call), fields(tool = "sessions_spawn"))]
    async fn spawn(&self, call: &ToolCall) -> Result<ToolResult, ToolError> {
        let task = call
            .arguments
            .get("task")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolError::InvalidArgument("missing required parameter: task".into())
            })?;

        let model = call.arguments.get("model").and_then(|v| v.as_str());
        let mode = call.arguments.get("mode").and_then(|v| v.as_str());
        let timeout = call
            .arguments
            .get("timeoutSeconds")
            .and_then(|v| v.as_u64());

        match self.spawner.spawn_session(task, model, mode, timeout).await {
            Ok(output) => Ok(ToolResult {
                tool_call_id: call.tool_call_id,
                output,
                is_error: false,
            }),
            Err(e) => Ok(ToolResult {
                tool_call_id: call.tool_call_id,
                output: e,
                is_error: true,
            }),
        }
    }

    #[instrument(skip(self, call), fields(tool = "sessions_send"))]
    async fn send(&self, call: &ToolCall) -> Result<ToolResult, ToolError> {
        let message = call
            .arguments
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolError::InvalidArgument("missing required parameter: message".into())
            })?;

        let session_key = call.arguments.get("sessionKey").and_then(|v| v.as_str());
        let label = call.arguments.get("label").and_then(|v| v.as_str());

        if session_key.is_none() && label.is_none() {
            return Err(ToolError::InvalidArgument(
                "either sessionKey or label is required".into(),
            ));
        }

        match self
            .spawner
            .send_message(session_key, label, message)
            .await
        {
            Ok(output) => Ok(ToolResult {
                tool_call_id: call.tool_call_id,
                output,
                is_error: false,
            }),
            Err(e) => Ok(ToolResult {
                tool_call_id: call.tool_call_id,
                output: e,
                is_error: true,
            }),
        }
    }
}

#[async_trait]
impl<S: SessionSpawner> ToolExecutor for SpawnToolExecutor<S> {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        match call.tool_name.as_str() {
            "sessions_spawn" => self.spawn(&call).await,
            "sessions_send" => self.send(&call).await,
            other => Err(ToolError::NotFound(other.into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune_core::ToolCallId;

    struct MockSpawner;

    #[async_trait]
    impl SessionSpawner for MockSpawner {
        async fn spawn_session(
            &self,
            task: &str,
            _model: Option<&str>,
            _mode: Option<&str>,
            _timeout: Option<u64>,
        ) -> Result<String, String> {
            Ok(format!(
                "{{\"sessionId\": \"sub-1\", \"task\": \"{task}\", \"status\": \"running\"}}"
            ))
        }

        async fn send_message(
            &self,
            session_key: Option<&str>,
            _label: Option<&str>,
            message: &str,
        ) -> Result<String, String> {
            let key = session_key.unwrap_or("unknown");
            Ok(format!(
                "{{\"delivered\": true, \"to\": \"{key}\", \"message\": \"{message}\"}}"
            ))
        }
    }

    fn make_call(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: name.into(),
            arguments: args,
        }
    }

    #[tokio::test]
    async fn spawn_returns_session_info() {
        let exec = SpawnToolExecutor::new(MockSpawner);
        let call = make_call(
            "sessions_spawn",
            serde_json::json!({"task": "build a feature"}),
        );
        let result = exec.execute(call).await.unwrap();
        assert!(!result.is_error);
        assert!(result.output.contains("sub-1"));
    }

    #[tokio::test]
    async fn send_requires_target() {
        let exec = SpawnToolExecutor::new(MockSpawner);
        let call = make_call(
            "sessions_send",
            serde_json::json!({"message": "hello"}),
        );
        let err = exec.execute(call).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }

    #[tokio::test]
    async fn send_with_session_key() {
        let exec = SpawnToolExecutor::new(MockSpawner);
        let call = make_call(
            "sessions_send",
            serde_json::json!({"sessionKey": "agent:main", "message": "hello"}),
        );
        let result = exec.execute(call).await.unwrap();
        assert!(!result.is_error);
        assert!(result.output.contains("delivered"));
    }
}
