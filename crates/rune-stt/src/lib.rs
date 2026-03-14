#![doc = "Speech-to-text engine for Rune: provider trait, OpenAI Whisper implementation, and runtime configuration."]

pub mod config;
pub mod error;
pub mod openai;

pub use config::SttConfig;
pub use error::SttError;

use async_trait::async_trait;
use tracing::info;

/// The result of a transcription request.
#[derive(Debug, Clone)]
pub struct TranscriptionResult {
    /// The transcribed text.
    pub text: String,
    /// Detected language (BCP-47), if the provider reports it.
    pub language: Option<String>,
    /// Duration of the input audio in seconds, if the provider reports it.
    pub duration_seconds: Option<f64>,
}

/// Trait implemented by every STT backend.
#[async_trait]
pub trait SttProvider: Send + Sync {
    /// Transcribe raw audio bytes with the given MIME type (e.g. `"audio/wav"`).
    async fn transcribe(
        &self,
        audio: &[u8],
        mime_type: &str,
    ) -> Result<TranscriptionResult, SttError>;
}

/// High-level STT engine that wraps an [`SttProvider`] with configuration and
/// an enable/disable toggle.
pub struct SttEngine {
    provider: Box<dyn SttProvider>,
    config: SttConfig,
}

impl SttEngine {
    /// Create a new engine backed by `provider`.
    pub fn new(provider: Box<dyn SttProvider>, config: SttConfig) -> Self {
        Self { provider, config }
    }

    /// Transcribe `audio` bytes with the specified MIME type.
    ///
    /// Returns `Err(SttError::Disabled)` when the engine is disabled.
    pub async fn transcribe(
        &self,
        audio: &[u8],
        mime_type: &str,
    ) -> Result<TranscriptionResult, SttError> {
        if !self.config.enabled {
            return Err(SttError::Disabled);
        }

        let api_key = self.config.api_key.as_deref().unwrap_or("");
        if api_key.is_empty() {
            return Err(SttError::Config(
                "API key is required but not configured".to_owned(),
            ));
        }

        info!(
            provider = %self.config.provider,
            model = %self.config.model,
            mime_type,
            audio_len = audio.len(),
            "transcribing audio",
        );

        self.provider.transcribe(audio, mime_type).await
    }

    /// Whether the engine is currently enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Enable STT transcription.
    pub fn enable(&mut self) {
        self.config.enabled = true;
    }

    /// Disable STT transcription.
    pub fn disable(&mut self) {
        self.config.enabled = false;
    }

    /// Return a reference to the current configuration.
    pub fn config(&self) -> &SttConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stub provider that returns a fixed transcription.
    struct StubProvider;

    #[async_trait]
    impl SttProvider for StubProvider {
        async fn transcribe(
            &self,
            _audio: &[u8],
            _mime_type: &str,
        ) -> Result<TranscriptionResult, SttError> {
            Ok(TranscriptionResult {
                text: "hello world".to_owned(),
                language: Some("en".to_owned()),
                duration_seconds: Some(1.5),
            })
        }
    }

    #[tokio::test]
    async fn disabled_engine_returns_error() {
        let config = SttConfig {
            enabled: false,
            ..Default::default()
        };
        let engine = SttEngine::new(Box::new(StubProvider), config);
        let err = engine.transcribe(b"fake", "audio/wav").await.unwrap_err();
        assert!(matches!(err, SttError::Disabled));
    }

    #[tokio::test]
    async fn enabled_engine_without_key_returns_config_error() {
        let config = SttConfig {
            enabled: true,
            api_key: None,
            ..Default::default()
        };
        let engine = SttEngine::new(Box::new(StubProvider), config);
        let err = engine.transcribe(b"fake", "audio/wav").await.unwrap_err();
        assert!(matches!(err, SttError::Config(_)));
    }

    #[tokio::test]
    async fn enabled_engine_transcribes() {
        let config = SttConfig {
            enabled: true,
            api_key: Some("test-key".to_owned()),
            ..Default::default()
        };
        let engine = SttEngine::new(Box::new(StubProvider), config);
        let result = engine.transcribe(b"fake", "audio/wav").await.unwrap();
        assert_eq!(result.text, "hello world");
        assert_eq!(result.language.as_deref(), Some("en"));
        assert_eq!(result.duration_seconds, Some(1.5));
    }

    #[test]
    fn enable_disable_toggle() {
        let config = SttConfig::default();
        let mut engine = SttEngine::new(Box::new(StubProvider), config);

        assert!(!engine.is_enabled());
        engine.enable();
        assert!(engine.is_enabled());
        engine.disable();
        assert!(!engine.is_enabled());
    }
}
