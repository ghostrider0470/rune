//! Inter-agent comms tool — lets the agent send and receive messages to/from peer agents.

use std::sync::Arc;

use async_trait::async_trait;
use rune_core::ToolCategory;

use crate::definition::{ToolCall, ToolDefinition, ToolResult};
use crate::error::ToolError;
use crate::executor::ToolExecutor;

/// A summary of a received comms message (for returning to the model).
#[derive(serde::Serialize)]
pub struct CommsMessageSummary {
    pub id: String,
    pub from: String,
    pub subject: String,
    pub body: String,
    pub priority: String,
    pub created_at: Option<String>,
}

/// Trait for comms operations, implemented by the runtime layer.
#[async_trait]
pub trait CommsOps: Send + Sync {
    /// Send a message to the peer agent.
    async fn send_message(
        &self,
        to: &str,
        msg_type: &str,
        subject: &str,
        body: &str,
        priority: &str,
    ) -> Result<String, String>;

    /// Read messages from inbox. Returns summaries and optionally archives them.
    async fn read_inbox(
        &self,
        mark_read: bool,
    ) -> Result<Vec<CommsMessageSummary>, String>;
}

/// Tool executor for inter-agent comms (handles both comms_send and comms_read).
pub struct CommsToolExecutor<C: CommsOps> {
    comms: Arc<C>,
}

impl<C: CommsOps> CommsToolExecutor<C> {
    pub fn new(comms: Arc<C>) -> Self {
        Self { comms }
    }
}

#[async_trait]
impl<C: CommsOps> ToolExecutor for CommsToolExecutor<C> {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        match call.tool_name.as_str() {
            "comms_send" => self.handle_send(call).await,
            "comms_read" => self.handle_read(call).await,
            other => Err(ToolError::UnknownTool { name: other.to_string() }),
        }
    }
}

impl<C: CommsOps> CommsToolExecutor<C> {
    async fn handle_send(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let args = &call.arguments;
        let to = args
            .get("to")
            .and_then(|v| v.as_str())
            .unwrap_or("horizon-ai");
        let msg_type = args
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("status");
        let subject = args
            .get("subject")
            .and_then(|v| v.as_str())
            .unwrap_or("message from rune");
        let body = args.get("body").and_then(|v| v.as_str()).unwrap_or("");
        let priority = args
            .get("priority")
            .and_then(|v| v.as_str())
            .unwrap_or("p1");

        if body.is_empty() {
            return Err(ToolError::InvalidArgument("body is required".into()));
        }

        match self
            .comms
            .send_message(to, msg_type, subject, body, priority)
            .await
        {
            Ok(id) => Ok(ToolResult {
                tool_call_id: call.tool_call_id,
                output: format!("Message sent: {id}"),
                is_error: false,
                tool_execution_id: None,
            }),
            Err(e) => Err(ToolError::ExecutionFailed(format!(
                "comms send failed: {e}"
            ))),
        }
    }

    async fn handle_read(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let mark_read = call
            .arguments
            .get("mark_read")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        match self.comms.read_inbox(mark_read).await {
            Ok(messages) => {
                let output = if messages.is_empty() {
                    "No unread messages in inbox.".to_string()
                } else {
                    let count = messages.len();
                    let json = serde_json::to_string_pretty(&messages)
                        .unwrap_or_else(|_| format!("{count} messages (serialization failed)"));
                    format!("{count} message(s) in inbox:\n{json}")
                };
                Ok(ToolResult {
                    tool_call_id: call.tool_call_id,
                    output,
                    is_error: false,
                    tool_execution_id: None,
                })
            }
            Err(e) => Err(ToolError::ExecutionFailed(format!(
                "comms read failed: {e}"
            ))),
        }
    }
}

/// Tool definition for `comms_send` — send a message to a peer agent.
pub fn comms_send_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: "comms_send".into(),
        description: "Send a message to a peer agent (e.g. OpenClaw). The message is delivered \
            as a JSON file to the peer's inbox directory and will be picked up on the next poll."
            .into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "to": {
                    "type": "string",
                    "description": "Recipient agent ID (e.g. 'openclaw'). Defaults to 'horizon-ai'."
                },
                "subject": {
                    "type": "string",
                    "description": "Message subject line."
                },
                "body": {
                    "type": "string",
                    "description": "Message body content."
                },
                "type": {
                    "type": "string",
                    "description": "Message type: 'status', 'request', 'question', 'directive'. Defaults to 'status'."
                },
                "priority": {
                    "type": "string",
                    "description": "Priority: 'p0' (urgent), 'p1' (normal), 'p2' (low). Defaults to 'p1'."
                }
            },
            "required": ["body"]
        }),
        category: ToolCategory::External,
        requires_approval: false,
    }
}

/// Tool definition for `comms_read` — check inbox for messages from peer agents.
pub fn comms_read_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: "comms_read".into(),
        description: "Check your comms inbox for messages from peer agents (e.g. OpenClaw). \
            Returns unread messages as JSON. Use this to check if OpenClaw or other agents have \
            sent you directives, questions, or status updates."
            .into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "mark_read": {
                    "type": "boolean",
                    "description": "Move processed messages to an archive subfolder. Defaults to true."
                }
            }
        }),
        category: ToolCategory::External,
        requires_approval: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune_core::ToolCallId;

    struct MockComms;

    #[async_trait]
    impl CommsOps for MockComms {
        async fn send_message(
            &self,
            _to: &str,
            _msg_type: &str,
            subject: &str,
            _body: &str,
            _priority: &str,
        ) -> Result<String, String> {
            Ok(format!("msg-test-{subject}"))
        }

        async fn read_inbox(
            &self,
            _mark_read: bool,
        ) -> Result<Vec<CommsMessageSummary>, String> {
            Ok(vec![CommsMessageSummary {
                id: "msg-001".to_string(),
                from: "openclaw".to_string(),
                subject: "test directive".to_string(),
                body: "Please fix CI".to_string(),
                priority: "p1".to_string(),
                created_at: Some("2026-03-27T12:00:00Z".to_string()),
            }])
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
    async fn send_returns_message_id() {
        let exec = CommsToolExecutor::new(Arc::new(MockComms));
        let call = make_call("comms_send", serde_json::json!({
            "subject": "hello",
            "body": "test body"
        }));
        let result = exec.execute(call).await.unwrap();
        assert!(!result.is_error);
        assert!(result.output.contains("Message sent:"));
    }

    #[tokio::test]
    async fn empty_body_returns_error() {
        let exec = CommsToolExecutor::new(Arc::new(MockComms));
        let call = make_call("comms_send", serde_json::json!({
            "subject": "hello",
            "body": ""
        }));
        let err = exec.execute(call).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }

    #[tokio::test]
    async fn missing_body_returns_error() {
        let exec = CommsToolExecutor::new(Arc::new(MockComms));
        let call = make_call("comms_send", serde_json::json!({
            "subject": "hello"
        }));
        let err = exec.execute(call).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }

    #[tokio::test]
    async fn defaults_are_applied() {
        let exec = CommsToolExecutor::new(Arc::new(MockComms));
        let call = make_call("comms_send", serde_json::json!({
            "body": "status update"
        }));
        let result = exec.execute(call).await.unwrap();
        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn read_returns_messages() {
        let exec = CommsToolExecutor::new(Arc::new(MockComms));
        let call = make_call("comms_read", serde_json::json!({}));
        let result = exec.execute(call).await.unwrap();
        assert!(!result.is_error);
        assert!(result.output.contains("1 message(s)"));
        assert!(result.output.contains("test directive"));
    }

    #[tokio::test]
    async fn unknown_tool_rejected() {
        let exec = CommsToolExecutor::new(Arc::new(MockComms));
        let call = make_call("comms_delete", serde_json::json!({}));
        let err = exec.execute(call).await.unwrap_err();
        assert!(matches!(err, ToolError::UnknownTool { .. }));
    }
}
