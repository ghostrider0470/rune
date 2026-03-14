//! Ollama local model provider — OpenAI-compatible endpoint.
//!
//! Default base URL: `http://localhost:11434/v1`.
//! No API key required. Model discovery via `GET /api/tags` on the
//! non-v1 Ollama HTTP API.

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use tracing::debug;

use super::ModelProvider;
use super::openai::OpenAiProvider;
use crate::error::ModelError;
use crate::types::{CompletionRequest, CompletionResponse};

/// Default OpenAI-compatible endpoint for Ollama.
const DEFAULT_BASE_URL: &str = "http://localhost:11434/v1";

/// Default Ollama native API base (used for model discovery).
const DEFAULT_OLLAMA_BASE: &str = "http://localhost:11434";

/// Ollama provider wrapping [`OpenAiProvider`].
#[derive(Debug)]
pub struct OllamaProvider {
    inner: OpenAiProvider,
    /// Base URL for the native Ollama API (without `/v1`).
    ollama_base: String,
    client: Client,
}

impl OllamaProvider {
    /// Create a new Ollama provider with the default local endpoint.
    pub fn new() -> Self {
        Self {
            inner: OpenAiProvider::new(DEFAULT_BASE_URL, "ollama"),
            ollama_base: DEFAULT_OLLAMA_BASE.to_string(),
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Create an Ollama provider with a custom base URL.
    ///
    /// `base_url` should be the OpenAI-compatible endpoint (e.g. `http://host:11434/v1`).
    /// The native Ollama API base is inferred by stripping the `/v1` suffix.
    pub fn with_base_url(base_url: &str) -> Self {
        let url = if base_url.is_empty() {
            DEFAULT_BASE_URL
        } else {
            base_url
        };
        let ollama_base = url
            .trim_end_matches('/')
            .strip_suffix("/v1")
            .unwrap_or(url)
            .to_string();

        Self {
            inner: OpenAiProvider::new(url, "ollama"),
            ollama_base,
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Returns the constructed chat completions URL.
    #[must_use]
    pub fn url(&self) -> &str {
        self.inner.url()
    }

    /// Discover locally available models via the Ollama `/api/tags` endpoint.
    ///
    /// Returns a list of model names installed on the Ollama instance.
    pub async fn list_models(&self) -> Result<Vec<OllamaModel>, ModelError> {
        let url = format!("{}/api/tags", self.ollama_base);
        debug!(url = %url, "Ollama model discovery");

        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| ModelError::Provider(format!("Ollama discovery failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(ModelError::Provider(format!(
                "Ollama /api/tags returned HTTP {status}: {body}"
            )));
        }

        let tags: OllamaTagsResponse = resp
            .json()
            .await
            .map_err(|e| ModelError::Provider(format!("failed to parse Ollama tags: {e}")))?;

        Ok(tags.models)
    }
}

impl Default for OllamaProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// Response from Ollama's `/api/tags` endpoint.
#[derive(Debug, Deserialize)]
pub struct OllamaTagsResponse {
    pub models: Vec<OllamaModel>,
}

/// A single model entry from the Ollama tags response.
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct OllamaModel {
    pub name: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub digest: String,
    #[serde(default)]
    pub modified_at: String,
}

#[async_trait]
impl ModelProvider for OllamaProvider {
    async fn complete(
        &self,
        request: &CompletionRequest,
    ) -> Result<CompletionResponse, ModelError> {
        self.inner.complete(request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_url() {
        let p = OllamaProvider::new();
        assert_eq!(p.url(), "http://localhost:11434/v1/chat/completions");
        assert_eq!(p.ollama_base, "http://localhost:11434");
    }

    #[test]
    fn custom_url() {
        let p = OllamaProvider::with_base_url("http://192.168.1.100:11434/v1");
        assert_eq!(p.url(), "http://192.168.1.100:11434/v1/chat/completions");
        assert_eq!(p.ollama_base, "http://192.168.1.100:11434");
    }

    #[test]
    fn empty_base_url_uses_default() {
        let p = OllamaProvider::with_base_url("");
        assert_eq!(p.url(), "http://localhost:11434/v1/chat/completions");
    }

    #[test]
    fn base_url_without_v1_suffix() {
        let p = OllamaProvider::with_base_url("http://myhost:11434");
        assert_eq!(p.ollama_base, "http://myhost:11434");
    }
}
