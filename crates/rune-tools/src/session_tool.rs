//! Implementation of session management tools.
//!
//! These tools allow the agent to list, inspect, and manage sessions
//! through the tool interface rather than direct API calls.

use async_trait::async_trait;
use serde_json::Value;
use tracing::instrument;

use crate::definition::{ToolCall, ToolResult};
use crate::error::ToolError;
use crate::executor::ToolExecutor;

/// Trait for querying session state.
/// Implemented by the runtime/store layer and injected into the tool executor.
fn validate_session_status_payload(output: &str) -> Result<(), String> {
    let value: Value = serde_json::from_str(output)
        .map_err(|e| format!("session_status must return JSON card output: {e}"))?;

    let object = value
        .as_object()
        .ok_or_else(|| "session_status must return a JSON object".to_string())?;

    let status = object
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| "session_status card missing required string field: status".to_string())?;

    if status.trim().is_empty() {
        return Err("session_status card field `status` must not be empty".to_string());
    }

    if let Some(unresolved) = object.get("unresolved") {
        unresolved.as_array().ok_or_else(|| {
            "session_status card field `unresolved` must be an array when present".to_string()
        })?;
    }

    Ok(())
}

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

    /// Get session status (usage, time, model) for the current or targeted session.
    async fn session_status(&self, session_id: Option<&str>) -> Result<String, String>;
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

    #[instrument(skip(self, call), fields(tool = "session_status"))]
    async fn session_status(&self, call: &ToolCall) -> Result<ToolResult, ToolError> {
        let session_id = call
            .arguments
            .get("sessionKey")
            .and_then(|v| v.as_str())
            .or_else(|| call.arguments.get("session_id").and_then(|v| v.as_str()))
            .or_else(|| call.arguments.get("id").and_then(|v| v.as_str()));

        match self.query.session_status(session_id).await {
            Ok(output) => match validate_session_status_payload(&output) {
                Ok(()) => Ok(ToolResult {
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
            },
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

        async fn session_status(&self, session_id: Option<&str>) -> Result<String, String> {
            let target = session_id.unwrap_or("current-session");
            Ok(format!(
                concat!(
                    "{{",
                    "\"session_id\": \"{}\", ",
                    "\"runtime\": \"kind=direct | channel=local | status=running\", ",
                    "\"status\": \"running\", ",
                    "\"current_model\": \"gpt-5.4\", ",
                    "\"prompt_tokens\": 1000, ",
                    "\"completion_tokens\": 234, ",
                    "\"total_tokens\": 1234, ",
                    "\"approval_mode\": \"on-miss\", ",
                    "\"security_mode\": \"allowlist\", ",
                    "\"unresolved\": []",
                    "}}"
                ),
                target
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
        assert!(!result.is_error);
        assert!(result.output.contains("gpt-5.4"));
        assert!(result.output.contains("current-session"));
        assert!(result.output.contains("\"status\": \"running\""));
    }

    #[tokio::test]
    async fn session_status_accepts_session_key_alias() {
        let exec = SessionToolExecutor::new(MockSessionQuery);
        let call = make_call(
            "session_status",
            serde_json::json!({"sessionKey": "session-42"}),
        );
        let result = exec.execute(call).await.unwrap();
        assert!(!result.is_error);
        assert!(result.output.contains("session-42"));
    }

    #[tokio::test]
    async fn session_status_accepts_legacy_id_aliases() {
        let exec = SessionToolExecutor::new(MockSessionQuery);

        let call = make_call(
            "session_status",
            serde_json::json!({"session_id": "session-a"}),
        );
        let result = exec.execute(call).await.unwrap();
        assert!(result.output.contains("session-a"));

        let call = make_call("session_status", serde_json::json!({"id": "session-b"}));
        let result = exec.execute(call).await.unwrap();
        assert!(result.output.contains("session-b"));
    }

    struct InvalidStatusSessionQuery;

    #[async_trait]
    impl SessionQuery for InvalidStatusSessionQuery {
        async fn list_sessions(
            &self,
            _limit: Option<usize>,
            _kinds: Option<Vec<String>>,
        ) -> Result<String, String> {
            Ok("[]".to_string())
        }

        async fn get_session(&self, session_id: &str) -> Result<String, String> {
            Ok(format!("{{\"id\":\"{session_id}\"}}"))
        }

        async fn get_history(
            &self,
            _session_id: &str,
            _limit: Option<usize>,
        ) -> Result<String, String> {
            Ok("[]".to_string())
        }

        async fn session_status(&self, _session_id: Option<&str>) -> Result<String, String> {
            Ok("status: running".to_string())
        }
    }

    #[tokio::test]
    async fn session_status_rejects_non_json_payloads() {
        let exec = SessionToolExecutor::new(InvalidStatusSessionQuery);
        let call = make_call("session_status", serde_json::json!({}));
        let result = exec.execute(call).await.unwrap();
        assert!(result.is_error);
        assert!(
            result
                .output
                .contains("session_status must return JSON card output")
        );
    }
}
