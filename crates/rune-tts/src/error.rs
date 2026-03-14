/// Errors produced by the TTS subsystem.
#[derive(Debug, thiserror::Error)]
pub enum TtsError {
    #[error("TTS disabled")]
    Disabled,

    #[error("provider error: {0}")]
    Provider(String),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("configuration error: {0}")]
    Config(String),
}
