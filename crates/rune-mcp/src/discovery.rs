//! Configuration types for MCP server connections.
//!
//! These structures are typically deserialized from the application config and
//! passed to [`McpManager::connect_all`](crate::McpManager::connect_all).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Transport mechanism for reaching an MCP server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpTransportKind {
    /// Communicate over a child process's stdin/stdout.
    Stdio,
    /// Communicate over HTTP (POST for requests, optional SSE for events).
    Http,
}

/// Configuration for a single MCP server connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Human-readable name used as a key in the connection map.
    pub name: String,

    /// Which transport to use.
    pub transport: McpTransportKind,

    /// Command to spawn (required for `Stdio` transport).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,

    /// Arguments passed to the spawned command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,

    /// Extra environment variables injected into the subprocess.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,

    /// Server URL (required for `Http` transport).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl McpServerConfig {
    /// Validate that required fields are present for the chosen transport.
    pub fn validate(&self) -> Result<(), crate::error::McpError> {
        match self.transport {
            McpTransportKind::Stdio => {
                if self.command.is_none() {
                    return Err(crate::error::McpError::init_failed(format!(
                        "server '{}': stdio transport requires a 'command'",
                        self.name,
                    )));
                }
            }
            McpTransportKind::Http => {
                if self.url.is_none() {
                    return Err(crate::error::McpError::init_failed(format!(
                        "server '{}': http transport requires a 'url'",
                        self.name,
                    )));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdio_config_validates_command_present() {
        let cfg = McpServerConfig {
            name: "fs".into(),
            transport: McpTransportKind::Stdio,
            command: Some("mcp-server-filesystem".into()),
            args: Some(vec!["/tmp".into()]),
            env: None,
            url: None,
        };
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn stdio_config_rejects_missing_command() {
        let cfg = McpServerConfig {
            name: "fs".into(),
            transport: McpTransportKind::Stdio,
            command: None,
            args: None,
            env: None,
            url: None,
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn http_config_validates_url_present() {
        let cfg = McpServerConfig {
            name: "remote".into(),
            transport: McpTransportKind::Http,
            command: None,
            args: None,
            env: None,
            url: Some("http://localhost:3001".into()),
        };
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn http_config_rejects_missing_url() {
        let cfg = McpServerConfig {
            name: "remote".into(),
            transport: McpTransportKind::Http,
            command: None,
            args: None,
            env: None,
            url: None,
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn transport_kind_roundtrips() {
        let json = serde_json::to_string(&McpTransportKind::Stdio).unwrap();
        assert_eq!(json, "\"stdio\"");
        let restored: McpTransportKind = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, McpTransportKind::Stdio);
    }

    #[test]
    fn server_config_roundtrips() {
        let cfg = McpServerConfig {
            name: "test".into(),
            transport: McpTransportKind::Http,
            command: None,
            args: None,
            env: Some(HashMap::from([("API_KEY".into(), "secret".into())])),
            url: Some("http://localhost:8080".into()),
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let restored: McpServerConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.name, "test");
        assert_eq!(restored.url.as_deref(), Some("http://localhost:8080"));
    }
}
