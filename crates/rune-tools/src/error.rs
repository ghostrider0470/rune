use thiserror::Error;

/// Errors produced by the tool subsystem.
#[derive(Debug, Error)]
pub enum ToolError {
    #[error("unknown tool: {name}")]
    UnknownTool { name: String },

    #[error("tool not found: {0}")]
    NotFound(String),

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    #[error("invalid arguments for tool {tool}: {reason}")]
    InvalidArguments { tool: String, reason: String },

    #[error("tool execution failed: {0}")]
    ExecutionFailed(String),

    #[error("tool execution failed: {message}")]
    ExecutionFailedStructured { message: String },

    #[error("approval required for tool {tool}: {details}")]
    ApprovalRequired { tool: String, details: String },

    #[error("approval denied for tool {tool}")]
    ApprovalDenied { tool: String },

    #[error("tool circuit breaker open for {tool}: {message}")]
    CircuitOpen { tool: String, message: String },
}
