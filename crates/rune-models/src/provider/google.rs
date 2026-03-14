//! Google Gemini provider — OpenAI-compatible endpoint.
//!
//! Uses the Gemini OpenAI-compatible API at
//! `https://generativelanguage.googleapis.com/v1beta/openai/`.
//! Model names: `gemini-2.0-flash`, `gemini-2.5-pro`, etc.

use async_trait::async_trait;

use super::ModelProvider;
use super::openai::OpenAiProvider;
use crate::error::ModelError;
use crate::types::{CompletionRequest, CompletionResponse};

/// Default base URL for Google Gemini's OpenAI-compatible endpoint.
const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/openai";

/// Google Gemini provider wrapping [`OpenAiProvider`].
#[derive(Debug)]
pub struct GoogleProvider {
    inner: OpenAiProvider,
}

impl GoogleProvider {
    /// Create a new Google Gemini provider with the default endpoint.
    pub fn new(api_key: &str) -> Self {
        Self {
            inner: OpenAiProvider::new(DEFAULT_BASE_URL, api_key),
        }
    }

    /// Create a Google Gemini provider with a custom base URL.
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

    /// Returns the constructed URL (useful for testing).
    #[must_use]
    pub fn url(&self) -> &str {
        self.inner.url()
    }
}

#[async_trait]
impl ModelProvider for GoogleProvider {
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
        let p = GoogleProvider::new("test-key");
        assert_eq!(
            p.url(),
            "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
        );
    }

    #[test]
    fn custom_url() {
        let p = GoogleProvider::with_base_url("https://custom.example.com/v1", "key");
        assert_eq!(p.url(), "https://custom.example.com/v1/chat/completions");
    }

    #[test]
    fn empty_base_url_uses_default() {
        let p = GoogleProvider::with_base_url("", "key");
        assert_eq!(
            p.url(),
            "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
        );
    }
}
