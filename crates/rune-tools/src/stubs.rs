use async_trait::async_trait;
use rune_core::ToolCategory;

use crate::definition::{ToolCall, ToolDefinition, ToolResult};
use crate::error::ToolError;
use crate::executor::ToolExecutor;
use crate::registry::ToolRegistry;

/// A stub executor that returns a placeholder message for every call.
/// Full implementations will be wired in later waves.
pub struct StubExecutor;

#[async_trait]
impl ToolExecutor for StubExecutor {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        Ok(ToolResult {
            tool_call_id: call.tool_call_id,
            output: format!(
                "[stub] {} executed with args: {}",
                call.tool_name, call.arguments
            ),
            is_error: false,
            tool_execution_id: None,
        })
    }
}

/// Register all built-in tool stubs into the given registry.
pub fn register_builtin_stubs(registry: &mut ToolRegistry) {
    let builtins = [
        ToolDefinition {
            name: "read_file".into(),
            description: "Read the contents of a file.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file to read" },
                    "offset": { "type": "integer", "description": "Line number to start from (1-indexed)" },
                    "limit": { "type": "integer", "description": "Maximum number of lines to read" }
                },
                "required": ["path"]
            }),
            category: ToolCategory::FileRead,
            requires_approval: false,
        },
        ToolDefinition {
            name: "write_file".into(),
            description: "Write content to a file, creating it if needed.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file to write" },
                    "content": { "type": "string", "description": "Content to write" }
                },
                "required": ["path", "content"]
            }),
            category: ToolCategory::FileWrite,
            requires_approval: false,
        },
        ToolDefinition {
            name: "edit_file".into(),
            description: "Edit a file by replacing exact text.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file to edit" },
                    "old_string": { "type": "string", "description": "Exact text to find" },
                    "new_string": { "type": "string", "description": "Replacement text" }
                },
                "required": ["path", "old_string", "new_string"]
            }),
            category: ToolCategory::FileWrite,
            requires_approval: false,
        },
        ToolDefinition {
            name: "list_files".into(),
            description: "List files in a directory.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory path" },
                    "recursive": { "type": "boolean", "description": "Whether to list recursively" }
                },
                "required": ["path"]
            }),
            category: ToolCategory::FileRead,
            requires_approval: false,
        },
        ToolDefinition {
            name: "search_files".into(),
            description: "Search for text patterns in files.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Search pattern (regex)" },
                    "path": { "type": "string", "description": "Directory to search in" },
                    "include": { "type": "string", "description": "File glob to include" }
                },
                "required": ["pattern"]
            }),
            category: ToolCategory::FileRead,
            requires_approval: false,
        },
        ToolDefinition {
            name: "execute_command".into(),
            description: "Execute a shell command.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Shell command to execute" },
                    "workdir": { "type": "string", "description": "Working directory" },
                    "timeout": { "type": "integer", "description": "Timeout in seconds" }
                },
                "required": ["command"]
            }),
            category: ToolCategory::ProcessExec,
            requires_approval: true,
        },
        ToolDefinition {
            name: "list_sessions".into(),
            description: "List active sessions.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "status": { "type": "string", "description": "Filter by status" }
                }
            }),
            category: ToolCategory::SessionControl,
            requires_approval: false,
        },
        ToolDefinition {
            name: "get_session_status".into(),
            description: "Get the status of a specific session.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session ID to query" }
                },
                "required": ["session_id"]
            }),
            category: ToolCategory::SessionControl,
            requires_approval: false,
        },
    ];

    for tool in builtins {
        registry.register(tool);
    }

    // Register real tool definitions (executors are wired separately)
    registry.register(crate::web_fetch_tool::web_fetch_tool_definition());
    registry.register(crate::git_tool::git_tool_definition());
}

/// Validate that a tool call's arguments satisfy the `required` fields in the tool schema.
/// Returns `Ok(())` or a `ToolError::InvalidArguments`.
pub fn validate_arguments(def: &ToolDefinition, args: &serde_json::Value) -> Result<(), ToolError> {
    let required = def.parameters.get("required").and_then(|v| v.as_array());

    if let Some(required_fields) = required {
        let obj = args.as_object();
        for field in required_fields {
            if let Some(field_name) = field.as_str() {
                let present = obj.map(|o| o.contains_key(field_name)).unwrap_or(false);
                if !present {
                    return Err(ToolError::InvalidArguments {
                        tool: def.name.clone(),
                        reason: format!("missing required field: {field_name}"),
                    });
                }
            }
        }
    }

    Ok(())
}
