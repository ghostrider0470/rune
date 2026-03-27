//! Shared response parsing and error mapping for OpenAI-compatible APIs.

use reqwest::Response;
use serde::Deserialize;

use crate::error::ModelError;
use crate::types::{CompletionResponse, FinishReason, ToolCallRequest, Usage};

#[derive(Deserialize)]
pub(crate) struct ApiResponse {
    pub choices: Option<Vec<ApiChoice>>,
    pub usage: Option<ApiUsage>,
    pub error: Option<ApiError>,
}

#[derive(Deserialize)]
pub(crate) struct ApiChoice {
    pub message: Option<ApiMessage>,
    pub finish_reason: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct ApiMessage {
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCallRequest>>,
}

#[derive(Deserialize)]
pub(crate) struct ApiUsage {
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
    pub prompt_tokens_details: Option<ApiPromptTokensDetails>,
    pub input_tokens_details: Option<ApiPromptTokensDetails>,
    pub cache_creation_input_tokens: Option<u32>,
    pub cache_read_input_tokens: Option<u32>,
}

#[derive(Deserialize)]
pub(crate) struct ApiPromptTokensDetails {
    pub cached_tokens: Option<u32>,
    pub cache_read_input_tokens: Option<u32>,
    pub cache_creation_input_tokens: Option<u32>,
}

#[derive(Deserialize)]
pub(crate) struct ApiError {
    pub message: Option<String>,
    pub code: Option<String>,
}

/// Map an HTTP error response to a [`ModelError`].
pub(crate) async fn map_error_response(resp: Response) -> ModelError {
    let status = resp.status().as_u16();
    let retry_after = resp
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());

    let body = resp.text().await.unwrap_or_default();

    let api_msg = serde_json::from_str::<ApiResponse>(&body)
        .ok()
        .and_then(|r| r.error)
        .map(|e| {
            let code = e.code.unwrap_or_default();
            let msg = e.message.unwrap_or_else(|| body.clone());
            (code, msg)
        });

    let (code, message) = api_msg.unwrap_or_else(|| (String::new(), body));

    match status {
        401 | 403 => ModelError::Auth(message),
        404 if code.contains("DeploymentNotFound") || message.contains("DeploymentNotFound") => {
            ModelError::DeploymentNotFound(message)
        }
        404 => ModelError::Provider(message),
        429 => {
            if message.to_lowercase().contains("quota") {
                ModelError::QuotaExhausted(message)
            } else {
                ModelError::RateLimited {
                    message,
                    retry_after_secs: retry_after,
                }
            }
        }
        400 if is_unsupported_api_version(&code, &message) => {
            ModelError::UnsupportedApiVersion(message)
        }
        400 if code.contains("context_length") || message.contains("context_length") => {
            ModelError::ContextLengthExceeded(message)
        }
        400 if code.contains("content_filter") || message.contains("content_filter") => {
            ModelError::ContentFiltered(message)
        }
        400 => ModelError::Provider(message),
        500..=599 => ModelError::Transient(message),
        _ => ModelError::Provider(format!("HTTP {status}: {message}")),
    }
}

/// Parse a successful API response body.
pub(crate) fn parse_response(api: ApiResponse) -> Result<CompletionResponse, ModelError> {
    let choice = api
        .choices
        .and_then(|mut c| {
            if c.is_empty() {
                None
            } else {
                Some(c.remove(0))
            }
        })
        .ok_or_else(|| ModelError::Provider("no choices in response".into()))?;

    let finish_reason = choice.finish_reason.as_deref().map(|fr| match fr {
        "stop" => FinishReason::Stop,
        "length" => FinishReason::Length,
        "tool_calls" => FinishReason::ToolCalls,
        "content_filter" => FinishReason::ContentFilter,
        _ => FinishReason::Stop,
    });

    let message = choice.message.unwrap_or(ApiMessage {
        content: None,
        tool_calls: None,
    });

    let usage = api
        .usage
        .map(|u| {
            let (cached_prompt_tokens, uncached_prompt_tokens) = extract_cached_usage(&u);
            Usage {
                prompt_tokens: u.prompt_tokens.unwrap_or(0),
                completion_tokens: u.completion_tokens.unwrap_or(0),
                total_tokens: u.total_tokens.unwrap_or(0),
                cached_prompt_tokens,
                uncached_prompt_tokens,
            }
        })
        .unwrap_or_default();

    Ok(CompletionResponse {
        content: message.content,
        usage,
        finish_reason,
        tool_calls: message.tool_calls.unwrap_or_default(),
    })
}

/// Extract cached/uncached prompt token counts from an [`ApiUsage`].
///
/// Handles multiple provider formats:
/// - OpenAI: `prompt_tokens_details.cached_tokens`
/// - Azure/Anthropic-via-OpenAI: `input_tokens_details.cache_read_input_tokens`
/// - Anthropic native: top-level `cache_read_input_tokens` / `cache_creation_input_tokens`
///
/// Returns `(cached_prompt_tokens, uncached_prompt_tokens)`.
pub(crate) fn extract_cached_usage(u: &ApiUsage) -> (Option<u32>, Option<u32>) {
    let prompt_tokens = u.prompt_tokens.unwrap_or(0);

    let cached_prompt_tokens = u
        .prompt_tokens_details
        .as_ref()
        .and_then(|d| d.cached_tokens.or(d.cache_read_input_tokens))
        .or_else(|| {
            u.input_tokens_details
                .as_ref()
                .and_then(|d| d.cached_tokens.or(d.cache_read_input_tokens))
        })
        .or(u.cache_read_input_tokens);

    let uncached_prompt_tokens = u
        .prompt_tokens_details
        .as_ref()
        .and_then(|d| d.cache_creation_input_tokens)
        .or_else(|| {
            u.input_tokens_details
                .as_ref()
                .and_then(|d| d.cache_creation_input_tokens)
        })
        .or(u.cache_creation_input_tokens)
        .or_else(|| cached_prompt_tokens.map(|cached| prompt_tokens.saturating_sub(cached)));

    (cached_prompt_tokens, uncached_prompt_tokens)
}

// ── SSE streaming types (OpenAI-compatible) ─────────────────────────

/// A single chunk from the OpenAI streaming API (SSE `data:` payload).
#[derive(Deserialize)]
pub(crate) struct StreamChunkResponse {
    pub choices: Option<Vec<StreamChunkChoice>>,
    pub usage: Option<ApiUsage>,
}

#[derive(Deserialize)]
pub(crate) struct StreamChunkChoice {
    pub delta: Option<StreamDelta>,
    pub finish_reason: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct StreamDelta {
    pub content: Option<String>,
    pub tool_calls: Option<Vec<StreamToolCallDelta>>,
}

#[derive(Deserialize)]
pub(crate) struct StreamToolCallDelta {
    pub index: Option<usize>,
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub call_type: Option<String>,
    pub function: Option<StreamFunctionDelta>,
}

#[derive(Deserialize)]
pub(crate) struct StreamFunctionDelta {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

// ── Anthropic error format ──────────────────────────────────────────

/// Anthropic API error envelope: `{ "type": "error", "error": { "type": "...", "message": "..." } }`.
#[derive(Deserialize)]
struct AnthropicApiErrorEnvelope {
    error: Option<AnthropicApiError>,
}

#[derive(Deserialize)]
struct AnthropicApiError {
    #[serde(rename = "type")]
    error_type: Option<String>,
    message: Option<String>,
}

/// Map an HTTP error response from the Anthropic Messages API to a [`ModelError`].
///
/// Anthropic uses a different error format than OpenAI:
/// - Error type is in `error.type` (e.g. `authentication_error`, `rate_limit_error`)
/// - HTTP 529 signals overloaded (transient)
/// - 400 `invalid_request_error` may indicate context length or other request issues
pub(crate) async fn map_anthropic_error_response(resp: Response) -> ModelError {
    let status = resp.status().as_u16();
    let retry_after = resp
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());

    let body = resp.text().await.unwrap_or_default();

    let parsed = serde_json::from_str::<AnthropicApiErrorEnvelope>(&body)
        .ok()
        .and_then(|e| e.error)
        .map(|e| {
            let error_type = e.error_type.unwrap_or_default();
            let msg = e.message.unwrap_or_else(|| body.clone());
            (error_type, msg)
        });

    let (error_type, message) = parsed.unwrap_or_else(|| (String::new(), body));

    match status {
        401 | 403 => ModelError::Auth(message),
        404 => ModelError::Provider(message),
        429 => {
            if message.to_lowercase().contains("quota") {
                ModelError::QuotaExhausted(message)
            } else {
                ModelError::RateLimited {
                    message,
                    retry_after_secs: retry_after,
                }
            }
        }
        529 => ModelError::Transient(message),
        400 if is_anthropic_context_length(&error_type, &message) => {
            ModelError::ContextLengthExceeded(message)
        }
        400 if error_type == "invalid_request_error"
            && (message.to_lowercase().contains("content filter")
                || message.to_lowercase().contains("content_filter")) =>
        {
            ModelError::ContentFiltered(message)
        }
        400 => ModelError::Provider(message),
        500..=599 => ModelError::Transient(message),
        _ => ModelError::Provider(format!("HTTP {status}: {message}")),
    }
}

/// Detect Anthropic context-length errors.
///
/// Anthropic signals this as `invalid_request_error` with messages containing
/// keywords like "too many tokens" or "context length".
fn is_anthropic_context_length(error_type: &str, message: &str) -> bool {
    if error_type != "invalid_request_error" {
        return false;
    }
    let msg_lower = message.to_lowercase();
    msg_lower.contains("too many tokens")
        || msg_lower.contains("context length")
        || msg_lower.contains("maximum number of tokens")
        || msg_lower.contains("token limit")
}

/// Detect Azure-specific "unsupported API version" errors.
///
/// Azure returns error codes like `InvalidApiVersionIdentifier` or messages
/// mentioning the api-version when the requested version is not supported.
fn is_unsupported_api_version(code: &str, message: &str) -> bool {
    let code_lower = code.to_lowercase();
    let msg_lower = message.to_lowercase();
    code_lower.contains("invalidapiversionidentifier")
        || code_lower.contains("invalidapiversion")
        || (msg_lower.contains("api version") || msg_lower.contains("api-version"))
            && (msg_lower.contains("not supported")
                || msg_lower.contains("invalid")
                || msg_lower.contains("not found")
                || msg_lower.contains("unsupported"))
}
