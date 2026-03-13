//! Embedded PostgreSQL fallback for zero-config local development.
//!
//! When no `DATABASE_URL` is configured, the system starts a managed
//! PostgreSQL instance via the `postgresql_embedded` crate. Data is
//! persisted under the configured `db_dir` (default `/data/db`) so it
//! survives daemon restarts.

use std::path::{Path, PathBuf};

use postgresql_embedded::{PostgreSQL, Settings, VersionReq};
use tracing::info;

use crate::error::StoreError;

/// A managed embedded PostgreSQL instance.
///
/// Holds the running server handle. The server is stopped when this
/// value is dropped (via the `postgresql_embedded` `Drop` impl).
pub struct EmbeddedPg {
    pg: PostgreSQL,
    database_url: String,
}

impl EmbeddedPg {
    /// Bootstrap an embedded PostgreSQL instance.
    ///
    /// * `data_dir` — durable directory for PG data (e.g. `/data/db`).
    ///   Both the installation artefacts and the cluster data live here.
    /// * `database_name` — the database to create (default: `"rune"`).
    ///
    /// On success returns the handle **and** a connection URL suitable
    /// for [`crate::pool::create_pool`] / [`crate::pool::run_migrations`].
    pub async fn start(data_dir: &Path, database_name: &str) -> Result<Self, StoreError> {
        let data_dir = PathBuf::from(data_dir);

        // Ensure directories exist.
        std::fs::create_dir_all(&data_dir).map_err(|e| {
            StoreError::Database(format!(
                "failed to create embedded PG data dir {}: {e}",
                data_dir.display()
            ))
        })?;

        let installation_dir = data_dir.join("pg_install");
        let pg_data_dir = data_dir.join("pg_data");

        let settings = Settings {
            version: VersionReq::parse("=16").unwrap_or(postgresql_embedded::LATEST.clone()),
            installation_dir,
            data_dir: pg_data_dir,
            temporary: false,
            ..Settings::default()
        };

        let port = settings.port;
        let host = settings.host.clone();
        let username = settings.username.clone();
        let password = settings.password.clone();

        info!(
            port,
            host = %host,
            data_dir = %data_dir.display(),
            "starting embedded PostgreSQL"
        );

        let mut pg = PostgreSQL::new(settings);

        pg.setup()
            .await
            .map_err(|e| StoreError::Database(format!("embedded PG setup failed: {e}")))?;

        pg.start()
            .await
            .map_err(|e| StoreError::Database(format!("embedded PG start failed: {e}")))?;

        // Create the application database if it doesn't exist yet.
        let db_exists = pg.database_exists(database_name).await.unwrap_or(false);
        if !db_exists {
            info!(database = database_name, "creating application database");
            pg.create_database(database_name).await.map_err(|e| {
                StoreError::Database(format!("failed to create database '{database_name}': {e}"))
            })?;
        }

        let database_url =
            format!("postgresql://{username}:{password}@{host}:{port}/{database_name}");

        info!(port, database = database_name, "embedded PostgreSQL ready");

        Ok(Self { pg, database_url })
    }

    /// The connection URL for the embedded instance.
    pub fn database_url(&self) -> &str {
        &self.database_url
    }

    /// Gracefully stop the embedded server.
    ///
    /// Also called automatically on drop, but explicit stop lets you
    /// handle errors.
    pub async fn stop(&self) -> Result<(), StoreError> {
        self.pg
            .stop()
            .await
            .map_err(|e| StoreError::Database(format!("embedded PG stop failed: {e}")))
    }
}
