use async_trait::async_trait;

use crate::definition::{ToolCall, ToolResult};
use crate::error::ToolError;

/// Executes a tool call and produces a result.
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    /// Execute the given tool call.
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError>;
}

/// Hook for checking whether a tool invocation requires and has received approval.
#[async_trait]
pub trait ApprovalCheck: Send + Sync {
    /// Returns `Ok(())` if the call is permitted, or an appropriate `ToolError` otherwise.
    async fn check(&self, call: &ToolCall, requires_approval: bool) -> Result<(), ToolError>;
}

/// A no-op approval check that always permits execution.
pub struct AlwaysAllow;

#[async_trait]
impl ApprovalCheck for AlwaysAllow {
    async fn check(&self, _call: &ToolCall, _requires_approval: bool) -> Result<(), ToolError> {
        Ok(())
    }
}
