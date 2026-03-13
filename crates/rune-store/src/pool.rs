//! Connection pool creation and migration runner.

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel_async::AsyncPgConnection;
use diesel_async::pooled_connection::AsyncDieselConnectionManager;
use diesel_async::pooled_connection::deadpool::Pool;
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};

use crate::error::StoreError;

/// Async connection pool backed by deadpool.
pub type PgPool = Pool<AsyncPgConnection>;

/// Embedded migrations compiled into the binary.
pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

/// Create an async connection pool for the given database URL.
pub fn create_pool(database_url: &str, max_size: usize) -> Result<PgPool, StoreError> {
    let config = AsyncDieselConnectionManager::<AsyncPgConnection>::new(database_url);
    Pool::builder(config)
        .max_size(max_size)
        .build()
        .map_err(|e| StoreError::Database(format!("failed to build connection pool: {e}")))
}

/// Run all pending Diesel migrations using a synchronous connection.
pub fn run_migrations(database_url: &str) -> Result<(), StoreError> {
    let mut conn = PgConnection::establish(database_url)
        .map_err(|e| StoreError::Database(format!("failed to connect for migrations: {e}")))?;
    conn.run_pending_migrations(MIGRATIONS)
        .map_err(|e| StoreError::Migration(e.to_string()))?;
    Ok(())
}
