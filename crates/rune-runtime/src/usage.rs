use rune_models::Usage;

/// Accumulates token usage across multiple model calls within a turn.
#[derive(Clone, Debug, Default)]
pub struct UsageAccumulator {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub cached_prompt_tokens: u32,
    pub model_calls: u32,
}

impl UsageAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, usage: &Usage) {
        self.prompt_tokens += usage.prompt_tokens;
        self.completion_tokens += usage.completion_tokens;
        self.total_tokens += usage.total_tokens;
        self.cached_prompt_tokens += usage.cached_prompt_tokens;
        self.model_calls += 1;
    }
}
