#![cfg(feature = "sqlite")]

use chrono::Utc;
use rune_config::{AppConfig, StorageBackend};
use rune_store::build_repos;
use rune_store::models::NewSession;
use uuid::Uuid;

async fn build_sqlite_repos(config: &AppConfig) -> (rune_store::RepoSet, rune_store::StorageInfo) {
    #[cfg(feature = "postgres")]
    {
        let (repos, info, embedded_pg) = build_repos(config)
            .await
            .expect("sqlite repo factory should build");
        assert!(
            embedded_pg.is_none(),
            "sqlite resolution should not start embedded postgres"
        );
        (repos, info)
    }

    #[cfg(not(feature = "postgres"))]
    {
        build_repos(config)
            .await
            .expect("sqlite repo factory should build")
    }
}

fn new_session() -> NewSession {
    let now = Utc::now();
    NewSession {
        id: Uuid::now_v7(),
        kind: "interactive".into(),
        status: "active".into(),
        workspace_root: Some("/tmp/factory".into()),
        channel_ref: None,
        requester_session_id: None,
        latest_turn_id: None,
        metadata: serde_json::json!({"factory": true}),
        created_at: now,
        updated_at: now,
        last_activity_at: now,
    }
}

#[tokio::test]
async fn auto_backend_without_database_url_resolves_to_sqlite_factory_path() {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let sqlite_path = temp.path().join("nested").join("state").join("rune-auto.db");

    let mut config = AppConfig::default();
    config.database.backend = StorageBackend::Auto;
    config.database.database_url = None;
    config.database.sqlite_path = Some(sqlite_path.clone());
    config.paths.db_dir = temp.path().join("unused-db-dir");

    let (repos, info) = build_sqlite_repos(&config).await;

    assert_eq!(info.backend_name, "sqlite");
    assert_eq!(info.database_url, None);
    assert!(!info.pgvector_available());
    assert!(sqlite_path.exists(), "sqlite database file should be created");

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
        .expect("session should round-trip through sqlite repo set");

    assert_eq!(created.id, session.id);
    assert_eq!(found.id, session.id);
}

#[tokio::test]
async fn explicit_sqlite_backend_uses_default_db_dir_path_when_sqlite_path_is_unset() {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let db_dir = temp.path().join("db-root");

    let mut config = AppConfig::default();
    config.database.backend = StorageBackend::Sqlite;
    config.database.database_url = Some("postgres://ignored.example/rune".into());
    config.database.sqlite_path = None;
    config.paths.db_dir = db_dir.clone();

    let (_repos, info) = build_sqlite_repos(&config).await;

    assert_eq!(info.backend_name, "sqlite");
    assert_eq!(info.database_url, None);
    assert!(db_dir.join("rune.db").exists());
}
