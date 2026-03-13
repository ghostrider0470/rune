//! Standard OpenAI-compatible provider (works with Azure OpenAI too).

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
    use_azure_auth: bool,
    client: Client,
}

impl OpenAiProvider {
    /// Create a new OpenAI provider with Bearer auth.
    pub fn new(endpoint: &str, api_key: &str) -> Self {
        let base = endpoint.trim_end_matches('/');
        let url = format!("{base}/chat/completions");
        Self {
            url,
            api_key: api_key.to_owned(),
            use_azure_auth: false,
            client: Client::new(),
        }
    }

    /// Create a provider using Azure `api-key` header auth.
    pub fn azure(endpoint: &str, api_key: &str) -> Self {
        let base = endpoint.trim_end_matches('/');
        let url = format!("{base}/chat/completions");
        Self {
            url,
            api_key: api_key.to_owned(),
            use_azure_auth: true,
            client: Client::new(),
        }
    }

    /// Returns the constructed URL.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }
}

/// Azure/newer OpenAI models use `max_completion_tokens` instead of `max_tokens`.
#[derive(Debug, serde::Serialize)]
struct OpenAiRequest<'a> {
    messages: &'a [crate::types::ChatMessage],
    #[serde(skip_serializing_if = "Option::is_none")]
    model: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: &'a Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: &'a Option<Vec<crate::types::ToolDefinition>>,
}

#[async_trait]
impl ModelProvider for OpenAiProvider {
    async fn complete(
        &self,
        request: &CompletionRequest,
    ) -> Result<CompletionResponse, ModelError> {
        debug!(url = %self.url, azure = self.use_azure_auth, "OpenAI completion request");

        let body = OpenAiRequest {
            messages: &request.messages,
            model: &request.model,
            temperature: &request.temperature,
            max_completion_tokens: request.max_tokens,
            tools: &request.tools,
        };

        let mut req = self
            .client
            .post(&self.url)
            .header("Content-Type", "application/json");

        req = if self.use_azure_auth {
            req.header("api-key", &self.api_key)
        } else {
            req.header("Authorization", format!("Bearer {}", self.api_key))
        };

        let resp = req.json(&body).send().await?;

        if !resp.status().is_success() {
            return Err(map_error_response(resp).await);
        }

        let api_resp: ApiResponse = resp.json().await?;
        parse_response(api_resp)
    }
}
