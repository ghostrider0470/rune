use rune_core::TranscriptItem;
use rune_models::{ChatMessage, FunctionCall, Role, ToolCallRequest};
use rune_store::models::TranscriptItemRow;

use crate::compaction::CompactionStrategy;
use crate::memory::MemoryContext;
use crate::workspace::WorkspaceContext;

/// Builds the prompt messages from session history, system instructions, and context.
#[derive(Clone)]
pub struct ContextAssembler {
    system_instructions: String,
}

impl ContextAssembler {
    pub fn new(system_instructions: impl Into<String>) -> Self {
        Self {
            system_instructions: system_instructions.into(),
        }
    }

    /// Assemble prompt messages from persisted transcript rows.
    ///
    /// Produces: [system (with optional workspace + memory context)] + transcript items
    /// converted to ChatMessages, then passed through the compaction strategy.
    pub fn assemble(
        &self,
        transcript_rows: &[TranscriptItemRow],
        compaction: &dyn CompactionStrategy,
        workspace: Option<&WorkspaceContext>,
        memory: Option<&MemoryContext>,
        extra_system_sections: &[String],
    ) -> Vec<ChatMessage> {
        let mut messages = Vec::with_capacity(transcript_rows.len() + 1);

        // System message with optional workspace + memory context
        let mut sections = vec![self.system_instructions.clone()];

        if let Some(workspace) = workspace {
            let workspace_section = workspace.format_for_prompt();
            if !workspace_section.is_empty() {
                sections.push(workspace_section);
            }
        }

        if let Some(mem) = memory {
            let mem_section = mem.format_for_prompt();
            if !mem_section.is_empty() {
                sections.push(mem_section);
            }
        }

        sections.extend(
            extra_system_sections
                .iter()
                .filter(|section| !section.trim().is_empty())
                .cloned(),
        );

        let system_content = sections.join("\n\n");

        messages.push(ChatMessage {
            role: Role::System,
            content: Some(system_content),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        });

        // Convert transcript rows to chat messages
        for row in transcript_rows {
            if let Some(msg) = self.row_to_chat_message(row) {
                messages.push(msg);
            }
        }

        compaction.compact(messages)
    }

    fn row_to_chat_message(&self, row: &TranscriptItemRow) -> Option<ChatMessage> {
        let item: TranscriptItem = serde_json::from_value(row.payload.clone()).ok()?;

        match item {
            TranscriptItem::UserMessage { message } => Some(ChatMessage {
                role: Role::User,
                content: Some(message.content),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            }),
            TranscriptItem::AssistantMessage { content } => Some(ChatMessage {
                role: Role::Assistant,
                content: Some(content),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            }),
            TranscriptItem::ToolRequest {
                tool_call_id,
                tool_name,
                arguments,
            } => Some(ChatMessage {
                role: Role::Assistant,
                content: None,
                name: None,
                tool_call_id: None,
                tool_calls: Some(vec![ToolCallRequest {
                    id: tool_call_id.to_string(),
                    call_type: "function".to_string(),
                    function: FunctionCall {
                        name: tool_name,
                        arguments: arguments.to_string(),
                    },
                }]),
            }),
            TranscriptItem::ToolResult {
                tool_call_id,
                output,
                ..
            } => Some(ChatMessage {
                role: Role::Tool,
                content: Some(output),
                name: None,
                tool_call_id: Some(tool_call_id.to_string()),
                tool_calls: None,
            }),
            _ => None,
        }
    }
}
