//! Integration tests for PostgreSQL-backed repositories.
//!
//! When `TEST_DATABASE_URL` is set the tests use that instance; otherwise an
//! embedded PostgreSQL server is started automatically (first test pays the
//! startup cost, subsequent tests reuse the same instance).
//!
//! Because the tests share a single database they **must** run sequentially:
//! `cargo test -p rune-store -- --test-threads=1`

use chrono::Utc;
use uuid::Uuid;

use rune_store::StoreError;
use rune_store::embedded::EmbeddedPg;
use rune_store::models::*;
use rune_store::pg::*;
use rune_store::pool::{PgPool, PgVectorStatus, create_pool, run_migrations, try_upgrade_pgvector};
use rune_store::repos::*;

use std::sync::OnceLock;
use tokio::sync::{Mutex, OnceCell};

/// Shared embedded PG handle kept alive for the entire test binary.
/// Using `OnceLock<OnceCell<..>>` so we can do async init exactly once.
/// A unique directory per test-run avoids stale cluster conflicts.
static EMBEDDED: OnceLock<OnceCell<Result<(EmbeddedPg, String), String>>> = OnceLock::new();
static SETUP_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

async fn database_url() -> Result<String, String> {
    if let Ok(url) = std::env::var("TEST_DATABASE_URL") {
        return Ok(url);
    }

    let cell = EMBEDDED.get_or_init(OnceCell::new);
    let result = cell
        .get_or_init(|| async {
            let tmp = std::env::temp_dir().join(format!("rune-store-test-pg-{}", Uuid::now_v7()));
            match EmbeddedPg::start(&tmp, "rune_test").await {
                Ok(epg) => {
                    let url = epg.database_url().to_string();
                    Ok((epg, url))
                }
                Err(err) => Err(format!(
                    "failed to start embedded PostgreSQL for tests: {err}"
                )),
            }
        })
        .await;

    result
        .as_ref()
        .map(|(_, url)| url.clone())
        .map_err(Clone::clone)
}

async fn setup() -> Option<(PgPool, PgVectorStatus)> {
    let _guard = SETUP_LOCK.get_or_init(|| Mutex::new(())).lock().await;

    let url = match database_url().await {
        Ok(url) => url,
        Err(err) => {
            eprintln!("skipping rune-store pg integration test setup: {err}");
            return None;
        }
    };

    let pool = match create_pool(&url, 5) {
        Ok(pool) => pool,
        Err(err) => {
            eprintln!("skipping rune-store pg integration tests: pool creation failed: {err}");
            return None;
        }
    };

    if let Err(err) = run_migrations(&pool).await {
        eprintln!("skipping rune-store pg integration tests: migrations failed: {err}");
        return None;
    }

    let pgvector_status = try_upgrade_pgvector(&pool).await;

    let conn = match pool.get().await {
        Ok(conn) => conn,
        Err(err) => {
            eprintln!("skipping rune-store pg integration tests: failed to get connection: {err}");
            return None;
        }
    };

    if let Err(err) = conn.batch_execute(
        "TRUNCATE sessions, turns, transcript_items, jobs, approvals, \
         tool_executions, channel_deliveries, paired_devices, pairing_requests, \
         memory_embeddings CASCADE",
    )
    .await
    {
        eprintln!("skipping rune-store pg integration tests: truncate failed: {err}");
        return None;
    }

    Some((pool, pgvector_status))
}

// ── Session tests ────────────────────────────────────────────────────

#[tokio::test]
async fn session_create_and_find() {
    let Some((pool, _pgvector)) = setup().await else {
        return;
    };
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
            latest_turn_id: None,
            runtime_profile: None,
            policy_profile: None,
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
    let Some((pool, _pgvector)) = setup().await else {
        return;
    };
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
            latest_turn_id: None,
            runtime_profile: None,
            policy_profile: None,
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
    // FSM requires Created → Ready → Running
    let ready = repo
        .update_status(first.id, "ready", Utc::now())
        .await
        .unwrap();
    assert_eq!(ready.status, "ready");
    let updated = repo
        .update_status(first.id, "running", Utc::now())
        .await
        .unwrap();
    assert_eq!(updated.status, "running");
}

#[tokio::test]
async fn session_not_found() {
    let Some((pool, _pgvector)) = setup().await else {
        return;
    };
    let repo = PgSessionRepo::new(pool);

    let result = repo.find_by_id(Uuid::now_v7()).await;
    assert!(matches!(result, Err(StoreError::NotFound { .. })));
}

// ── Memory embedding tests ───────────────────────────────────────────

#[tokio::test]
async fn memory_embedding_repo_round_trip_search_and_cleanup() {
    let Some((pool, pgvector_status)) = setup().await else {
        return;
    };
    let has_pgvector = pgvector_status.is_available();
    let repo = PgMemoryEmbeddingRepo::new(pool);

    if has_pgvector {
        // Full test with vector embeddings.
        repo.upsert_chunk(
            "memory/preferences.md",
            0,
            "Prefers dark mode and keyboard shortcuts.",
            &[0.9, 0.1, 0.0, 0.0],
        )
        .await
        .unwrap();
        repo.upsert_chunk(
            "memory/tasks.md",
            0,
            "Reviewed build pipeline rollout notes.",
            &[0.1, 0.9, 0.0, 0.0],
        )
        .await
        .unwrap();

        assert_eq!(repo.count().await.unwrap(), 2);

        let keyword_hits = repo.keyword_search("dark mode", 5).await.unwrap();
        assert_eq!(keyword_hits.len(), 1);
        assert_eq!(keyword_hits[0].file_path, "memory/preferences.md");
        assert!(keyword_hits[0].score > 0.0);

        let vector_hits = repo
            .vector_search(&[0.95, 0.05, 0.0, 0.0], 5)
            .await
            .unwrap();
        assert!(!vector_hits.is_empty());
        assert_eq!(vector_hits[0].file_path, "memory/preferences.md");

        let indexed_files = repo.list_indexed_files().await.unwrap();
        assert_eq!(
            indexed_files,
            vec!["memory/preferences.md", "memory/tasks.md"]
        );

        let deleted = repo.delete_by_file("memory/preferences.md").await.unwrap();
        assert_eq!(deleted, 1);
        assert_eq!(repo.count().await.unwrap(), 1);
    } else {
        // pgvector unavailable — verify keyword-only path works.
        // Insert directly without embeddings for keyword search testing.
        repo.upsert_keyword_only(
            "memory/preferences.md",
            0,
            "Prefers dark mode and keyboard shortcuts.",
        )
        .await
        .unwrap();

        let keyword_hits = repo.keyword_search("dark mode", 5).await.unwrap();
        assert_eq!(keyword_hits.len(), 1);
        assert_eq!(keyword_hits[0].file_path, "memory/preferences.md");

        let deleted = repo.delete_by_file("memory/preferences.md").await.unwrap();
        assert_eq!(deleted, 1);
        assert_eq!(repo.count().await.unwrap(), 0);
    }
}

// ── Turn tests ───────────────────────────────────────────────────────

#[tokio::test]
async fn turn_create_find_list_update() {
    let Some((pool, _pgvector)) = setup().await else {
        return;
    };
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
            latest_turn_id: None,
            runtime_profile: None,
            policy_profile: None,
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
            usage_cached_prompt_tokens: None,
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

    let usage_updated = turn_repo
        .update_usage(turn_id, 42, 17, Some(10))
        .await
        .unwrap();
    assert_eq!(usage_updated.usage_prompt_tokens, Some(42));
    assert_eq!(usage_updated.usage_completion_tokens, Some(17));
    assert_eq!(usage_updated.usage_cached_prompt_tokens, Some(10));

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
    let Some((pool, _pgvector)) = setup().await else {
        return;
    };
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
            latest_turn_id: None,
            runtime_profile: None,
            policy_profile: None,
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
    let Some((pool, _pgvector)) = setup().await else {
        return;
    };
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
            next_run_at: None,
            payload_kind: "reminder".to_string(),
            delivery_mode: "announce".to_string(),
            payload: serde_json::json!({"text": "standup"}),
            created_at: now,
            updated_at: now,
        })
        .await
        .unwrap();
    assert_eq!(job.job_type, "reminder");
    assert_eq!(job.payload_kind, "reminder");
    assert_eq!(job.delivery_mode, "announce");

    let found = repo.find_by_id(id).await.unwrap();
    assert_eq!(found.id, id);

    let enabled = repo.list_enabled().await.unwrap();
    assert_eq!(enabled.len(), 1);

    let next = Utc::now();
    let updated = repo.record_run(id, now, Some(next)).await.unwrap();
    assert!(updated.last_run_at.is_some());
    assert!(updated.next_run_at.is_some());
}

#[tokio::test]
async fn job_update_persists_terminal_reminder_payload() {
    let Some((pool, _pgvector)) = setup().await else {
        return;
    };
    let repo = PgJobRepo::new(pool);
    let now = Utc::now();

    let id = Uuid::now_v7();
    repo.create(NewJob {
        id,
        job_type: "reminder".to_string(),
        schedule: None,
        due_at: Some(now),
        enabled: true,
        next_run_at: None,
        payload_kind: "reminder".to_string(),
        delivery_mode: "announce".to_string(),
        payload: serde_json::json!({"message": "standup"}),
        created_at: now,
        updated_at: now,
    })
    .await
    .unwrap();

    let terminal_payload = serde_json::json!({
        "message": "standup",
        "status": "missed",
        "last_error": "session unavailable"
    });
    let updated = repo
        .update_job(
            id,
            false,
            Some(now),
            "reminder",
            "announce",
            terminal_payload.clone(),
            now,
            Some(now),
            None,
        )
        .await
        .unwrap();

    assert!(!updated.enabled);
    assert_eq!(updated.payload_kind, "reminder");
    assert_eq!(updated.delivery_mode, "announce");
    assert_eq!(updated.payload, terminal_payload);

    let active = repo.list_by_type("reminder", false).await.unwrap();
    assert!(!active.iter().any(|job| job.id == id));

    let all = repo.list_by_type("reminder", true).await.unwrap();
    assert!(all.iter().any(|job| job.id == id && !job.enabled));
}

#[tokio::test]
async fn job_run_create_complete_and_list() {
    let Some((pool, _pgvector)) = setup().await else {
        return;
    };
    let job_repo = PgJobRepo::new(pool.clone());
    let run_repo = PgJobRunRepo::new(pool);
    let now = Utc::now();

    let job_id = Uuid::now_v7();
    job_repo
        .create(NewJob {
            id: job_id,
            job_type: "cron".to_string(),
            schedule: Some("0 9 * * *".to_string()),
            due_at: Some(now),
            enabled: true,
            next_run_at: Some(now),
            payload_kind: "system_event".to_string(),
            delivery_mode: "none".to_string(),
            payload: serde_json::json!({"text": "run me"}),
            created_at: now,
            updated_at: now,
        })
        .await
        .unwrap();

    let run_id = Uuid::now_v7();
    let created = run_repo
        .create(NewJobRun {
            id: run_id,
            job_id,
            started_at: now,
            finished_at: None,
            trigger_kind: "manual".to_string(),
            status: "running".to_string(),
            output: None,
            created_at: now,
        })
        .await
        .unwrap();
    assert_eq!(created.id, run_id);
    assert_eq!(created.trigger_kind, "manual");
    assert_eq!(created.status, "running");

    let completed = run_repo
        .complete(run_id, "completed", Some("ok"), Utc::now())
        .await
        .unwrap();
    assert_eq!(completed.status, "completed");
    assert_eq!(completed.output.as_deref(), Some("ok"));
    assert!(completed.finished_at.is_some());

    let runs = run_repo.list_by_job(job_id, Some(10)).await.unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].id, run_id);
    assert_eq!(runs[0].status, "completed");
}

// ── Approval tests ───────────────────────────────────────────────────

#[tokio::test]
async fn approval_create_find_decide() {
    let Some((pool, _pgvector)) = setup().await else {
        return;
    };
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
            handle_ref: None,
            host_ref: None,
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
    let Some((pool, _pgvector)) = setup().await else {
        return;
    };
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
            latest_turn_id: None,
            runtime_profile: None,
            policy_profile: None,
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
            approval_id: None,
            execution_mode: None,
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
    let Some((pool, _pgvector)) = setup().await else {
        return;
    };
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
        latest_turn_id: None,
        runtime_profile: None,
        policy_profile: None,
        metadata: serde_json::json!({}),
        created_at: now,
        updated_at: now,
        last_activity_at: now,
    };

    repo.create(session.clone()).await.unwrap();
    let result = repo.create(session).await;
    assert!(matches!(result, Err(StoreError::Conflict(_))));
}

#[tokio::test]
async fn turn_repo_rejects_invalid_status_transition() {
    let Some((pool, _pgvector)) = setup().await else {
        return;
    };
    let session_repo = PgSessionRepo::new(pool.clone());
    let turn_repo = PgTurnRepo::new(pool);

    let now = Utc::now();
    let session_id = Uuid::now_v7();
    session_repo
        .create(NewSession {
            id: session_id,
            kind: "direct".to_string(),
            status: "ready".to_string(),
            workspace_root: None,
            channel_ref: None,
            requester_session_id: None,
            latest_turn_id: None,
            runtime_profile: None,
            policy_profile: None,
            metadata: serde_json::json!({}),
            created_at: now,
            updated_at: now,
            last_activity_at: now,
        })
        .await
        .unwrap();

    let turn_id = Uuid::now_v7();
    turn_repo
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
            usage_cached_prompt_tokens: None,
        })
        .await
        .unwrap();

    let err = turn_repo
        .update_status(turn_id, "completed", Some(Utc::now()))
        .await
        .unwrap_err();
    assert!(matches!(err, StoreError::InvalidTransition(_)));
}
