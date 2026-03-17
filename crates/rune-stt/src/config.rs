use serde::{Deserialize, Serialize};

/// Configuration for the STT engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SttConfig {
    /// Whether STT is enabled.
    pub enabled: bool,

    /// Provider identifier (currently `"openai"`).
    pub provider: String,

    /// API key for the chosen provider.
    pub api_key: Option<String>,

    /// Model identifier (e.g. `"whisper-1"`).
    pub model: String,
}

impl Default for SttConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: "openai".to_owned(),
            api_key: None,
            model: "whisper-1".to_owned(),
        }
    }
}
