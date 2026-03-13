//! Standard OpenAI-compatible provider.

use async_trait::async_trait;
use reqwest::Client;
use tracing::debug;

use super::ModelProvider;
use super::response::{ApiResponse, map_error_response, parse_response};
use crate::error::ModelError;
use crate::types::{CompletionRequest, CompletionResponse};

/// OpenAI-compatible provider.
#[derive(Debug)]
pub struct OpenAiProvider {
    url: String,
    api_key: String,
    client: Client,
}

impl OpenAiProvider {
    /// Create a new OpenAI provider.
    ///
    /// `endpoint` should be the base URL, e.g. `https://api.openai.com/v1`.
    pub fn new(endpoint: &str, api_key: &str) -> Self {
        let base = endpoint.trim_end_matches('/');
        let url = format!("{base}/chat/completions");
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
impl ModelProvider for OpenAiProvider {
    async fn complete(
        &self,
        request: &CompletionRequest,
    ) -> Result<CompletionResponse, ModelError> {
        debug!(url = %self.url, "OpenAI completion request");

        let resp = self
            .client
            .post(&self.url)
            .header("Authorization", format!("Bearer {}", self.api_key))
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
