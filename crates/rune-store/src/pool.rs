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

/// Whether pgvector is available in the connected PostgreSQL instance.
#[derive(Debug, Clone)]
pub enum PgVectorStatus {
    Available,
    Unavailable(String),
}

impl PgVectorStatus {
    pub fn is_available(&self) -> bool {
        matches!(self, PgVectorStatus::Available)
    }
}

/// Attempt to enable the pgvector extension and add the vector column + index
/// to `memory_embeddings`. Returns [`PgVectorStatus::Available`] if all steps
/// succeed, or [`PgVectorStatus::Unavailable`] with a reason string if any
/// step fails. All SQL is idempotent (`IF NOT EXISTS` / `IF NOT EXISTS`).
pub fn try_upgrade_pgvector(database_url: &str) -> PgVectorStatus {
    let mut conn = match PgConnection::establish(database_url) {
        Ok(c) => c,
        Err(e) => return PgVectorStatus::Unavailable(format!("connection failed: {e}")),
    };

    if let Err(e) =
        diesel::sql_query("CREATE EXTENSION IF NOT EXISTS vector").execute(&mut conn)
    {
        return PgVectorStatus::Unavailable(format!("CREATE EXTENSION vector failed: {e}"));
    }

    if let Err(e) = diesel::sql_query(
        "ALTER TABLE memory_embeddings ADD COLUMN IF NOT EXISTS embedding vector(1536)",
    )
    .execute(&mut conn)
    {
        return PgVectorStatus::Unavailable(format!("ADD COLUMN embedding failed: {e}"));
    }

    if let Err(e) = diesel::sql_query(
        "CREATE INDEX IF NOT EXISTS idx_memory_embedding \
         ON memory_embeddings USING ivfflat (embedding vector_cosine_ops) WITH (lists = 100)",
    )
    .execute(&mut conn)
    {
        return PgVectorStatus::Unavailable(format!("CREATE INDEX ivfflat failed: {e}"));
    }

    PgVectorStatus::Available
}
