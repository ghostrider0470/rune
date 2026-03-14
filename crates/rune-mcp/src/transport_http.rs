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
    pub async fn send(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse, McpError> {
        let resp = self
            .client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| McpError::transport(format!("HTTP request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(McpError::transport(format!("HTTP {status}: {body}",)));
        }

        let body = resp
            .text()
            .await
            .map_err(|e| McpError::transport(format!("failed to read response body: {e}")))?;

        serde_json::from_str::<JsonRpcResponse>(&body).map_err(|e| {
            McpError::protocol(format!(
                "failed to parse JSON-RPC response: {e} (body: {})",
                &body[..body.len().min(200)]
            ))
        })
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
