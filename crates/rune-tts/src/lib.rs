#![doc = "Text-to-speech engine for Rune: provider trait, OpenAI and ElevenLabs implementations, and auto-mode configuration."]

pub mod config;
pub mod elevenlabs;
pub mod error;
pub mod openai;

pub use config::{TtsAutoMode, TtsConfig};
pub use error::TtsError;

use async_trait::async_trait;
use tracing::info;

/// Metadata about a voice offered by a TTS provider.
#[derive(Debug, Clone)]
pub struct VoiceInfo {
    /// Provider-specific voice identifier.
    pub id: String,
    /// Human-readable voice name.
    pub name: String,
    /// Optional BCP-47 language tag (e.g. `"en-US"`).
    pub language: Option<String>,
}

/// Trait implemented by every TTS backend.
#[async_trait]
pub trait TtsProvider: Send + Sync {
    /// Synthesize `text` into audio bytes using the given `voice` and `model`.
    async fn synthesize(&self, text: &str, voice: &str, model: &str) -> Result<Vec<u8>, TtsError>;

    /// Return the list of voices this provider exposes statically.
    fn available_voices(&self) -> Vec<VoiceInfo>;
}

/// High-level TTS engine that wraps a [`TtsProvider`] with configuration and
/// an enable/disable toggle.
pub struct TtsEngine {
    provider: Box<dyn TtsProvider>,
    config: TtsConfig,
}

impl TtsEngine {
    /// Create a new engine backed by `provider`.
    pub fn new(provider: Box<dyn TtsProvider>, config: TtsConfig) -> Self {
        Self { provider, config }
    }

    /// Synthesize `text` using the configured default voice and model.
    ///
    /// Returns `Err(TtsError::Disabled)` if the engine is currently disabled.
    pub async fn convert(&self, text: &str) -> Result<Vec<u8>, TtsError> {
        if !self.config.enabled {
            return Err(TtsError::Disabled);
        }

        let api_key = self.config.api_key.as_deref().unwrap_or("");
        if api_key.is_empty() {
            return Err(TtsError::Config(
                "API key is required but not configured".to_owned(),
            ));
        }

        info!(
            provider = %self.config.provider,
            voice = %self.config.voice,
            model = %self.config.model,
            text_len = text.len(),
            "synthesizing speech",
        );

        self.provider
            .synthesize(text, &self.config.voice, &self.config.model)
            .await
    }

    /// Synthesize with explicit voice and model overrides.
    pub async fn convert_with(
        &self,
        text: &str,
        voice: &str,
        model: &str,
    ) -> Result<Vec<u8>, TtsError> {
        if !self.config.enabled {
            return Err(TtsError::Disabled);
        }

        let api_key = self.config.api_key.as_deref().unwrap_or("");
        if api_key.is_empty() {
            return Err(TtsError::Config(
                "API key is required but not configured".to_owned(),
            ));
        }

        info!(
            provider = %self.config.provider,
            voice = %voice,
            model = %model,
            text_len = text.len(),
            "synthesizing speech",
        );

        self.provider.synthesize(text, voice, model).await
    }

    /// Whether the engine is currently enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Enable TTS synthesis.
    pub fn enable(&mut self) {
        self.config.enabled = true;
    }

    /// Disable TTS synthesis.
    pub fn disable(&mut self) {
        self.config.enabled = false;
    }

    /// Return the active auto-mode setting.
    pub fn auto_mode(&self) -> &TtsAutoMode {
        &self.config.auto_mode
    }

    /// Return a reference to the current configuration.
    pub fn config(&self) -> &TtsConfig {
        &self.config
    }

    /// List voices available from the underlying provider.
    pub fn available_voices(&self) -> Vec<VoiceInfo> {
        self.provider.available_voices()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trivial provider that echoes the input back as bytes.
    struct EchoProvider;

    #[async_trait]
    impl TtsProvider for EchoProvider {
        async fn synthesize(
            &self,
            text: &str,
            _voice: &str,
            _model: &str,
        ) -> Result<Vec<u8>, TtsError> {
            Ok(text.as_bytes().to_vec())
        }

        fn available_voices(&self) -> Vec<VoiceInfo> {
            vec![VoiceInfo {
                id: "echo".to_owned(),
                name: "Echo".to_owned(),
                language: Some("en-US".to_owned()),
            }]
        }
    }

    #[tokio::test]
    async fn disabled_engine_returns_error() {
        let config = TtsConfig {
            enabled: false,
            ..Default::default()
        };
        let engine = TtsEngine::new(Box::new(EchoProvider), config);
        let err = engine.convert("hello").await.unwrap_err();
        assert!(matches!(err, TtsError::Disabled));
    }

    #[tokio::test]
    async fn enabled_engine_without_key_returns_config_error() {
        let config = TtsConfig {
            enabled: true,
            api_key: None,
            ..Default::default()
        };
        let engine = TtsEngine::new(Box::new(EchoProvider), config);
        let err = engine.convert("hello").await.unwrap_err();
        assert!(matches!(err, TtsError::Config(_)));
    }


    #[tokio::test]
    async fn convert_with_without_key_returns_config_error() {
        let config = TtsConfig {
            enabled: true,
            api_key: None,
            ..Default::default()
        };
        let engine = TtsEngine::new(Box::new(EchoProvider), config);
        let err = engine
            .convert_with("hello", "custom-voice", "custom-model")
            .await
            .unwrap_err();
        assert!(matches!(err, TtsError::Config(_)));
    }

    #[tokio::test]
    async fn enabled_engine_synthesizes() {
        let config = TtsConfig {
            enabled: true,
            api_key: Some("test-key".to_owned()),
            ..Default::default()
        };
        let engine = TtsEngine::new(Box::new(EchoProvider), config);
        let audio = engine.convert("hello world").await.unwrap();
        assert_eq!(audio, b"hello world");
    }

    #[test]
    fn enable_disable_toggle() {
        let config = TtsConfig::default();
        let mut engine = TtsEngine::new(Box::new(EchoProvider), config);

        assert!(!engine.is_enabled());
        engine.enable();
        assert!(engine.is_enabled());
        engine.disable();
        assert!(!engine.is_enabled());
    }

    #[test]
    fn available_voices_delegates() {
        let config = TtsConfig::default();
        let engine = TtsEngine::new(Box::new(EchoProvider), config);
        let voices = engine.available_voices();
        assert_eq!(voices.len(), 1);
        assert_eq!(voices[0].id, "echo");
    }
}
