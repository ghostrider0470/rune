//! Contract tests: generic test functions exercised against every storage backend.
//!
//! The `sqlite_contract` module uses an in-memory SQLite DB (fast, no deps).
//! A `pg_contract` module can be added using embedded PG for parity testing.

use chrono::{DateTime, Utc};
use serde_json::json;
use uuid::Uuid;

use rune_store::error::StoreError;
use rune_store::models::*;
use rune_store::repos::*;

// ── Helper factories ─────────────────────────────────────────────────

fn now() -> DateTime<Utc> {
    Utc::now()
}

fn new_session() -> NewSession {
    NewSession {
        id: Uuid::now_v7(),
        kind: "interactive".into(),
        status: "active".into(),
        workspace_root: Some("/tmp/test".into()),
        channel_ref: None,
        requester_session_id: None,
        latest_turn_id: None,
        metadata: json!({"key": "value"}),
        created_at: now(),
        updated_at: now(),
        last_activity_at: now(),
    }
}

fn new_turn(session_id: Uuid) -> NewTurn {
    NewTurn {
        id: Uuid::now_v7(),
        session_id,
        trigger_kind: "user_message".into(),
        status: "running".into(),
        model_ref: Some("test/model".into()),
        started_at: now(),
        ended_at: None,
        usage_prompt_tokens: None,
        usage_completion_tokens: None,
    }
}

fn new_transcript_item(session_id: Uuid, seq: i32) -> NewTranscriptItem {
    NewTranscriptItem {
        id: Uuid::now_v7(),
        session_id,
        turn_id: None,
        seq,
        kind: "user_message".into(),
        payload: json!({"text": "hello"}),
        created_at: now(),
    }
}

fn new_job() -> NewJob {
    NewJob {
        id: Uuid::now_v7(),
        job_type: "reminder".into(),
        schedule: Some("0 * * * *".into()),
        due_at: None,
        enabled: true,
        payload_kind: "reminder".into(),
        delivery_mode: "announce".into(),
        payload: json!({"msg": "test"}),
        created_at: now(),
        updated_at: now(),
    }
}

fn new_job_run(job_id: Uuid) -> NewJobRun {
    NewJobRun {
        id: Uuid::now_v7(),
        job_id,
        started_at: now(),
        finished_at: None,
        trigger_kind: "due".into(),
        status: "running".into(),
        output: None,
        created_at: now(),
    }
}

fn new_approval() -> NewApproval {
    NewApproval {
        id: Uuid::now_v7(),
        subject_type: "tool_call".into(),
        subject_id: Uuid::now_v7(),
        reason: "dangerous tool".into(),
        presented_payload: json!({"tool": "rm"}),
        created_at: now(),
        handle_ref: None,
        host_ref: None,
    }
}

fn new_tool_execution(session_id: Uuid, turn_id: Uuid) -> NewToolExecution {
    NewToolExecution {
        id: Uuid::now_v7(),
        tool_call_id: Uuid::now_v7(),
        session_id,
        turn_id,
        tool_name: "file_read".into(),
        arguments: json!({"path": "/tmp"}),
        status: "running".into(),
        started_at: now(),
        approval_id: None,
        execution_mode: None,
    }
}

fn new_paired_device() -> NewPairedDevice {
    NewPairedDevice {
        id: Uuid::now_v7(),
        name: "test-device".into(),
        public_key: format!("pk_{}", Uuid::now_v7()),
        role: "operator".into(),
        scopes: json!(["read", "write"]),
        token_hash: format!("hash_{}", Uuid::now_v7()),
        token_expires_at: now() + chrono::Duration::hours(24),
        paired_at: now(),
        created_at: now(),
    }
}

fn new_pairing_request() -> NewPairingRequest {
    NewPairingRequest {
        id: Uuid::now_v7(),
        device_name: "pending-device".into(),
        public_key: format!("pk_{}", Uuid::now_v7()),
        challenge: "challenge123".into(),
        created_at: now(),
        expires_at: now() + chrono::Duration::hours(1),
    }
}

// ── Generic contract tests ───────────────────────────────────────────

async fn test_session_crud(repo: &dyn SessionRepo) {
    let s = new_session();
    let id = s.id;

    // Create
    let created = repo.create(s).await.unwrap();
    assert_eq!(created.id, id);
    assert_eq!(created.kind, "interactive");

    // Find
    let found = repo.find_by_id(id).await.unwrap();
    assert_eq!(found.id, id);

    // List
    let list = repo.list(10, 0).await.unwrap();
    assert!(!list.is_empty());

    // Update status
    let updated = repo.update_status(id, "completed", now()).await.unwrap();
    assert_eq!(updated.status, "completed");

    // Update metadata
    let updated = repo
        .update_metadata(id, json!({"new": true}), now())
        .await
        .unwrap();
    assert_eq!(updated.metadata, json!({"new": true}));

    // Delete
    assert!(repo.delete(id).await.unwrap());
    assert!(!repo.delete(id).await.unwrap());

    // Not found
    match repo.find_by_id(id).await {
        Err(StoreError::NotFound { .. }) => {}
        other => panic!("expected NotFound, got: {other:?}"),
    }
}

async fn test_session_channel_ref(repo: &dyn SessionRepo) {
    let mut s = new_session();
    s.channel_ref = Some("test-channel".into());
    let id = s.id;
    repo.create(s).await.unwrap();

    let found = repo.find_by_channel_ref("test-channel").await.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().id, id);

    let not_found = repo.find_by_channel_ref("nonexistent").await.unwrap();
    assert!(not_found.is_none());
}

async fn test_turn_crud(session_repo: &dyn SessionRepo, turn_repo: &dyn TurnRepo) {
    let s = new_session();
    let sid = s.id;
    session_repo.create(s).await.unwrap();

    let t = new_turn(sid);
    let tid = t.id;
    let created = turn_repo.create(t).await.unwrap();
    assert_eq!(created.id, tid);

    let found = turn_repo.find_by_id(tid).await.unwrap();
    assert_eq!(found.session_id, sid);

    let list = turn_repo.list_by_session(sid).await.unwrap();
    assert_eq!(list.len(), 1);

    let updated = turn_repo
        .update_status(tid, "completed", Some(now()))
        .await
        .unwrap();
    assert_eq!(updated.status, "completed");
    assert!(updated.ended_at.is_some());

    let updated = turn_repo.update_usage(tid, 100, 50).await.unwrap();
    assert_eq!(updated.usage_prompt_tokens, Some(100));
    assert_eq!(updated.usage_completion_tokens, Some(50));
}

async fn test_transcript_crud(
    session_repo: &dyn SessionRepo,
    transcript_repo: &dyn TranscriptRepo,
) {
    let s = new_session();
    let sid = s.id;
    session_repo.create(s).await.unwrap();

    let item = new_transcript_item(sid, 1);
    let created = transcript_repo.append(item).await.unwrap();
    assert_eq!(created.seq, 1);

    let item2 = new_transcript_item(sid, 2);
    transcript_repo.append(item2).await.unwrap();

    let list = transcript_repo.list_by_session(sid).await.unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].seq, 1);
    assert_eq!(list[1].seq, 2);

    let deleted = transcript_repo.delete_by_session(sid).await.unwrap();
    assert_eq!(deleted, 2);

    let list = transcript_repo.list_by_session(sid).await.unwrap();
    assert!(list.is_empty());
}

async fn test_job_crud(repo: &dyn JobRepo) {
    let j = new_job();
    let jid = j.id;

    let created = repo.create(j).await.unwrap();
    assert_eq!(created.id, jid);
    assert!(created.enabled);

    let found = repo.find_by_id(jid).await.unwrap();
    assert_eq!(found.job_type, "reminder");

    let enabled = repo.list_enabled().await.unwrap();
    assert!(enabled.iter().any(|j| j.id == jid));

    let by_type = repo.list_by_type("reminder", false).await.unwrap();
    assert!(by_type.iter().any(|j| j.id == jid));

    let terminal_payload = json!({
        "message": "test",
        "status": "missed",
        "last_error": "session unavailable"
    });
    let updated = repo
        .update_job(
            jid,
            false,
            None,
            "reminder",
            "announce",
            terminal_payload.clone(),
            now(),
            None,
            None,
        )
        .await
        .unwrap();
    assert!(!updated.enabled);
    assert_eq!(updated.payload_kind, "reminder");
    assert_eq!(updated.delivery_mode, "announce");
    assert_eq!(updated.payload, terminal_payload);

    let active = repo.list_by_type("reminder", false).await.unwrap();
    assert!(!active.iter().any(|j| j.id == jid));

    let all = repo.list_by_type("reminder", true).await.unwrap();
    assert!(all.iter().any(|j| j.id == jid && !j.enabled));

    let recorded = repo.record_run(jid, now(), Some(now())).await.unwrap();
    assert!(recorded.last_run_at.is_some());

    assert!(repo.delete(jid).await.unwrap());
}

async fn test_job_run_crud(job_repo: &dyn JobRepo, run_repo: &dyn JobRunRepo) {
    let j = new_job();
    let jid = j.id;
    job_repo.create(j).await.unwrap();

    let r = new_job_run(jid);
    let rid = r.id;
    let created = run_repo.create(r).await.unwrap();
    assert_eq!(created.id, rid);
    assert_eq!(created.trigger_kind, "due");
    assert_eq!(created.status, "running");

    let completed = run_repo
        .complete(rid, "success", Some("all good"), now())
        .await
        .unwrap();
    assert_eq!(completed.status, "success");
    assert!(completed.finished_at.is_some());

    let list = run_repo.list_by_job(jid, Some(10)).await.unwrap();
    assert_eq!(list.len(), 1);
}

async fn test_approval_crud(repo: &dyn ApprovalRepo) {
    let a = new_approval();
    let aid = a.id;

    let created = repo.create(a).await.unwrap();
    assert_eq!(created.id, aid);
    assert!(created.decision.is_none());

    let found = repo.find_by_id(aid).await.unwrap();
    assert_eq!(found.reason, "dangerous tool");

    let pending = repo.list(true).await.unwrap();
    assert!(pending.iter().any(|a| a.id == aid));

    let decided = repo.decide(aid, "approved", "admin", now()).await.unwrap();
    assert_eq!(decided.decision, Some("approved".into()));

    let pending_after = repo.list(true).await.unwrap();
    assert!(!pending_after.iter().any(|a| a.id == aid));

    let all = repo.list(false).await.unwrap();
    assert!(all.iter().any(|a| a.id == aid));
}

async fn test_tool_approval_policy(repo: &dyn ToolApprovalPolicyRepo) {
    // Initially empty
    let policies = repo.list_policies().await.unwrap();
    let initial_count = policies.len();

    // Set policy
    let policy = repo.set_policy("file_write", "allow").await.unwrap();
    assert_eq!(policy.tool_name, "file_write");
    assert_eq!(policy.decision, "allow");

    // Get policy
    let found = repo.get_policy("file_write").await.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().decision, "allow");

    // Update policy (upsert)
    let updated = repo.set_policy("file_write", "deny").await.unwrap();
    assert_eq!(updated.decision, "deny");

    // List
    let policies = repo.list_policies().await.unwrap();
    assert_eq!(policies.len(), initial_count + 1);

    // Clear
    assert!(repo.clear_policy("file_write").await.unwrap());
    assert!(!repo.clear_policy("file_write").await.unwrap());

    let gone = repo.get_policy("file_write").await.unwrap();
    assert!(gone.is_none());
}

async fn test_tool_execution_crud(
    session_repo: &dyn SessionRepo,
    turn_repo: &dyn TurnRepo,
    exec_repo: &dyn ToolExecutionRepo,
) {
    let s = new_session();
    let sid = s.id;
    session_repo.create(s).await.unwrap();

    let t = new_turn(sid);
    let tid = t.id;
    turn_repo.create(t).await.unwrap();

    let e = new_tool_execution(sid, tid);
    let eid = e.id;
    let created = exec_repo.create(e).await.unwrap();
    assert_eq!(created.id, eid);
    assert_eq!(created.status, "running");

    let found = exec_repo.find_by_id(eid).await.unwrap();
    assert_eq!(found.tool_name, "file_read");

    let completed = exec_repo
        .complete(eid, "success", Some("read 42 bytes"), None, now())
        .await
        .unwrap();
    assert_eq!(completed.status, "success");

    let recent = exec_repo.list_recent(10).await.unwrap();
    assert!(recent.iter().any(|e| e.id == eid));
}

async fn test_memory_embedding_crud(repo: &dyn MemoryEmbeddingRepo) {
    // Upsert
    repo.upsert_chunk("test/file.md", 0, "hello world", &[])
        .await
        .unwrap();
    repo.upsert_chunk("test/file.md", 1, "goodbye world", &[])
        .await
        .unwrap();

    // Count
    let count = repo.count().await.unwrap();
    assert!(count >= 2);

    // List indexed files
    let files = repo.list_indexed_files().await.unwrap();
    assert!(files.contains(&"test/file.md".to_string()));

    // Keyword search
    let results = repo.keyword_search("hello", 10).await.unwrap();
    assert!(!results.is_empty());
    assert_eq!(results[0].file_path, "test/file.md");

    // Vector search (returns empty on SQLite)
    let _vec_results = repo.vector_search(&[], 10).await.unwrap();

    // Upsert update (same key)
    repo.upsert_chunk("test/file.md", 0, "updated text", &[])
        .await
        .unwrap();

    // Delete by file
    let deleted = repo.delete_by_file("test/file.md").await.unwrap();
    assert_eq!(deleted, 2);
}

async fn test_device_crud(repo: &dyn DeviceRepo) {
    let d = new_paired_device();
    let did = d.id;
    let pk = d.public_key.clone();
    let th = d.token_hash.clone();

    let created = repo.create_device(d).await.unwrap();
    assert_eq!(created.id, did);

    let found = repo.find_device_by_id(did).await.unwrap();
    assert_eq!(found.name, "test-device");

    let by_pk = repo.find_device_by_public_key(&pk).await.unwrap();
    assert!(by_pk.is_some());

    let by_th = repo.find_device_by_token_hash(&th).await.unwrap();
    assert!(by_th.is_some());

    let list = repo.list_devices().await.unwrap();
    assert!(list.iter().any(|d| d.id == did));

    let new_hash = format!("newhash_{}", Uuid::now_v7());
    let updated = repo
        .update_token(did, &new_hash, now() + chrono::Duration::hours(48))
        .await
        .unwrap();
    assert_eq!(updated.token_hash, new_hash);

    let updated = repo.update_role(did, "admin", json!(["*"])).await.unwrap();
    assert_eq!(updated.role, "admin");

    repo.touch_last_seen(did, now()).await.unwrap();

    assert!(repo.delete_device(did).await.unwrap());
    assert!(!repo.delete_device(did).await.unwrap());
}

async fn test_pairing_request_crud(repo: &dyn DeviceRepo) {
    let r = new_pairing_request();
    let rid = r.id;

    let created = repo.create_pairing_request(r).await.unwrap();
    assert_eq!(created.id, rid);

    let pending = repo.list_pending_requests().await.unwrap();
    assert!(pending.iter().any(|r| r.id == rid));

    let taken = repo.take_pairing_request(rid).await.unwrap();
    assert!(taken.is_some());

    // Should be gone now
    let taken_again = repo.take_pairing_request(rid).await.unwrap();
    assert!(taken_again.is_none());
}

async fn test_datetime_roundtrip(repo: &dyn SessionRepo) {
    let ts = chrono::DateTime::parse_from_rfc3339("2026-03-17T12:34:56.789012Z")
        .unwrap()
        .with_timezone(&Utc);
    let mut s = new_session();
    s.created_at = ts;
    s.updated_at = ts;
    s.last_activity_at = ts;
    let id = s.id;

    repo.create(s).await.unwrap();
    let found = repo.find_by_id(id).await.unwrap();

    // Verify microsecond precision round-trips
    assert_eq!(found.created_at, ts);
}

async fn test_json_roundtrip(repo: &dyn SessionRepo) {
    let complex_json = json!({
        "nested": {"array": [1, 2, 3], "null_val": null},
        "string": "hello \"world\"",
        "bool": true,
    });
    let mut s = new_session();
    s.metadata = complex_json.clone();
    let id = s.id;

    repo.create(s).await.unwrap();
    let found = repo.find_by_id(id).await.unwrap();
    assert_eq!(found.metadata, complex_json);
}

async fn test_claim_due_jobs(repo: &dyn JobRepo) {
    let past = now() - chrono::Duration::seconds(60);
    let future = now() + chrono::Duration::hours(1);

    // Create a due cron job (next_run_at in the past).
    let mut due_job = new_job();
    due_job.job_type = "cron".into();
    due_job.schedule = Some(r#"{"kind":"every","every_ms":60000}"#.into());
    let due_id = due_job.id;
    repo.create(due_job).await.unwrap();
    // Set next_run_at to the past.
    repo.record_run(due_id, past, Some(past)).await.unwrap();

    // Create a not-yet-due cron job.
    let mut future_job = new_job();
    future_job.job_type = "cron".into();
    future_job.schedule = Some(r#"{"kind":"every","every_ms":60000}"#.into());
    let future_id = future_job.id;
    repo.create(future_job).await.unwrap();
    repo.record_run(future_id, now(), Some(future))
        .await
        .unwrap();

    let claim_now = now();
    let stale_before = claim_now - chrono::Duration::seconds(300);

    // First claim should return only the due job.
    let claimed = repo
        .claim_due_jobs("cron", claim_now, stale_before, 10)
        .await
        .unwrap();
    assert_eq!(claimed.len(), 1, "should claim exactly the one due job");
    assert_eq!(claimed[0].id, due_id);
    assert!(claimed[0].claimed_at.is_some());

    // Second claim should return nothing (job already claimed, claim is fresh).
    let claimed2 = repo
        .claim_due_jobs("cron", now(), stale_before, 10)
        .await
        .unwrap();
    assert!(
        claimed2.is_empty(),
        "duplicate claim must return empty (job is already claimed)"
    );
}

async fn test_claim_stale_reclaim(repo: &dyn JobRepo) {
    // Use a far-past next_run_at so it's "due" even at an old claim timestamp.
    let far_past = now() - chrono::Duration::seconds(3600);

    // Create a due cron job.
    let mut job = new_job();
    job.job_type = "cron".into();
    job.schedule = Some(r#"{"kind":"every","every_ms":60000}"#.into());
    let jid = job.id;
    repo.create(job).await.unwrap();
    repo.record_run(jid, far_past, Some(far_past)).await.unwrap();

    let old_claim_time = now() - chrono::Duration::seconds(600);
    let fresh_stale_before = now() - chrono::Duration::seconds(300);

    // Simulate an old claim (as if a previous supervisor crashed 10 min ago).
    let first_claimed = repo
        .claim_due_jobs("cron", old_claim_time, old_claim_time, 10)
        .await
        .unwrap();
    assert_eq!(first_claimed.len(), 1, "should claim the due job");

    // Now reclaim with a fresh stale_before — old claim should be expired.
    let reclaimed = repo
        .claim_due_jobs("cron", now(), fresh_stale_before, 10)
        .await
        .unwrap();
    assert_eq!(
        reclaimed.len(),
        1,
        "stale claim should be reclaimable by another supervisor"
    );
    assert_eq!(reclaimed[0].id, jid);
}

async fn test_release_claim(repo: &dyn JobRepo) {
    let past = now() - chrono::Duration::seconds(60);

    let mut job = new_job();
    job.job_type = "cron".into();
    job.schedule = Some(r#"{"kind":"every","every_ms":60000}"#.into());
    let jid = job.id;
    repo.create(job).await.unwrap();
    repo.record_run(jid, past, Some(past)).await.unwrap();

    // Claim the job.
    let claimed = repo
        .claim_due_jobs("cron", now(), now() - chrono::Duration::seconds(300), 10)
        .await
        .unwrap();
    assert_eq!(claimed.len(), 1);

    // Release the claim.
    repo.release_claim(jid).await.unwrap();

    // After release, the job should be re-claimable immediately.
    let reclaimed = repo
        .claim_due_jobs("cron", now(), now() - chrono::Duration::seconds(300), 10)
        .await
        .unwrap();
    assert_eq!(
        reclaimed.len(),
        1,
        "released job should be immediately reclaimable"
    );
}

async fn test_claim_due_reminders(repo: &dyn JobRepo) {
    let past = now() - chrono::Duration::seconds(60);

    // Create a due reminder.
    let mut reminder = new_job();
    reminder.job_type = "reminder".into();
    reminder.due_at = Some(past);
    let rid = reminder.id;
    repo.create(reminder).await.unwrap();

    let claim_now = now();
    let stale_before = claim_now - chrono::Duration::seconds(300);

    let claimed = repo
        .claim_due_jobs("reminder", claim_now, stale_before, 10)
        .await
        .unwrap();
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].id, rid);

    // Second claim returns empty.
    let claimed2 = repo
        .claim_due_jobs("reminder", now(), stale_before, 10)
        .await
        .unwrap();
    assert!(claimed2.is_empty());
}

// ── SQLite contract module ───────────────────────────────────────────

#[cfg(feature = "sqlite")]
mod sqlite_contract {
    use super::*;
    use rune_store::sqlite::*;

    async fn repos() -> (
        SqliteSessionRepo,
        SqliteTurnRepo,
        SqliteTranscriptRepo,
        SqliteJobRepo,
        SqliteJobRunRepo,
        SqliteApprovalRepo,
        SqliteToolApprovalPolicyRepo,
        SqliteToolExecutionRepo,
        SqliteMemoryEmbeddingRepo,
        SqliteDeviceRepo,
    ) {
        let conn = open_memory().await.unwrap();
        (
            SqliteSessionRepo::new(conn.clone()),
            SqliteTurnRepo::new(conn.clone()),
            SqliteTranscriptRepo::new(conn.clone()),
            SqliteJobRepo::new(conn.clone()),
            SqliteJobRunRepo::new(conn.clone()),
            SqliteApprovalRepo::new(conn.clone()),
            SqliteToolApprovalPolicyRepo::new(conn.clone()),
            SqliteToolExecutionRepo::new(conn.clone()),
            SqliteMemoryEmbeddingRepo::new(conn.clone()),
            SqliteDeviceRepo::new(conn),
        )
    }

    #[tokio::test]
    async fn session_crud() {
        let (s, ..) = repos().await;
        test_session_crud(&s).await;
    }

    #[tokio::test]
    async fn session_channel_ref() {
        let (s, ..) = repos().await;
        test_session_channel_ref(&s).await;
    }

    #[tokio::test]
    async fn turn_crud() {
        let (s, t, ..) = repos().await;
        test_turn_crud(&s, &t).await;
    }

    #[tokio::test]
    async fn transcript_crud() {
        let (s, _, tr, ..) = repos().await;
        test_transcript_crud(&s, &tr).await;
    }

    #[tokio::test]
    async fn job_crud() {
        let (_, _, _, j, ..) = repos().await;
        test_job_crud(&j).await;
    }

    #[tokio::test]
    async fn job_run_crud() {
        let (_, _, _, j, jr, ..) = repos().await;
        test_job_run_crud(&j, &jr).await;
    }

    #[tokio::test]
    async fn approval_crud() {
        let (_, _, _, _, _, a, ..) = repos().await;
        test_approval_crud(&a).await;
    }

    #[tokio::test]
    async fn tool_approval_policy() {
        let (_, _, _, _, _, _, tap, ..) = repos().await;
        test_tool_approval_policy(&tap).await;
    }

    #[tokio::test]
    async fn tool_execution_crud() {
        let (s, t, _, _, _, _, _, te, ..) = repos().await;
        test_tool_execution_crud(&s, &t, &te).await;
    }

    #[tokio::test]
    async fn memory_embedding_crud() {
        let (_, _, _, _, _, _, _, _, me, _) = repos().await;
        test_memory_embedding_crud(&me).await;
    }

    #[tokio::test]
    async fn device_crud() {
        let (_, _, _, _, _, _, _, _, _, d) = repos().await;
        test_device_crud(&d).await;
    }

    #[tokio::test]
    async fn pairing_request_crud() {
        let (_, _, _, _, _, _, _, _, _, d) = repos().await;
        test_pairing_request_crud(&d).await;
    }

    #[tokio::test]
    async fn datetime_roundtrip() {
        let (s, ..) = repos().await;
        test_datetime_roundtrip(&s).await;
    }

    #[tokio::test]
    async fn json_roundtrip() {
        let (s, ..) = repos().await;
        test_json_roundtrip(&s).await;
    }

    #[tokio::test]
    async fn claim_due_jobs() {
        let (_, _, _, j, ..) = repos().await;
        test_claim_due_jobs(&j).await;
    }

    #[tokio::test]
    async fn claim_stale_reclaim() {
        let (_, _, _, j, ..) = repos().await;
        test_claim_stale_reclaim(&j).await;
    }

    #[tokio::test]
    async fn release_claim() {
        let (_, _, _, j, ..) = repos().await;
        test_release_claim(&j).await;
    }

    #[tokio::test]
    async fn claim_due_reminders() {
        let (_, _, _, j, ..) = repos().await;
        test_claim_due_reminders(&j).await;
    }
}
