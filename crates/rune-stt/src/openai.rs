use async_trait::async_trait;
use reqwest::Client;
use reqwest::multipart::{Form, Part};
use serde::Deserialize;
use tracing::debug;

use crate::error::SttError;
use crate::{SttProvider, TranscriptionResult};

/// Response body from the OpenAI transcription endpoint.
#[derive(Debug, Deserialize)]
struct WhisperResponse {
    text: String,
}

/// OpenAI Whisper STT provider.
pub struct OpenAiStt {
    client: Client,
    api_key: String,
}

impl OpenAiStt {
    /// Create a new OpenAI STT provider with the given API key.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.into(),
        }
    }
}

/// Map a MIME type to the file extension that the Whisper API expects in the
/// multipart `filename` field.
fn extension_for_mime(mime_type: &str) -> Result<&'static str, SttError> {
    match mime_type {
        "audio/mpeg" | "audio/mp3" => Ok("mp3"),
        "audio/mp4" | "audio/m4a" => Ok("m4a"),
        "audio/wav" | "audio/x-wav" => Ok("wav"),
        "audio/webm" => Ok("webm"),
        "audio/flac" | "audio/x-flac" => Ok("flac"),
        "audio/ogg" => Ok("ogg"),
        other => Err(SttError::UnsupportedFormat(other.to_owned())),
    }
}

#[async_trait]
impl SttProvider for OpenAiStt {
    async fn transcribe(
        &self,
        audio: &[u8],
        mime_type: &str,
    ) -> Result<TranscriptionResult, SttError> {
        let ext = extension_for_mime(mime_type)?;
        let filename = format!("audio.{ext}");

        debug!(
            mime_type,
            audio_len = audio.len(),
            "openai whisper transcription request",
        );

        let file_part = Part::bytes(audio.to_vec())
            .file_name(filename)
            .mime_str(mime_type)
            .map_err(|e| SttError::Provider(format!("failed to set MIME type: {e}")))?;

        let form = Form::new()
            .text("model", "whisper-1")
            .part("file", file_part);

        let response = self
            .client
            .post("https://api.openai.com/v1/audio/transcriptions")
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let detail = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown".to_owned());
            return Err(SttError::Provider(format!(
                "OpenAI returned {status}: {detail}"
            )));
        }

        let whisper: WhisperResponse = response.json().await?;

        Ok(TranscriptionResult {
            text: whisper.text,
            language: None,
            duration_seconds: None,
        })
    }
}
