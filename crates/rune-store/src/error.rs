//! Store-level error types.

use thiserror::Error;

/// Persistence and database errors.
#[derive(Debug, Error)]
pub enum StoreError {
    /// A Diesel query error.
    #[error("database query error: {0}")]
    Diesel(#[from] diesel::result::Error),

    /// A Diesel connection error.
    #[error("database connection error: {0}")]
    Connection(#[from] diesel::ConnectionError),

    /// An async connection or query execution error.
    #[error("async database error: {0}")]
    AsyncConnection(#[from] diesel_async::pooled_connection::deadpool::PoolError),

    /// Failed to build the database connection pool.
    #[error("database pool build error: {0}")]
    PoolBuild(#[from] diesel_async::pooled_connection::deadpool::BuildError),

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

    /// Embedded PostgreSQL bootstrap failure.
    #[error("embedded postgresql error: {0}")]
    EmbeddedPostgres(#[from] postgresql_embedded::Error),
}

impl From<serde_json::Error> for StoreError {
    fn from(err: serde_json::Error) -> Self {
        Self::Serialization(err.to_string())
    }
}
