//! Anthropic provider (direct API and Azure-hosted).
//!
//! Supports both direct Anthropic API and Azure AI Services-hosted Anthropic models.

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::error::ModelError;
use crate::provider::ModelProvider;
use crate::types::{CompletionRequest, CompletionResponse, FinishReason, MessagePart, Usage};
use crate::provider::response::map_anthropic_error_response;

/// Anthropic API mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnthropicMode {
    Direct,
    Azure,
}

/// Anthropic provider.
#[derive(Clone, Debug)]
pub struct AnthropicProvider {
    url: String,
    api_key: String,
    mode: AnthropicMode,
    client: Client,
}

impl AnthropicProvider {
    /// Create a direct Anthropic API provider.
    pub fn direct(api_key: &str) -> Self {
        Self {
            url: "https://api.anthropic.com/v1/messages".into(),
            api_key: api_key.into(),
            mode: AnthropicMode::Direct,
            client: Client::new(),
        }
    }

    /// Create an Azure-hosted Anthropic provider.
    pub fn azure(endpoint: &str, deployment: &str, api_version: &str, api_key: &str) -> Self {
        Self {
            url: format!(
                "{}/models/chat/completions?api-version={}&deployment={}",
                endpoint.trim_end_matches('/'),
                api_version,
                deployment,
            ),
            api_key: api_key.into(),
            mode: AnthropicMode::Azure,
            client: Client::new(),
        }
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
    content: Vec<AnthropicContentBlock>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum AnthropicContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { source: AnthropicImageSource },
}

#[derive(Debug, Serialize)]
struct AnthropicImageSource {
    #[serde(rename = "type")]
    source_type: String,
    media_type: String,
    data: String,
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
                    if let Some(content) = anthropic_content_blocks(msg) {
                        messages.push(AnthropicMessage {
                            role: "user".into(),
                            content,
                        });
                    }
                }
                crate::types::Role::Assistant => {
                    if let Some(content) = msg.content.clone() {
                        messages.push(AnthropicMessage {
                            role: "assistant".into(),
                            content: vec![AnthropicContentBlock::Text { text: content }],
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
                    .header("anthropic-version", "2023-06-01");
            }
            AnthropicMode::Azure => {
                req = req.header("api-key", &self.api_key);
            }
        }

        let resp = req.send().await.map_err(ModelError::from)?;
        if !resp.status().is_success() {
            return Err(map_anthropic_error_response(resp).await);
        }

        let api_resp: AnthropicResponse = resp.json().await?;
        let text = api_resp
            .content
            .iter()
            .filter(|b| b.content_type == "text")
            .filter_map(|b| b.text.clone())
            .collect::<Vec<_>>()
            .join("\n");

        Ok(CompletionResponse {
            content: if text.is_empty() { None } else { Some(text) },
            usage: Usage {
                prompt_tokens: api_resp.usage.input_tokens,
                completion_tokens: api_resp.usage.output_tokens,
                total_tokens: api_resp.usage.input_tokens + api_resp.usage.output_tokens,
                cached_prompt_tokens: api_resp.usage.cache_read_input_tokens,
                uncached_prompt_tokens: api_resp
                    .usage
                    .cache_creation_input_tokens
                    .map(|created| api_resp.usage.input_tokens.saturating_sub(created)),
            },
            finish_reason: api_resp.stop_reason.as_deref().map(map_finish_reason),
            tool_calls: Vec::new(),
        })
    }
}

fn anthropic_content_blocks(msg: &crate::types::ChatMessage) -> Option<Vec<AnthropicContentBlock>> {
    if let Some(parts) = &msg.content_parts {
        let mut blocks = Vec::new();
        for part in parts {
            match part {
                MessagePart::Text { text } => {
                    if !text.is_empty() {
                        blocks.push(AnthropicContentBlock::Text { text: text.clone() });
                    }
                }
                MessagePart::ImageUrl { image_url } => {
                    let Some((media_type, data)) = parse_anthropic_image_source(&image_url.url) else {
                        continue;
                    };
                    blocks.push(AnthropicContentBlock::Image {
                        source: AnthropicImageSource {
                            source_type: "base64".into(),
                            media_type,
                            data,
                        },
                    });
                }
            }
        }
        if !blocks.is_empty() {
            return Some(blocks);
        }
    }

    msg.content
        .as_ref()
        .filter(|content| !content.is_empty())
        .map(|content| vec![AnthropicContentBlock::Text { text: content.clone() }])
}

fn parse_anthropic_image_source(url: &str) -> Option<(String, String)> {
    let (meta, data) = url.strip_prefix("data:")?.split_once(",")?;
    if !meta.ends_with(";base64") {
        return None;
    }
    let media_type = meta.trim_end_matches(";base64");
    if !(media_type == "image/jpeg"
        || media_type == "image/png"
        || media_type == "image/gif"
        || media_type == "image/webp")
    {
        return None;
    }
    Some((media_type.to_string(), data.to_string()))
}

fn map_finish_reason(reason: &str) -> FinishReason {
    match reason {
        "max_tokens" => FinishReason::Length,
        "tool_use" => FinishReason::ToolCalls,
        _ => FinishReason::Stop,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_base64_data_urls_for_anthropic_images() {
        let parsed = parse_anthropic_image_source("data:image/png;base64,QUJDRA==").unwrap();
        assert_eq!(parsed.0, "image/png");
        assert_eq!(parsed.1, "QUJDRA==");
    }

    #[test]
    fn rejects_non_base64_or_unsupported_anthropic_images() {
        assert!(parse_anthropic_image_source("https://example.test/photo.jpg").is_none());
        assert!(parse_anthropic_image_source("data:image/svg+xml;base64,PHN2Zz4=").is_none());
        assert!(parse_anthropic_image_source("data:image/png,raw").is_none());
    }

    #[test]
    fn preserves_text_when_multimodal_images_are_not_anthropic_compatible() {
        let message = crate::types::ChatMessage {
            role: crate::types::Role::User,
            content: Some("Describe this image".into()),
            content_parts: Some(vec![
                MessagePart::Text {
                    text: "Describe this image".into(),
                },
                MessagePart::ImageUrl {
                    image_url: crate::types::ImageUrlPart {
                        url: "https://example.test/photo.jpg".into(),
                    },
                },
            ]),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        };

        let blocks = anthropic_content_blocks(&message).unwrap();
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], AnthropicContentBlock::Text { text } if text == "Describe this image"));
    }

    #[test]
    fn emits_anthropic_image_blocks_for_data_urls() {
        let message = crate::types::ChatMessage {
            role: crate::types::Role::User,
            content: Some("Describe this image".into()),
            content_parts: Some(vec![
                MessagePart::Text {
                    text: "Describe this image".into(),
                },
                MessagePart::ImageUrl {
                    image_url: crate::types::ImageUrlPart {
                        url: "data:image/png;base64,QUJDRA==".into(),
                    },
                },
            ]),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        };

        let blocks = anthropic_content_blocks(&message).unwrap();
        assert_eq!(blocks.len(), 2);
        assert!(matches!(&blocks[1], AnthropicContentBlock::Image { source } if source.media_type == "image/png" && source.data == "QUJDRA=="));
    }
}
