use rune_core::TranscriptItem;
use rune_models::{ChatMessage, FunctionCall, Role, ToolCallRequest};
use rune_store::models::TranscriptItemRow;

use crate::compaction::CompactionStrategy;

/// Builds the prompt messages from session history, system instructions, and context.
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
    /// Produces: [system] + transcript items converted to ChatMessages,
    /// then passed through the compaction strategy.
    pub fn assemble(
        &self,
        transcript_rows: &[TranscriptItemRow],
        compaction: &dyn CompactionStrategy,
    ) -> Vec<ChatMessage> {
        let mut messages = Vec::with_capacity(transcript_rows.len() + 1);

        // System message
        messages.push(ChatMessage {
            role: Role::System,
            content: Some(self.system_instructions.clone()),
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
        // Deserialize the payload to a TranscriptItem to get the typed variant
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
            // Status notes, approval items, subagent results don't map to chat messages
            _ => None,
        }
    }
}
