//! Store-level error types.

use thiserror::Error;

/// Persistence and database errors.
#[derive(Debug, Error)]
pub enum StoreError {
    /// A database query or connection error.
    #[error("database error: {0}")]
    Database(String),

    /// The requested entity was not found.
    #[error("{entity} not found: {id}")]
    NotFound {
        entity: &'static str,
        id: String,
    },

    /// A constraint or uniqueness violation.
    #[error("conflict: {0}")]
    Conflict(String),

    /// Schema migration failure.
    #[error("migration error: {0}")]
    Migration(String),

    /// Serialization/deserialization of stored JSON payloads.
    #[error("serialization error: {0}")]
    Serialization(String),
}

impl From<serde_json::Error> for StoreError {
    fn from(err: serde_json::Error) -> Self {
        Self::Serialization(err.to_string())
    }
}
