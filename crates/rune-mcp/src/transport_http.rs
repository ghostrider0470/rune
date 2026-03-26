//! HTTP transport for MCP.
//!
//! Sends JSON-RPC requests as HTTP POST and optionally connects to an SSE
//! endpoint for server-initiated events.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use reqwest::Client;
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
    /// Monotonically increasing request id counter.
    next_id: Arc<AtomicU64>,
}

impl HttpTransport {
    /// Connect to an HTTP MCP server at the given URL.
    ///
    /// This constructor validates reachability by default but does **not**
    /// perform the MCP `initialize` handshake -- that is the caller's
    /// responsibility.
    pub async fn connect(url: impl Into<String>) -> Result<Self, McpError> {
        let url = url.into();

        let client = Client::builder()
            .build()
            .map_err(|e| McpError::transport(format!("failed to build HTTP client: {e}")))?;

        debug!(url = %url, "HTTP transport created");

        Ok(Self {
            url,
            client,
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
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
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
            .header("Content-Type", "application/json")
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

    #[tokio::test]
    async fn connect_builds_transport() {
        let transport = HttpTransport::connect("http://localhost:9999").await;
        assert!(transport.is_ok());
        let t = transport.unwrap();
        assert_eq!(t.url(), "http://localhost:9999");
    }

    #[tokio::test]
    async fn next_id_increments() {
        let t = HttpTransport::connect("http://localhost:9999")
            .await
            .unwrap();
        let a = t.next_request_id();
        let b = t.next_request_id();
        assert_eq!(b, a + 1);
    }
}
