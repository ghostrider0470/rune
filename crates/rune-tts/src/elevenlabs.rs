use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;
use tracing::debug;

use crate::error::TtsError;
use crate::{TtsProvider, VoiceInfo};

/// ElevenLabs TTS provider.
pub struct ElevenLabsTts {
    client: Client,
    api_key: String,
}

impl ElevenLabsTts {
    /// Create a new ElevenLabs TTS provider with the given API key.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.into(),
        }
    }
}

#[async_trait]
impl TtsProvider for ElevenLabsTts {
    async fn synthesize(&self, text: &str, voice: &str, model: &str) -> Result<Vec<u8>, TtsError> {
        debug!(
            voice,
            model,
            text_len = text.len(),
            "elevenlabs tts request"
        );

        let url = format!("https://api.elevenlabs.io/v1/text-to-speech/{voice}");

        let body = json!({
            "text": text,
            "model_id": model,
        });

        let response = self
            .client
            .post(&url)
            .header("xi-api-key", &self.api_key)
            .header("Content-Type", "application/json")
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
                "ElevenLabs returned {status}: {detail}"
            )));
        }

        let bytes = response.bytes().await?;
        Ok(bytes.to_vec())
    }

    fn available_voices(&self) -> Vec<VoiceInfo> {
        // ElevenLabs voices are dynamic (fetched via API); return an empty
        // list here. Callers that need the catalogue should query the
        // ElevenLabs voice-listing endpoint directly.
        Vec::new()
    }
}
