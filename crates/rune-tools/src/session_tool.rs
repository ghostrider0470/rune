//! Implementation of session management tools.
//!
//! These tools allow the agent to list, inspect, and manage sessions
//! through the tool interface rather than direct API calls.

use async_trait::async_trait;
use tracing::instrument;

use crate::definition::{ToolCall, ToolResult};
use crate::error::ToolError;
use crate::executor::ToolExecutor;

/// Trait for querying session state.
/// Implemented by the runtime/store layer and injected into the tool executor.
#[async_trait]
pub trait SessionQuery: Send + Sync {
    /// List active sessions with optional filters.
    async fn list_sessions(
        &self,
        limit: Option<usize>,
        kinds: Option<Vec<String>>,
    ) -> Result<String, String>;

    /// Get details for a specific session.
    async fn get_session(&self, session_id: &str) -> Result<String, String>;

    /// Get session history/transcript.
    async fn get_history(&self, session_id: &str, limit: Option<usize>) -> Result<String, String>;

    /// Get current session status (usage, time, model).
    async fn session_status(&self) -> Result<String, String>;
}

/// Executor for session management tools.
pub struct SessionToolExecutor<Q: SessionQuery> {
    query: Q,
}

impl<Q: SessionQuery> SessionToolExecutor<Q> {
    /// Create a new session tool executor with the given query backend.
    pub fn new(query: Q) -> Self {
        Self { query }
    }

    #[instrument(skip(self, call), fields(tool = "sessions_list"))]
    async fn sessions_list(&self, call: &ToolCall) -> Result<ToolResult, ToolError> {
        let limit = call
            .arguments
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);

        let kinds = call
            .arguments
            .get("kinds")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            });

        match self.query.list_sessions(limit, kinds).await {
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

    #[instrument(skip(self, call), fields(tool = "sessions_history"))]
    async fn sessions_history(&self, call: &ToolCall) -> Result<ToolResult, ToolError> {
        let session_id = call
            .arguments
            .get("sessionKey")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolError::InvalidArgument("missing required parameter: sessionKey".into())
            })?;

        let limit = call
            .arguments
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);

        match self.query.get_history(session_id, limit).await {
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

    #[instrument(skip(self, call), fields(tool = "session_status"))]
    async fn session_status(&self, call: &ToolCall) -> Result<ToolResult, ToolError> {
        match self.query.session_status().await {
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
impl<Q: SessionQuery> ToolExecutor for SessionToolExecutor<Q> {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        match call.tool_name.as_str() {
            "sessions_list" => self.sessions_list(&call).await,
            "sessions_history" => self.sessions_history(&call).await,
            "session_status" => self.session_status(&call).await,
            other => Err(ToolError::NotFound(other.into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune_core::ToolCallId;

    struct MockSessionQuery;

    #[async_trait]
    impl SessionQuery for MockSessionQuery {
        async fn list_sessions(
            &self,
            limit: Option<usize>,
            _kinds: Option<Vec<String>>,
        ) -> Result<String, String> {
            let l = limit.unwrap_or(10);
            Ok(format!(
                "[{{\"id\": \"session-1\", \"status\": \"running\"}}] (limit: {l})"
            ))
        }

        async fn get_session(&self, session_id: &str) -> Result<String, String> {
            Ok(format!(
                "{{\"id\": \"{session_id}\", \"status\": \"running\"}}"
            ))
        }

        async fn get_history(
            &self,
            session_id: &str,
            limit: Option<usize>,
        ) -> Result<String, String> {
            let l = limit.unwrap_or(50);
            Ok(format!(
                "[{{\"role\": \"user\", \"content\": \"hello\"}}] (session: {session_id}, limit: {l})"
            ))
        }

        async fn session_status(&self) -> Result<String, String> {
            Ok("{\"model\": \"gpt-5.4\", \"usage\": {\"tokens\": 1234}}".into())
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
    async fn list_sessions_returns_json() {
        let exec = SessionToolExecutor::new(MockSessionQuery);
        let call = make_call("sessions_list", serde_json::json!({"limit": 5}));
        let result = exec.execute(call).await.unwrap();
        assert!(!result.is_error);
        assert!(result.output.contains("session-1"));
        assert!(result.output.contains("limit: 5"));
    }

    #[tokio::test]
    async fn history_requires_session_key() {
        let exec = SessionToolExecutor::new(MockSessionQuery);
        let call = make_call("sessions_history", serde_json::json!({}));
        let err = exec.execute(call).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }

    #[tokio::test]
    async fn session_status_returns_info() {
        let exec = SessionToolExecutor::new(MockSessionQuery);
        let call = make_call("session_status", serde_json::json!({}));
        let result = exec.execute(call).await.unwrap();
        assert!(result.output.contains("gpt-5.4"));
    }
}
