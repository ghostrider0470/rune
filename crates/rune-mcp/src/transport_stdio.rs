//! STDIO transport for MCP.
//!
//! Spawns a child process and communicates over line-delimited JSON on
//! stdin (writes) and stdout (reads).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, oneshot};
use tracing::{debug, error, warn};

use crate::error::McpError;
use crate::protocol::{JsonRpcRequest, JsonRpcResponse};

/// A transport that talks to an MCP server over a subprocess's stdin/stdout.
///
/// Each instance owns the child process. When the transport is shut down (via
/// [`StdioTransport::shutdown`]) the child process is killed.
pub struct StdioTransport {
    /// Shared writer to the child's stdin.
    stdin: Arc<Mutex<tokio::process::ChildStdin>>,
    /// In-flight requests waiting for their response, keyed by JSON-RPC id.
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>>,
    /// Monotonically increasing request id counter.
    next_id: Arc<AtomicU64>,
    /// Handle to the child process (kept so we can kill it on shutdown).
    child: Arc<Mutex<Child>>,
    /// Background reader task handle.
    reader_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl StdioTransport {
    /// Spawn the MCP server process and set up the transport.
    pub async fn start(
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<Self, McpError> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .envs(env)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd
            .spawn()
            .map_err(|e| McpError::transport(format!("failed to spawn '{command}': {e}")))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| McpError::transport("child process stdin not available"))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| McpError::transport("child process stdout not available"))?;

        let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<JsonRpcResponse>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Spawn a background task that reads line-delimited JSON from stdout.
        let pending_clone = Arc::clone(&pending);
        let reader_handle = tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        let line = line.trim().to_string();
                        if line.is_empty() {
                            continue;
                        }
                        match serde_json::from_str::<JsonRpcResponse>(&line) {
                            Ok(resp) => {
                                let id = resp.id;
                                let mut map = pending_clone.lock().await;
                                if let Some(tx) = map.remove(&id) {
                                    if tx.send(resp).is_err() {
                                        warn!(id, "response receiver dropped");
                                    }
                                } else {
                                    debug!(id, "received response for unknown request id");
                                }
                            }
                            Err(e) => {
                                debug!(
                                    error = %e,
                                    line_preview = &line[..line.len().min(120)],
                                    "ignoring non-JSON-RPC line from MCP server stdout",
                                );
                            }
                        }
                    }
                    Ok(None) => {
                        debug!("MCP server stdout closed");
                        break;
                    }
                    Err(e) => {
                        error!(error = %e, "error reading MCP server stdout");
                        break;
                    }
                }
            }
        });

        Ok(Self {
            stdin: Arc::new(Mutex::new(stdin)),
            pending,
            next_id: Arc::new(AtomicU64::new(1)),
            child: Arc::new(Mutex::new(child)),
            reader_handle: Mutex::new(Some(reader_handle)),
        })
    }

    /// Allocate the next JSON-RPC request id.
    pub fn next_request_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Send a JSON-RPC request and wait for the matching response.
    pub async fn send(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse, McpError> {
        let id = request.id;

        // Register the pending response channel **before** writing so we never
        // miss a fast reply.
        let (tx, rx) = oneshot::channel();
        {
            let mut map = self.pending.lock().await;
            map.insert(id, tx);
        }

        // Serialize and write the request as a single line.
        let mut line = serde_json::to_string(&request)
            .map_err(|e| McpError::protocol(format!("failed to serialize request: {e}")))?;
        line.push('\n');

        {
            let mut stdin = self.stdin.lock().await;
            stdin
                .write_all(line.as_bytes())
                .await
                .map_err(|e| McpError::transport(format!("failed to write to stdin: {e}")))?;
            stdin
                .flush()
                .await
                .map_err(|e| McpError::transport(format!("failed to flush stdin: {e}")))?;
        }

        // Wait for the reader task to deliver the response.
        rx.await.map_err(|_| {
            McpError::transport("response channel closed (server process may have exited)")
        })
    }

    /// Kill the child process and clean up.
    pub async fn shutdown(&self) {
        // Kill the child process.
        if let Ok(mut child) = self.child.try_lock() {
            let _ = child.kill().await;
        }

        // Abort the reader task.
        if let Some(handle) = self.reader_handle.lock().await.take() {
            handle.abort();
        }

        // Drop all pending waiters so they get an error.
        let mut map = self.pending.lock().await;
        map.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn start_nonexistent_command_returns_transport_error() {
        let result = StdioTransport::start(
            "/usr/bin/this-command-does-not-exist-rune-test",
            &[],
            &HashMap::new(),
        )
        .await;
        match result {
            Err(McpError::Transport(msg)) => {
                assert!(msg.contains("this-command-does-not-exist-rune-test"));
            }
            Err(other) => panic!("expected Transport error, got: {other}"),
            Ok(_) => panic!("expected error, got Ok"),
        }
    }
}
