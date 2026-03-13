use thiserror::Error;

/// Errors produced by the tool subsystem.
#[derive(Debug, Error)]
pub enum ToolError {
    #[error("unknown tool: {name}")]
    UnknownTool { name: String },

    #[error("invalid arguments for tool {tool}: {reason}")]
    InvalidArguments { tool: String, reason: String },

    #[error("tool execution failed: {message}")]
    ExecutionFailed { message: String },

    #[error("approval required for tool {tool}")]
    ApprovalRequired { tool: String },

    #[error("approval denied for tool {tool}")]
    ApprovalDenied { tool: String },
}
