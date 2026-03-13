use thiserror::Error;

/// Errors produced by the runtime engine.
#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("session not found: {0}")]
    SessionNotFound(String),

    #[error("invalid session state for operation: expected {expected}, got {actual}")]
    InvalidSessionState { expected: String, actual: String },

    #[error("invalid turn state transition: {from} -> {to}")]
    InvalidTurnTransition { from: String, to: String },

    #[error("model error: {0}")]
    Model(#[from] rune_models::ModelError),

    #[error("tool error: {0}")]
    Tool(#[from] rune_tools::ToolError),

    #[error("store error: {0}")]
    Store(#[from] rune_store::StoreError),

    #[error("context assembly error: {0}")]
    ContextAssembly(String),

    #[error("turn execution aborted: {0}")]
    Aborted(String),

    #[error("max tool iterations ({0}) exceeded")]
    MaxToolIterations(u32),
}
