use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde_json::json;
use tracing::debug;

use crate::error::TtsError;
use crate::{TtsProvider, VoiceInfo};

/// OpenAI-compatible text-to-speech provider implementation.
pub struct OpenAiTts {
    client: Client,
    api_key: String,
    base_url: String,
    api_version: Option<String>,
}

impl OpenAiTts {
    /// Create a new OpenAI-compatible TTS provider with the given API key.
    pub fn new(
        api_key: impl Into<String>,
        base_url: impl Into<String>,
        api_version: Option<String>,
    ) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.into(),
            base_url: normalize_base_url(&base_url.into()),
            api_version,
        }
    }
}

fn normalize_base_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        "https://api.openai.com/v1".to_string()
    } else {
        trimmed.to_string()
    }
}

fn build_speech_url(base_url: &str, model: &str, api_version: Option<&str>) -> String {
    if base_url.contains("cognitiveservices.azure.com") || base_url.contains("openai.azure.com") {
        let version = api_version.unwrap_or("2025-03-01-preview");
        let root = base_url
            .trim_end_matches('/')
            .trim_end_matches("/openai/v1")
            .trim_end_matches("/openai");
        format!("{root}/openai/deployments/{model}/audio/speech?api-version={version}")
    } else {
        format!("{}/audio/speech", base_url.trim_end_matches('/'))
    }
}

#[async_trait]
impl TtsProvider for OpenAiTts {
    async fn synthesize(&self, text: &str, voice: &str, model: &str) -> Result<Vec<u8>, TtsError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(TtsError::Config(
                "text-to-speech input is empty".to_string(),
            ));
        }

        let url = build_speech_url(&self.base_url, model, self.api_version.as_deref());

        debug!(voice, model, text_len = text.len(), base_url = %self.base_url, url = %url, "openai-compatible tts request");

        let body = json!({
            "model": model,
            "voice": voice,
            "input": text,
            "format": "mp3"
        });

        let response = self
            .client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        if status != StatusCode::OK {
            let body = response.text().await.unwrap_or_default();
            return Err(TtsError::Provider(format!(
                "OpenAI TTS request failed with status {status}: {body}"
            )));
        }

        let audio = response.bytes().await?;
        if audio.is_empty() {
            return Err(TtsError::Provider(
                "OpenAI TTS returned an empty audio payload".to_string(),
            ));
        }

        Ok(audio.to_vec())
    }

    fn available_voices(&self) -> Vec<VoiceInfo> {
        [
            "alloy", "ash", "coral", "echo", "fable", "onyx", "nova", "sage", "shimmer",
        ]
        .into_iter()
        .map(|id| VoiceInfo {
            id: id.to_string(),
            name: id[..1].to_uppercase() + &id[1..],
            language: None,
        })
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{build_speech_url, normalize_base_url};

    #[test]
    fn normalizes_empty_base_url_to_openai_default() {
        assert_eq!(normalize_base_url("   "), "https://api.openai.com/v1");
    }

    #[test]
    fn trims_trailing_slash_from_base_url() {
        assert_eq!(
            normalize_base_url("https://api.openai.com/v1/"),
            "https://api.openai.com/v1"
        );
    }

    #[test]
    fn builds_openai_speech_url() {
        assert_eq!(
            build_speech_url("https://api.openai.com/v1", "tts-1", None),
            "https://api.openai.com/v1/audio/speech"
        );
    }

    #[test]
    fn builds_azure_speech_url_from_openai_v1_base() {
        assert_eq!(
            build_speech_url(
                "https://example.openai.azure.com/openai/v1",
                "gpt-4o-mini-tts",
                Some("2025-03-01-preview")
            ),
            "https://example.openai.azure.com/openai/deployments/gpt-4o-mini-tts/audio/speech?api-version=2025-03-01-preview"
        );
    }

    #[test]
    fn builds_azure_speech_url_from_openai_base() {
        assert_eq!(
            build_speech_url(
                "https://example.cognitiveservices.azure.com/openai",
                "my-tts-deployment",
                None
            ),
            "https://example.cognitiveservices.azure.com/openai/deployments/my-tts-deployment/audio/speech?api-version=2025-03-01-preview"
        );
    }
}
