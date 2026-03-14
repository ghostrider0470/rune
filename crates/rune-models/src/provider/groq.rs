//! Groq provider — OpenAI-compatible endpoint.
//!
//! Uses the Groq API at `https://api.groq.com/openai/v1`.
//! Supports Groq-hosted models via a standard OpenAI chat completions interface.

use async_trait::async_trait;

use super::ModelProvider;
use super::openai::OpenAiProvider;
use crate::error::ModelError;
use crate::types::{CompletionRequest, CompletionResponse};

/// Default base URL for the Groq API.
const DEFAULT_BASE_URL: &str = "https://api.groq.com/openai/v1";

/// Groq provider wrapping [`OpenAiProvider`].
#[derive(Debug)]
pub struct GroqProvider {
    inner: OpenAiProvider,
}

impl GroqProvider {
    /// Create a new Groq provider with the default endpoint.
    pub fn new(api_key: &str) -> Self {
        Self {
            inner: OpenAiProvider::new(DEFAULT_BASE_URL, api_key),
        }
    }

    /// Create a Groq provider with a custom base URL.
    pub fn with_base_url(base_url: &str, api_key: &str) -> Self {
        let url = if base_url.is_empty() {
            DEFAULT_BASE_URL
        } else {
            base_url
        };
        Self {
            inner: OpenAiProvider::new(url, api_key),
        }
    }

    /// Returns the constructed URL.
    #[must_use]
    pub fn url(&self) -> &str {
        self.inner.url()
    }
}

#[async_trait]
impl ModelProvider for GroqProvider {
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
        let p = GroqProvider::new("test-key");
        assert_eq!(p.url(), "https://api.groq.com/openai/v1/chat/completions");
    }

    #[test]
    fn custom_url() {
        let p = GroqProvider::with_base_url("https://custom.groq.example.com/v1", "key");
        assert_eq!(
            p.url(),
            "https://custom.groq.example.com/v1/chat/completions"
        );
    }

    #[test]
    fn empty_base_url_uses_default() {
        let p = GroqProvider::with_base_url("", "key");
        assert_eq!(p.url(), "https://api.groq.com/openai/v1/chat/completions");
    }
}
