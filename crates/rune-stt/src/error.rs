/// Errors produced by the STT subsystem.
#[derive(Debug, thiserror::Error)]
pub enum SttError {
    #[error("STT disabled")]
    Disabled,

    #[error("provider error: {0}")]
    Provider(String),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),

    #[error("configuration error: {0}")]
    Config(String),
}
