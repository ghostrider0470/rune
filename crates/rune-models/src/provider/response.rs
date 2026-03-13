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
        .map(|u| Usage {
            prompt_tokens: u.prompt_tokens.unwrap_or(0),
            completion_tokens: u.completion_tokens.unwrap_or(0),
            total_tokens: u.total_tokens.unwrap_or(0),
        })
        .unwrap_or_default();

    Ok(CompletionResponse {
        content: message.content,
        usage,
        finish_reason,
        tool_calls: message.tool_calls.unwrap_or_default(),
    })
}
