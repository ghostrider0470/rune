use rune_models::{ChatMessage, Role};

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
#[derive(Debug)]
pub struct TokenBudgetCompaction {
    /// Maximum context window size in tokens.
    context_window: usize,
    /// Number of recent messages to always preserve verbatim.
    preserve_tail: usize,
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
        }
    }

    /// Rough token estimate: ~4 characters per token.
    fn estimate_tokens(msg: &ChatMessage) -> usize {
        let content_len = msg.content.as_deref().map_or(0, |c| c.len());
        let tool_calls_len = msg
            .tool_calls
            .as_ref()
            .map_or(0, |calls| {
                calls.iter().map(|tc| {
                    tc.function.name.len() + tc.function.arguments.len()
                }).sum()
            });
        (content_len + tool_calls_len) / 4
    }

    /// Summarize a slice of messages into a truncated plain-text summary.
    fn summarize(messages: &[ChatMessage]) -> String {
        const MAX_SUMMARY_TOKENS: usize = 2000;
        const MAX_SUMMARY_CHARS: usize = MAX_SUMMARY_TOKENS * 4;

        let mut summary = String::from("[Earlier conversation summary]\n");
        for msg in messages {
            if let Some(content) = &msg.content {
                let role = match msg.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::Tool => "tool",
                };
                summary.push_str(role);
                summary.push_str(": ");
                summary.push_str(content);
                summary.push('\n');

                if summary.len() >= MAX_SUMMARY_CHARS {
                    summary.truncate(MAX_SUMMARY_CHARS);
                    summary.push_str("...");
                    break;
                }
            }
        }
        summary
    }
}

impl Default for TokenBudgetCompaction {
    fn default() -> Self {
        Self {
            context_window: 128_000,
            preserve_tail: 20,
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
        let long_content = "x".repeat(10_000);
        let messages = vec![msg(Role::User, &long_content)];
        let summary = TokenBudgetCompaction::summarize(&messages);
        // 2000 tokens * 4 chars = 8000 chars max + "..." + header
        assert!(summary.len() <= 8100);
    }
}
