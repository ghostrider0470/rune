use rune_models::ChatMessage;

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
