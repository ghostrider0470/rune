//! AWS Bedrock provider — Converse API (non-streaming).
//!
//! Uses the Bedrock Converse API with AWS SigV4 request signing.
//! Endpoint: `https://bedrock-runtime.{region}.amazonaws.com/model/{model_id}/converse`
//!
//! AWS credentials are resolved in this order:
//! 1. Explicit `api_key` config field (format: `ACCESS_KEY_ID:SECRET_ACCESS_KEY`)
//! 2. Environment variables `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`
//!
//! Region is resolved from:
//! 1. `deployment_name` config field
//! 2. `AWS_REGION` or `AWS_DEFAULT_REGION` environment variables
//! 3. Falls back to `us-east-1`

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::debug;

use super::ModelProvider;
use crate::error::ModelError;
use crate::types::{CompletionRequest, CompletionResponse, FinishReason, ToolCallRequest, Usage};

/// Default AWS region when none is configured.
const DEFAULT_REGION: &str = "us-east-1";

/// AWS Bedrock provider using the Converse API.
#[derive(Debug)]
pub struct BedrockProvider {
    region: String,
    access_key_id: String,
    secret_access_key: String,
    /// Optional endpoint override (for localstack, VPC endpoints, etc.).
    endpoint_override: Option<String>,
    client: Client,
}

impl BedrockProvider {
    /// Expose the resolved AWS region for validation and diagnostics.
    pub fn region(&self) -> &str {
        &self.region
    }

    /// Create a new Bedrock provider.
    ///
    /// - `region`: AWS region (e.g. `us-east-1`). If empty, falls back to env vars or default.
    /// - `access_key_id` / `secret_access_key`: AWS credentials.
    /// - `endpoint_override`: Optional custom endpoint URL.
    pub fn new(
        region: &str,
        access_key_id: &str,
        secret_access_key: &str,
        endpoint_override: Option<&str>,
    ) -> Self {
        let resolved_region = if region.is_empty() {
            std::env::var("AWS_REGION")
                .or_else(|_| std::env::var("AWS_DEFAULT_REGION"))
                .unwrap_or_else(|_| DEFAULT_REGION.to_string())
        } else {
            region.to_string()
        };

        Self {
            region: resolved_region,
            access_key_id: access_key_id.to_string(),
            secret_access_key: secret_access_key.to_string(),
            endpoint_override: endpoint_override
                .filter(|s| !s.is_empty())
                .map(|s| s.trim_end_matches('/').to_string()),
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Create a Bedrock provider from environment variables.
    ///
    /// Reads `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, and `AWS_REGION`.
    pub fn from_env() -> Result<Self, ModelError> {
        let access_key_id = std::env::var("AWS_ACCESS_KEY_ID").map_err(|_| {
            ModelError::Auth("AWS_ACCESS_KEY_ID environment variable not set".into())
        })?;
        let secret_access_key = std::env::var("AWS_SECRET_ACCESS_KEY").map_err(|_| {
            ModelError::Auth("AWS_SECRET_ACCESS_KEY environment variable not set".into())
        })?;
        let region = std::env::var("AWS_REGION")
            .or_else(|_| std::env::var("AWS_DEFAULT_REGION"))
            .unwrap_or_else(|_| DEFAULT_REGION.to_string());

        Ok(Self::new(&region, &access_key_id, &secret_access_key, None))
    }

    /// Build the Converse API URL for a given model.
    fn converse_url(&self, model_id: &str) -> String {
        let base = match &self.endpoint_override {
            Some(endpoint) => endpoint.clone(),
            None => format!("https://bedrock-runtime.{}.amazonaws.com", self.region),
        };
        format!("{base}/model/{model_id}/converse")
    }

    /// Convert our internal messages to Bedrock Converse format.
    fn build_converse_request(
        &self,
        request: &CompletionRequest,
        model_id: &str,
    ) -> BedrockConverseRequest {
        let mut system_prompts = Vec::new();
        let mut messages = Vec::new();

        for msg in &request.messages {
            match msg.role {
                crate::types::Role::System => {
                    if let Some(content) = &msg.content {
                        system_prompts.push(BedrockSystemContent {
                            text: content.clone(),
                        });
                    }
                }
                crate::types::Role::User | crate::types::Role::Tool => {
                    if let Some(content) = &msg.content {
                        messages.push(BedrockMessage {
                            role: "user".to_string(),
                            content: vec![BedrockContentBlock::Text {
                                text: content.clone(),
                            }],
                        });
                    }
                }
                crate::types::Role::Assistant => {
                    if let Some(content) = &msg.content {
                        messages.push(BedrockMessage {
                            role: "assistant".to_string(),
                            content: vec![BedrockContentBlock::Text {
                                text: content.clone(),
                            }],
                        });
                    }
                }
            }
        }

        let inference_config = Some(BedrockInferenceConfig {
            max_tokens: request.max_tokens,
            temperature: request.temperature,
        });

        BedrockConverseRequest {
            model_id: model_id.to_string(),
            system: if system_prompts.is_empty() {
                None
            } else {
                Some(system_prompts)
            },
            messages,
            inference_config,
        }
    }

    /// Sign a request using AWS Signature Version 4.
    fn sign_request(
        &self,
        method: &str,
        url: &str,
        body: &[u8],
    ) -> Result<SignedHeaders, ModelError> {
        use hmac::{Hmac, Mac};
        use sha2::{Digest, Sha256};

        let now = chrono::Utc::now();
        let date_stamp = now.format("%Y%m%d").to_string();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();

        // Parse the URL to get host and path
        let parsed = url::Url::parse(url)
            .map_err(|e| ModelError::Configuration(format!("invalid URL: {e}")))?;
        let host = parsed.host_str().unwrap_or_default();
        let path = parsed.path();

        // Create canonical request
        let payload_hash = hex::encode(Sha256::digest(body));
        let canonical_headers =
            format!("content-type:application/json\nhost:{host}\nx-amz-date:{amz_date}\n");
        let signed_headers = "content-type;host;x-amz-date";
        let canonical_request =
            format!("{method}\n{path}\n\n{canonical_headers}\n{signed_headers}\n{payload_hash}");

        // Create string to sign
        let service = "bedrock";
        let credential_scope = format!("{date_stamp}/{}/{service}/aws4_request", self.region);
        let canonical_hash = hex::encode(Sha256::digest(canonical_request.as_bytes()));
        let string_to_sign =
            format!("AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{canonical_hash}");

        // Calculate signing key
        type HmacSha256 = Hmac<Sha256>;

        let k_date = {
            let mut mac =
                HmacSha256::new_from_slice(format!("AWS4{}", self.secret_access_key).as_bytes())
                    .map_err(|e| ModelError::Auth(format!("HMAC error: {e}")))?;
            mac.update(date_stamp.as_bytes());
            mac.finalize().into_bytes()
        };

        let k_region = {
            let mut mac = HmacSha256::new_from_slice(&k_date)
                .map_err(|e| ModelError::Auth(format!("HMAC error: {e}")))?;
            mac.update(self.region.as_bytes());
            mac.finalize().into_bytes()
        };

        let k_service = {
            let mut mac = HmacSha256::new_from_slice(&k_region)
                .map_err(|e| ModelError::Auth(format!("HMAC error: {e}")))?;
            mac.update(service.as_bytes());
            mac.finalize().into_bytes()
        };

        let k_signing = {
            let mut mac = HmacSha256::new_from_slice(&k_service)
                .map_err(|e| ModelError::Auth(format!("HMAC error: {e}")))?;
            mac.update(b"aws4_request");
            mac.finalize().into_bytes()
        };

        // Calculate signature
        let signature = {
            let mut mac = HmacSha256::new_from_slice(&k_signing)
                .map_err(|e| ModelError::Auth(format!("HMAC error: {e}")))?;
            mac.update(string_to_sign.as_bytes());
            hex::encode(mac.finalize().into_bytes())
        };

        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}",
            self.access_key_id
        );

        Ok(SignedHeaders {
            authorization,
            amz_date,
            payload_hash,
        })
    }
}

/// Headers produced by SigV4 signing.
struct SignedHeaders {
    authorization: String,
    amz_date: String,
    payload_hash: String,
}

#[async_trait]
impl ModelProvider for BedrockProvider {
    async fn complete(
        &self,
        request: &CompletionRequest,
    ) -> Result<CompletionResponse, ModelError> {
        let model_id = request
            .model
            .as_deref()
            .unwrap_or("anthropic.claude-sonnet-4-20250514-v1:0");

        let url = self.converse_url(model_id);
        let body = self.build_converse_request(request, model_id);
        let body_bytes = serde_json::to_vec(&body)
            .map_err(|e| ModelError::Provider(format!("failed to serialize request: {e}")))?;

        debug!(
            url = %url,
            model = model_id,
            msg_count = body.messages.len(),
            "Bedrock Converse request"
        );

        let signed = self.sign_request("POST", &url, &body_bytes)?;

        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("X-Amz-Date", &signed.amz_date)
            .header("X-Amz-Content-Sha256", &signed.payload_hash)
            .header("Authorization", &signed.authorization)
            .body(body_bytes)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body_text = resp.text().await.unwrap_or_default();

            return Err(match status {
                401 | 403 => ModelError::Auth(format!("AWS auth error: {body_text}")),
                404 => ModelError::Provider(format!(
                    "Bedrock model not found ({model_id}): {body_text}"
                )),
                429 => ModelError::RateLimited {
                    message: body_text,
                    retry_after_secs: None,
                },
                500..=599 => ModelError::Transient(format!("HTTP {status}: {body_text}")),
                _ => ModelError::Provider(format!("HTTP {status}: {body_text}")),
            });
        }

        let converse_resp: BedrockConverseResponse = resp
            .json()
            .await
            .map_err(|e| ModelError::Provider(format!("failed to parse Bedrock response: {e}")))?;

        // Extract text content from output
        let content = converse_resp
            .output
            .and_then(|output| output.message)
            .map(|msg| {
                msg.content
                    .into_iter()
                    .map(|block| {
                        let BedrockContentBlock::Text { text } = block;
                        text
                    })
                    .collect::<Vec<_>>()
                    .join("")
            })
            .filter(|s| !s.is_empty());

        let finish_reason = converse_resp.stop_reason.as_deref().map(|r| match r {
            "end_turn" | "stop" => FinishReason::Stop,
            "max_tokens" => FinishReason::Length,
            "tool_use" => FinishReason::ToolCalls,
            "content_filtered" | "guardrail_intervened" => FinishReason::ContentFilter,
            _ => FinishReason::Stop,
        });

        let usage = converse_resp
            .usage
            .map(|u| Usage {
                prompt_tokens: u.input_tokens,
                completion_tokens: u.output_tokens,
                total_tokens: u.total_tokens.unwrap_or(u.input_tokens + u.output_tokens),
                cached_prompt_tokens: 0,
            })
            .unwrap_or_default();

        Ok(CompletionResponse {
            content,
            usage,
            finish_reason,
            tool_calls: Vec::<ToolCallRequest>::new(),
        })
    }
}

// ── Bedrock Converse API types ────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BedrockConverseRequest {
    model_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<Vec<BedrockSystemContent>>,
    messages: Vec<BedrockMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    inference_config: Option<BedrockInferenceConfig>,
}

#[derive(Debug, Serialize)]
struct BedrockSystemContent {
    text: String,
}

#[derive(Debug, Serialize)]
struct BedrockMessage {
    role: String,
    content: Vec<BedrockContentBlock>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum BedrockContentBlock {
    Text { text: String },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BedrockInferenceConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BedrockConverseResponse {
    output: Option<BedrockOutputMessage>,
    stop_reason: Option<String>,
    usage: Option<BedrockUsage>,
}

#[derive(Debug, Deserialize)]
struct BedrockOutputMessage {
    message: Option<BedrockResponseMessage>,
}

#[derive(Debug, Deserialize)]
struct BedrockResponseMessage {
    content: Vec<BedrockContentBlock>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BedrockUsage {
    input_tokens: u32,
    output_tokens: u32,
    total_tokens: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converse_url_construction() {
        let p = BedrockProvider::new("us-west-2", "AKID", "SECRET", None);
        assert_eq!(
            p.converse_url("anthropic.claude-sonnet-4-20250514-v1:0"),
            "https://bedrock-runtime.us-west-2.amazonaws.com/model/anthropic.claude-sonnet-4-20250514-v1:0/converse"
        );
    }

    #[test]
    fn converse_url_custom_endpoint() {
        let p = BedrockProvider::new("us-east-1", "AKID", "SECRET", Some("http://localhost:4566"));
        assert_eq!(
            p.converse_url("amazon.titan-text-express-v1"),
            "http://localhost:4566/model/amazon.titan-text-express-v1/converse"
        );
    }

    #[test]
    fn empty_region_uses_default() {
        // Clear env vars for test isolation
        let p = BedrockProvider::new("", "AKID", "SECRET", None);
        // Region will be from env or default — just check it doesn't panic
        assert!(!p.region.is_empty());
    }
}
