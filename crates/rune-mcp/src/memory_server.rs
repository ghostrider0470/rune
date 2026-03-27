//! MCP Memory Server — exposes Rune's shared vector memory as an MCP tool server.
//!
//! Designed to run as a stdio-based MCP server that Claude Code, Codex, and other
//! MCP-aware agents can connect to for shared knowledge recall and storage.
//!
//! ## Tools exposed
//!
//! - `memory_recall` — semantic search over shared memory
//! - `memory_store` — store a fact directly
//! - `memory_list` — list stored memories
//! - `memory_delete` — delete a memory by ID
//!
//! ## Usage
//!
//! ```bash
//! rune mcp-memory-server --rune-url http://127.0.0.1:18790
//! ```
//!
//! Or in Claude Code's MCP config:
//! ```json
//! {
//!   "mcpServers": {
//!     "rune-memory": {
//!       "command": "rune",
//!       "args": ["mcp-memory-server"]
//!     }
//!   }
//! }
//! ```

use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use tracing::{debug, error, info};

use crate::protocol::{JsonRpcRequest, JsonRpcResponse, JsonRpcError, MCP_PROTOCOL_VERSION};

/// Default Rune gateway URL for memory API.
const DEFAULT_RUNE_URL: &str = "http://127.0.0.1:18790";

/// Run the MCP memory server over stdio.
///
/// Reads JSON-RPC requests from stdin, dispatches to the memory API,
/// and writes JSON-RPC responses to stdout.
pub async fn run_stdio_server(rune_url: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let base_url = rune_url.unwrap_or_else(|| DEFAULT_RUNE_URL.to_string());
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    info!(url = %base_url, "MCP memory server starting (stdio)");

    let stdin = io::stdin();
    let stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                error!(error = %e, "failed to read stdin");
                break;
            }
        };

        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let err_resp = json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": {"code": -32700, "message": format!("Parse error: {e}")}
                });
                writeln!(stdout.lock(), "{}", serde_json::to_string(&err_resp)?)?;
                continue;
            }
        };

        debug!(method = %request.method, id = request.id, "MCP request");

        let response = handle_request(&request, &base_url, &client).await;
        let json = serde_json::to_string(&response)?;
        writeln!(stdout.lock(), "{json}")?;
        stdout.lock().flush()?;
    }

    Ok(())
}

/// Dispatch a JSON-RPC request to the appropriate handler.
async fn handle_request(
    req: &JsonRpcRequest,
    base_url: &str,
    client: &reqwest::Client,
) -> JsonRpcResponse {
    match req.method.as_str() {
        "initialize" => handle_initialize(req),
        "tools/list" => handle_tools_list(req),
        "tools/call" => handle_tools_call(req, base_url, client).await,
        "notifications/initialized" | "initialized" => {
            // Client ack — no response needed for notifications, but respond anyway
            JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: req.id,
                result: Some(json!({})),
                error: None,
            }
        }
        _ => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: req.id,
            result: None,
            error: Some(JsonRpcError {
                code: -32601,
                message: format!("Method not found: {}", req.method),
                data: None,
            }),
        },
    }
}

fn handle_initialize(req: &JsonRpcRequest) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: req.id,
        result: Some(json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "rune-memory",
                "version": "0.1.0"
            }
        })),
        error: None,
    }
}

fn handle_tools_list(req: &JsonRpcRequest) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: req.id,
        result: Some(json!({
            "tools": [
                {
                    "name": "memory_recall",
                    "description": "Semantic recall from Rune's shared vector memory. Returns memories similar to the query.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "query": {
                                "type": "string",
                                "description": "Search query for semantic recall"
                            },
                            "top_k": {
                                "type": "integer",
                                "description": "Max results to return (default 10)",
                                "default": 10
                            }
                        },
                        "required": ["query"]
                    }
                },
                {
                    "name": "memory_store",
                    "description": "Store a fact in Rune's shared vector memory for cross-agent access.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "fact": {
                                "type": "string",
                                "description": "The fact to store"
                            },
                            "category": {
                                "type": "string",
                                "description": "Category (decisions, preferences, architecture, people, projects, general)",
                                "default": "general"
                            }
                        },
                        "required": ["fact"]
                    }
                },
                {
                    "name": "memory_list",
                    "description": "List stored memories with pagination.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "limit": {
                                "type": "integer",
                                "description": "Max results (default 50)",
                                "default": 50
                            },
                            "offset": {
                                "type": "integer",
                                "description": "Pagination offset",
                                "default": 0
                            }
                        }
                    }
                },
                {
                    "name": "memory_delete",
                    "description": "Delete a memory by its UUID.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "id": {
                                "type": "string",
                                "description": "Memory UUID to delete"
                            }
                        },
                        "required": ["id"]
                    }
                }
            ]
        })),
        error: None,
    }
}

async fn handle_tools_call(
    req: &JsonRpcRequest,
    base_url: &str,
    client: &reqwest::Client,
) -> JsonRpcResponse {
    let params = req.params.as_ref();
    let tool_name = params
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("");
    let arguments = params
        .and_then(|p| p.get("arguments"))
        .cloned()
        .unwrap_or_else(|| json!({}));

    let result = match tool_name {
        "memory_recall" => call_recall(base_url, client, &arguments).await,
        "memory_store" => call_store(base_url, client, &arguments).await,
        "memory_list" => call_list(base_url, client, &arguments).await,
        "memory_delete" => call_delete(base_url, client, &arguments).await,
        _ => Err(format!("Unknown tool: {tool_name}")),
    };

    match result {
        Ok(content) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: req.id,
            result: Some(json!({
                "content": [{
                    "type": "text",
                    "text": serde_json::to_string_pretty(&content).unwrap_or_default()
                }]
            })),
            error: None,
        },
        Err(e) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: req.id,
            result: Some(json!({
                "content": [{
                    "type": "text",
                    "text": format!("Error: {e}")
                }],
                "isError": true
            })),
            error: None,
        },
    }
}

async fn call_recall(
    base_url: &str,
    client: &reqwest::Client,
    args: &Value,
) -> Result<Value, String> {
    let resp = client
        .post(format!("{base_url}/api/v1/memory/recall"))
        .json(args)
        .send()
        .await
        .map_err(|e| format!("HTTP error: {e}"))?;

    resp.json::<Value>()
        .await
        .map_err(|e| format!("JSON parse error: {e}"))
}

async fn call_store(
    base_url: &str,
    client: &reqwest::Client,
    args: &Value,
) -> Result<Value, String> {
    let resp = client
        .post(format!("{base_url}/api/v1/memory/store"))
        .json(args)
        .send()
        .await
        .map_err(|e| format!("HTTP error: {e}"))?;

    resp.json::<Value>()
        .await
        .map_err(|e| format!("JSON parse error: {e}"))
}

async fn call_list(
    base_url: &str,
    client: &reqwest::Client,
    args: &Value,
) -> Result<Value, String> {
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50);
    let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0);

    let resp = client
        .get(format!(
            "{base_url}/api/v1/memory/list?limit={limit}&offset={offset}"
        ))
        .send()
        .await
        .map_err(|e| format!("HTTP error: {e}"))?;

    resp.json::<Value>()
        .await
        .map_err(|e| format!("JSON parse error: {e}"))
}

async fn call_delete(
    base_url: &str,
    client: &reqwest::Client,
    args: &Value,
) -> Result<Value, String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "id is required".to_string())?;

    let resp = client
        .delete(format!("{base_url}/api/v1/memory/{id}"))
        .send()
        .await
        .map_err(|e| format!("HTTP error: {e}"))?;

    resp.json::<Value>()
        .await
        .map_err(|e| format!("JSON parse error: {e}"))
}
