use std::io;

/// Errors produced by MCP client operations.
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("transport error: {0}")]
    Transport(String),

    #[error("protocol error: {0}")]
    Protocol(String),

    #[error("server not found: {0}")]
    ServerNotFound(String),

    #[error("tool not found: {server}/{tool}")]
    ToolNotFound { server: String, tool: String },

    #[error("initialization failed: {0}")]
    InitFailed(String),

    #[error("io error: {0}")]
    Io(#[from] io::Error),
}

impl McpError {
    /// Convenience constructor for transport-level failures.
    pub fn transport(msg: impl Into<String>) -> Self {
        Self::Transport(msg.into())
    }

    /// Convenience constructor for protocol-level failures.
    pub fn protocol(msg: impl Into<String>) -> Self {
        Self::Protocol(msg.into())
    }

    /// Convenience constructor for initialization failures.
    pub fn init_failed(msg: impl Into<String>) -> Self {
        Self::InitFailed(msg.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_transport_error() {
        let err = McpError::transport("connection refused");
        assert_eq!(err.to_string(), "transport error: connection refused");
    }

    #[test]
    fn display_tool_not_found() {
        let err = McpError::ToolNotFound {
            server: "filesystem".into(),
            tool: "read_file".into(),
        };
        assert_eq!(err.to_string(), "tool not found: filesystem/read_file");
    }

    #[test]
    fn from_io_error() {
        let io_err = io::Error::new(io::ErrorKind::BrokenPipe, "pipe closed");
        let err: McpError = io_err.into();
        assert!(matches!(err, McpError::Io(_)));
        assert!(err.to_string().contains("pipe closed"));
    }
}
