use std::path::{Path, PathBuf};

use chrono::Utc;
use rune_models::{ChatMessage, Role};
use tracing::{debug, warn};

/// Strategy for compacting/pruning transcript history before prompt assembly.
pub trait CompactionStrategy: Send + Sync {
    /// Compact the given messages, returning a (possibly shorter) message list.
    /// The implementation may summarize, drop, or leave messages unchanged.
    fn compact(&self, messages: Vec<ChatMessage>) -> Vec<ChatMessage>;
}

/// No-op compaction: passes messages through unchanged.
#[derive(Debug, Default)]
pub struct NoOpCompaction;

impl CompactionStrategy for NoOpCompaction {
    fn compact(&self, messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
        messages
    }
}

/// Token-budget compaction: when estimated tokens exceed a threshold, the
/// oldest messages beyond a preserved tail are summarized into a single
/// system message.
///
/// When a `workspace_root` is configured, compaction also flushes a structured
/// summary to today's daily memory notes (`memory/YYYY-MM-DD.md`) so that
/// context is not permanently lost when messages are dropped.
#[derive(Debug)]
pub struct TokenBudgetCompaction {
    /// Maximum context window size in tokens.
    context_window: usize,
    /// Number of recent messages to always preserve verbatim.
    preserve_tail: usize,
    /// Workspace root for memory flush. When `Some`, compaction writes a
    /// summary to today's daily notes before dropping messages.
    workspace_root: Option<PathBuf>,
}

impl TokenBudgetCompaction {
    /// Create a new token-budget compaction strategy.
    ///
    /// - `context_window`: model context window size in tokens (default 128000)
    /// - `preserve_tail`: number of recent messages to keep verbatim (default 20)
    pub fn new(context_window: usize, preserve_tail: usize) -> Self {
        Self {
            context_window,
            preserve_tail,
            workspace_root: None,
        }
    }

    /// Enable memory flush: compaction will write a summary to today's daily
    /// notes before dropping messages.
    pub fn with_memory_flush(mut self, workspace_root: impl Into<PathBuf>) -> Self {
        self.workspace_root = Some(workspace_root.into());
        self
    }

    fn truncate_utf8_boundary(text: &str, max_len: usize) -> usize {
        if max_len >= text.len() {
            return text.len();
        }
        let mut idx = max_len;
        while idx > 0 && !text.is_char_boundary(idx) {
            idx -= 1;
        }
        idx
    }

    /// Estimate token count using word/punctuation splitting for better accuracy
    /// than the naive 4-chars-per-token heuristic.
    ///
    /// Splits on whitespace and punctuation boundaries, then counts resulting
    /// fragments. This approximates BPE tokenization more closely (~1.3 tokens
    /// per whitespace-delimited word for English text).
    fn estimate_tokens(msg: &ChatMessage) -> usize {
        fn count_text_tokens(text: &str) -> usize {
            if text.is_empty() {
                return 0;
            }
            let mut tokens = 0usize;
            for word in text.split_whitespace() {
                if word.len() <= 4 {
                    tokens += 1;
                } else {
                    // Longer words are often split into sub-word tokens by BPE.
                    // Approximate: 1 token per 4 chars, minimum 1.
                    tokens += word.len().div_ceil(4);
                }
            }
            // Account for punctuation and special characters getting their own tokens
            tokens += text.chars().filter(|c| c.is_ascii_punctuation()).count() / 2;
            tokens.max(1)
        }

        let content_tokens = msg
            .content
            .as_deref()
            .map_or(0, count_text_tokens);
        let tool_calls_tokens = msg
            .tool_calls
            .as_ref()
            .map_or(0, |calls| {
                calls.iter().map(|tc| {
                    count_text_tokens(&tc.function.name)
                        + count_text_tokens(&tc.function.arguments)
                }).sum()
            });
        content_tokens + tool_calls_tokens
    }

    /// Summarize a slice of messages into a structured extraction.
    ///
    /// Extracts key topics, decisions, and questions from the conversation
    /// rather than blindly concatenating raw text, preserving semantic density.
    fn summarize(messages: &[ChatMessage]) -> String {
        const MAX_SUMMARY_CHARS: usize = 6000;

        let mut summary = String::from(
            "[Earlier conversation summary]: The following is a structured summary \
             of earlier messages that were compacted to save context space.\n\n",
        );

        // Extract user questions/requests and assistant answers/decisions
        for msg in messages {
            let Some(content) = &msg.content else { continue };
            let trimmed = content.trim();
            if trimmed.is_empty() {
                continue;
            }

            let (prefix, max_excerpt) = match msg.role {
                Role::User => ("User asked: ", 200),
                Role::Assistant => ("Assistant responded: ", 300),
                Role::Tool => ("Tool returned: ", 150),
                Role::System => continue,
            };

            let excerpt = if trimmed.len() > max_excerpt {
                format!(
                    "{}...",
                    &trimmed[..Self::truncate_utf8_boundary(trimmed, max_excerpt)]
                )
            } else {
                trimmed.to_string()
            };

            let line = format!("- {prefix}{excerpt}\n");
            if summary.len() + line.len() > MAX_SUMMARY_CHARS {
                summary.push_str("- (earlier context truncated)\n");
                break;
            }
            summary.push_str(&line);
        }

        summary
    }

    /// Create a structured memory flush note from messages being dropped.
    ///
    /// Extracts key points from the conversation: user questions, assistant
    /// decisions/answers, and tool results — producing a concise note suitable
    /// for daily memory.
    fn build_flush_note(messages: &[ChatMessage]) -> String {
        const MAX_FLUSH_CHARS: usize = 4000;

        let mut note = String::from("\n## [Session context summary]\n\n");
        let mut chars_written = note.len();

        for msg in messages {
            let Some(content) = &msg.content else {
                continue;
            };

            let trimmed = content.trim();
            if trimmed.is_empty() {
                continue;
            }

            let prefix = match msg.role {
                Role::User => "- **User**: ",
                Role::Assistant => "- **Assistant**: ",
                Role::Tool => "- **Tool**: ",
                Role::System => continue, // Skip system messages in flush
            };

            // Truncate long individual messages to keep the note concise
            let max_msg_len = 300;
            let excerpt = if trimmed.len() > max_msg_len {
                format!("{}...", &trimmed[..Self::truncate_utf8_boundary(trimmed, max_msg_len)])
            } else {
                trimmed.to_string()
            };

            let line = format!("{prefix}{excerpt}\n");
            if chars_written + line.len() > MAX_FLUSH_CHARS {
                note.push_str("- *(truncated — more context was dropped)*\n");
                break;
            }
            note.push_str(&line);
            chars_written += line.len();
        }

        note
    }

    /// Flush a summary to today's daily memory notes.
    fn flush_to_memory(workspace_root: &Path, flush_note: &str) {
        let today = Utc::now().date_naive();
        let memory_dir = workspace_root.join("memory");
        let daily_path = memory_dir.join(format!("{}.md", today.format("%Y-%m-%d")));

        // Ensure memory directory exists
        if let Err(e) = std::fs::create_dir_all(&memory_dir) {
            warn!(error = %e, "failed to create memory directory for compaction flush");
            return;
        }

        // Append to existing daily notes, or create new file
        let existing = std::fs::read_to_string(&daily_path).unwrap_or_default();
        let new_content = if existing.is_empty() {
            format!("# {}\n{flush_note}", today.format("%Y-%m-%d"))
        } else {
            format!("{existing}{flush_note}")
        };

        match std::fs::write(&daily_path, new_content) {
            Ok(()) => debug!(
                path = %daily_path.display(),
                "flushed compaction summary to daily memory"
            ),
            Err(e) => warn!(
                error = %e,
                path = %daily_path.display(),
                "failed to flush compaction summary to daily memory"
            ),
        }
    }
}

impl Default for TokenBudgetCompaction {
    fn default() -> Self {
        Self {
            context_window: 128_000,
            preserve_tail: 20,
            workspace_root: None,
        }
    }
}

impl CompactionStrategy for TokenBudgetCompaction {
    fn compact(&self, messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
        let total_tokens: usize = messages.iter().map(Self::estimate_tokens).sum();
        let threshold = (self.context_window * 80) / 100;

        if total_tokens <= threshold {
            return messages;
        }

        let len = messages.len();
        if len <= self.preserve_tail {
            // Not enough messages to compact — keep all.
            return messages;
        }

        let split_at = len - self.preserve_tail;
        let (old, recent) = messages.split_at(split_at);

        // Memory flush: persist a structured summary before dropping messages
        if let Some(workspace_root) = &self.workspace_root {
            let flush_note = Self::build_flush_note(old);
            Self::flush_to_memory(workspace_root, &flush_note);
        }

        let summary_text = Self::summarize(old);
        let summary_msg = ChatMessage {
            role: Role::System,
            content: Some(summary_text),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        };

        let mut result = Vec::with_capacity(1 + recent.len());
        result.push(summary_msg);
        result.extend_from_slice(recent);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn msg(role: Role, content: &str) -> ChatMessage {
        ChatMessage {
            role,
            content: Some(content.to_string()),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }
    }

    #[test]
    fn noop_passes_through() {
        let messages = vec![msg(Role::User, "hello")];
        let result = NoOpCompaction.compact(messages.clone());
        assert_eq!(result.len(), messages.len());
    }

    #[test]
    fn under_budget_passes_through() {
        let compaction = TokenBudgetCompaction::new(128_000, 20);
        let messages = vec![
            msg(Role::User, "hello"),
            msg(Role::Assistant, "hi there"),
        ];
        let result = compaction.compact(messages.clone());
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn over_budget_compacts_old_messages() {
        // Use a tiny context window so we can trigger compaction easily.
        let compaction = TokenBudgetCompaction::new(100, 2);
        let mut messages = Vec::new();
        // Create enough messages to exceed 80 tokens (80% of 100)
        for i in 0..30 {
            messages.push(msg(Role::User, &format!("message number {} with some padding text to inflate token count", i)));
        }
        let result = compaction.compact(messages);
        // Should have 1 summary + 2 preserved tail messages = 3
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].role, Role::System);
        assert!(result[0].content.as_ref().unwrap().starts_with("[Earlier conversation summary]"));
        // Structured summary should contain user extractions
        assert!(result[0].content.as_ref().unwrap().contains("User asked:"));
    }

    #[test]
    fn preserves_tail_when_few_messages() {
        let compaction = TokenBudgetCompaction::new(10, 20);
        let messages = vec![
            msg(Role::User, "a"),
            msg(Role::Assistant, "b"),
        ];
        // Even if over threshold, if len <= preserve_tail, keep all
        let result = compaction.compact(messages.clone());
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn summary_truncates_long_content() {
        let long_content = "x ".repeat(5_000);
        let messages = vec![msg(Role::User, &long_content)];
        let summary = TokenBudgetCompaction::summarize(&messages);
        // Structured summary: 6000 chars max + header
        assert!(summary.len() <= 6200);
    }

    // -- Memory flush tests --

    #[test]
    fn flush_note_captures_user_and_assistant() {
        let messages = vec![
            msg(Role::User, "What database should we use?"),
            msg(Role::Assistant, "I recommend PostgreSQL for this workload."),
            msg(Role::User, "What about SQLite?"),
            msg(Role::Assistant, "SQLite works for single-node, but PG scales better."),
        ];

        let note = TokenBudgetCompaction::build_flush_note(&messages);
        assert!(note.contains("[Session context summary]"));
        assert!(note.contains("**User**"));
        assert!(note.contains("**Assistant**"));
        assert!(note.contains("database"));
        assert!(note.contains("PostgreSQL"));
    }

    #[test]
    fn flush_note_skips_system_messages() {
        let messages = vec![
            msg(Role::System, "You are a helpful assistant."),
            msg(Role::User, "Hello"),
        ];

        let note = TokenBudgetCompaction::build_flush_note(&messages);
        assert!(!note.contains("helpful assistant"));
        assert!(note.contains("Hello"));
    }

    #[test]
    fn flush_note_truncates_long_messages() {
        let long = "x".repeat(500);
        let messages = vec![msg(Role::User, &long)];

        let note = TokenBudgetCompaction::build_flush_note(&messages);
        // Should be truncated to ~300 chars + "..."
        assert!(note.contains("..."));
        assert!(note.len() < 500);
    }

    #[test]
    fn compaction_flushes_to_daily_memory() {
        let tmp = TempDir::new().unwrap();
        let compaction = TokenBudgetCompaction::new(100, 2)
            .with_memory_flush(tmp.path());

        let mut messages = Vec::new();
        for i in 0..30 {
            let role = if i % 2 == 0 { Role::User } else { Role::Assistant };
            messages.push(msg(role, &format!("message {} with padding to inflate token count for compaction", i)));
        }

        let result = compaction.compact(messages);
        assert_eq!(result.len(), 3); // 1 summary + 2 tail

        // Check that a daily memory file was created
        let today = Utc::now().date_naive();
        let daily_path = tmp.path().join("memory").join(format!("{}.md", today.format("%Y-%m-%d")));
        assert!(daily_path.exists(), "daily memory file should exist after compaction flush");

        let content = std::fs::read_to_string(&daily_path).unwrap();
        assert!(content.contains("[Session context summary]"));
        assert!(content.contains("message"));
    }

    #[test]
    fn compaction_appends_to_existing_daily_memory() {
        let tmp = TempDir::new().unwrap();
        let memory_dir = tmp.path().join("memory");
        std::fs::create_dir_all(&memory_dir).unwrap();

        let today = Utc::now().date_naive();
        let daily_path = memory_dir.join(format!("{}.md", today.format("%Y-%m-%d")));
        std::fs::write(&daily_path, "# Existing notes\n\n- Did some work\n").unwrap();

        let compaction = TokenBudgetCompaction::new(100, 2)
            .with_memory_flush(tmp.path());

        let mut messages = Vec::new();
        for i in 0..30 {
            messages.push(msg(Role::User, &format!("message {} with padding to inflate token count for compaction", i)));
        }

        compaction.compact(messages);

        let content = std::fs::read_to_string(&daily_path).unwrap();
        assert!(content.starts_with("# Existing notes"));
        assert!(content.contains("[Session context summary]"));
    }

    #[test]
    fn no_flush_without_workspace_root() {
        // Default compaction without workspace_root should not crash
        let compaction = TokenBudgetCompaction::new(100, 2);
        let mut messages = Vec::new();
        for i in 0..30 {
            messages.push(msg(Role::User, &format!("message {} with padding to inflate token count", i)));
        }

        let result = compaction.compact(messages);
        assert_eq!(result.len(), 3);
    }
}
