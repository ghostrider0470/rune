use async_trait::async_trait;
use reqwest::Client;
use reqwest::multipart::{Form, Part};
use serde::Deserialize;
use tracing::debug;

use crate::error::SttError;
use crate::{SttProvider, TranscriptionResult};

#[derive(Debug, Deserialize)]
struct WhisperResponse {
    text: String,
}

pub struct OpenAiStt {
    client: Client,
    api_key: String,
    base_url: String,
    api_version: Option<String>,
    model: String,
}

impl OpenAiStt {
    pub fn new(
        api_key: impl Into<String>,
        base_url: impl Into<String>,
        api_version: Option<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.into(),
            base_url: normalize_base_url(&base_url.into()),
            api_version,
            model: model.into(),
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

fn build_transcriptions_url(base_url: &str, model: &str, api_version: Option<&str>) -> String {
    if base_url.contains("cognitiveservices.azure.com") {
        let version = api_version.unwrap_or("2025-03-01-preview");
        let root = base_url
            .trim_end_matches('/')
            .trim_end_matches("/openai/v1")
            .trim_end_matches("/openai");
        format!("{root}/openai/deployments/{model}/audio/transcriptions?api-version={version}")
    } else {
        format!("{}/audio/transcriptions", base_url.trim_end_matches('/'))
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
        let url =
            build_transcriptions_url(&self.base_url, &self.model, self.api_version.as_deref());

        debug!(
            mime_type,
            audio_len = audio.len(),
            base_url = %self.base_url,
            url = %url,
            model = %self.model,
            "openai-compatible transcription request",
        );

        let file_part = Part::bytes(audio.to_vec())
            .file_name(filename)
            .mime_str(mime_type)
            .map_err(|e| SttError::Provider(format!("failed to set MIME type: {e}")))?;

        let form = Form::new()
            .text("model", self.model.clone())
            .part("file", file_part);

        let response = self
            .client
            .post(url)
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
