//! Inter-agent comms tool — lets the agent send messages to peer agents.

use std::sync::Arc;

use async_trait::async_trait;

use crate::definition::{ToolCall, ToolResult};
use crate::error::ToolError;
use crate::executor::ToolExecutor;

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
}

/// Tool executor for inter-agent comms.
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
        let args = &call.arguments;
        let to = args.get("to").and_then(|v| v.as_str()).unwrap_or("horizon-ai");
        let msg_type = args.get("type").and_then(|v| v.as_str()).unwrap_or("status");
        let subject = args.get("subject").and_then(|v| v.as_str()).unwrap_or("message from rune");
        let body = args.get("body").and_then(|v| v.as_str()).unwrap_or("");
        let priority = args.get("priority").and_then(|v| v.as_str()).unwrap_or("p1");

        if body.is_empty() {
            return Err(ToolError::InvalidArgument("body is required".into()));
        }

        match self.comms.send_message(to, msg_type, subject, body, priority).await {
            Ok(id) => Ok(ToolResult {
                tool_call_id: call.tool_call_id,
                output: format!("Message sent: {id}"),
                is_error: false,
                tool_execution_id: None,
            }),
            Err(e) => Err(ToolError::ExecutionFailed(format!("comms send failed: {e}"))),
        }
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
    }

    fn make_call(args: serde_json::Value) -> ToolCall {
        ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: "comms_send".into(),
            arguments: args,
        }
    }

    #[tokio::test]
    async fn send_returns_message_id() {
        let exec = CommsToolExecutor::new(Arc::new(MockComms));
        let call = make_call(serde_json::json!({
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
        let call = make_call(serde_json::json!({
            "subject": "hello",
            "body": ""
        }));
        let err = exec.execute(call).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }

    #[tokio::test]
    async fn missing_body_returns_error() {
        let exec = CommsToolExecutor::new(Arc::new(MockComms));
        let call = make_call(serde_json::json!({
            "subject": "hello"
        }));
        let err = exec.execute(call).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }

    #[tokio::test]
    async fn defaults_are_applied() {
        let exec = CommsToolExecutor::new(Arc::new(MockComms));
        // Only provide the required body; all other fields should use defaults
        let call = make_call(serde_json::json!({
            "body": "status update"
        }));
        let result = exec.execute(call).await.unwrap();
        assert!(!result.is_error);
    }
}
