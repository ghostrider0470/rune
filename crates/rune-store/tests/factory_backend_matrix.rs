#![cfg(feature = "sqlite")]

use chrono::Utc;
use rune_config::{AppConfig, StorageBackend, VectorBackend};
use rune_store::build_repos;
use rune_store::models::NewSession;
use uuid::Uuid;

#[cfg(feature = "postgres")]
type BuildReposResult = (
    rune_store::RepoSet,
    rune_store::StorageInfo,
    Option<rune_store::embedded::EmbeddedPg>,
);

#[cfg(not(feature = "postgres"))]
type BuildReposResult = (rune_store::RepoSet, rune_store::StorageInfo);

async fn build_backend_matrix_repos(config: &AppConfig) -> BuildReposResult {
    build_repos(config)
        .await
        .expect("backend matrix repo factory should build")
}

fn new_session() -> NewSession {
    let now = Utc::now();
    NewSession {
        id: Uuid::now_v7(),
        kind: "interactive".into(),
        status: "active".into(),
        workspace_root: Some("/tmp/backend-matrix".into()),
        channel_ref: None,
        requester_session_id: None,
        latest_turn_id: None,
        runtime_profile: None,
        policy_profile: None,
        metadata: serde_json::json!({"matrix": true}),
        created_at: now,
        updated_at: now,
        last_activity_at: now,
    }
}

#[cfg(feature = "postgres")]
fn assert_sqlite_result(result: &BuildReposResult) {
    assert!(
        result.2.is_none(),
        "sqlite resolution should not start embedded postgres"
    );
}

#[cfg(not(feature = "postgres"))]
fn assert_sqlite_result(_result: &BuildReposResult) {}

#[cfg(feature = "postgres")]
fn assert_postgres_result(result: &BuildReposResult) {
    let embedded = result.2.as_ref();
    assert!(
        embedded.is_some() || result.1.database_url.is_some(),
        "postgres resolution should use either embedded postgres or a configured database url"
    );
}

#[cfg(not(feature = "postgres"))]
fn assert_postgres_result(_result: &BuildReposResult) {}

async fn assert_session_round_trip(repos: &rune_store::RepoSet) {
    let session = new_session();
    let created = repos
        .session_repo
        .create(session.clone())
        .await
        .expect("session should be persisted through factory repo set");
    let found = repos
        .session_repo
        .find_by_id(session.id)
        .await
        .expect("session should round-trip through factory repo set");

    assert_eq!(created.id, session.id);
    assert_eq!(found.id, session.id);
}

#[tokio::test]
async fn sqlite_integrated_matrix_row_builds_and_round_trips() {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let sqlite_path = temp.path().join("matrix-sqlite-integrated.db");

    let mut config = AppConfig::default();
    config.database.backend = StorageBackend::Sqlite;
    config.database.sqlite_path = Some(sqlite_path.clone());
    config.database.database_url = None;
    config.vector.backend = VectorBackend::Integrated;
    config.vector.lancedb_uri = None;
    config.paths.db_dir = temp.path().join("db-dir");

    let result = build_backend_matrix_repos(&config).await;
    let repos = &result.0;
    let info = &result.1;

    assert_eq!(info.backend_name, "sqlite");
    assert_eq!(info.database_url, None);
    assert!(!info.pgvector_available());
    assert!(
        sqlite_path.exists(),
        "sqlite database file should be created"
    );
    assert_sqlite_result(&result);
    assert_session_round_trip(repos).await;
}

#[tokio::test]
async fn sqlite_lancedb_matrix_row_builds_and_round_trips() {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let sqlite_path = temp.path().join("matrix-sqlite-lancedb.db");
    let lancedb_path = temp.path().join("vectors");

    let mut config = AppConfig::default();
    config.database.backend = StorageBackend::Sqlite;
    config.database.sqlite_path = Some(sqlite_path);
    config.database.database_url = None;
    config.vector.backend = VectorBackend::LanceDb;
    config.vector.lancedb_uri = Some(lancedb_path.to_string_lossy().to_string());
    config.paths.db_dir = temp.path().join("db-dir");

    let result = build_backend_matrix_repos(&config).await;
    let repos = &result.0;
    let info = &result.1;

    assert_eq!(info.backend_name, "sqlite + lancedb");
    assert_eq!(info.database_url, None);
    assert!(!info.pgvector_available());
    assert_sqlite_result(&result);
    assert_session_round_trip(repos).await;
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn postgres_integrated_matrix_row_builds_and_round_trips() {
    let temp = tempfile::tempdir().expect("temp dir should be created");

    let mut config = AppConfig::default();
    config.database.backend = StorageBackend::Postgres;
    config.database.database_url = std::env::var("TEST_DATABASE_URL").ok();
    config.database.sqlite_path = None;
    config.vector.backend = VectorBackend::Integrated;
    config.vector.lancedb_uri = None;
    config.paths.db_dir = temp.path().join("db-dir");

    let result = build_backend_matrix_repos(&config).await;
    let repos = &result.0;
    let info = &result.1;

    assert!(
        info.backend_name.starts_with("postgres"),
        "expected postgres backend label, got {}",
        info.backend_name
    );
    assert_postgres_result(&result);
    assert!(
        info.database_url.is_some(),
        "postgres storage info should report an active database url"
    );
    assert_session_round_trip(repos).await;
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn postgres_auto_matrix_row_prefers_database_url_over_sqlite_path() {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let sqlite_path = temp.path().join("ignored-auto.db");

    let mut config = AppConfig::default();
    config.database.backend = StorageBackend::Auto;
    config.database.database_url = Some("postgres://user:pass@localhost:5432/rune".into());
    config.database.sqlite_path = Some(sqlite_path);
    config.vector.backend = VectorBackend::Integrated;
    config.vector.lancedb_uri = None;
    config.paths.db_dir = temp.path().join("db-dir");

    let result = build_backend_matrix_repos(&config).await;
    let info = &result.1;

    assert!(
        info.backend_name.starts_with("postgres"),
        "auto backend should resolve database_url to postgres, got {}",
        info.backend_name
    );
    assert_eq!(
        info.database_url.as_deref(),
        Some("postgres://user:pass@localhost:5432/rune")
    );
    assert_postgres_result(&result);
}
