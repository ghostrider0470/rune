#![doc = "MCP (Model Context Protocol) client for Rune: manages connections to external MCP tool servers over STDIO and HTTP transports."]

pub mod discovery;
pub mod error;
pub mod protocol;
pub mod transport_http;
pub mod transport_stdio;

use std::collections::HashMap;

use serde_json::Value;
use tracing::{debug, info, warn};

use crate::discovery::{McpServerConfig, McpTransportKind};
use crate::error::McpError;
use crate::protocol::{
    ClientCapabilities, ClientInfo, InitializeParams, InitializeResult, JsonRpcRequest,
    MCP_PROTOCOL_VERSION, McpTool, McpToolResult, ToolsListResult,
};
use crate::transport_http::HttpTransport;
use crate::transport_stdio::StdioTransport;

// ---------------------------------------------------------------------------
// Transport abstraction (internal enum dispatch -- no dyn trait needed)
// ---------------------------------------------------------------------------

/// Internal transport handle. We use an enum rather than a trait object so that
/// the manager can hold heterogeneous transports without boxing.
enum TransportHandle {
    Stdio(StdioTransport),
    Http(HttpTransport),
}

impl TransportHandle {
    async fn send(&self, request: JsonRpcRequest) -> Result<protocol::JsonRpcResponse, McpError> {
        match self {
            Self::Stdio(t) => t.send(request).await,
            Self::Http(t) => t.send(request).await,
        }
    }

    fn next_request_id(&self) -> u64 {
        match self {
            Self::Stdio(t) => t.next_request_id(),
            Self::Http(t) => t.next_request_id(),
        }
    }

    async fn shutdown(&self) {
        match self {
            Self::Stdio(t) => t.shutdown().await,
            Self::Http(t) => t.shutdown().await,
        }
    }
}

// ---------------------------------------------------------------------------
// Per-server connection state
// ---------------------------------------------------------------------------

/// State for a single connected MCP server.
struct McpConnection {
    /// The transport used to talk to this server.
    transport: TransportHandle,
    /// Server information returned during the handshake.
    #[allow(dead_code)]
    server_info: Option<protocol::ServerInfo>,
    /// Cached tool definitions from the last `tools/list` call.
    tools: Vec<McpTool>,
}

// ---------------------------------------------------------------------------
// McpManager
// ---------------------------------------------------------------------------

/// Central manager that holds connections to one or more MCP servers.
///
/// # Usage
///
/// ```ignore
/// let mut mgr = McpManager::new();
/// mgr.connect_all(&configs).await?;
///
/// for tool in mgr.list_tools() {
///     println!("{}: {:?}", tool.0, tool.1.name);
/// }
///
/// let result = mgr.call_tool("filesystem", "read_file", json!({"path": "/tmp/x.txt"})).await?;
/// mgr.disconnect_all().await;
/// ```
pub struct McpManager {
    connections: HashMap<String, McpConnection>,
}

impl McpManager {
    /// Create a new, empty manager with no connections.
    #[must_use]
    pub fn new() -> Self {
        Self {
            connections: HashMap::new(),
        }
    }

    /// Connect to all configured MCP servers.
    ///
    /// Each config is validated, the transport is started, the MCP `initialize`
    /// handshake is performed, and the tools list is fetched and cached.
    ///
    /// Servers that fail to connect are logged and skipped -- the remaining
    /// servers are still connected.
    pub async fn connect_all(&mut self, configs: &[McpServerConfig]) -> Result<(), McpError> {
        for cfg in configs {
            if let Err(e) = self.connect_one(cfg).await {
                warn!(server = %cfg.name, error = %e, "skipping MCP server that failed to connect");
            }
        }
        Ok(())
    }

    /// Connect to a single MCP server according to its configuration.
    async fn connect_one(&mut self, cfg: &McpServerConfig) -> Result<(), McpError> {
        cfg.validate()?;

        info!(server = %cfg.name, transport = ?cfg.transport, "connecting to MCP server");

        let transport = match cfg.transport {
            McpTransportKind::Stdio => {
                let command = cfg.command.as_deref().unwrap_or_default();
                let args = cfg.args.clone().unwrap_or_default();
                let env = cfg.env.clone().unwrap_or_default();
                let t = StdioTransport::start(command, &args, &env).await?;
                TransportHandle::Stdio(t)
            }
            McpTransportKind::Http => {
                let url = cfg.url.as_deref().unwrap_or_default();
                let t = HttpTransport::connect(url).await?;
                TransportHandle::Http(t)
            }
        };

        // --- MCP initialize handshake ---
        let init_result = self.do_initialize(&transport).await?;

        info!(
            server = %cfg.name,
            protocol_version = %init_result.protocol_version,
            server_name = init_result.server_info.as_ref().map(|s| s.name.as_str()).unwrap_or("unknown"),
            "MCP handshake completed",
        );

        // --- Send initialized notification ---
        // The MCP spec requires the client to send `notifications/initialized`
        // after a successful initialize response. We fire-and-forget (no id).
        // Since our transport expects a response for every request, we send
        // this as a notification by writing a request with id=0 and ignoring
        // the result.  In practice many servers simply do not reply to
        // notifications; the reader task will discard any response.
        let notif_id = transport.next_request_id();
        let notif = JsonRpcRequest::new(notif_id, "notifications/initialized", None);
        // Best-effort: if this fails we still consider the connection alive.
        if let Err(e) = transport.send(notif).await {
            debug!(server = %cfg.name, error = %e, "initialized notification send failed (non-fatal)");
        }

        // --- Fetch available tools ---
        let tools = self.do_list_tools(&transport).await.unwrap_or_else(|e| {
            warn!(server = %cfg.name, error = %e, "failed to list tools, assuming none");
            Vec::new()
        });

        info!(server = %cfg.name, tool_count = tools.len(), "MCP server tools loaded");

        self.connections.insert(
            cfg.name.clone(),
            McpConnection {
                transport,
                server_info: init_result.server_info,
                tools,
            },
        );

        Ok(())
    }

    /// Perform the `initialize` JSON-RPC call.
    async fn do_initialize(
        &self,
        transport: &TransportHandle,
    ) -> Result<InitializeResult, McpError> {
        let id = transport.next_request_id();
        let params = InitializeParams {
            protocol_version: MCP_PROTOCOL_VERSION.to_string(),
            capabilities: ClientCapabilities {},
            client_info: ClientInfo {
                name: "rune".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
        };

        let request = JsonRpcRequest::new(
            id,
            "initialize",
            Some(serde_json::to_value(&params).map_err(|e| {
                McpError::protocol(format!("failed to serialize initialize params: {e}"))
            })?),
        );

        let response = transport.send(request).await?;
        let result_value = response.into_result()?;

        serde_json::from_value::<InitializeResult>(result_value)
            .map_err(|e| McpError::init_failed(format!("malformed initialize result: {e}")))
    }

    /// Perform the `tools/list` JSON-RPC call.
    async fn do_list_tools(&self, transport: &TransportHandle) -> Result<Vec<McpTool>, McpError> {
        let id = transport.next_request_id();
        let request = JsonRpcRequest::new(id, "tools/list", None);
        let response = transport.send(request).await?;
        let result_value = response.into_result()?;

        let list: ToolsListResult = serde_json::from_value(result_value)
            .map_err(|e| McpError::protocol(format!("malformed tools/list result: {e}")))?;

        Ok(list.tools)
    }

    /// Disconnect and shut down all server connections.
    pub async fn disconnect_all(&mut self) {
        for (name, conn) in self.connections.drain() {
            info!(server = %name, "disconnecting MCP server");
            conn.transport.shutdown().await;
        }
    }

    /// Return every tool from every connected server, tagged with the server name.
    ///
    /// The returned tuples are `(server_name, tool_definition)`.
    #[must_use]
    pub fn list_tools(&self) -> Vec<(&str, &McpTool)> {
        let mut out = Vec::new();
        for (name, conn) in &self.connections {
            for tool in &conn.tools {
                out.push((name.as_str(), tool));
            }
        }
        out
    }

    /// Return tools from a specific server.
    pub fn list_server_tools(&self, server: &str) -> Result<&[McpTool], McpError> {
        let conn = self
            .connections
            .get(server)
            .ok_or_else(|| McpError::ServerNotFound(server.to_string()))?;
        Ok(&conn.tools)
    }

    /// Invoke a tool on the named server.
    pub async fn call_tool(
        &self,
        server: &str,
        tool_name: &str,
        arguments: Value,
    ) -> Result<McpToolResult, McpError> {
        let conn = self
            .connections
            .get(server)
            .ok_or_else(|| McpError::ServerNotFound(server.to_string()))?;

        // Verify the tool actually exists on this server.
        if !conn.tools.iter().any(|t| t.name == tool_name) {
            return Err(McpError::ToolNotFound {
                server: server.to_string(),
                tool: tool_name.to_string(),
            });
        }

        let id = conn.transport.next_request_id();
        let params = serde_json::json!({
            "name": tool_name,
            "arguments": arguments,
        });

        let request = JsonRpcRequest::new(id, "tools/call", Some(params));
        let response = conn.transport.send(request).await?;
        let result_value = response.into_result()?;

        serde_json::from_value::<McpToolResult>(result_value)
            .map_err(|e| McpError::protocol(format!("malformed tools/call result: {e}")))
    }

    /// Refresh the tool list for a specific server by re-invoking `tools/list`.
    pub async fn refresh_tools(&mut self, server: &str) -> Result<(), McpError> {
        let conn = self
            .connections
            .get(server)
            .ok_or_else(|| McpError::ServerNotFound(server.to_string()))?;

        let tools = self.do_list_tools(&conn.transport).await?;

        // Re-borrow mutably to update.
        let conn = self.connections.get_mut(server).unwrap();
        conn.tools = tools;
        Ok(())
    }

    /// Return the names of all connected servers.
    #[must_use]
    pub fn connected_servers(&self) -> Vec<&str> {
        self.connections.keys().map(|s| s.as_str()).collect()
    }

    /// Return `true` if the manager has no active connections.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.connections.is_empty()
    }
}

impl Default for McpManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_manager_is_empty() {
        let mgr = McpManager::new();
        assert!(mgr.is_empty());
        assert!(mgr.list_tools().is_empty());
        assert!(mgr.connected_servers().is_empty());
    }

    #[test]
    fn list_server_tools_returns_not_found() {
        let mgr = McpManager::new();
        let err = mgr.list_server_tools("nonexistent").unwrap_err();
        assert!(matches!(err, McpError::ServerNotFound(_)));
    }

    #[tokio::test]
    async fn call_tool_returns_not_found_for_missing_server() {
        let mgr = McpManager::new();
        let err = mgr
            .call_tool("ghost", "some_tool", serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(matches!(err, McpError::ServerNotFound(_)));
    }

    #[tokio::test]
    async fn disconnect_all_on_empty_is_noop() {
        let mut mgr = McpManager::new();
        mgr.disconnect_all().await;
        assert!(mgr.is_empty());
    }

    #[tokio::test]
    async fn connect_all_with_empty_configs_is_noop() {
        let mut mgr = McpManager::new();
        let result = mgr.connect_all(&[]).await;
        assert!(result.is_ok());
        assert!(mgr.is_empty());
    }
}
