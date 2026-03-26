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
    NotFound { entity: &'static str, id: String },

    /// A constraint or uniqueness violation.
    #[error("conflict: {0}")]
    Conflict(String),

    /// Schema migration failure.
    #[error("migration error: {0}")]
    Migration(String),

    /// Embedded PostgreSQL bootstrap or lifecycle error.
    #[error("embedded postgres error: {0}")]
    EmbeddedPg(String),

    /// Serialization/deserialization of stored JSON payloads.
    #[error("serialization error: {0}")]
    Serialization(String),

    /// An invalid state transition was attempted.
    #[error("invalid transition: {0}")]
    InvalidTransition(String),
}

impl From<serde_json::Error> for StoreError {
    fn from(err: serde_json::Error) -> Self {
        Self::Serialization(err.to_string())
    }
}

#[cfg(feature = "postgres")]
impl From<tokio_postgres::Error> for StoreError {
    fn from(err: tokio_postgres::Error) -> Self {
        let msg = err.to_string();
        if msg.contains("duplicate key") || msg.contains("unique constraint") {
            StoreError::Conflict(msg)
        } else {
            StoreError::Database(msg)
        }
    }
}

#[cfg(feature = "postgres")]
impl From<deadpool_postgres::PoolError> for StoreError {
    fn from(err: deadpool_postgres::PoolError) -> Self {
        StoreError::Database(format!("pool error: {err}"))
    }
}

#[cfg(feature = "sqlite")]
impl From<rusqlite::Error> for StoreError {
    fn from(err: rusqlite::Error) -> Self {
        match &err {
            rusqlite::Error::QueryReturnedNoRows => Self::NotFound {
                entity: "record",
                id: "unknown".to_string(),
            },
            rusqlite::Error::SqliteFailure(ffi_err, _)
                if ffi_err.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
                    || ffi_err.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY =>
            {
                Self::Conflict(err.to_string())
            }
            _ => Self::Database(err.to_string()),
        }
    }
}

#[cfg(feature = "sqlite")]
impl From<tokio_rusqlite::Error<rusqlite::Error>> for StoreError {
    fn from(err: tokio_rusqlite::Error<rusqlite::Error>) -> Self {
        match err {
            tokio_rusqlite::Error::Error(e) => Self::from(e),
            other => Self::Database(other.to_string()),
        }
    }
}

#[cfg(feature = "cosmos")]
impl From<azure_core::Error> for StoreError {
    fn from(err: azure_core::Error) -> Self {
        let msg = err.to_string();
        if msg.contains("NotFound") || msg.contains("404") {
            StoreError::NotFound {
                entity: "document",
                id: "unknown".to_string(),
            }
        } else if msg.contains("Conflict") || msg.contains("409") {
            StoreError::Conflict(msg)
        } else {
            StoreError::Database(msg)
        }
    }
}
