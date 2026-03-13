use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use rune_config::AppConfig;
use rune_store::database::connect;
use rune_store::models::{NewJob, NewSession, NewTranscriptItem, NewTurn};
use rune_store::repos::{JobRepo, SessionRepo, TranscriptRepo, TurnRepo};
use uuid::Uuid;

fn test_config(name: &str) -> AppConfig {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let root = std::env::temp_dir().join(format!("rune-store-{name}-{nanos}"));

    AppConfig {
        database: rune_config::DatabaseConfig {
            database_url: None,
            max_connections: 4,
            run_migrations: true,
        },
        paths: rune_config::PathsConfig {
            db_dir: root.join("db"),
            sessions_dir: root.join("sessions"),
            memory_dir: root.join("memory"),
            media_dir: root.join("media"),
            skills_dir: root.join("skills"),
            logs_dir: root.join("logs"),
            backups_dir: root.join("backups"),
            config_dir: root.join("config"),
            secrets_dir: root.join("secrets"),
        },
        ..AppConfig::default()
    }
}

#[tokio::test]
async fn embedded_bootstrap_and_session_crud() {
    let config = test_config("session-crud");
    let runtime = connect(&config).await.expect("connect runtime");
    let store = runtime.store;

    let now = Utc::now();
    let session = SessionRepo::create(
        &*store,
        NewSession {
            id: Uuid::now_v7(),
            kind: "direct".to_string(),
            status: "created".to_string(),
            workspace_root: Some("/tmp/workspace".to_string()),
            channel_ref: None,
            requester_session_id: None,
            created_at: now,
            updated_at: now,
            last_activity_at: now,
        },
    )
    .await
    .expect("create session");

    let fetched = SessionRepo::find_by_id(&*store, session.id)
        .await
        .expect("find session");
    assert_eq!(fetched.id, session.id);

    let listed = SessionRepo::list(&*store, 10, 0)
        .await
        .expect("list sessions");
    assert!(!listed.is_empty());

    let updated = SessionRepo::update_status(&*store, session.id, "ready", Utc::now())
        .await
        .expect("update session");
    assert_eq!(updated.status, "ready");
}

#[tokio::test]
async fn turn_transcript_and_job_crud() {
    let config = test_config("turn-transcript-job");
    let runtime = connect(&config).await.expect("connect runtime");
    let store = runtime.store;

    let now = Utc::now();
    let session = SessionRepo::create(
        &*store,
        NewSession {
            id: Uuid::now_v7(),
            kind: "direct".to_string(),
            status: "running".to_string(),
            workspace_root: None,
            channel_ref: None,
            requester_session_id: None,
            created_at: now,
            updated_at: now,
            last_activity_at: now,
        },
    )
    .await
    .expect("create session");

    let turn = TurnRepo::create(
        &*store,
        NewTurn {
            id: Uuid::now_v7(),
            session_id: session.id,
            trigger_kind: "user_message".to_string(),
            status: "started".to_string(),
            model_ref: Some("fake-model".to_string()),
            started_at: now,
            ended_at: None,
            usage_prompt_tokens: None,
            usage_completion_tokens: None,
        },
    )
    .await
    .expect("create turn");

    let listed_turns = TurnRepo::list_by_session(&*store, session.id)
        .await
        .expect("list turns by session");
    assert_eq!(listed_turns.len(), 1);
    assert_eq!(listed_turns[0].id, turn.id);

    let transcript_item = TranscriptRepo::append(
        &*store,
        NewTranscriptItem {
            id: Uuid::now_v7(),
            session_id: session.id,
            turn_id: Some(turn.id),
            seq: 0,
            kind: "user_message".to_string(),
            payload: serde_json::json!({"kind": "user_message", "message": {"channel_id": null, "sender_id": "user", "sender_display_name": null, "message_id": null, "reply_to_message_id": null, "content": "hello", "attachments": [], "metadata": null}}),
            created_at: now,
        },
    )
    .await
    .expect("append transcript item");
    assert_eq!(transcript_item.seq, 0);

    let transcript = TranscriptRepo::list_by_session(&*store, session.id)
        .await
        .expect("list transcript");
    assert_eq!(transcript.len(), 1);

    let job = JobRepo::create(
        &*store,
        NewJob {
            id: Uuid::now_v7(),
            job_type: "cron".to_string(),
            schedule: Some("0 * * * *".to_string()),
            due_at: None,
            enabled: true,
            payload: serde_json::json!({"kind": "heartbeat"}),
            created_at: now,
            updated_at: now,
        },
    )
    .await
    .expect("create job");

    let enabled_jobs = JobRepo::list_enabled(&*store)
        .await
        .expect("list enabled jobs");
    assert_eq!(enabled_jobs.len(), 1);
    assert_eq!(enabled_jobs[0].id, job.id);

    let recorded = JobRepo::record_run(&*store, job.id, now, Some(now))
        .await
        .expect("record job run");
    assert!(recorded.last_run_at.is_some());
    assert_eq!(
        recorded.last_run_at.map(|value| value.timestamp()),
        Some(now.timestamp())
    );
}
