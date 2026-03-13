//! Database pool setup, migration execution, and embedded PostgreSQL fallback.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use diesel::Connection;
use diesel_async::async_connection_wrapper::AsyncConnectionWrapper;
use diesel_migrations::MigrationHarness;
use postgresql_embedded::{PostgreSQL, Settings, VersionReq};
use rune_config::AppConfig;
use tracing::info;

use crate::error::StoreError;
use crate::repos::{PgPool, PgStore, build_pool};

const MIGRATIONS: diesel_migrations::EmbeddedMigrations =
    diesel_migrations::embed_migrations!("./migrations");
const EMBEDDED_DB_NAME: &str = "rune";

/// Stateful holder for an embedded PostgreSQL instance.
pub struct EmbeddedPostgres {
    _server: PostgreSQL,
    database_url: String,
}

impl EmbeddedPostgres {
    pub async fn start(base_dir: &Path) -> Result<Self, StoreError> {
        fs::create_dir_all(base_dir).map_err(|error| StoreError::Migration(error.to_string()))?;

        let installation_dir = base_dir.join("embedded-installation");
        let data_dir = base_dir.join("embedded-data");
        let password_file = base_dir.join("embedded.pgpass");

        let settings = Settings {
            version: VersionReq::parse("=16")?,
            installation_dir,
            password_file,
            data_dir,
            temporary: false,
            ..Settings::default()
        };

        let mut server = PostgreSQL::new(settings);
        server.setup().await?;
        server.start().await?;
        if !server.database_exists(EMBEDDED_DB_NAME).await? {
            server.create_database(EMBEDDED_DB_NAME).await?;
        }
        let database_url = server.settings().url(EMBEDDED_DB_NAME);

        Ok(Self {
            _server: server,
            database_url,
        })
    }

    #[must_use]
    pub fn database_url(&self) -> &str {
        &self.database_url
    }
}

/// Database resources assembled from configuration.
pub struct StoreRuntime {
    pub store: Arc<PgStore>,
    pub pool: PgPool,
    pub database_url: String,
    pub embedded: Option<EmbeddedPostgres>,
}

/// Build the store runtime from application config.
pub async fn connect(config: &AppConfig) -> Result<StoreRuntime, StoreError> {
    let (database_url, embedded) = match &config.database.database_url {
        Some(database_url) => (database_url.clone(), None),
        None => {
            let embedded =
                EmbeddedPostgres::start(&embedded_base_dir(&config.paths.db_dir)).await?;
            (embedded.database_url().to_string(), Some(embedded))
        }
    };

    let pool = build_pool(&database_url, config.database.max_connections)?;

    if config.database.run_migrations {
        let migration_url = database_url.clone();
        tokio::task::spawn_blocking(move || run_migrations(&migration_url))
            .await
            .map_err(|error| StoreError::Migration(error.to_string()))??;
    }

    info!(database_url = %redact_database_url(&database_url), "database ready");

    let store = Arc::new(PgStore::new(pool.clone()));
    Ok(StoreRuntime {
        store,
        pool,
        database_url,
        embedded,
    })
}

fn embedded_base_dir(db_dir: &Path) -> PathBuf {
    db_dir.join("embedded")
}

/// Run embedded Diesel migrations against the provided database URL.
pub fn run_migrations(database_url: &str) -> Result<(), StoreError> {
    let mut connection =
        AsyncConnectionWrapper::<diesel_async::AsyncPgConnection>::establish(database_url)?;
    connection
        .run_pending_migrations(MIGRATIONS)
        .map(|_| ())
        .map_err(|error: Box<dyn std::error::Error + Send + Sync>| {
            StoreError::Migration(error.to_string())
        })
}

fn redact_database_url(database_url: &str) -> String {
    match database_url.split_once('@') {
        Some((prefix, suffix)) => match prefix.split_once("://") {
            Some((scheme, _)) => format!("{scheme}://***@{suffix}"),
            None => "postgres://***".to_string(),
        },
        None => database_url.to_string(),
    }
}
