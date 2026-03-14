//! Message tool for cross-channel messaging operations.

use async_trait::async_trait;
use tracing::instrument;

use crate::definition::{ToolCall, ToolResult};
use crate::error::ToolError;
use crate::executor::ToolExecutor;

/// Trait for message delivery, implemented by the channel layer.
#[async_trait]
pub trait MessageDelivery: Send + Sync {
    /// Send a message to a channel. Returns delivery confirmation JSON.
    async fn deliver(
        &self,
        channel_id: Option<&str>,
        content: &str,
        reply_to: Option<&str>,
    ) -> Result<String, String>;
}

/// Tool executor for the `message` tool.
pub struct MessageToolExecutor<D: MessageDelivery> {
    delivery: D,
}

impl<D: MessageDelivery> MessageToolExecutor<D> {
    /// Create a new message tool executor.
    pub fn new(delivery: D) -> Self {
        Self { delivery }
    }

    #[instrument(skip(self, call), fields(tool = "message"))]
    async fn handle(&self, call: &ToolCall) -> Result<ToolResult, ToolError> {
        let content = call
            .arguments
            .get("content")
            .or_else(|| call.arguments.get("message"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolError::InvalidArgument("missing required parameter: content".into())
            })?;

        let channel_id = call.arguments.get("channelId").and_then(|v| v.as_str());
        let reply_to = call.arguments.get("replyTo").and_then(|v| v.as_str());

        match self.delivery.deliver(channel_id, content, reply_to).await {
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
impl<D: MessageDelivery> ToolExecutor for MessageToolExecutor<D> {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        match call.tool_name.as_str() {
            "message" => self.handle(&call).await,
            other => Err(ToolError::NotFound(other.into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune_core::ToolCallId;

    struct MockDelivery;

    #[async_trait]
    impl MessageDelivery for MockDelivery {
        async fn deliver(
            &self,
            channel_id: Option<&str>,
            content: &str,
            _reply_to: Option<&str>,
        ) -> Result<String, String> {
            let ch = channel_id.unwrap_or("default");
            Ok(format!(
                "{{\"delivered\": true, \"channel\": \"{ch}\", \"length\": {}}}",
                content.len()
            ))
        }
    }

    fn make_call(args: serde_json::Value) -> ToolCall {
        ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: "message".into(),
            arguments: args,
        }
    }

    #[tokio::test]
    async fn deliver_message() {
        let exec = MessageToolExecutor::new(MockDelivery);
        let call = make_call(serde_json::json!({"content": "hello world"}));
        let result = exec.execute(call).await.unwrap();
        assert!(!result.is_error);
        assert!(result.output.contains("delivered"));
    }

    #[tokio::test]
    async fn missing_content_rejected() {
        let exec = MessageToolExecutor::new(MockDelivery);
        let call = make_call(serde_json::json!({}));
        let err = exec.execute(call).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }
}
