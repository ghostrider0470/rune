#![doc = "MCP (Model Context Protocol) client for Rune: manages connections to external MCP tool servers over STDIO and HTTP transports."]

pub mod discovery;
pub mod error;
pub mod memory_server;
pub mod protocol;
pub mod transport_http;
pub mod transport_stdio;

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use rune_core::ToolCategory;
use rune_tools::{ToolCall, ToolDefinition, ToolError, ToolExecutor, ToolResult};
use serde_json::Value;
use tracing::{debug, info, warn};

use crate::discovery::{McpServerConfig, McpTransportKind};
use crate::error::McpError;
use crate::protocol::{
    ClientCapabilities, ClientInfo, InitializeParams, InitializeResult, JsonRpcRequest,
    MCP_PROTOCOL_VERSION, ToolsListResult,
};
use crate::transport_http::HttpTransport;
use crate::transport_stdio::StdioTransport;

pub use crate::protocol::{McpTool, McpToolResult};

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

    async fn notify(&self, method: &str, params: Option<Value>) -> Result<(), McpError> {
        match self {
            Self::Stdio(t) => t.notify(method, params).await,
            Self::Http(t) => t.notify(method, params).await,
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
            if !cfg.enabled {
                info!(server = %cfg.name, "skipping disabled MCP server");
                continue;
            }
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
                let t = StdioTransport::start(command, &args, &cfg.env, cfg.cwd.as_deref()).await?;
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
        if let Err(e) = transport.notify("notifications/initialized", None).await {
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

    /// Return every MCP tool definition with a `server__tool` prefix applied.
    #[must_use]
    pub fn list_prefixed_tools(&self) -> Vec<McpTool> {
        let mut tools = Vec::new();
        for (server, tool) in self.list_tools() {
            let mut tool = tool.clone();
            tool.name = format!("{server}__{}", tool.name);
            tools.push(tool);
        }
        tools.sort_by(|left, right| left.name.cmp(&right.name));
        tools
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

    /// Invoke a tool using the `server__tool` registry name.
    pub async fn call_prefixed_tool(
        &self,
        prefixed_tool_name: &str,
        arguments: Value,
    ) -> Result<McpToolResult, McpError> {
        let (server, tool_name) = prefixed_tool_name.split_once("__").ok_or_else(|| {
            McpError::protocol(format!(
                "invalid MCP tool name '{prefixed_tool_name}'; expected server__tool"
            ))
        })?;

        self.call_tool(server, tool_name, arguments).await
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

/// Bridges MCP tools into Rune's tool registry and execution surface.
pub struct McpToolExecutor {
    manager: Arc<McpManager>,
}

impl McpToolExecutor {
    #[must_use]
    pub fn new(manager: Arc<McpManager>) -> Self {
        Self { manager }
    }

    /// Convert connected MCP tools into Rune tool definitions.
    #[must_use]
    pub fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.manager
            .list_prefixed_tools()
            .into_iter()
            .map(|tool| ToolDefinition {
                name: tool.name,
                description: tool.description.unwrap_or_default(),
                parameters: tool.input_schema,
                category: ToolCategory::External,
                requires_approval: false,
            })
            .collect()
    }
}

#[async_trait]
impl ToolExecutor for McpToolExecutor {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let result = self
            .manager
            .call_prefixed_tool(&call.tool_name, call.arguments)
            .await
            .map_err(|error| ToolError::ExecutionFailed(error.to_string()))?;

        let mut lines = Vec::new();
        for item in &result.content {
            match item {
                protocol::McpContent::Text { text } => lines.push(text.clone()),
                protocol::McpContent::Resource { uri, text } => {
                    if let Some(text) = text {
                        lines.push(text.clone());
                    } else {
                        lines.push(format!("[resource] {uri}"));
                    }
                }
                protocol::McpContent::Image { mime_type, .. } => {
                    lines.push(format!("[image] {mime_type}"));
                }
            }
        }

        Ok(ToolResult {
            tool_call_id: call.tool_call_id,
            output: lines.join("\n"),
            is_error: result.is_error.unwrap_or(false),
            tool_execution_id: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune_core::ToolCallId;
    use std::fs;
    use std::path::PathBuf;

    fn write_stdio_server_script(tool_name: &str, description: &str, result_json: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("rune-mcp-test-{}.py", uuid::Uuid::now_v7()));
        let script = format!(
            r#"import json
import sys

TOOL_NAME = {tool_name:?}
DESCRIPTION = {description:?}
RESULT = json.loads({result_json:?})

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    msg = json.loads(line)
    method = msg.get("method")
    if method == "initialize":
        print(json.dumps({{
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": {{
                "protocolVersion": "2024-11-05",
                "capabilities": {{"tools": {{"listChanged": True}}}},
                "serverInfo": {{"name": "test-server", "version": "1.0.0"}}
            }}
        }}), flush=True)
    elif method == "tools/list":
        print(json.dumps({{
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": {{
                "tools": [{{
                    "name": TOOL_NAME,
                    "description": DESCRIPTION,
                    "inputSchema": {{
                        "type": "object",
                        "properties": {{
                            "path": {{"type": "string"}},
                            "query": {{"type": "string"}}
                        }}
                    }}
                }}]
            }}
        }}), flush=True)
    elif method == "tools/call":
        print(json.dumps({{
            "jsonrpc": "2.0",
            "id": msg["id"],
            "result": RESULT
        }}), flush=True)
"#
        );
        fs::write(&path, script).unwrap();
        path
    }

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

    #[tokio::test]
    async fn connect_http_server_registers_prefixed_tools_and_executes_calls() {
        let script_path = write_stdio_server_script(
            "read_file",
            "Read a file",
            r#"{"content":[{"type":"text","text":"demo output"}],"isError":false}"#,
        );

        let mut manager = McpManager::new();
        manager
            .connect_all(&[McpServerConfig {
                name: "filesystem".into(),
                transport: McpTransportKind::Stdio,
                command: Some("python3".into()),
                args: Some(vec![script_path.display().to_string()]),
                env: HashMap::new(),
                cwd: None,
                url: None,
                enabled: true,
            }])
            .await
            .unwrap();

        let tools = manager.list_prefixed_tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "filesystem__read_file");

        let result = manager
            .call_prefixed_tool(
                "filesystem__read_file",
                serde_json::json!({ "path": "/tmp/demo" }),
            )
            .await
            .unwrap();
        assert_eq!(result.content.len(), 1);

        let _ = fs::remove_file(script_path);
    }

    #[tokio::test]
    async fn tool_executor_maps_mcp_results_into_rune_output() {
        let script_path = write_stdio_server_script(
            "lookup",
            "Lookup a note",
            r#"{"content":[{"type":"text","text":"first line"},{"type":"resource","uri":"memory://note/42","text":"second line"}],"isError":false}"#,
        );

        let mut manager = McpManager::new();
        manager
            .connect_all(&[McpServerConfig {
                name: "notes".into(),
                transport: McpTransportKind::Stdio,
                command: Some("python3".into()),
                args: Some(vec![script_path.display().to_string()]),
                env: HashMap::new(),
                cwd: None,
                url: None,
                enabled: true,
            }])
            .await
            .unwrap();

        let executor = McpToolExecutor::new(Arc::new(manager));
        let defs = executor.tool_definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "notes__lookup");

        let result = executor
            .execute(ToolCall {
                tool_call_id: ToolCallId::new(),
                tool_name: "notes__lookup".into(),
                arguments: serde_json::json!({ "query": "note" }),
            })
            .await
            .unwrap();
        assert_eq!(result.output, "first line\nsecond line");
        assert!(!result.is_error);

        let _ = fs::remove_file(script_path);
    }
}
