//! Anthropic provider (direct API and Azure-hosted).
//!
//! Supports both direct Anthropic API and Azure AI Services-hosted Anthropic models.
//! Azure pattern: `{endpoint}/v1/messages` with `api-key` header.
//! Direct pattern: `https://api.anthropic.com/v1/messages` with `x-api-key` header.

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::debug;

use super::ModelProvider;
use crate::error::ModelError;
use crate::types::{CompletionRequest, CompletionResponse, FinishReason, Usage};

/// Anthropic API mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnthropicMode {
    Direct,
    Azure,
}

/// Anthropic provider.
#[derive(Debug)]
pub struct AnthropicProvider {
    url: String,
    api_key: String,
    mode: AnthropicMode,
    api_version: String,
    client: Client,
}

impl AnthropicProvider {
    /// Create a direct Anthropic API provider.
    pub fn direct(api_key: &str) -> Self {
        Self {
            url: "https://api.anthropic.com/v1/messages".into(),
            api_key: api_key.to_owned(),
            mode: AnthropicMode::Direct,
            api_version: "2023-06-01".into(),
            client: Client::new(),
        }
    }

    /// Create an Azure-hosted Anthropic provider.
    pub fn azure(endpoint: &str, api_key: &str, api_version: &str) -> Self {
        let base = endpoint.trim_end_matches('/');
        Self {
            url: format!("{base}/v1/messages"),
            api_key: api_key.to_owned(),
            mode: AnthropicMode::Azure,
            api_version: api_version.into(),
            client: Client::new(),
        }
    }

    /// Returns the constructed URL.
    pub fn url(&self) -> &str {
        &self.url
    }
}

#[derive(Debug, Serialize)]
struct AnthropicRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Debug, Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<ContentBlock>,
    usage: AnthropicUsage,
    #[serde(default)]
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    content_type: String,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
    #[serde(default)]
    cache_read_input_tokens: Option<u32>,
    #[serde(default)]
    cache_creation_input_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct AnthropicErrorResp {
    error: AnthropicErrorBody,
}

#[derive(Debug, Deserialize)]
struct AnthropicErrorBody {
    #[serde(rename = "type")]
    error_type: String,
    message: String,
}

#[async_trait]
impl ModelProvider for AnthropicProvider {
    async fn complete(
        &self,
        request: &CompletionRequest,
    ) -> Result<CompletionResponse, ModelError> {
        debug!(url = %self.url, mode = ?self.mode, "Anthropic completion request");

        let mut system = None;
        let mut messages = Vec::new();

        for msg in &request.messages {
            match msg.role {
                crate::types::Role::System => {
                    system = msg.content.clone();
                }
                crate::types::Role::User | crate::types::Role::Tool => {
                    if let Some(content) = &msg.content {
                        messages.push(AnthropicMessage {
                            role: "user".into(),
                            content: content.clone(),
                        });
                    }
                }
                crate::types::Role::Assistant => {
                    if let Some(content) = &msg.content {
                        messages.push(AnthropicMessage {
                            role: "assistant".into(),
                            content: content.clone(),
                        });
                    }
                }
            }
        }

        let model_name = request
            .model
            .as_deref()
            .unwrap_or("claude-sonnet-4-20250514");
        let body = AnthropicRequest {
            model: model_name,
            max_tokens: request.max_tokens.unwrap_or(4096),
            system,
            messages,
            temperature: request.temperature,
        };

        let mut req = self.client.post(&self.url).json(&body);

        match self.mode {
            AnthropicMode::Direct => {
                req = req
                    .header("x-api-key", &self.api_key)
                    .header("anthropic-version", &self.api_version);
            }
            AnthropicMode::Azure => {
                req = req
                    .header("api-key", &self.api_key)
                    .header("x-api-key", &self.api_key)
                    .header("api-version", &self.api_version)
                    .header("anthropic-version", &self.api_version);
            }
        }

        let resp = req.send().await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body_text = resp.text().await.unwrap_or_default();

            if let Ok(err) = serde_json::from_str::<AnthropicErrorResp>(&body_text) {
                let msg = format!("{}: {}", err.error.error_type, err.error.message);
                return Err(match status {
                    401 => ModelError::Auth(msg),
                    429 => ModelError::RateLimited {
                        message: msg,
                        retry_after_secs: None,
                    },
                    _ => ModelError::Provider(msg),
                });
            }

            return Err(ModelError::Provider(format!("HTTP {status}: {body_text}")));
        }

        let api_resp: AnthropicResponse = resp.json().await?;

        let content = api_resp
            .content
            .iter()
            .filter(|b| b.content_type == "text")
            .filter_map(|b| b.text.as_deref())
            .collect::<Vec<_>>()
            .join("");

        let finish_reason = api_resp.stop_reason.as_deref().map(|r| match r {
            "end_turn" | "stop" => FinishReason::Stop,
            "max_tokens" => FinishReason::Length,
            "tool_use" => FinishReason::ToolCalls,
            _ => FinishReason::Stop,
        });

        Ok(CompletionResponse {
            content: if content.is_empty() {
                None
            } else {
                Some(content)
            },
            usage: {
                let cached = api_resp.usage.cache_read_input_tokens;
                let created = api_resp.usage.cache_creation_input_tokens;
                let input = api_resp.usage.input_tokens;
                Usage {
                    prompt_tokens: input,
                    completion_tokens: api_resp.usage.output_tokens,
                    total_tokens: input + api_resp.usage.output_tokens,
                    cached_prompt_tokens: cached,
                    uncached_prompt_tokens: created
                        .or_else(|| cached.map(|c| input.saturating_sub(c))),

                }
            },
            finish_reason,
            tool_calls: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_url() {
        let p = AnthropicProvider::direct("test-key");
        assert_eq!(p.url(), "https://api.anthropic.com/v1/messages");
        assert_eq!(p.mode, AnthropicMode::Direct);
    }

    #[test]
    fn azure_url_construction() {
        let p = AnthropicProvider::azure(
            "https://my-resource.services.ai.azure.com/anthropic",
            "azure-key",
            "2023-06-01",
        );
        assert_eq!(
            p.url(),
            "https://my-resource.services.ai.azure.com/anthropic/v1/messages"
        );
        assert_eq!(p.mode, AnthropicMode::Azure);
    }

    #[test]
    fn azure_url_trailing_slash() {
        let p = AnthropicProvider::azure(
            "https://resource.ai.azure.com/anthropic/",
            "key",
            "2023-06-01",
        );
        assert_eq!(
            p.url(),
            "https://resource.ai.azure.com/anthropic/v1/messages"
        );
    }
}
