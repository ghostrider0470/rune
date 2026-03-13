//! Azure AI Foundry provider — single endpoint, single key, multiple model families.
//!
//! Routes requests based on the model name:
//! - `claude-*` → Anthropic Messages API (`/anthropic/v1/messages`)
//! - Everything else → OpenAI Chat Completions (`/openai/v1/chat/completions`)
//!
//! This matches how Azure AI Services exposes models through a unified resource.

use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use serde::Serialize;
use tracing::debug;

use super::ModelProvider;
use super::response::{ApiResponse, map_error_response, parse_response};
use crate::error::ModelError;
use crate::types::{
    ChatMessage, CompletionRequest, CompletionResponse, FinishReason, Usage,
};

/// Azure AI Foundry provider.
#[derive(Debug)]
pub struct AzureFoundryProvider {
    /// Base URL like `https://my-resource.services.ai.azure.com`
    base_url: String,
    api_key: String,
    api_version: String,
    client: Client,
}

impl AzureFoundryProvider {
    pub fn new(base_url: &str, api_key: &str) -> Self {
        Self::with_api_version(base_url, api_key, "2023-06-01")
    }

    pub fn with_api_version(base_url: &str, api_key: &str, api_version: &str) -> Self {
        let base = base_url.trim_end_matches('/');
        Self {
            base_url: base.to_owned(),
            api_key: api_key.to_owned(),
            api_version: api_version.to_owned(),
            client: Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
        }
    }

    fn is_anthropic_model(model: &str) -> bool {
        model.starts_with("claude")
    }

    async fn complete_anthropic(
        &self,
        request: &CompletionRequest,
        model: &str,
    ) -> Result<CompletionResponse, ModelError> {
        let url = format!("{}/anthropic/v1/messages", self.base_url);

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

        let body = AnthropicRequest {
            model,
            max_tokens: request.max_tokens.unwrap_or(8192),
            system: system.as_deref(),
            messages: &messages,
        };

        debug!(
            url = %url,
            model,
            msg_count = messages.len(),
            "Azure Foundry → Anthropic"
        );

        let resp = self
            .client
            .post(&url)
            .header("api-key", &self.api_key)
            .header("anthropic-version", &self.api_version)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(map_error_response(resp).await);
        }

        let anthropic_resp: AnthropicResponse = resp.json().await.map_err(|e| {
            ModelError::Provider(format!("failed to parse Anthropic response: {e}"))
        })?;

        let content = anthropic_resp
            .content
            .into_iter()
            .filter_map(|block| {
                if block.block_type == "text" {
                    block.text
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("");

        Ok(CompletionResponse {
            content: if content.is_empty() {
                None
            } else {
                Some(content)
            },
            finish_reason: Some(match anthropic_resp.stop_reason.as_deref() {
                Some("end_turn") => FinishReason::Stop,
                Some("max_tokens") => FinishReason::Length,
                Some("tool_use") => FinishReason::ToolCalls,
                _ => FinishReason::Stop,
            }),
            usage: Usage {
                prompt_tokens: anthropic_resp.usage.input_tokens,
                completion_tokens: anthropic_resp.usage.output_tokens,
                total_tokens: anthropic_resp.usage.input_tokens
                    + anthropic_resp.usage.output_tokens,
            },
            tool_calls: vec![],
        })
    }

    async fn complete_openai(
        &self,
        request: &CompletionRequest,
        model: &str,
    ) -> Result<CompletionResponse, ModelError> {
        let url = format!("{}/openai/v1/chat/completions", self.base_url);

        let body = OpenAiRequest {
            messages: &request.messages,
            model,
            temperature: request.temperature,
            max_completion_tokens: request.max_tokens,
            tools: &request.tools,
        };

        debug!(
            url = %url,
            model,
            msg_count = request.messages.len(),
            "Azure Foundry → OpenAI"
        );

        let resp = self
            .client
            .post(&url)
            .header("api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(map_error_response(resp).await);
        }

        let api_resp: ApiResponse = resp.json().await?;
        parse_response(api_resp)
    }
}

#[async_trait]
impl ModelProvider for AzureFoundryProvider {
    async fn complete(
        &self,
        request: &CompletionRequest,
    ) -> Result<CompletionResponse, ModelError> {
        let model = request
            .model
            .as_deref()
            .unwrap_or("gpt-5.4");

        if Self::is_anthropic_model(model) {
            self.complete_anthropic(request, model).await
        } else {
            self.complete_openai(request, model).await
        }
    }
}

// ── Anthropic types ──────────────────────────────────────────────────

#[derive(Serialize)]
struct AnthropicRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<&'a str>,
    messages: &'a [AnthropicMessage],
}

#[derive(Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Debug, serde::Deserialize)]
struct AnthropicResponse {
    content: Vec<ContentBlock>,
    stop_reason: Option<String>,
    usage: AnthropicUsage,
}

#[derive(Debug, serde::Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    text: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

// ── OpenAI types ─────────────────────────────────────────────────────

#[derive(Serialize)]
struct OpenAiRequest<'a> {
    messages: &'a [ChatMessage],
    model: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: &'a Option<Vec<crate::types::ToolDefinition>>,
}
