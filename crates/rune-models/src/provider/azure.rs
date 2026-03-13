//! Azure OpenAI provider — correct URL construction with deployment/api-version.

use async_trait::async_trait;
use reqwest::Client;
use tracing::debug;

use super::ModelProvider;
use super::response::{ApiResponse, map_error_response, parse_response};
use crate::error::ModelError;
use crate::types::{CompletionRequest, CompletionResponse};

/// Azure OpenAI provider.
///
/// URL pattern:
/// `{endpoint}/openai/deployments/{deployment}/chat/completions?api-version={api_version}`
#[derive(Debug)]
pub struct AzureOpenAiProvider {
    url: String,
    api_key: String,
    client: Client,
}

impl AzureOpenAiProvider {
    /// Create a new Azure OpenAI provider.
    ///
    /// `endpoint` should be the base URL, e.g. `https://my-resource.openai.azure.com`.
    /// Trailing slashes are stripped.
    pub fn new(endpoint: &str, deployment: &str, api_version: &str, api_key: &str) -> Self {
        let base = endpoint.trim_end_matches('/');
        let url = format!(
            "{base}/openai/deployments/{deployment}/chat/completions?api-version={api_version}"
        );
        Self {
            url,
            api_key: api_key.to_owned(),
            client: Client::new(),
        }
    }

    /// Returns the constructed URL (useful for testing).
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }
}

#[async_trait]
impl ModelProvider for AzureOpenAiProvider {
    async fn complete(
        &self,
        request: &CompletionRequest,
    ) -> Result<CompletionResponse, ModelError> {
        debug!(url = %self.url, "Azure OpenAI completion request");

        let resp = self
            .client
            .post(&self.url)
            .header("api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(request)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(map_error_response(resp).await);
        }

        let api_resp: ApiResponse = resp.json().await?;
        parse_response(api_resp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_construction_basic() {
        let p = AzureOpenAiProvider::new(
            "https://my-resource.openai.azure.com",
            "gpt-4o",
            "2024-06-01",
            "test-key",
        );
        assert_eq!(
            p.url(),
            "https://my-resource.openai.azure.com/openai/deployments/gpt-4o/chat/completions?api-version=2024-06-01"
        );
    }

    #[test]
    fn url_construction_trailing_slash() {
        let p = AzureOpenAiProvider::new(
            "https://my-resource.openai.azure.com/",
            "gpt-4o-mini",
            "2025-01-01",
            "key",
        );
        assert_eq!(
            p.url(),
            "https://my-resource.openai.azure.com/openai/deployments/gpt-4o-mini/chat/completions?api-version=2025-01-01"
        );
    }

    #[test]
    fn url_construction_custom_endpoint() {
        let p = AzureOpenAiProvider::new(
            "https://custom.azure-api.net/v1",
            "my-deploy",
            "2024-02-15-preview",
            "k",
        );
        assert_eq!(
            p.url(),
            "https://custom.azure-api.net/v1/openai/deployments/my-deploy/chat/completions?api-version=2024-02-15-preview"
        );
    }
}
