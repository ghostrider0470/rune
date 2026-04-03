//! Storage backend factory: resolves config → repo instances.

use std::sync::Arc;

use rune_config::{AppConfig, StorageBackend, VectorBackend};
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
    pub memory_fact_repo: Arc<dyn MemoryFactRepo>,
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

    let (mut repos, mut info, embedded_pg) = match resolved {
        #[cfg(feature = "sqlite")]
        ResolvedBackend::Sqlite => {
            let (repos, info) = build_sqlite_repos(config).await?;
            (repos, info, None)
        }
        ResolvedBackend::Postgres => build_pg_repos(config).await?,
        #[cfg(feature = "cosmos")]
        ResolvedBackend::Cosmos => {
            let (repos, info) = build_cosmos_repos(config).await?;
            (repos, info, None)
        }
    };

    let vector_label = maybe_override_vector_repos(config, &mut repos).await?;
    if !vector_label.is_empty() {
        // StorageInfo.backend_name is &'static str, so we use a leaked string.
        // This is fine — it's called once at startup.
        let combined = format!("{}{}", info.backend_name, vector_label);
        info.backend_name = Box::leak(combined.into_boxed_str());
    }

    Ok((repos, info, embedded_pg))
}

/// Build all repositories from the application config (no-postgres build).
#[cfg(not(feature = "postgres"))]
pub async fn build_repos(config: &AppConfig) -> Result<(RepoSet, StorageInfo), StoreError> {
    let resolved = resolve_backend(&config.database);

    let (mut repos, mut info) = match resolved {
        #[cfg(feature = "sqlite")]
        ResolvedBackend::Sqlite => build_sqlite_repos(config).await?,
        #[cfg(feature = "cosmos")]
        ResolvedBackend::Cosmos => build_cosmos_repos(config).await?,
    };

    let vector_label = maybe_override_vector_repos(config, &mut repos).await?;
    if !vector_label.is_empty() {
        let combined = format!("{}{}", info.backend_name, vector_label);
        info.backend_name = Box::leak(combined.into_boxed_str());
    }

    Ok((repos, info))
}

// ── Backend resolution ───────────────────────────────────────────────

enum ResolvedBackend {
    #[cfg(feature = "sqlite")]
    Sqlite,
    #[cfg(feature = "postgres")]
    Postgres,
    #[cfg(feature = "cosmos")]
    Cosmos,
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
        StorageBackend::Cosmos => {
            #[cfg(feature = "cosmos")]
            return ResolvedBackend::Cosmos;
            #[cfg(not(feature = "cosmos"))]
            panic!("storage backend set to 'cosmos' but the 'cosmos' feature is not compiled in");
        }
        StorageBackend::AzureSql => {
            panic!(
                "storage backend set to 'azure_sql' but Azure SQL Database support is not implemented yet; track issue #782 and use PostgreSQL or SQLite today"
            );
        }
        StorageBackend::Auto => {
            if db.database_url.is_some() {
                #[cfg(feature = "postgres")]
                return ResolvedBackend::Postgres;
                #[cfg(not(feature = "postgres"))]
                panic!("DATABASE_URL is set but the 'postgres' feature is not compiled in");
            }
            if db.cosmos_endpoint.is_some() {
                #[cfg(feature = "cosmos")]
                return ResolvedBackend::Cosmos;
                #[cfg(not(feature = "cosmos"))]
                panic!("cosmos_endpoint is set but the 'cosmos' feature is not compiled in");
            }
            if db.azure_sql_server.is_some()
                || db.azure_sql_database.is_some()
                || db.azure_sql_user.is_some()
                || db.azure_sql_password.is_some()
                || db.azure_sql_access_token.is_some()
            {
                panic!(
                    "Azure SQL Database configuration detected but support is not implemented yet; track issue #782 and use Azure Database for PostgreSQL or SQLite today"
                );
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

// ── Vector backend resolution ────────────────────────────────────────

/// Resolve which vector backend to use.
fn resolve_vector_backend(config: &AppConfig) -> VectorBackend {
    match &config.vector.backend {
        VectorBackend::Auto => {
            if config.vector.lancedb_uri.is_some() {
                VectorBackend::LanceDb
            } else {
                VectorBackend::Integrated
            }
        }
        other => other.clone(),
    }
}

/// Optionally override vector repos based on the vector backend config.
/// Returns the vector backend label for StorageInfo.
async fn maybe_override_vector_repos(
    config: &AppConfig,
    repos: &mut RepoSet,
) -> Result<&'static str, StoreError> {
    let vector_backend = resolve_vector_backend(config);

    match vector_backend {
        #[cfg(feature = "lancedb")]
        VectorBackend::LanceDb => {
            let uri = config.vector.lancedb_uri.clone().unwrap_or_else(|| {
                config
                    .paths
                    .db_dir
                    .join("vectors")
                    .to_string_lossy()
                    .to_string()
            });
            let lance = crate::lancedb::LanceStore::new(&uri, config.vector.embedding_dims).await?;
            repos.memory_embedding_repo = Arc::new(lance.clone());
            repos.memory_fact_repo = Arc::new(lance);
            Ok(" + lancedb")
        }
        #[cfg(not(feature = "lancedb"))]
        VectorBackend::LanceDb => Err(StoreError::Database(
            "vector backend set to 'lancedb' but the 'lancedb' feature is not compiled in".into(),
        )),
        VectorBackend::None => {
            // Keep the store backend's stubs (SQLite returns empty, Cosmos has vector support).
            // No override needed — the integrated repos already handle "no results" gracefully.
            Ok(" + no-vector")
        }
        VectorBackend::Integrated | VectorBackend::Auto => {
            // Keep whatever the store backend provided.
            Ok("")
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
        process_handle_repo: Arc::new(SqliteProcessHandleRepo::new(conn.clone())),
        memory_fact_repo: Arc::new(SqliteMemoryFactRepo::new(conn)),
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
        process_handle_repo: Arc::new(PgProcessHandleRepo::new(pool.clone())),
        memory_fact_repo: Arc::new(PgMemoryFactRepo::new(pool)),
    };

    let info = StorageInfo {
        backend_name,
        pgvector_status: Some(pgvector_status),
        database_url: Some(database_url),
    };

    Ok((repos, info, embedded_pg))
}

// ── Cosmos builder ──────────────────────────────────────────────────

#[cfg(feature = "cosmos")]
async fn build_cosmos_repos(config: &AppConfig) -> Result<(RepoSet, StorageInfo), StoreError> {
    use crate::cosmos::CosmosStore;

    let endpoint = config
        .database
        .cosmos_endpoint
        .as_ref()
        .ok_or_else(|| StoreError::Database("cosmos_endpoint is required".into()))?;
    let key = config
        .database
        .cosmos_key
        .as_ref()
        .ok_or_else(|| StoreError::Database("cosmos_key is required".into()))?;

    info!(endpoint = %endpoint, "connecting to Cosmos DB NoSQL");

    let store = CosmosStore::new(endpoint, key, config.database.run_migrations).await?;

    let repos = RepoSet {
        session_repo: Arc::new(store.clone()),
        turn_repo: Arc::new(store.clone()),
        transcript_repo: Arc::new(store.clone()),
        job_repo: Arc::new(store.clone()),
        job_run_repo: Arc::new(store.clone()),
        approval_repo: Arc::new(store.clone()),
        tool_approval_repo: Arc::new(store.clone()),
        memory_embedding_repo: Arc::new(store.clone()),
        tool_execution_repo: Arc::new(store.clone()),
        device_repo: Arc::new(store.clone()),
        process_handle_repo: Arc::new(store.clone()),
        memory_fact_repo: Arc::new(store),
    };

    let info = StorageInfo {
        backend_name: "cosmos (nosql)",
        #[cfg(feature = "postgres")]
        pgvector_status: None,
        database_url: None,
    };

    Ok((repos, info))
}
