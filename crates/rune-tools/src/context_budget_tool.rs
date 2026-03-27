use rune_core::ToolCategory;

use crate::definition::ToolDefinition;

pub fn context_budget_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: "context_budget".into(),
        description: "Report current context budget usage across runtime partitions.".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {}
        }),
        category: ToolCategory::MemoryAccess,
        requires_approval: false,
    }
}

pub fn context_checkpoint_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: "context_checkpoint".into(),
        description: "Create a persistent pre-compaction checkpoint with task status, key decisions, and next step.".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "status": { "type": "string", "description": "Current task progress summary" },
                "key_decisions": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Significant decisions made so far"
                },
                "next_step": { "type": "string", "description": "Immediate next action required" }
            },
            "required": ["status", "next_step"]
        }),
        category: ToolCategory::MemoryAccess,
        requires_approval: false,
    }
}

pub fn context_gc_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: "context_gc".into(),
        description: "Trigger context garbage collection and partition-aware compaction.".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "aggressive": {
                    "type": "boolean",
                    "description": "Whether to use more aggressive compaction heuristics"
                }
            }
        }),
        category: ToolCategory::MemoryAccess,
        requires_approval: false,
    }
}
