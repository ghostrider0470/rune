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
impl From<diesel::result::Error> for StoreError {
    fn from(err: diesel::result::Error) -> Self {
        match err {
            diesel::result::Error::NotFound => Self::NotFound {
                entity: "record",
                id: "unknown".to_string(),
            },
            diesel::result::Error::DatabaseError(
                diesel::result::DatabaseErrorKind::UniqueViolation,
                info,
            ) => Self::Conflict(info.message().to_string()),
            other => Self::Database(other.to_string()),
        }
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
