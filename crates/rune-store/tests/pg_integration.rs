//! Integration tests for PostgreSQL-backed repositories.
//!
//! When `TEST_DATABASE_URL` is set the tests use that instance; otherwise an
//! embedded PostgreSQL server is started automatically (first test pays the
//! startup cost, subsequent tests reuse the same instance).
//!
//! Because the tests share a single database they **must** run sequentially:
//! `cargo test -p rune-store -- --test-threads=1`

use chrono::Utc;
use diesel_async::RunQueryDsl;
use uuid::Uuid;

use rune_store::StoreError;
use rune_store::embedded::EmbeddedPg;
use rune_store::models::*;
use rune_store::pg::*;
use rune_store::pool::{PgPool, create_pool, run_migrations};
use rune_store::repos::*;

use std::sync::OnceLock;
use tokio::sync::OnceCell;

/// Shared embedded PG handle kept alive for the entire test binary.
/// Using `OnceLock<OnceCell<..>>` so we can do async init exactly once.
/// A unique directory per test-run avoids stale cluster conflicts.
static EMBEDDED: OnceLock<OnceCell<(EmbeddedPg, String)>> = OnceLock::new();

async fn database_url() -> String {
    if let Ok(url) = std::env::var("TEST_DATABASE_URL") {
        return url;
    }

    let cell = EMBEDDED.get_or_init(OnceCell::new);
    let (_, url) = cell
        .get_or_init(|| async {
            let tmp = std::env::temp_dir()
                .join(format!("rune-store-test-pg-{}", Uuid::now_v7()));
            let epg = EmbeddedPg::start(&tmp, "rune_test")
                .await
                .expect("failed to start embedded PostgreSQL");
            let url = epg.database_url().to_string();
            (epg, url)
        })
        .await;

    url.clone()
}

async fn setup() -> PgPool {
    let url = database_url().await;
    run_migrations(&url).expect("migrations failed");
    let pool = create_pool(&url, 5).expect("pool creation failed");

    let mut conn = pool.get().await.expect("failed to get connection");
    diesel::sql_query(
        "TRUNCATE sessions, turns, transcript_items, jobs, approvals, \
         tool_executions, channel_deliveries CASCADE",
    )
    .execute(&mut conn)
    .await
    .expect("truncate failed");

    pool
}

// ── Session tests ────────────────────────────────────────────────────

#[tokio::test]
async fn session_create_and_find() {
    let pool = setup().await;
    let repo = PgSessionRepo::new(pool);

    let now = Utc::now();
    let id = Uuid::now_v7();
    let created = repo
        .create(NewSession {
            id,
            kind: "direct".to_string(),
            status: "created".to_string(),
            workspace_root: Some("/tmp/test".to_string()),
            channel_ref: None,
            requester_session_id: None,
            metadata: serde_json::json!({}),
            created_at: now,
            updated_at: now,
            last_activity_at: now,
        })
        .await
        .unwrap();

    assert_eq!(created.id, id);
    assert_eq!(created.kind, "direct");
    assert_eq!(created.status, "created");

    let found = repo.find_by_id(id).await.unwrap();
    assert_eq!(found.id, id);
    assert_eq!(found.workspace_root, Some("/tmp/test".to_string()));
}

#[tokio::test]
async fn session_list_and_update_status() {
    let pool = setup().await;
    let repo = PgSessionRepo::new(pool);
    let now = Utc::now();

    for i in 0..3 {
        repo.create(NewSession {
            id: Uuid::now_v7(),
            kind: "direct".to_string(),
            status: "created".to_string(),
            workspace_root: Some(format!("/tmp/test{i}")),
            channel_ref: None,
            requester_session_id: None,
            metadata: serde_json::json!({}),
            created_at: now,
            updated_at: now,
            last_activity_at: now,
        })
        .await
        .unwrap();
    }

    let all = repo.list(10, 0).await.unwrap();
    assert_eq!(all.len(), 3);

    let page = repo.list(2, 0).await.unwrap();
    assert_eq!(page.len(), 2);

    let first = &all[0];
    let updated = repo
        .update_status(first.id, "running", Utc::now())
        .await
        .unwrap();
    assert_eq!(updated.status, "running");
}

#[tokio::test]
async fn session_not_found() {
    let pool = setup().await;
    let repo = PgSessionRepo::new(pool);

    let result = repo.find_by_id(Uuid::now_v7()).await;
    assert!(matches!(result, Err(StoreError::NotFound { .. })));
}

// ── Turn tests ───────────────────────────────────────────────────────

#[tokio::test]
async fn turn_create_find_list_update() {
    let pool = setup().await;
    let session_repo = PgSessionRepo::new(pool.clone());
    let turn_repo = PgTurnRepo::new(pool);

    let now = Utc::now();
    let session_id = Uuid::now_v7();
    session_repo
        .create(NewSession {
            id: session_id,
            kind: "direct".to_string(),
            status: "running".to_string(),
            workspace_root: None,
            channel_ref: None,
            requester_session_id: None,
            metadata: serde_json::json!({}),
            created_at: now,
            updated_at: now,
            last_activity_at: now,
        })
        .await
        .unwrap();

    let turn_id = Uuid::now_v7();
    let turn = turn_repo
        .create(NewTurn {
            id: turn_id,
            session_id,
            trigger_kind: "user_message".to_string(),
            status: "started".to_string(),
            model_ref: Some("gpt-4".to_string()),
            started_at: now,
            ended_at: None,
            usage_prompt_tokens: None,
            usage_completion_tokens: None,
        })
        .await
        .unwrap();
    assert_eq!(turn.id, turn_id);
    assert_eq!(turn.status, "started");

    let found = turn_repo.find_by_id(turn_id).await.unwrap();
    assert_eq!(found.session_id, session_id);

    let by_session = turn_repo.list_by_session(session_id).await.unwrap();
    assert_eq!(by_session.len(), 1);

    // Update without ended_at
    let updated = turn_repo
        .update_status(turn_id, "model_calling", None)
        .await
        .unwrap();
    assert_eq!(updated.status, "model_calling");
    assert!(updated.ended_at.is_none());

    // Complete with ended_at
    let completed = turn_repo
        .update_status(turn_id, "completed", Some(Utc::now()))
        .await
        .unwrap();
    assert_eq!(completed.status, "completed");
    assert!(completed.ended_at.is_some());
}

// ── Transcript tests ─────────────────────────────────────────────────

#[tokio::test]
async fn transcript_append_and_list_ordered() {
    let pool = setup().await;
    let session_repo = PgSessionRepo::new(pool.clone());
    let repo = PgTranscriptRepo::new(pool);

    let now = Utc::now();
    let session_id = Uuid::now_v7();
    session_repo
        .create(NewSession {
            id: session_id,
            kind: "direct".to_string(),
            status: "running".to_string(),
            workspace_root: None,
            channel_ref: None,
            requester_session_id: None,
            metadata: serde_json::json!({}),
            created_at: now,
            updated_at: now,
            last_activity_at: now,
        })
        .await
        .unwrap();

    // Insert out of order to verify ordering
    repo.append(NewTranscriptItem {
        id: Uuid::now_v7(),
        session_id,
        turn_id: None,
        seq: 2,
        kind: "assistant_message".to_string(),
        payload: serde_json::json!({"content": "hello"}),
        created_at: now,
    })
    .await
    .unwrap();

    repo.append(NewTranscriptItem {
        id: Uuid::now_v7(),
        session_id,
        turn_id: None,
        seq: 1,
        kind: "user_message".to_string(),
        payload: serde_json::json!({"message": {"content": "hi"}}),
        created_at: now,
    })
    .await
    .unwrap();

    let items = repo.list_by_session(session_id).await.unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].seq, 1);
    assert_eq!(items[0].kind, "user_message");
    assert_eq!(items[1].seq, 2);
    assert_eq!(items[1].kind, "assistant_message");
}

// ── Job tests ────────────────────────────────────────────────────────

#[tokio::test]
async fn job_create_find_list_record_run() {
    let pool = setup().await;
    let repo = PgJobRepo::new(pool);
    let now = Utc::now();

    let id = Uuid::now_v7();
    let job = repo
        .create(NewJob {
            id,
            job_type: "reminder".to_string(),
            schedule: Some("0 9 * * *".to_string()),
            due_at: None,
            enabled: true,
            payload: serde_json::json!({"text": "standup"}),
            created_at: now,
            updated_at: now,
        })
        .await
        .unwrap();
    assert_eq!(job.job_type, "reminder");

    let found = repo.find_by_id(id).await.unwrap();
    assert_eq!(found.id, id);

    let enabled = repo.list_enabled().await.unwrap();
    assert_eq!(enabled.len(), 1);

    let next = Utc::now();
    let updated = repo.record_run(id, now, Some(next)).await.unwrap();
    assert!(updated.last_run_at.is_some());
    assert!(updated.next_run_at.is_some());
}

// ── Approval tests ───────────────────────────────────────────────────

#[tokio::test]
async fn approval_create_find_decide() {
    let pool = setup().await;
    let repo = PgApprovalRepo::new(pool);
    let now = Utc::now();

    let id = Uuid::now_v7();
    let approval = repo
        .create(NewApproval {
            id,
            subject_type: "tool_call".to_string(),
            subject_id: Uuid::now_v7(),
            reason: "destructive operation".to_string(),
            presented_payload: serde_json::json!({"command": "rm -rf /"}),
            created_at: now,
        })
        .await
        .unwrap();
    assert!(approval.decision.is_none());

    let found = repo.find_by_id(id).await.unwrap();
    assert_eq!(found.reason, "destructive operation");

    let decided = repo
        .decide(id, "deny", "operator", Utc::now())
        .await
        .unwrap();
    assert_eq!(decided.decision, Some("deny".to_string()));
    assert_eq!(decided.decided_by, Some("operator".to_string()));
    assert!(decided.decided_at.is_some());
}

// ── Tool execution tests ─────────────────────────────────────────────

#[tokio::test]
async fn tool_execution_create_find_complete() {
    let pool = setup().await;
    let session_repo = PgSessionRepo::new(pool.clone());
    let repo = PgToolExecutionRepo::new(pool);
    let now = Utc::now();

    let session_id = Uuid::now_v7();
    session_repo
        .create(NewSession {
            id: session_id,
            kind: "direct".to_string(),
            status: "running".to_string(),
            workspace_root: None,
            channel_ref: None,
            requester_session_id: None,
            metadata: serde_json::json!({}),
            created_at: now,
            updated_at: now,
            last_activity_at: now,
        })
        .await
        .unwrap();

    let id = Uuid::now_v7();
    let exec = repo
        .create(NewToolExecution {
            id,
            tool_call_id: Uuid::now_v7(),
            session_id,
            turn_id: Uuid::now_v7(),
            tool_name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "/tmp/test.txt"}),
            status: "running".to_string(),
            started_at: now,
        })
        .await
        .unwrap();
    assert_eq!(exec.tool_name, "read_file");

    let found = repo.find_by_id(id).await.unwrap();
    assert_eq!(found.status, "running");

    let completed = repo
        .complete(id, "completed", Some("file contents"), None, Utc::now())
        .await
        .unwrap();
    assert_eq!(completed.status, "completed");
    assert_eq!(completed.result_summary, Some("file contents".to_string()));
    assert!(completed.ended_at.is_some());
}

// ── Duplicate ID conflict ────────────────────────────────────────────

#[tokio::test]
async fn duplicate_session_returns_conflict() {
    let pool = setup().await;
    let repo = PgSessionRepo::new(pool);
    let now = Utc::now();
    let id = Uuid::now_v7();

    let session = NewSession {
        id,
        kind: "direct".to_string(),
        status: "created".to_string(),
        workspace_root: None,
        channel_ref: None,
        requester_session_id: None,
        metadata: serde_json::json!({}),
        created_at: now,
        updated_at: now,
        last_activity_at: now,
    };

    repo.create(session.clone()).await.unwrap();
    let result = repo.create(session).await;
    assert!(matches!(result, Err(StoreError::Conflict(_))));
}
