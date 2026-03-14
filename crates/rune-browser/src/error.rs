/// Errors produced by the browser snapshot engine.
#[derive(Debug, thiserror::Error)]
pub enum BrowserError {
    /// Chrome/Chromium is not reachable at the configured CDP endpoint.
    #[error("browser not available: {0}")]
    NotAvailable(String),

    /// A page navigation did not complete successfully.
    #[error("navigation failed: {0}")]
    NavigationFailed(String),

    /// Snapshot capture (accessibility tree fetch or conversion) failed.
    #[error("snapshot failed: {0}")]
    SnapshotFailed(String),

    /// An operation exceeded the configured timeout.
    #[error("timeout after {0}ms")]
    Timeout(u64),

    /// An HTTP-level error from reqwest.
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
}
