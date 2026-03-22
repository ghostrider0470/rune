use std::collections::HashSet;

use rune_core::TranscriptItem;
use rune_models::{ChatMessage, FunctionCall, Role, ToolCallRequest};
use rune_store::models::TranscriptItemRow;
use tracing::warn;

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

        // Convert transcript rows to chat messages.
        // Group consecutive ToolRequest items into a single Assistant message
        // with multiple tool_calls, as the OpenAI API requires.
        let mut i = 0;
        while i < transcript_rows.len() {
            let item: TranscriptItem =
                match serde_json::from_value(transcript_rows[i].payload.clone()) {
                    Ok(item) => item,
                    Err(_) => { i += 1; continue; }
                };

            if matches!(item, TranscriptItem::ToolRequest { .. }) {
                // Collect consecutive ToolRequests into one assistant message
                let mut tool_calls = Vec::new();
                while i < transcript_rows.len() {
                    let inner: TranscriptItem =
                        match serde_json::from_value(transcript_rows[i].payload.clone()) {
                            Ok(item) => item,
                            Err(_) => break,
                        };
                    if let TranscriptItem::ToolRequest { tool_call_id, tool_name, arguments } = inner {
                        tool_calls.push(ToolCallRequest {
                            id: tool_call_id.to_string(),
                            call_type: "function".to_string(),
                            function: FunctionCall {
                                name: tool_name,
                                arguments: arguments.to_string(),
                            },
                        });
                        i += 1;
                    } else {
                        break;
                    }
                }
                if !tool_calls.is_empty() {
                    messages.push(ChatMessage {
                        role: Role::Assistant,
                        content: None,
                        name: None,
                        tool_call_id: None,
                        tool_calls: Some(tool_calls),
                    });
                }
            } else if let Some(msg) = self.item_to_chat_message(item) {
                messages.push(msg);
                i += 1;
            } else {
                i += 1;
            }
        }

        sanitize_tool_calls(&mut messages);
        compaction.compact(messages)
    }

    fn item_to_chat_message(&self, item: TranscriptItem) -> Option<ChatMessage> {
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
            // ToolRequest is handled by the grouping logic in assemble()
            TranscriptItem::ToolRequest { .. } => None,
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

/// Ensures every `tool_call_id` in an Assistant message has a corresponding
/// `Role::Tool` response. Injects synthetic tool responses for any orphaned
/// tool calls. Model-agnostic — all providers require this invariant.
fn sanitize_tool_calls(messages: &mut Vec<ChatMessage>) {
    let mut i = 0;
    while i < messages.len() {
        let pending_ids: Vec<String> = match &messages[i] {
            ChatMessage { role: Role::Assistant, tool_calls: Some(calls), .. } if !calls.is_empty() => {
                calls.iter().map(|tc| tc.id.clone()).collect()
            }
            _ => { i += 1; continue; }
        };

        let mut outstanding: HashSet<String> = pending_ids.into_iter().collect();
        let mut j = i + 1;
        while j < messages.len() && messages[j].role == Role::Tool {
            if let Some(ref id) = messages[j].tool_call_id { outstanding.remove(id); }
            j += 1;
        }

        if !outstanding.is_empty() {
            warn!(orphaned_count = outstanding.len(), "injecting synthetic tool responses for orphaned tool_call(s)");
            let mut orphaned: Vec<String> = outstanding.into_iter().collect();
            orphaned.sort();
            let count = orphaned.len();
            for (offset, id) in orphaned.into_iter().enumerate() {
                messages.insert(j + offset, ChatMessage {
                    role: Role::Tool,
                    content: Some("[Tool call interrupted — no result available]".to_string()),
                    name: None,
                    tool_call_id: Some(id),
                    tool_calls: None,
                });
            }
            i = j + count;
        } else {
            i = j;
        }
    }
}
