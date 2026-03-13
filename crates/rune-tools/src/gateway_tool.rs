//! Gateway management tool.

use async_trait::async_trait;
use tracing::instrument;

use crate::definition::{ToolCall, ToolResult};
use crate::error::ToolError;
use crate::executor::ToolExecutor;

/// Trait for gateway management operations.
#[async_trait]
pub trait GatewayControl: Send + Sync {
    /// Get gateway status. Returns JSON.
    async fn status(&self) -> Result<String, String>;
    /// Start the gateway.
    async fn start(&self) -> Result<String, String>;
    /// Stop the gateway.
    async fn stop(&self) -> Result<String, String>;
    /// Restart the gateway.
    async fn restart(&self) -> Result<String, String>;
}

/// Tool executor for gateway management.
pub struct GatewayToolExecutor<G: GatewayControl> {
    gateway: G,
}

impl<G: GatewayControl> GatewayToolExecutor<G> {
    /// Create a new gateway tool executor.
    pub fn new(gateway: G) -> Self {
        Self { gateway }
    }

    #[instrument(skip(self, call), fields(tool = "gateway"))]
    async fn handle(&self, call: &ToolCall) -> Result<ToolResult, ToolError> {
        let action = call
            .arguments
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolError::InvalidArgument("missing required parameter: action".into())
            })?;

        let result = match action {
            "status" => self.gateway.status().await,
            "start" => self.gateway.start().await,
            "stop" => self.gateway.stop().await,
            "restart" => self.gateway.restart().await,
            other => {
                return Err(ToolError::InvalidArgument(format!(
                    "unknown gateway action: {other}"
                )));
            }
        };

        match result {
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
impl<G: GatewayControl> ToolExecutor for GatewayToolExecutor<G> {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        match call.tool_name.as_str() {
            "gateway" => self.handle(&call).await,
            other => Err(ToolError::NotFound(other.into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune_core::ToolCallId;

    struct MockGateway;

    #[async_trait]
    impl GatewayControl for MockGateway {
        async fn status(&self) -> Result<String, String> {
            Ok("{\"status\": \"running\", \"uptime\": 3600}".into())
        }
        async fn start(&self) -> Result<String, String> {
            Ok("{\"action\": \"start\", \"ok\": true}".into())
        }
        async fn stop(&self) -> Result<String, String> {
            Ok("{\"action\": \"stop\", \"ok\": true}".into())
        }
        async fn restart(&self) -> Result<String, String> {
            Ok("{\"action\": \"restart\", \"ok\": true}".into())
        }
    }

    fn make_call(args: serde_json::Value) -> ToolCall {
        ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: "gateway".into(),
            arguments: args,
        }
    }

    #[tokio::test]
    async fn status_returns_info() {
        let exec = GatewayToolExecutor::new(MockGateway);
        let call = make_call(serde_json::json!({"action": "status"}));
        let result = exec.execute(call).await.unwrap();
        assert!(result.output.contains("running"));
    }

    #[tokio::test]
    async fn restart_works() {
        let exec = GatewayToolExecutor::new(MockGateway);
        let call = make_call(serde_json::json!({"action": "restart"}));
        let result = exec.execute(call).await.unwrap();
        assert!(result.output.contains("restart"));
    }

    #[tokio::test]
    async fn unknown_action_rejected() {
        let exec = GatewayToolExecutor::new(MockGateway);
        let call = make_call(serde_json::json!({"action": "explode"}));
        let err = exec.execute(call).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }
}
