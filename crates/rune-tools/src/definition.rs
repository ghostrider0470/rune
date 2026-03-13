use rune_core::{ToolCallId, ToolCategory};
use serde::{Deserialize, Serialize};

/// Describes a tool available to the agent runtime.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Unique tool name (e.g. `read_file`).
    pub name: String,
    /// Human-readable description shown to the model.
    pub description: String,
    /// JSON Schema describing accepted parameters.
    pub parameters: serde_json::Value,
    /// Coarse capability bucket.
    pub category: ToolCategory,
    /// Whether invocation requires operator approval before execution.
    pub requires_approval: bool,
}

/// An inbound tool invocation request from the model.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub tool_call_id: ToolCallId,
    pub tool_name: String,
    pub arguments: serde_json::Value,
}

/// The result of executing a tool call.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: ToolCallId,
    pub output: String,
    pub is_error: bool,
}
