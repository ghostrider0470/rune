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

    /// Optional base URL for OpenAI-compatible providers.
    pub base_url: Option<String>,

    /// Optional API version for Azure OpenAI-compatible endpoints.
    pub api_version: Option<String>,

    /// Model identifier (e.g. `"gpt-4o-mini-transcribe"`).
    pub model: String,
}

impl Default for SttConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: "openai".to_owned(),
            api_key: None,
            base_url: None,
            api_version: None,
            model: "gpt-4o-mini-transcribe".to_owned(),
        }
    }
}
