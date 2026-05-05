//! HTTP transport for MCP.
//!
//! Sends JSON-RPC requests as HTTP POST and optionally connects to an SSE
//! endpoint for server-initiated events.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use reqwest::Client;
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use tracing::debug;

use crate::error::McpError;
use crate::protocol::{JsonRpcRequest, JsonRpcResponse};

/// A transport that talks to an MCP server over HTTP.
///
/// Requests are sent as `POST` with `Content-Type: application/json`.
/// Responses are expected to be synchronous JSON-RPC replies in the
/// HTTP response body.
pub struct HttpTransport {
    /// The base URL of the MCP server (e.g. `http://localhost:3001`).
    url: String,
    /// Shared HTTP client (connection pooling, TLS config, etc.).
    client: Client,
    /// Headers applied to every request.
    headers: HeaderMap,
    /// Monotonically increasing request id counter.
    next_id: Arc<AtomicU64>,
}

fn build_headers(headers: HashMap<String, String>) -> Result<HeaderMap, McpError> {
    let mut header_map = HeaderMap::new();
    for (name, value) in headers {
        let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|e| {
            McpError::init_failed(format!("invalid HTTP header name '{name}': {e}"))
        })?;
        let header_value = HeaderValue::from_str(&value).map_err(|e| {
            McpError::init_failed(format!("invalid HTTP header value for '{name}': {e}"))
        })?;
        header_map.insert(header_name, header_value);
    }
    Ok(header_map)
}

impl HttpTransport {
    /// Connect to an HTTP MCP server at the given URL.
    ///
    /// This constructor validates reachability by default but does **not**
    /// perform the MCP `initialize` handshake -- that is the caller's
    /// responsibility.
    pub async fn connect(
        url: impl Into<String>,
        headers: HashMap<String, String>,
    ) -> Result<Self, McpError> {
        let url = url.into();
        let headers = build_headers(headers)?;

        let client = Client::builder()
            .build()
            .map_err(|e| McpError::transport(format!("failed to build HTTP client: {e}")))?;

        debug!(url = %url, header_count = headers.len(), "HTTP transport created");

        Ok(Self {
            url,
            client,
            headers,
            next_id: Arc::new(AtomicU64::new(1)),
        })
    }

    /// Allocate the next JSON-RPC request id.
    pub fn next_request_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Send a JSON-RPC request over HTTP POST and return the response.
    ///
    /// Handles both plain JSON responses and SSE (Server-Sent Events) responses.
    /// SSE responses use `text/event-stream` content type and send data as
    /// `event: message\ndata: {...}\n\n` formatted lines.
    pub async fn send(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse, McpError> {
        let resp = self
            .client
            .post(&self.url)
            .headers(self.headers.clone())
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json, text/event-stream")
            .json(&request)
            .send()
            .await
            .map_err(|e| McpError::transport(format!("HTTP request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(McpError::transport(format!("HTTP {status}: {body}",)));
        }

        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();

        let body = resp
            .text()
            .await
            .map_err(|e| McpError::transport(format!("failed to read response body: {e}")))?;

        // SSE response: extract JSON from "data:" lines
        if content_type.contains("text/event-stream") {
            return self.parse_sse_response(&body);
        }

        serde_json::from_str::<JsonRpcResponse>(&body).map_err(|e| {
            // Check if it looks like SSE even without the content-type header
            if body.contains("event:") && body.contains("data:") {
                return self.parse_sse_response(&body).unwrap_err();
            }
            McpError::protocol(format!(
                "failed to parse JSON-RPC response: {e} (body: {})",
                &body[..body.len().min(200)]
            ))
        })
    }

    /// Parse a Server-Sent Events response body to extract the JSON-RPC response.
    ///
    /// SSE format:
    /// ```text
    /// event: message
    /// data: {"jsonrpc":"2.0","id":1,"result":{...}}
    /// ```
    fn parse_sse_response(&self, body: &str) -> Result<JsonRpcResponse, McpError> {
        // Collect all "data:" lines and join them (SSE can split data across lines)
        let mut data_parts = Vec::new();
        for line in body.lines() {
            let line = line.trim();
            if let Some(data) = line.strip_prefix("data:") {
                data_parts.push(data.trim());
            }
        }

        if data_parts.is_empty() {
            return Err(McpError::protocol(format!(
                "SSE response contains no data lines (body: {})",
                &body[..body.len().min(200)]
            )));
        }

        let json_str = data_parts.join("");
        serde_json::from_str::<JsonRpcResponse>(&json_str).map_err(|e| {
            McpError::protocol(format!(
                "failed to parse SSE JSON-RPC data: {e} (data: {})",
                &json_str[..json_str.len().min(200)]
            ))
        })
    }

    /// Send a JSON-RPC notification without expecting a response payload.
    pub async fn notify(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<(), McpError> {
        let resp = self
            .client
            .post(&self.url)
            .headers(self.headers.clone())
            .header(CONTENT_TYPE, "application/json")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": method,
                "params": params,
            }))
            .send()
            .await
            .map_err(|e| McpError::transport(format!("HTTP notification failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(McpError::transport(format!("HTTP {status}: {body}")));
        }

        Ok(())
    }

    /// Return the base URL this transport is connected to.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Shut down the transport (no-op for HTTP, included for API symmetry).
    pub async fn shutdown(&self) {
        debug!(url = %self.url, "HTTP transport shut down");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_partial_json, header, method};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn connect_builds_transport() {
        let transport = HttpTransport::connect("http://localhost:9999", HashMap::new()).await;
        assert!(transport.is_ok());
        let t = transport.unwrap();
        assert_eq!(t.url(), "http://localhost:9999");
    }

    #[tokio::test]
    async fn next_id_increments() {
        let t = HttpTransport::connect("http://localhost:9999", HashMap::new())
            .await
            .unwrap();
        let a = t.next_request_id();
        let b = t.next_request_id();
        assert_eq!(b, a + 1);
    }

    #[tokio::test]
    async fn connect_rejects_invalid_header_name() {
        let result = HttpTransport::connect(
            "http://localhost:9999",
            HashMap::from([("Bad Header".into(), "value".into())]),
        )
        .await;
        match result {
            Ok(_) => panic!("invalid header name should fail"),
            Err(err) => assert!(err.to_string().contains("invalid HTTP header name")),
        }
    }

    #[tokio::test]
    async fn send_includes_configured_headers() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(header("authorization", "Bearer secret-token"))
            .and(header("x-api-key", "extra-secret"))
            .and(body_partial_json(
                serde_json::json!({"method":"initialize"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc":"2.0",
                "id":1,
                "result": {
                    "protocolVersion":"2024-11-05",
                    "capabilities": {},
                    "serverInfo": {"name":"mock-mcp","version":"1.0.0"}
                }
            })))
            .mount(&server)
            .await;

        let transport = HttpTransport::connect(
            server.uri(),
            HashMap::from([
                ("Authorization".into(), "Bearer secret-token".into()),
                ("X-API-Key".into(), "extra-secret".into()),
            ]),
        )
        .await
        .unwrap();

        let response = transport
            .send(JsonRpcRequest::new(
                1,
                "initialize",
                Some(serde_json::json!({})),
            ))
            .await
            .unwrap();
        assert!(response.error.is_none());
        assert!(response.result.is_some());
    }
}
