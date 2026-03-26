//! Storage backend factory: resolves config → repo instances.

use std::sync::Arc;

use rune_config::{AppConfig, StorageBackend};
use tracing::info;

use crate::error::StoreError;
use crate::repos::*;

/// All repository instances needed by the application.
pub struct RepoSet {
    pub session_repo: Arc<dyn SessionRepo>,
    pub turn_repo: Arc<dyn TurnRepo>,
    pub transcript_repo: Arc<dyn TranscriptRepo>,
    pub job_repo: Arc<dyn JobRepo>,
    pub job_run_repo: Arc<dyn JobRunRepo>,
    pub approval_repo: Arc<dyn ApprovalRepo>,
    pub tool_approval_repo: Arc<dyn ToolApprovalPolicyRepo>,
    pub memory_embedding_repo: Arc<dyn MemoryEmbeddingRepo>,
    pub tool_execution_repo: Arc<dyn ToolExecutionRepo>,
    pub device_repo: Arc<dyn DeviceRepo>,
    pub process_handle_repo: Arc<dyn ProcessHandleRepo>,
}

/// Metadata about the active storage backend.
pub struct StorageInfo {
    pub backend_name: &'static str,
    #[cfg(feature = "postgres")]
    pub pgvector_status: Option<crate::pool::PgVectorStatus>,
    pub database_url: Option<String>,
}

impl StorageInfo {
    /// Whether pgvector is available (always false for SQLite).
    pub fn pgvector_available(&self) -> bool {
        #[cfg(feature = "postgres")]
        {
            self.pgvector_status
                .as_ref()
                .is_some_and(|s| s.is_available())
        }
        #[cfg(not(feature = "postgres"))]
        {
            false
        }
    }
}

/// Build all repositories from the application config.
///
/// Returns the repo set, storage metadata, and an optional embedded PG handle
/// (which must be kept alive for the process lifetime).
#[cfg(feature = "postgres")]
pub async fn build_repos(
    config: &AppConfig,
) -> Result<(RepoSet, StorageInfo, Option<crate::embedded::EmbeddedPg>), StoreError> {
    let resolved = resolve_backend(&config.database);

    match resolved {
        #[cfg(feature = "sqlite")]
        ResolvedBackend::Sqlite => {
            let (repos, info) = build_sqlite_repos(config).await?;
            Ok((repos, info, None))
        }
        ResolvedBackend::Postgres => build_pg_repos(config).await,
    }
}

/// Build all repositories from the application config (no-postgres build).
#[cfg(not(feature = "postgres"))]
pub async fn build_repos(config: &AppConfig) -> Result<(RepoSet, StorageInfo), StoreError> {
    let resolved = resolve_backend(&config.database);

    match resolved {
        #[cfg(feature = "sqlite")]
        ResolvedBackend::Sqlite => build_sqlite_repos(config).await,
    }
}

// ── Backend resolution ───────────────────────────────────────────────

enum ResolvedBackend {
    #[cfg(feature = "sqlite")]
    Sqlite,
    #[cfg(feature = "postgres")]
    Postgres,
}

fn resolve_backend(db: &rune_config::DatabaseConfig) -> ResolvedBackend {
    match db.backend {
        StorageBackend::Postgres => {
            #[cfg(feature = "postgres")]
            return ResolvedBackend::Postgres;
            #[cfg(not(feature = "postgres"))]
            panic!(
                "storage backend set to 'postgres' but the 'postgres' feature is not compiled in"
            );
        }
        StorageBackend::Sqlite => {
            #[cfg(feature = "sqlite")]
            return ResolvedBackend::Sqlite;
            #[cfg(not(feature = "sqlite"))]
            panic!("storage backend set to 'sqlite' but the 'sqlite' feature is not compiled in");
        }
        StorageBackend::Auto => {
            if db.database_url.is_some() {
                #[cfg(feature = "postgres")]
                return ResolvedBackend::Postgres;
                #[cfg(not(feature = "postgres"))]
                panic!("DATABASE_URL is set but the 'postgres' feature is not compiled in");
            }
            #[cfg(feature = "sqlite")]
            return ResolvedBackend::Sqlite;
            #[cfg(all(not(feature = "sqlite"), feature = "postgres"))]
            return ResolvedBackend::Postgres;
            #[cfg(not(any(feature = "sqlite", feature = "postgres")))]
            panic!("no storage backend available — enable 'sqlite' or 'postgres' feature");
        }
    }
}

// ── SQLite builder ───────────────────────────────────────────────────

#[cfg(feature = "sqlite")]
async fn build_sqlite_repos(config: &AppConfig) -> Result<(RepoSet, StorageInfo), StoreError> {
    use crate::sqlite::*;

    let path = config
        .database
        .sqlite_path
        .clone()
        .unwrap_or_else(|| config.paths.db_dir.join("rune.db"));

    let path_str = path.display().to_string();
    info!(path = %path_str, "opening SQLite database");

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            StoreError::Database(format!(
                "failed to create directory for SQLite DB {}: {e}",
                path.display()
            ))
        })?;
    }

    let conn = open_connection(&path_str).await?;

    let repos = RepoSet {
        session_repo: Arc::new(SqliteSessionRepo::new(conn.clone())),
        turn_repo: Arc::new(SqliteTurnRepo::new(conn.clone())),
        transcript_repo: Arc::new(SqliteTranscriptRepo::new(conn.clone())),
        job_repo: Arc::new(SqliteJobRepo::new(conn.clone())),
        job_run_repo: Arc::new(SqliteJobRunRepo::new(conn.clone())),
        approval_repo: Arc::new(SqliteApprovalRepo::new(conn.clone())),
        tool_approval_repo: Arc::new(SqliteToolApprovalPolicyRepo::new(conn.clone())),
        memory_embedding_repo: Arc::new(SqliteMemoryEmbeddingRepo::new(conn.clone())),
        tool_execution_repo: Arc::new(SqliteToolExecutionRepo::new(conn.clone())),
        device_repo: Arc::new(SqliteDeviceRepo::new(conn.clone())),
        process_handle_repo: Arc::new(SqliteProcessHandleRepo::new(conn)),
    };

    let info = StorageInfo {
        backend_name: "sqlite",
        #[cfg(feature = "postgres")]
        pgvector_status: None,
        database_url: None,
    };

    Ok((repos, info))
}

// ── Postgres builder ─────────────────────────────────────────────────

#[cfg(feature = "postgres")]
async fn build_pg_repos(
    config: &AppConfig,
) -> Result<(RepoSet, StorageInfo, Option<crate::embedded::EmbeddedPg>), StoreError> {
    use crate::embedded::EmbeddedPg;
    use crate::pg::*;
    use crate::pool;

    let (database_url, embedded_pg) = if let Some(ref url) = config.database.database_url {
        info!("using external PostgreSQL");
        (url.clone(), None)
    } else {
        info!("no DATABASE_URL configured — starting embedded PostgreSQL");
        let epg = EmbeddedPg::start(&config.paths.db_dir, "rune")
            .await
            .map_err(|e| StoreError::EmbeddedPg(e.to_string()))?;
        let url = epg.database_url().to_owned();
        (url, Some(epg))
    };

    let pool = pool::create_pool(&database_url, config.database.max_connections as usize)?;

    if config.database.run_migrations {
        info!("running pending database migrations");
        pool::run_migrations(&pool).await?;
    }

    let pgvector_status = pool::try_upgrade_pgvector(&pool).await;
    match &pgvector_status {
        pool::PgVectorStatus::Available => info!("pgvector available — vector search enabled"),
        pool::PgVectorStatus::Unavailable(reason) => {
            tracing::warn!(reason, "pgvector unavailable — keyword search only")
        }
    }

    let backend_name = if embedded_pg.is_some() {
        "postgres (embedded)"
    } else {
        "postgres (external)"
    };

    let repos = RepoSet {
        session_repo: Arc::new(PgSessionRepo::new(pool.clone())),
        turn_repo: Arc::new(PgTurnRepo::new(pool.clone())),
        transcript_repo: Arc::new(PgTranscriptRepo::new(pool.clone())),
        job_repo: Arc::new(PgJobRepo::new(pool.clone())),
        job_run_repo: Arc::new(PgJobRunRepo::new(pool.clone())),
        approval_repo: Arc::new(PgApprovalRepo::new(pool.clone())),
        tool_approval_repo: Arc::new(PgToolApprovalPolicyRepo::new(pool.clone())),
        memory_embedding_repo: Arc::new(PgMemoryEmbeddingRepo::new(pool.clone())),
        tool_execution_repo: Arc::new(PgToolExecutionRepo::new(pool.clone())),
        device_repo: Arc::new(PgDeviceRepo::new(pool.clone())),
        process_handle_repo: Arc::new(PgProcessHandleRepo::new(pool)),
    };

    let info = StorageInfo {
        backend_name,
        pgvector_status: Some(pgvector_status),
        database_url: Some(database_url),
    };

    Ok((repos, info, embedded_pg))
}
