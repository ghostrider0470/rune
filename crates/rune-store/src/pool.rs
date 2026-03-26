//! Connection pool creation and migration runner for tokio-postgres.

use deadpool_postgres::{Config, ManagerConfig, Pool, RecyclingMethod, Runtime};
use native_tls::TlsConnector;
use postgres_native_tls::MakeTlsConnector;
use tokio_postgres::NoTls;
use tracing::{debug, info, warn};

use crate::error::StoreError;

/// Async connection pool backed by deadpool-postgres.
pub type PgPool = Pool;

/// Create an async connection pool for the given database URL.
///
/// Automatically enables TLS if the URL contains `sslmode=require`.
pub fn create_pool(database_url: &str, max_size: usize) -> Result<PgPool, StoreError> {
    let mut cfg = Config::new();
    cfg.url = Some(database_url.to_string());
    cfg.manager = Some(ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    });
    cfg.pool = Some(deadpool_postgres::PoolConfig {
        max_size,
        ..Default::default()
    });

    let needs_tls =
        database_url.contains("sslmode=require") || database_url.contains("sslmode=verify");

    if needs_tls {
        let tls_connector = TlsConnector::builder()
            .build()
            .map_err(|e| StoreError::Database(format!("TLS connector build failed: {e}")))?;
        let pg_tls = MakeTlsConnector::new(tls_connector);
        cfg.create_pool(Some(Runtime::Tokio1), pg_tls)
            .map_err(|e| StoreError::Database(format!("failed to build TLS connection pool: {e}")))
    } else {
        cfg.create_pool(Some(Runtime::Tokio1), NoTls)
            .map_err(|e| StoreError::Database(format!("failed to build connection pool: {e}")))
    }
}

/// Run all pending migrations using an active pool connection.
///
/// Migrations are embedded as SQL files from the `migrations/` directory.
/// Tracks applied migrations in a `_rune_pg_migrations` table.
pub async fn run_migrations(pool: &PgPool) -> Result<(), StoreError> {
    let client = pool
        .get()
        .await
        .map_err(|e| StoreError::Database(format!("pool error for migrations: {e}")))?;

    // One-time fresh start: drop all old Diesel-era tables if they exist.
    // This runs only if __diesel_schema_migrations exists (proving old schema).
    let diesel_exists = client
        .query_opt(
            "SELECT 1 FROM information_schema.tables WHERE table_name = '__diesel_schema_migrations'",
            &[],
        )
        .await
        .unwrap_or(None)
        .is_some();

    if diesel_exists {
        warn!("detected old Diesel schema — dropping all tables for clean tokio-postgres migration");
        client.batch_execute(
            "DROP TABLE IF EXISTS __diesel_schema_migrations CASCADE;
             DROP TABLE IF EXISTS _rune_pg_migrations CASCADE;
             DROP TABLE IF EXISTS transcript_items CASCADE;
             DROP TABLE IF EXISTS tool_executions CASCADE;
             DROP TABLE IF EXISTS process_handles CASCADE;
             DROP TABLE IF EXISTS job_runs CASCADE;
             DROP TABLE IF EXISTS approvals CASCADE;
             DROP TABLE IF EXISTS channel_deliveries CASCADE;
             DROP TABLE IF EXISTS turns CASCADE;
             DROP TABLE IF EXISTS sessions CASCADE;
             DROP TABLE IF EXISTS jobs CASCADE;
             DROP TABLE IF EXISTS paired_devices CASCADE;
             DROP TABLE IF EXISTS pairing_requests CASCADE;
             DROP TABLE IF EXISTS memory_embeddings CASCADE;"
        ).await.map_err(|e| StoreError::Migration(format!("failed to drop old tables: {e}")))?;
        info!("old tables dropped — clean slate for migration");
    }

    // Create tracking table
    client
        .batch_execute(
            "CREATE TABLE IF NOT EXISTS _rune_pg_migrations (
            name TEXT PRIMARY KEY,
            applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )",
        )
        .await
        .map_err(|e| StoreError::Migration(format!("failed to create migration tracker: {e}")))?;

    // Detect prior Diesel migrations and seed our tracker from them.
    // Diesel used __diesel_schema_migrations with a "version" column matching
    // the directory name prefix (e.g. "2024-01-01-000000").
    let diesel_table_exists = client
        .query_opt(
            "SELECT 1 FROM information_schema.tables WHERE table_name = '__diesel_schema_migrations'",
            &[],
        )
        .await
        .map_err(|e| StoreError::Migration(format!("diesel check failed: {e}")))?
        .is_some();

    if diesel_table_exists {
        let diesel_versions: Vec<String> = client
            .query("SELECT version FROM __diesel_schema_migrations", &[])
            .await
            .unwrap_or_default()
            .iter()
            .map(|row| row.get(0))
            .collect();

        if !diesel_versions.is_empty() {
            info!(count = diesel_versions.len(), "detected prior Diesel migrations, seeding tracker");
            for version in &diesel_versions {
                let _ = client
                    .execute(
                        "INSERT INTO _rune_pg_migrations (name) VALUES ($1) ON CONFLICT DO NOTHING",
                        &[version],
                    )
                    .await;
            }
        }
    }

    // Get already-applied migrations
    let applied: Vec<String> = client
        .query(
            "SELECT name FROM _rune_pg_migrations ORDER BY name",
            &[],
        )
        .await
        .map_err(|e| StoreError::Migration(format!("failed to read applied migrations: {e}")))?
        .iter()
        .map(|row| row.get(0))
        .collect();

    // Embedded migrations -- ordered list of (name, sql)
    let migrations = embedded_migrations();

    let mut ran = 0;
    for (name, sql) in &migrations {
        if applied.contains(name) {
            continue;
        }
        debug!(migration = %name, "running migration");
        client
            .batch_execute(sql)
            .await
            .map_err(|e| StoreError::Migration(format!("migration {name} failed: {e}")))?;
        client
            .execute(
                "INSERT INTO _rune_pg_migrations (name) VALUES ($1)",
                &[name],
            )
            .await
            .map_err(|e| {
                StoreError::Migration(format!("failed to record migration {name}: {e}"))
            })?;
        ran += 1;
    }

    if ran > 0 {
        info!(count = ran, "applied pending migrations");
    }

    Ok(())
}

/// Embedded migration SQL files, ordered by directory name.
fn embedded_migrations() -> Vec<(String, String)> {
    vec![
        (
            "2024-01-01-000000".into(),
            include_str!("../migrations/2024-01-01-000000_create_tables/up.sql").into(),
        ),
        (
            "2026-03-13-000001".into(),
            include_str!("../migrations/2026-03-13-000001_add_session_metadata/up.sql").into(),
        ),
        (
            "2026-03-14-000002".into(),
            include_str!("../migrations/2026-03-14-000002_add_job_runs/up.sql").into(),
        ),
        (
            "2026-03-15-000004".into(),
            include_str!("../migrations/2026-03-15-000004_add_paired_devices/up.sql").into(),
        ),
        (
            "2026-03-16-000005".into(),
            include_str!("../migrations/2026-03-16-000005_add_memory_embeddings/up.sql").into(),
        ),
        (
            "2026-03-16-000006".into(),
            include_str!("../migrations/2026-03-16-000006_unique_token_hash/up.sql").into(),
        ),
        (
            "2026-03-18-000007".into(),
            include_str!("../migrations/2026-03-18-000007_add_process_handles/up.sql").into(),
        ),
        (
            "2026-03-18-000008".into(),
            include_str!("../migrations/2026-03-18-000008_add_latest_turn_id_to_sessions/up.sql")
                .into(),
        ),
        (
            "2026-03-18-000009".into(),
            include_str!("../migrations/2026-03-18-000009_add_scheduler_semantics/up.sql").into(),
        ),
        (
            "2026-03-19-000010".into(),
            include_str!("../migrations/2026-03-19-000010_add_durable_claims/up.sql").into(),
        ),
        (
            "2026-03-21-000011".into(),
            include_str!("../migrations/2026-03-21-000011_process_handle_audit_linkage/up.sql")
                .into(),
        ),
        (
            "2026-03-23-000012".into(),
            include_str!("../migrations/2026-03-23-000012_session_profile_fields/up.sql").into(),
        ),
    ]
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

/// Attempt to enable pgvector extension and add vector column + index.
pub async fn try_upgrade_pgvector(pool: &PgPool) -> PgVectorStatus {
    let client = match pool.get().await {
        Ok(c) => c,
        Err(e) => return PgVectorStatus::Unavailable(format!("pool error: {e}")),
    };

    if let Err(e) = client
        .batch_execute("CREATE EXTENSION IF NOT EXISTS vector")
        .await
    {
        return PgVectorStatus::Unavailable(format!("CREATE EXTENSION vector failed: {e}"));
    }

    if let Err(e) = client
        .batch_execute(
            "ALTER TABLE memory_embeddings ADD COLUMN IF NOT EXISTS embedding vector(1536)",
        )
        .await
    {
        return PgVectorStatus::Unavailable(format!("ADD COLUMN embedding failed: {e}"));
    }

    if let Err(e) = client
        .batch_execute(
            "CREATE INDEX IF NOT EXISTS idx_memory_embedding \
             ON memory_embeddings USING ivfflat (embedding vector_cosine_ops) WITH (lists = 100)",
        )
        .await
    {
        warn!(error = %e, "ivfflat index not created -- brute-force cosine search will be used");
    }

    PgVectorStatus::Available
}
