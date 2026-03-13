use thiserror::Error;

/// Errors returned by model providers.
#[derive(Debug, Error)]
pub enum ModelError {
    /// Authentication or permission failure.
    #[error("auth error: {0}")]
    Auth(String),

    /// Rate-limited — includes optional retry-after seconds.
    #[error("rate limited (retry after {retry_after_secs:?}s): {message}")]
    RateLimited {
        message: String,
        retry_after_secs: Option<u64>,
    },

    /// Request exceeded the model's context window.
    #[error("context length exceeded: {0}")]
    ContextLengthExceeded(String),

    /// Deployment not found or misnamed (Azure-specific).
    #[error("deployment not found: {0}")]
    DeploymentNotFound(String),

    /// Unsupported API version (Azure-specific).
    #[error("unsupported api version: {0}")]
    UnsupportedApiVersion(String),

    /// Content filtered / policy block.
    #[error("content filtered: {0}")]
    ContentFiltered(String),

    /// Quota exhaustion.
    #[error("quota exhausted: {0}")]
    QuotaExhausted(String),

    /// Configuration error (missing fields, invalid values).
    #[error("configuration error: {0}")]
    Configuration(String),

    /// Transient upstream / service error — safe to retry.
    #[error("transient error: {0}")]
    Transient(String),

    /// HTTP transport error.
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    /// Unexpected provider response.
    #[error("provider error: {0}")]
    Provider(String),
}
