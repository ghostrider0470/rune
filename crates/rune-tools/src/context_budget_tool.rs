use rune_core::ToolCategory;

use crate::definition::ToolDefinition;

pub fn context_budget_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: "context_budget".into(),
        description: "Report current context budget usage across runtime partitions, including overall usage, partition-level distribution, and recent compaction/checkpoint timestamps.".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "include_partitions": {
                    "type": "boolean",
                    "description": "Whether to include per-partition usage details in the report",
                    "default": true
                }
            },
            "additionalProperties": false
        }),
        category: ToolCategory::MemoryAccess,
        requires_approval: false,
    }
}

pub fn context_checkpoint_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: "context_checkpoint".into(),
        description: "Create a persistent pre-compaction checkpoint that records current task status, key decisions, and the immediate next step for restart continuity.".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "description": "Current task progress summary",
                    "minLength": 1
                },
                "key_decisions": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Significant decisions made so far",
                    "default": []
                },
                "next_step": {
                    "type": "string",
                    "description": "Immediate next action required",
                    "minLength": 1
                }
            },
            "required": ["status", "next_step"],
            "additionalProperties": false
        }),
        category: ToolCategory::MemoryAccess,
        requires_approval: false,
    }
}

pub fn context_gc_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: "context_gc".into(),
        description: "Trigger context garbage collection and partition-aware compaction, optionally using more aggressive cleanup heuristics when budgets are under pressure.".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "aggressive": {
                    "type": "boolean",
                    "description": "Whether to use more aggressive compaction heuristics",
                    "default": false
                },
                "create_checkpoint": {
                    "type": "boolean",
                    "description": "Whether to create a continuity checkpoint before compaction",
                    "default": true
                }
            },
            "additionalProperties": false
        }),
        category: ToolCategory::MemoryAccess,
        requires_approval: false,
    }
}
