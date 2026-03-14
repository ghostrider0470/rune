use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;
use tracing::debug;

use crate::error::TtsError;
use crate::{TtsProvider, VoiceInfo};

/// OpenAI TTS provider (`tts-1` / `tts-1-hd`).
pub struct OpenAiTts {
    client: Client,
    api_key: String,
}

impl OpenAiTts {
    /// Create a new OpenAI TTS provider with the given API key.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.into(),
        }
    }
}

#[async_trait]
impl TtsProvider for OpenAiTts {
    async fn synthesize(&self, text: &str, voice: &str, model: &str) -> Result<Vec<u8>, TtsError> {
        debug!(voice, model, text_len = text.len(), "openai tts request");

        let body = json!({
            "model": model,
            "input": text,
            "voice": voice,
        });

        let response = self
            .client
            .post("https://api.openai.com/v1/audio/speech")
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let detail = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown".to_owned());
            return Err(TtsError::Provider(format!(
                "OpenAI returned {status}: {detail}"
            )));
        }

        let bytes = response.bytes().await?;
        Ok(bytes.to_vec())
    }

    fn available_voices(&self) -> Vec<VoiceInfo> {
        ["alloy", "echo", "fable", "onyx", "nova", "shimmer"]
            .into_iter()
            .map(|id| VoiceInfo {
                id: id.to_owned(),
                name: id.to_owned(),
                language: None,
            })
            .collect()
    }
}
