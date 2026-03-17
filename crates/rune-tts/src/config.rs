use serde::{Deserialize, Serialize};

/// Controls when automatic TTS synthesis is triggered.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TtsAutoMode {
    /// Manual invocation only.
    #[default]
    Off,
    /// Synthesize every assistant response.
    Always,
    /// Synthesize only when the inbound message was audio.
    Inbound,
    /// Synthesize only when the response contains a `[tts]` tag.
    Tagged,
}

/// Configuration for the TTS engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TtsConfig {
    /// Whether TTS is enabled.
    pub enabled: bool,

    /// Provider identifier (`"openai"` or `"elevenlabs"`).
    pub provider: String,

    /// API key for the chosen provider.
    pub api_key: Option<String>,

    /// Default voice identifier.
    pub voice: String,

    /// Default model identifier.
    pub model: String,

    /// Auto-synthesis mode.
    pub auto_mode: TtsAutoMode,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: "openai".to_owned(),
            api_key: None,
            voice: "alloy".to_owned(),
            model: "tts-1".to_owned(),
            auto_mode: TtsAutoMode::Off,
        }
    }
}
