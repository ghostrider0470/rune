//! Mistral provider — OpenAI-compatible endpoint.
//!
//! Uses the Mistral API at `https://api.mistral.ai/v1`.

use async_trait::async_trait;

use super::ModelProvider;
use super::openai::OpenAiProvider;
use crate::error::ModelError;
use crate::types::{CompletionRequest, CompletionResponse};

/// Default base URL for the Mistral API.
const DEFAULT_BASE_URL: &str = "https://api.mistral.ai/v1";

/// Mistral provider wrapping [`OpenAiProvider`].
#[derive(Debug)]
pub struct MistralProvider {
    inner: OpenAiProvider,
}

impl MistralProvider {
    /// Create a new Mistral provider with the default endpoint.
    pub fn new(api_key: &str) -> Self {
        Self {
            inner: OpenAiProvider::new(DEFAULT_BASE_URL, api_key),
        }
    }

    /// Create a Mistral provider with a custom base URL.
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
impl ModelProvider for MistralProvider {
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
        let p = MistralProvider::new("test-key");
        assert_eq!(p.url(), "https://api.mistral.ai/v1/chat/completions");
    }

    #[test]
    fn custom_url() {
        let p = MistralProvider::with_base_url("https://proxy.example.com/mistral/v1", "key");
        assert_eq!(
            p.url(),
            "https://proxy.example.com/mistral/v1/chat/completions"
        );
    }

    #[test]
    fn empty_base_url_uses_default() {
        let p = MistralProvider::with_base_url("", "key");
        assert_eq!(p.url(), "https://api.mistral.ai/v1/chat/completions");
    }
}
