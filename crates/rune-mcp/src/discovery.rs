//! Configuration types for MCP server connections.
//!
//! These structures are typically deserialized from the application config and
//! passed to [`McpManager::connect_all`](crate::McpManager::connect_all).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::McpError;

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
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Working directory for the subprocess.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,

    /// Server URL (required for `Http` transport).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// Static HTTP headers sent with every request.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub http_headers: HashMap<String, String>,

    /// Header -> env-var map resolved at runtime for secrets.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub http_headers_env: HashMap<String, String>,

    /// Whether this server is enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
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

    pub fn resolved_http_headers(&self) -> Result<HashMap<String, String>, McpError> {
        let mut headers = self.http_headers.clone();
        for (header_name, env_name) in &self.http_headers_env {
            let value = std::env::var(env_name).map_err(|_| {
                McpError::init_failed(format!(
                    "server '{}': missing env var '{}' for HTTP header '{}'",
                    self.name, env_name, header_name
                ))
            })?;
            headers.insert(header_name.clone(), value);
        }
        Ok(headers)
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
            env: HashMap::new(),
            cwd: None,
            url: None,
            http_headers: HashMap::new(),
            http_headers_env: HashMap::new(),
            enabled: true,
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
            env: HashMap::new(),
            cwd: None,
            url: None,
            http_headers: HashMap::new(),
            http_headers_env: HashMap::new(),
            enabled: true,
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
            env: HashMap::new(),
            cwd: None,
            url: Some("http://localhost:3001".into()),
            http_headers: HashMap::new(),
            http_headers_env: HashMap::new(),
            enabled: true,
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
            env: HashMap::new(),
            cwd: None,
            url: None,
            http_headers: HashMap::new(),
            http_headers_env: HashMap::new(),
            enabled: true,
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn resolved_http_headers_merge_inline_and_env() {
        let cfg = McpServerConfig {
            name: "remote".into(),
            transport: McpTransportKind::Http,
            command: None,
            args: None,
            env: HashMap::new(),
            cwd: None,
            url: Some("http://localhost:3001".into()),
            http_headers: HashMap::from([("Authorization".into(), "Bearer inline".into())]),
            http_headers_env: HashMap::from([("X-API-Key".into(), "RUNE_TEST_MCP_HEADER".into())]),
            enabled: true,
        };
        unsafe {
            std::env::set_var("RUNE_TEST_MCP_HEADER", "secret-from-env");
        }
        let headers = cfg.resolved_http_headers().unwrap();
        assert_eq!(
            headers.get("Authorization").map(String::as_str),
            Some("Bearer inline")
        );
        assert_eq!(
            headers.get("X-API-Key").map(String::as_str),
            Some("secret-from-env")
        );
        unsafe {
            std::env::remove_var("RUNE_TEST_MCP_HEADER");
        }
    }

    #[test]
    fn resolved_http_headers_errors_when_env_missing() {
        let cfg = McpServerConfig {
            name: "remote".into(),
            transport: McpTransportKind::Http,
            command: None,
            args: None,
            env: HashMap::new(),
            cwd: None,
            url: Some("http://localhost:3001".into()),
            http_headers: HashMap::new(),
            http_headers_env: HashMap::from([(
                "Authorization".into(),
                "RUNE_TEST_MISSING_MCP_HEADER".into(),
            )]),
            enabled: true,
        };
        unsafe {
            std::env::remove_var("RUNE_TEST_MISSING_MCP_HEADER");
        }
        let err = cfg.resolved_http_headers().unwrap_err();
        assert!(err.to_string().contains("RUNE_TEST_MISSING_MCP_HEADER"));
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
            env: HashMap::from([("API_KEY".into(), "secret".into())]),
            cwd: None,
            url: Some("http://localhost:8080".into()),
            http_headers: HashMap::from([("Authorization".into(), "Bearer token".into())]),
            http_headers_env: HashMap::from([("X-API-Key".into(), "MCP_API_KEY".into())]),
            enabled: false,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let restored: McpServerConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.name, "test");
        assert_eq!(restored.url.as_deref(), Some("http://localhost:8080"));
        assert!(!restored.enabled);
    }
}
