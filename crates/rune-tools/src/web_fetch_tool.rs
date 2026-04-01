//! HTTP fetch tool for agents to retrieve web content, APIs, and issue trackers.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, COOKIE};
use tracing::instrument;

use crate::definition::{ToolCall, ToolDefinition, ToolResult};
use crate::error::ToolError;
use crate::executor::ToolExecutor;

/// Maximum response body size returned to the LLM context (50 KB).
const MAX_BODY_BYTES: usize = 50 * 1024;

/// Default request timeout.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const REDACTED_HEADER_VALUE: &str = "<redacted>";

/// Executor for the `web_fetch` tool.
///
/// Makes HTTP GET/POST requests and returns the response body, truncated
/// to fit within LLM context limits.
pub struct WebFetchToolExecutor {
    client: reqwest::Client,
}

impl WebFetchToolExecutor {
    /// Create a new web-fetch executor with default settings.
    pub fn new() -> Result<Self, ToolError> {
        let client = reqwest::Client::builder()
            .timeout(DEFAULT_TIMEOUT)
            .user_agent("rune-agent/0.1")
            .build()
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to build HTTP client: {e}")))?;
        Ok(Self { client })
    }

    /// Create from an existing reqwest client (useful for testing).
    pub fn with_client(client: reqwest::Client) -> Self {
        Self { client }
    }

    #[instrument(skip(self, call), fields(tool = "web_fetch"))]
    async fn handle(&self, call: &ToolCall) -> Result<ToolResult, ToolError> {
        let url = call
            .arguments
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments {
                tool: "web_fetch".into(),
                reason: "missing required field: url".into(),
            })?;

        let method = call
            .arguments
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("GET")
            .to_uppercase();

        // Parse optional headers
        let raw_headers: HashMap<String, String> = call
            .arguments
            .get("headers")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let (request_headers, display_headers) = match build_request_headers(&raw_headers) {
            Ok(parts) => parts,
            Err(error) => {
                return Ok(ToolResult {
                    tool_call_id: call.tool_call_id.clone(),
                    output: error,
                    is_error: true,
                    tool_execution_id: None,
                });
            }
        };

        let body = call
            .arguments
            .get("body")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Build the request
        let mut request = match method.as_str() {
            "GET" => self.client.get(url),
            "POST" => self.client.post(url),
            other => {
                return Ok(ToolResult {
                    tool_call_id: call.tool_call_id.clone(),
                    output: format!("Unsupported HTTP method: {other}. Use GET or POST."),
                    is_error: true,
                    tool_execution_id: None,
                });
            }
        };

        // Apply headers
        request = request.headers(request_headers);

        // Apply body (POST)
        if let Some(body_content) = body {
            request = request.body(body_content);
        }

        // Execute the request
        let response = match request.send().await {
            Ok(resp) => resp,
            Err(e) => {
                let msg = if e.is_timeout() {
                    format!("Request timed out after {}s", DEFAULT_TIMEOUT.as_secs())
                } else if e.is_connect() {
                    format!("Connection failed: {e}")
                } else {
                    format!("HTTP request failed: {e}")
                };
                return Ok(ToolResult {
                    tool_call_id: call.tool_call_id.clone(),
                    output: msg,
                    is_error: true,
                    tool_execution_id: None,
                });
            }
        };

        let status = response.status();
        let status_code = status.as_u16();

        // Collect selected response headers
        let response_headers: Vec<String> = response
            .headers()
            .iter()
            .filter(|(name, _)| {
                let n = name.as_str();
                matches!(
                    n,
                    "content-type"
                        | "content-length"
                        | "location"
                        | "x-ratelimit-remaining"
                        | "retry-after"
                )
            })
            .map(|(name, value)| format!("{}: {}", name, value.to_str().unwrap_or("<binary>")))
            .collect();

        // Read body text
        let full_body = match response.text().await {
            Ok(text) => text,
            Err(e) => {
                return Ok(ToolResult {
                    tool_call_id: call.tool_call_id.clone(),
                    output: format!("HTTP {status_code} — failed to read response body: {e}"),
                    is_error: true,
                    tool_execution_id: None,
                });
            }
        };

        // Truncate for LLM context
        let (body_text, truncated) = if full_body.len() > MAX_BODY_BYTES {
            let truncated_body = truncate_utf8(&full_body, MAX_BODY_BYTES);
            (truncated_body.to_string(), true)
        } else {
            (full_body.clone(), false)
        };

        // Format output
        let mut output = format!(
            "HTTP {status_code} {}\n",
            status.canonical_reason().unwrap_or("")
        );
        if !display_headers.is_empty() {
            output.push_str("Request headers:\n");
            for header in &display_headers {
                output.push_str(header);
                output.push('\n');
            }
        }
        if !response_headers.is_empty() {
            for h in &response_headers {
                output.push_str(h);
                output.push('\n');
            }
        }
        output.push('\n');
        output.push_str(&body_text);
        if truncated {
            output.push_str(&format!(
                "\n\n[truncated: showing {MAX_BODY_BYTES} of {} bytes]",
                full_body.len()
            ));
        }

        Ok(ToolResult {
            tool_call_id: call.tool_call_id.clone(),
            output,
            is_error: false,
            tool_execution_id: None,
        })
    }
}

fn build_request_headers(
    raw_headers: &HashMap<String, String>,
) -> Result<(HeaderMap, Vec<String>), String> {
    let mut request_headers = HeaderMap::new();
    let mut display_headers = Vec::new();
    let sensitive = sensitive_header_names();

    for (key, value) in raw_headers {
        let header_name = HeaderName::from_bytes(key.as_bytes())
            .map_err(|e| format!("Invalid header name '{key}': {e}"))?;
        let header_value = HeaderValue::from_str(value)
            .map_err(|e| format!("Invalid header value for '{key}': {e}"))?;
        let is_sensitive = sensitive.contains(header_name.as_str());
        request_headers.append(header_name.clone(), header_value);
        let rendered_value = if is_sensitive {
            REDACTED_HEADER_VALUE
        } else {
            value.as_str()
        };
        display_headers.push(format!("{}: {}", header_name.as_str(), rendered_value));
    }

    display_headers.sort();
    Ok((request_headers, display_headers))
}

fn sensitive_header_names() -> HashSet<&'static str> {
    HashSet::from([
        AUTHORIZATION.as_str(),
        COOKIE.as_str(),
        "proxy-authorization",
        "x-api-key",
        "x-auth-token",
        "x-csrf-token",
    ])
}

/// Truncate a string to at most `max_bytes` without splitting a UTF-8 codepoint.
fn truncate_utf8(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[async_trait]
impl ToolExecutor for WebFetchToolExecutor {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        match call.tool_name.as_str() {
            "web_fetch" => self.handle(&call).await,
            other => Err(ToolError::NotFound(other.into())),
        }
    }
}

/// Return the `ToolDefinition` for registration in the tool registry.
pub fn web_fetch_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: "web_fetch".into(),
        description: "Fetch content from a URL via HTTP GET or POST. Returns status code, selected headers, and response body (truncated to 50KB for LLM context).".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch"
                },
                "method": {
                    "type": "string",
                    "description": "HTTP method: GET or POST (default: GET)",
                    "enum": ["GET", "POST"]
                },
                "headers": {
                    "type": "object",
                    "description": "Optional HTTP headers as key-value pairs",
                    "additionalProperties": { "type": "string" }
                },
                "body": {
                    "type": "string",
                    "description": "Optional request body (for POST requests)"
                }
            },
            "required": ["url"]
        }),
        category: rune_core::ToolCategory::External,
        requires_approval: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune_core::ToolCallId;

    fn make_call(args: serde_json::Value) -> ToolCall {
        ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: "web_fetch".into(),
            arguments: args,
        }
    }

    #[test]
    fn truncate_utf8_ascii() {
        assert_eq!(truncate_utf8("hello world", 5), "hello");
    }

    #[test]
    fn truncate_utf8_multibyte() {
        // '€' is 3 bytes (E2 82 AC)
        let s = "a€b";
        // at max_bytes=2, we can't fit '€' so we get just "a"
        assert_eq!(truncate_utf8(s, 2), "a");
        // at max_bytes=4, we get "a€"
        assert_eq!(truncate_utf8(s, 4), "a€");
    }

    #[test]
    fn truncate_utf8_no_truncation() {
        assert_eq!(truncate_utf8("short", 100), "short");
    }

    #[test]
    fn definition_schema_has_required_url() {
        let def = web_fetch_tool_definition();
        assert_eq!(def.name, "web_fetch");
        let required = def.parameters["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v.as_str() == Some("url")));
    }

    #[tokio::test]
    async fn missing_url_returns_error() {
        let exec = WebFetchToolExecutor::new().unwrap();
        let call = make_call(serde_json::json!({}));
        let err = exec.execute(call).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArguments { .. }));
    }

    #[tokio::test]
    async fn unsupported_method_returns_error_result() {
        let exec = WebFetchToolExecutor::new().unwrap();
        let call = make_call(serde_json::json!({"url": "http://example.com", "method": "DELETE"}));
        let result = exec.execute(call).await.unwrap();
        assert!(result.is_error);
        assert!(result.output.contains("Unsupported HTTP method"));
    }

    #[tokio::test]
    async fn unknown_tool_name_rejected() {
        let exec = WebFetchToolExecutor::new().unwrap();
        let call = ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: "not_web_fetch".into(),
            arguments: serde_json::json!({}),
        };
        let err = exec.execute(call).await.unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
    }

    #[test]
    fn build_request_headers_redacts_sensitive_values() {
        let raw_headers = HashMap::from([
            ("authorization".to_string(), "Bearer secret".to_string()),
            ("x-trace-id".to_string(), "trace-123".to_string()),
        ]);

        let (request_headers, display_headers) = build_request_headers(&raw_headers).unwrap();

        assert_eq!(
            request_headers.get("authorization").unwrap(),
            &HeaderValue::from_static("Bearer secret")
        );
        assert!(display_headers.contains(&"authorization: <redacted>".to_string()));
        assert!(display_headers.contains(&"x-trace-id: trace-123".to_string()));
    }

    #[test]
    fn build_request_headers_rejects_invalid_header_name() {
        let raw_headers = HashMap::from([("bad header".to_string(), "value".to_string())]);

        let error = build_request_headers(&raw_headers).unwrap_err();

        assert!(error.contains("Invalid header name"));
    }
}
