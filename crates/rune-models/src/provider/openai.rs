//! Standard OpenAI-compatible provider (works with Azure OpenAI too).

use async_trait::async_trait;
use reqwest::Client;
use tracing::{debug, warn};

use super::ModelProvider;
use super::response::{ApiResponse, map_error_response, parse_response};
use super::response::{StreamChunkResponse, StreamDelta};
use crate::error::ModelError;
use crate::types::{
    CompletionRequest, CompletionResponse, FinishReason, FunctionCall, StreamEvent,
    ToolCallRequest, Usage,
};

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
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
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
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Returns the constructed URL.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Build an authenticated request builder for the completions endpoint.
    fn build_request(&self) -> reqwest::RequestBuilder {
        let mut req = self
            .client
            .post(&self.url)
            .header("Content-Type", "application/json");

        req = if self.use_azure_auth {
            req.header("api-key", &self.api_key)
        } else {
            req.header("Authorization", format!("Bearer {}", self.api_key))
        };

        req
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
    #[serde(skip_serializing_if = "is_false")]
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<StreamOptions>,
}

fn is_false(v: &bool) -> bool {
    !v
}

#[derive(Debug, serde::Serialize)]
struct StreamOptions {
    include_usage: bool,
}

#[async_trait]
impl ModelProvider for OpenAiProvider {
    async fn complete(
        &self,
        request: &CompletionRequest,
    ) -> Result<CompletionResponse, ModelError> {
        let body = OpenAiRequest {
            messages: &request.messages,
            model: &request.model,
            temperature: &request.temperature,
            max_completion_tokens: request.max_tokens,
            tools: &request.tools,
            stream: false,
            stream_options: None,
        };

        let body_json = serde_json::to_string(&body).unwrap_or_default();
        debug!(
            url = %self.url,
            azure = self.use_azure_auth,
            model = ?body.model,
            msg_count = body.messages.len(),
            body_len = body_json.len(),
            "OpenAI completion request"
        );

        let resp = self.build_request().json(&body).send().await?;

        if !resp.status().is_success() {
            return Err(map_error_response(resp).await);
        }

        let api_resp: ApiResponse = resp.json().await?;
        parse_response(api_resp)
    }

    async fn complete_stream(
        &self,
        request: &CompletionRequest,
    ) -> Result<tokio::sync::mpsc::Receiver<StreamEvent>, ModelError> {
        let body = OpenAiRequest {
            messages: &request.messages,
            model: &request.model,
            temperature: &request.temperature,
            max_completion_tokens: request.max_tokens,
            tools: &request.tools,
            stream: true,
            stream_options: Some(StreamOptions {
                include_usage: true,
            }),
        };

        debug!(
            url = %self.url,
            azure = self.use_azure_auth,
            model = ?body.model,
            msg_count = body.messages.len(),
            "OpenAI streaming completion request"
        );

        let resp = self
            .build_request()
            .json(&body)
            // Override timeout for streaming — responses may take several minutes.
            .timeout(std::time::Duration::from_secs(600))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(map_error_response(resp).await);
        }

        let (tx, rx) = tokio::sync::mpsc::channel(64);

        tokio::spawn(async move {
            if let Err(e) = parse_sse_stream(resp, &tx).await {
                warn!(error = %e, "SSE stream parsing failed, sending partial response");
            }
            // Sender is dropped here, signalling the receiver that streaming is done.
        });

        Ok(rx)
    }
}

/// Parse an SSE stream from an OpenAI-compatible API, forwarding events to `tx`.
async fn parse_sse_stream(
    mut resp: reqwest::Response,
    tx: &tokio::sync::mpsc::Sender<StreamEvent>,
) -> Result<(), ModelError> {
    let mut buffer = String::new();
    let mut content = String::new();
    let mut finish_reason: Option<FinishReason> = None;
    let mut usage = Usage::default();

    // Tool-call accumulators (indexed by `index` from delta).
    let mut tc_ids: Vec<String> = Vec::new();
    let mut tc_types: Vec<String> = Vec::new();
    let mut tc_names: Vec<String> = Vec::new();
    let mut tc_args: Vec<String> = Vec::new();

    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| ModelError::Provider(format!("stream read error: {e}")))?
    {
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        // Process all complete lines in the buffer.
        while let Some(newline_pos) = buffer.find('\n') {
            let line = buffer[..newline_pos].trim_end_matches('\r').to_string();
            buffer = buffer[newline_pos + 1..].to_string();

            if line.is_empty() || !line.starts_with("data: ") {
                continue;
            }

            let data = &line[6..];

            if data == "[DONE]" {
                // Assemble final response and send Done event.
                let tool_calls = assemble_tool_calls(&tc_ids, &tc_types, &tc_names, &tc_args);
                let _ = tx
                    .send(StreamEvent::Done(CompletionResponse {
                        content: if content.is_empty() {
                            None
                        } else {
                            Some(content)
                        },
                        usage,
                        finish_reason,
                        tool_calls,
                    }))
                    .await;
                return Ok(());
            }

            // Parse the JSON chunk.
            let chunk_resp: StreamChunkResponse = match serde_json::from_str(data) {
                Ok(r) => r,
                Err(_) => continue,
            };

            // Extract usage from the final chunk (when stream_options.include_usage is set).
            if let Some(ref u) = chunk_resp.usage {
                let (cached, uncached) = super::response::extract_cached_usage(u);
                usage = Usage {
                    prompt_tokens: u.prompt_tokens.unwrap_or(0),
                    completion_tokens: u.completion_tokens.unwrap_or(0),
                    total_tokens: u.total_tokens.unwrap_or(0),
                    cached_prompt_tokens: cached,
                    uncached_prompt_tokens: uncached,
                };
            }

            if let Some(ref choices) = chunk_resp.choices {
                if let Some(choice) = choices.first() {
                    // Update finish reason.
                    if let Some(ref fr) = choice.finish_reason {
                        finish_reason = Some(parse_finish_reason(fr));
                    }

                    // Process delta content and tool calls.
                    if let Some(ref delta) = choice.delta {
                        process_delta(
                            delta,
                            &mut content,
                            &mut tc_ids,
                            &mut tc_types,
                            &mut tc_names,
                            &mut tc_args,
                            tx,
                        )
                        .await;
                    }
                }
            }
        }
    }

    // Stream ended without [DONE] — send what we have.
    let tool_calls = assemble_tool_calls(&tc_ids, &tc_types, &tc_names, &tc_args);
    let _ = tx
        .send(StreamEvent::Done(CompletionResponse {
            content: if content.is_empty() {
                None
            } else {
                Some(content)
            },
            usage,
            finish_reason,
            tool_calls,
        }))
        .await;

    Ok(())
}

/// Process a single streaming delta, updating accumulators and forwarding text.
async fn process_delta(
    delta: &StreamDelta,
    content: &mut String,
    tc_ids: &mut Vec<String>,
    tc_types: &mut Vec<String>,
    tc_names: &mut Vec<String>,
    tc_args: &mut Vec<String>,
    tx: &tokio::sync::mpsc::Sender<StreamEvent>,
) {
    // Forward text content.
    if let Some(ref text) = delta.content {
        content.push_str(text);
        let _ = tx.send(StreamEvent::TextDelta(text.clone())).await;
    }

    // Accumulate tool call deltas.
    if let Some(ref tool_call_deltas) = delta.tool_calls {
        for tc_delta in tool_call_deltas {
            let idx = tc_delta.index.unwrap_or(0);

            // Extend vectors if needed.
            while tc_ids.len() <= idx {
                tc_ids.push(String::new());
                tc_types.push("function".to_string());
                tc_names.push(String::new());
                tc_args.push(String::new());
            }

            if let Some(ref id) = tc_delta.id {
                tc_ids[idx] = id.clone();
            }
            if let Some(ref ct) = tc_delta.call_type {
                tc_types[idx] = ct.clone();
            }
            if let Some(ref func) = tc_delta.function {
                if let Some(ref name) = func.name {
                    tc_names[idx] = name.clone();
                }
                if let Some(ref args) = func.arguments {
                    tc_args[idx].push_str(args);
                }
            }
        }
    }
}

/// Assemble accumulated tool call deltas into final [`ToolCallRequest`] items.
fn assemble_tool_calls(
    ids: &[String],
    types: &[String],
    names: &[String],
    args: &[String],
) -> Vec<ToolCallRequest> {
    ids.iter()
        .enumerate()
        .filter(|(_, id)| !id.is_empty())
        .map(|(i, id)| ToolCallRequest {
            id: id.clone(),
            call_type: types.get(i).cloned().unwrap_or_else(|| "function".into()),
            function: FunctionCall {
                name: names.get(i).cloned().unwrap_or_default(),
                arguments: args.get(i).cloned().unwrap_or_default(),
            },
        })
        .collect()
}

fn parse_finish_reason(s: &str) -> FinishReason {
    match s {
        "stop" => FinishReason::Stop,
        "length" => FinishReason::Length,
        "tool_calls" => FinishReason::ToolCalls,
        "content_filter" => FinishReason::ContentFilter,
        _ => FinishReason::Stop,
    }
}
