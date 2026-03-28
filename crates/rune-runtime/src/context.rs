use std::collections::HashSet;

const STABLE_PREFIX_PADDING: &str = concat!(
    "## Prompt Cache Padding\n\n",
    "This stable prefix padding exists to help upstream providers like Azure OpenAI\n",
    "cross the automatic prompt-prefix caching threshold. Keep this text deterministic\n",
    "for a given runtime build so repeated turns share the same cached prefix.\n\n",
    "Cache padding block 01. Cache padding block 02. Cache padding block 03. Cache padding block 04.\n",
    "Cache padding block 05. Cache padding block 06. Cache padding block 07. Cache padding block 08.\n",
    "Cache padding block 09. Cache padding block 10. Cache padding block 11. Cache padding block 12.\n",
    "Cache padding block 13. Cache padding block 14. Cache padding block 15. Cache padding block 16.\n",
    "Cache padding block 17. Cache padding block 18. Cache padding block 19. Cache padding block 20.\n",
    "Cache padding block 21. Cache padding block 22. Cache padding block 23. Cache padding block 24.\n",
    "Cache padding block 25. Cache padding block 26. Cache padding block 27. Cache padding block 28.\n",
    "Cache padding block 29. Cache padding block 30. Cache padding block 31. Cache padding block 32.\n",
    "Cache padding block 33. Cache padding block 34. Cache padding block 35. Cache padding block 36.\n",
    "Cache padding block 37. Cache padding block 38. Cache padding block 39. Cache padding block 40.\n",
    "Cache padding block 41. Cache padding block 42. Cache padding block 43. Cache padding block 44.\n",
    "Cache padding block 45. Cache padding block 46. Cache padding block 47. Cache padding block 48.\n",
    "Cache padding block 49. Cache padding block 50. Cache padding block 51. Cache padding block 52.\n",
    "Cache padding block 53. Cache padding block 54. Cache padding block 55. Cache padding block 56.\n",
    "Cache padding block 57. Cache padding block 58. Cache padding block 59. Cache padding block 60.\n"
);

use rune_core::{AttachmentRef, TranscriptItem};
use rune_models::{ChatMessage, FunctionCall, ImageFilePart, ImageUrlPart, MessagePart, Role, ToolCallRequest};
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

        sections.push(STABLE_PREFIX_PADDING.to_string());

        let system_content = sections.join("\n\n");

        messages.push(ChatMessage {
            role: Role::System,
            content: Some(system_content),
            content_parts: None,
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
                    Err(_) => {
                        i += 1;
                        continue;
                    }
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
                    if let TranscriptItem::ToolRequest {
                        tool_call_id,
                        tool_name,
                        arguments,
                    } = inner
                    {
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
                        content_parts: None,
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
            TranscriptItem::UserMessage { message } => {
                let content = render_user_message_content(&message.content, &message.attachments);
                let content_parts = build_user_message_parts(&message.content, &message.attachments);
                if content.trim().is_empty() && content_parts.is_none() {
                    return None;
                }
                Some(ChatMessage {
                    role: Role::User,
                    content: Some(content),
                    content_parts,
                    name: None,
                    tool_call_id: None,
                    tool_calls: None,
                })
            }
            TranscriptItem::AssistantMessage { content } => {
                if content.trim().is_empty() {
                    return None;
                }
                Some(ChatMessage {
                    role: Role::Assistant,
                    content: Some(content),
                    content_parts: None,
                    name: None,
                    tool_call_id: None,
                    tool_calls: None,
                })
            }
            // ToolRequest is handled by the grouping logic in assemble()
            TranscriptItem::ToolRequest { .. } => None,
            TranscriptItem::ToolResult {
                tool_call_id,
                output,
                ..
            } => Some(ChatMessage {
                role: Role::Tool,
                content: Some(output),
                content_parts: None,
                name: None,
                tool_call_id: Some(tool_call_id.to_string()),
                tool_calls: None,
            }),
            _ => None,
        }
    }
}

/// Ensures every `tool_call_id` in an Assistant message has a corresponding
/// `Role::Tool` response, AND every `Role::Tool` message has a preceding
/// Assistant message with a matching `tool_calls` entry. Injects synthetic
/// responses for orphaned tool calls, and removes orphaned tool results
/// (e.g. after compaction drops the assistant message but preserves the result).
fn sanitize_tool_calls(messages: &mut Vec<ChatMessage>) {
    // Pass 1: Remove orphaned Role::Tool messages that have no preceding
    // Assistant message with a matching tool_call_id. This can happen when
    // compaction drops older messages including the assistant tool_calls
    // but preserves the tool result in the tail.
    let mut known_call_ids: HashSet<String> = HashSet::new();
    let mut to_remove = Vec::new();
    for (idx, msg) in messages.iter().enumerate() {
        if let Some(ref calls) = msg.tool_calls {
            for tc in calls {
                known_call_ids.insert(tc.id.clone());
            }
        }
        if msg.role == Role::Tool {
            if let Some(ref id) = msg.tool_call_id {
                if !known_call_ids.contains(id) {
                    to_remove.push(idx);
                }
            }
        }
    }
    if !to_remove.is_empty() {
        warn!(
            orphaned_results = to_remove.len(),
            "removing orphaned tool result messages with no preceding tool_calls"
        );
        for idx in to_remove.into_iter().rev() {
            messages.remove(idx);
        }
    }

    // Pass 2: Inject synthetic tool responses for assistant tool_calls
    // that have no corresponding Role::Tool response.
    let mut i = 0;
    while i < messages.len() {
        let pending_ids: Vec<String> = match &messages[i] {
            ChatMessage {
                role: Role::Assistant,
                tool_calls: Some(calls),
                ..
            } if !calls.is_empty() => calls.iter().map(|tc| tc.id.clone()).collect(),
            _ => {
                i += 1;
                continue;
            }
        };

        let mut outstanding: HashSet<String> = pending_ids.into_iter().collect();
        let mut j = i + 1;
        while j < messages.len() && messages[j].role == Role::Tool {
            if let Some(ref id) = messages[j].tool_call_id {
                outstanding.remove(id);
            }
            j += 1;
        }

        if !outstanding.is_empty() {
            warn!(
                orphaned_count = outstanding.len(),
                "injecting synthetic tool responses for orphaned tool_call(s)"
            );
            let mut orphaned: Vec<String> = outstanding.into_iter().collect();
            orphaned.sort();
            let count = orphaned.len();
            for (offset, id) in orphaned.into_iter().enumerate() {
                messages.insert(
                    j + offset,
                    ChatMessage {
                        role: Role::Tool,
                        content: Some("[Tool call interrupted — no result available]".to_string()),
                        content_parts: None,
                        name: None,
                        tool_call_id: Some(id),
                        tool_calls: None,
                    },
                );
            }
            i = j + count;
        } else {
            i = j;
        }
    }
}


fn build_user_message_parts(content: &str, attachments: &[AttachmentRef]) -> Option<Vec<MessagePart>> {
    let trimmed = content.trim();
    let mut parts = Vec::new();

    if !trimmed.is_empty() {
        parts.push(MessagePart::Text {
            text: trimmed.to_string(),
        });
    }

    for attachment in attachments {
        let mime = attachment.mime_type.as_deref().unwrap_or("");
        if !mime.starts_with("image/") {
            continue;
        }

        if let Some(provider_file_id) = attachment.provider_file_id.as_deref() {
            parts.push(MessagePart::ImageFile {
                image_file: ImageFilePart {
                    file_id: provider_file_id.to_string(),
                },
            });
            continue;
        }

        let Some(url) = attachment.url.as_deref() else {
            continue;
        };
        parts.push(MessagePart::ImageUrl {
            image_url: ImageUrlPart {
                url: url.to_string(),
            },
        });
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts)
    }
}

fn render_user_message_content(content: &str, attachments: &[AttachmentRef]) -> String {
    let trimmed = content.trim();
    if attachments.is_empty() {
        return trimmed.to_string();
    }

    let mut rendered = String::new();
    if !trimmed.is_empty() {
        rendered.push_str(trimmed);
        rendered.push_str("\n\n");
    }

    rendered.push_str("[Attachments]\n");
    for attachment in attachments {
        rendered.push_str("- ");
        rendered.push_str(&format_attachment_ref(attachment));
        rendered.push('\n');
    }

    rendered.trim_end().to_string()
}

fn format_attachment_ref(attachment: &AttachmentRef) -> String {
    let mut line = attachment.name.clone();
    let mut details = Vec::new();

    if let Some(mime) = attachment.mime_type.as_deref() {
        details.push(mime.to_string());
    }
    if let Some(size_bytes) = attachment.size_bytes {
        details.push(format!("{} bytes", size_bytes));
    }
    if let Some(provider_file_id) = attachment.provider_file_id.as_deref() {
        details.push(format!("provider_file_id={provider_file_id}"));
    }
    if let Some(url) = attachment.url.as_deref() {
        details.push(format!("url={url}"));
    }

    if !details.is_empty() {
        line.push_str(" (");
        line.push_str(&details.join(", "));
        line.push(')');
    }

    line
}

#[cfg(test)]
mod attachment_prompt_tests {
    use super::{build_user_message_parts, format_attachment_ref, render_user_message_content, ContextAssembler};
    use rune_core::{AttachmentRef, NormalizedMessage, TranscriptItem};
    use rune_models::{MessagePart, Role};
    use rune_store::models::TranscriptItemRow;
    use uuid::Uuid;

    use crate::NoOpCompaction;

    #[test]
    fn formats_attachment_only_user_messages_into_prompt_content() {
        let item = TranscriptItem::UserMessage {
            message: NormalizedMessage {
                channel_id: None,
                sender_id: "user".into(),
                sender_display_name: None,
                message_id: Some("m1".into()),
                reply_to_message_id: None,
                content: String::new(),
                attachments: vec![AttachmentRef {
                    name: "invoice.pdf".into(),
                    mime_type: Some("application/pdf".into()),
                    size_bytes: Some(1234),
                    url: Some("https://example.test/invoice.pdf".into()),
                    provider_file_id: Some("file_123".into()),
                }],
                metadata: serde_json::Value::Null,
            },
        };
        let row = TranscriptItemRow {
            id: Uuid::now_v7(),
            session_id: Uuid::now_v7(),
            turn_id: None,
            seq: 1,
            kind: "user_message".into(),
            payload: serde_json::to_value(item).unwrap(),
            created_at: chrono::Utc::now(),
        };

        let assembler = ContextAssembler::new("system");
        let messages = assembler.assemble(&[row], &NoOpCompaction, None, None, &[]);

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].role, Role::User);
        let content = messages[1].content.as_deref().unwrap();
        assert!(content.contains("[Attachments]"));
        assert!(content.contains("invoice.pdf"));
        assert!(content.contains("application/pdf"));
        assert!(content.contains("provider_file_id=file_123"));
    }

    #[test]
    fn appends_attachment_summary_after_text_content() {
        let rendered = render_user_message_content(
            "Please summarize this",
            &[AttachmentRef {
                name: "notes.txt".into(),
                mime_type: Some("text/plain".into()),
                size_bytes: None,
                url: None,
                provider_file_id: None,
            }],
        );

        assert!(rendered.starts_with("Please summarize this\n\n[Attachments]\n- notes.txt"));
    }

    #[test]
    fn formats_attachment_ref_compactly() {
        let formatted = format_attachment_ref(&AttachmentRef {
            name: "photo.jpg".into(),
            mime_type: Some("image/jpeg".into()),
            size_bytes: Some(42),
            url: None,
            provider_file_id: Some("abc".into()),
        });

        assert_eq!(formatted, "photo.jpg (image/jpeg, 42 bytes, provider_file_id=abc)");
    }
    #[test]
    fn builds_multimodal_parts_for_user_text_and_image_attachments() {
        let parts = build_user_message_parts(
            "Describe this image",
            &[
                AttachmentRef {
                    name: "photo.jpg".into(),
                    mime_type: Some("image/jpeg".into()),
                    size_bytes: Some(42),
                    url: Some("https://example.test/photo.jpg".into()),
                    provider_file_id: None,
                },
                AttachmentRef {
                    name: "notes.txt".into(),
                    mime_type: Some("text/plain".into()),
                    size_bytes: None,
                    url: Some("https://example.test/notes.txt".into()),
                    provider_file_id: None,
                },
            ],
        )
        .expect("expected multimodal parts");

        assert!(matches!(&parts[0], MessagePart::Text { text } if text == "Describe this image"));
        assert!(matches!(&parts[1], MessagePart::ImageUrl { image_url } if image_url.url == "https://example.test/photo.jpg"));
        assert_eq!(parts.len(), 2);
    }

    #[test]
    fn builds_multimodal_parts_from_provider_file_id_when_available() {
        let parts = build_user_message_parts(
            "Describe this image",
            &[AttachmentRef {
                name: "photo.jpg".into(),
                mime_type: Some("image/jpeg".into()),
                size_bytes: Some(42),
                url: Some("telegram-file:abc123".into()),
                provider_file_id: Some("abc123".into()),
            }],
        )
        .expect("expected multimodal parts");

        assert!(matches!(&parts[0], MessagePart::Text { text } if text == "Describe this image"));
        assert!(matches!(&parts[1], MessagePart::ImageFile { image_file } if image_file.file_id == "abc123"));
        assert_eq!(parts.len(), 2);
    }

}
