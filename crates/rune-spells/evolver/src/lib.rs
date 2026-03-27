#![doc = "Minimal evolver spell crate scaffold so the SPELL.md manifest is backed by a workspace member."]

use async_trait::async_trait;
use rune_core::ToolCategory;
use rune_tools::{ToolCall, ToolDefinition, ToolError, ToolExecutor, ToolResult};
use serde::{Deserialize, Serialize};

pub fn evolve_status_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: "evolve_status".into(),
        description: "Report the current local evolver spell scaffold status.".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }),
        category: ToolCategory::External,
        requires_approval: false,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolverStatus {
    pub implemented: bool,
    pub message: String,
}

#[derive(Default)]
pub struct EvolverStatusExecutor;

#[async_trait]
impl ToolExecutor for EvolverStatusExecutor {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let payload = EvolverStatus {
            implemented: false,
            message: "evolver spell scaffold present; full evolution pipeline not implemented in this crate yet".into(),
        };
        let output = serde_json::to_string_pretty(&payload)
            .map_err(|e| ToolError::ExecutionFailed(format!("serialize evolver status: {e}")))?;
        Ok(ToolResult {
            tool_call_id: call.tool_call_id,
            output,
            is_error: false,
            tool_execution_id: None,
        })
    }
}
