//! PostgreSQL repository implementations using `tokio-postgres`.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::error::StoreError;
use crate::models::*;
use crate::pool::PgPool;
use crate::repos::*;

// ══════════════════════════════════════════════════════════════════════
// Row mapping helpers
// ══════════════════════════════════════════════════════════════════════

fn row_to_session(row: &tokio_postgres::Row) -> SessionRow {
    SessionRow {
        id: row.get("id"),
        kind: row.get("kind"),
        status: row.get("status"),
        workspace_root: row.get("workspace_root"),
        channel_ref: row.get("channel_ref"),
        requester_session_id: row.get("requester_session_id"),
        latest_turn_id: row.get("latest_turn_id"),
        runtime_profile: row.get("runtime_profile"),
        policy_profile: row.get("policy_profile"),
        metadata: row.get("metadata"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        last_activity_at: row.get("last_activity_at"),
    }
}

fn row_to_turn(row: &tokio_postgres::Row) -> TurnRow {
    TurnRow {
        id: row.get("id"),
        session_id: row.get("session_id"),
        trigger_kind: row.get("trigger_kind"),
        status: row.get("status"),
        model_ref: row.get("model_ref"),
        started_at: row.get("started_at"),
        ended_at: row.get("ended_at"),
        usage_prompt_tokens: row.get("usage_prompt_tokens"),
        usage_completion_tokens: row.get("usage_completion_tokens"),
    }
}

fn row_to_transcript_item(row: &tokio_postgres::Row) -> TranscriptItemRow {
    TranscriptItemRow {
        id: row.get("id"),
        session_id: row.get("session_id"),
        turn_id: row.get("turn_id"),
        seq: row.get("seq"),
        kind: row.get("kind"),
        payload: row.get("payload"),
        created_at: row.get("created_at"),
    }
}

fn row_to_job(row: &tokio_postgres::Row) -> JobRow {
    JobRow {
        id: row.get("id"),
        job_type: row.get("job_type"),
        schedule: row.get("schedule"),
        due_at: row.get("due_at"),
        enabled: row.get("enabled"),
        last_run_at: row.get("last_run_at"),
        next_run_at: row.get("next_run_at"),
        payload_kind: row.get("payload_kind"),
        delivery_mode: row.get("delivery_mode"),
        payload: row.get("payload"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        claimed_at: row.get("claimed_at"),
    }
}

fn row_to_job_run(row: &tokio_postgres::Row) -> JobRunRow {
    JobRunRow {
        id: row.get("id"),
        job_id: row.get("job_id"),
        started_at: row.get("started_at"),
        finished_at: row.get("finished_at"),
        trigger_kind: row.get("trigger_kind"),
        status: row.get("status"),
        output: row.get("output"),
        created_at: row.get("created_at"),
    }
}

fn row_to_approval(row: &tokio_postgres::Row) -> ApprovalRow {
    ApprovalRow {
        id: row.get("id"),
        subject_type: row.get("subject_type"),
        subject_id: row.get("subject_id"),
        reason: row.get("reason"),
        decision: row.get("decision"),
        decided_by: row.get("decided_by"),
        decided_at: row.get("decided_at"),
        presented_payload: row.get("presented_payload"),
        created_at: row.get("created_at"),
        handle_ref: row.get("handle_ref"),
        host_ref: row.get("host_ref"),
    }
}

fn row_to_tool_execution(row: &tokio_postgres::Row) -> ToolExecutionRow {
    ToolExecutionRow {
        id: row.get("id"),
        tool_call_id: row.get("tool_call_id"),
        session_id: row.get("session_id"),
        turn_id: row.get("turn_id"),
        tool_name: row.get("tool_name"),
        arguments: row.get("arguments"),
        status: row.get("status"),
        result_summary: row.get("result_summary"),
        error_summary: row.get("error_summary"),
        started_at: row.get("started_at"),
        ended_at: row.get("ended_at"),
        approval_id: row.get("approval_id"),
        execution_mode: row.get("execution_mode"),
    }
}

fn row_to_process_handle(row: &tokio_postgres::Row) -> ProcessHandleRow {
    ProcessHandleRow {
        process_id: row.get("process_id"),
        tool_call_id: row.get("tool_call_id"),
        session_id: row.get("session_id"),
        command: row.get("command"),
        cwd: row.get("cwd"),
        status: row.get("status"),
        exit_code: row.get("exit_code"),
        started_at: row.get("started_at"),
        ended_at: row.get("ended_at"),
        execution_mode: row.get("execution_mode"),
        tool_execution_id: row.get("tool_execution_id"),
    }
}

fn row_to_paired_device(row: &tokio_postgres::Row) -> PairedDeviceRow {
    PairedDeviceRow {
        id: row.get("id"),
        name: row.get("name"),
        public_key: row.get("public_key"),
        role: row.get("role"),
        scopes: row.get("scopes"),
        token_hash: row.get("token_hash"),
        token_expires_at: row.get("token_expires_at"),
        paired_at: row.get("paired_at"),
        last_seen_at: row.get("last_seen_at"),
        created_at: row.get("created_at"),
    }
}

fn row_to_pairing_request(row: &tokio_postgres::Row) -> PairingRequestRow {
    PairingRequestRow {
        id: row.get("id"),
        device_name: row.get("device_name"),
        public_key: row.get("public_key"),
        challenge: row.get("challenge"),
        created_at: row.get("created_at"),
        expires_at: row.get("expires_at"),
    }
}

// ══════════════════════════════════════════════════════════════════════
// PgSessionRepo
// ══════════════════════════════════════════════════════════════════════

/// PostgreSQL-backed session repository.
#[derive(Clone)]
pub struct PgSessionRepo {
    pool: PgPool,
}

impl PgSessionRepo {
    /// Create a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SessionRepo for PgSessionRepo {
    async fn create(&self, s: NewSession) -> Result<SessionRow, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let row = client
            .query_one(
                "INSERT INTO sessions (
                    id, kind, status, workspace_root, channel_ref,
                    requester_session_id, latest_turn_id,
                    runtime_profile, policy_profile, metadata,
                    created_at, updated_at, last_activity_at
                ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
                RETURNING *",
                &[
                    &s.id, &s.kind, &s.status, &s.workspace_root, &s.channel_ref,
                    &s.requester_session_id, &s.latest_turn_id,
                    &s.runtime_profile, &s.policy_profile, &s.metadata,
                    &s.created_at, &s.updated_at, &s.last_activity_at,
                ],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(row_to_session(&row))
    }

    async fn find_by_id(&self, id: Uuid) -> Result<SessionRow, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let row = client
            .query_opt("SELECT * FROM sessions WHERE id = $1", &[&id])
            .await
            .map_err(StoreError::from)?
            .ok_or_else(|| StoreError::NotFound {
                entity: "session",
                id: id.to_string(),
            })?;
        Ok(row_to_session(&row))
    }

    async fn list(&self, limit: i64, offset: i64) -> Result<Vec<SessionRow>, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let rows = client
            .query(
                "SELECT * FROM sessions ORDER BY created_at DESC LIMIT $1 OFFSET $2",
                &[&limit, &offset],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(rows.iter().map(row_to_session).collect())
    }

    async fn find_by_channel_ref(
        &self,
        channel_ref: &str,
    ) -> Result<Option<SessionRow>, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let row = client
            .query_opt(
                "SELECT * FROM sessions \
                 WHERE channel_ref = $1 \
                 AND status NOT IN ('completed','failed','cancelled') \
                 ORDER BY created_at DESC LIMIT 1",
                &[&channel_ref],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(row.as_ref().map(row_to_session))
    }

    async fn update_status(
        &self,
        id: Uuid,
        status: &str,
        updated_at: DateTime<Utc>,
    ) -> Result<SessionRow, StoreError> {
        // Parse and validate target status before querying.
        let target: rune_core::SessionStatus = status
            .parse()
            .map_err(|e: rune_core::CoreError| StoreError::InvalidTransition(e.to_string()))?;

        let client = self.pool.get().await.map_err(StoreError::from)?;

        // Read current status and validate the FSM transition.
        let current_row = client
            .query_opt("SELECT status FROM sessions WHERE id = $1", &[&id])
            .await
            .map_err(StoreError::from)?
            .ok_or_else(|| StoreError::NotFound {
                entity: "session",
                id: id.to_string(),
            })?;
        let current_str: String = current_row.get("status");
        if let Ok(current) = current_str.parse::<rune_core::SessionStatus>() {
            if let Err(e) = current.transition(target) {
                return Err(StoreError::InvalidTransition(e.to_string()));
            }
        }

        let row = client
            .query_opt(
                "UPDATE sessions SET status = $1, updated_at = $2, last_activity_at = $2 \
                 WHERE id = $3 RETURNING *",
                &[&status, &updated_at, &id],
            )
            .await
            .map_err(StoreError::from)?
            .ok_or_else(|| StoreError::NotFound {
                entity: "session",
                id: id.to_string(),
            })?;
        Ok(row_to_session(&row))
    }

    async fn update_metadata(
        &self,
        id: Uuid,
        metadata: serde_json::Value,
        updated_at: DateTime<Utc>,
    ) -> Result<SessionRow, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let row = client
            .query_opt(
                "UPDATE sessions SET metadata = $1, updated_at = $2, last_activity_at = $2 \
                 WHERE id = $3 RETURNING *",
                &[&metadata, &updated_at, &id],
            )
            .await
            .map_err(StoreError::from)?
            .ok_or_else(|| StoreError::NotFound {
                entity: "session",
                id: id.to_string(),
            })?;
        Ok(row_to_session(&row))
    }

    async fn update_latest_turn(
        &self,
        id: Uuid,
        turn_id: Uuid,
        updated_at: DateTime<Utc>,
    ) -> Result<SessionRow, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let row = client
            .query_opt(
                "UPDATE sessions SET latest_turn_id = $1, updated_at = $2, last_activity_at = $2 \
                 WHERE id = $3 RETURNING *",
                &[&turn_id, &updated_at, &id],
            )
            .await
            .map_err(StoreError::from)?
            .ok_or_else(|| StoreError::NotFound {
                entity: "session",
                id: id.to_string(),
            })?;
        Ok(row_to_session(&row))
    }

    async fn delete(&self, id: Uuid) -> Result<bool, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let n = client
            .execute("DELETE FROM sessions WHERE id = $1", &[&id])
            .await
            .map_err(StoreError::from)?;
        Ok(n > 0)
    }

    async fn list_active_channel_sessions(&self) -> Result<Vec<SessionRow>, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let rows = client
            .query(
                "SELECT * FROM sessions \
                 WHERE kind = 'Channel' \
                 AND channel_ref IS NOT NULL \
                 AND status NOT IN ('completed','failed','cancelled') \
                 ORDER BY created_at DESC",
                &[],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(rows.iter().map(row_to_session).collect())
    }

    async fn mark_stale_completed(&self, stale_secs: i64) -> Result<u64, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let cutoff = Utc::now() - chrono::Duration::seconds(stale_secs);
        let n = client
            .execute(
                "UPDATE sessions SET status = 'completed' \
                 WHERE status = 'running' AND last_activity_at < $1",
                &[&cutoff],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(n)
    }
}

// ══════════════════════════════════════════════════════════════════════
// PgTurnRepo
// ══════════════════════════════════════════════════════════════════════

/// PostgreSQL-backed turn repository.
#[derive(Clone)]
pub struct PgTurnRepo {
    pool: PgPool,
}

impl PgTurnRepo {
    /// Create a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TurnRepo for PgTurnRepo {
    async fn create(&self, t: NewTurn) -> Result<TurnRow, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let row = client
            .query_one(
                "INSERT INTO turns (
                    id, session_id, trigger_kind, status, model_ref,
                    started_at, ended_at, usage_prompt_tokens, usage_completion_tokens
                ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
                RETURNING *",
                &[
                    &t.id, &t.session_id, &t.trigger_kind, &t.status, &t.model_ref,
                    &t.started_at, &t.ended_at, &t.usage_prompt_tokens, &t.usage_completion_tokens,
                ],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(row_to_turn(&row))
    }

    async fn find_by_id(&self, id: Uuid) -> Result<TurnRow, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let row = client
            .query_opt("SELECT * FROM turns WHERE id = $1", &[&id])
            .await
            .map_err(StoreError::from)?
            .ok_or_else(|| StoreError::NotFound {
                entity: "turn",
                id: id.to_string(),
            })?;
        Ok(row_to_turn(&row))
    }

    async fn list_by_session(&self, session_id: Uuid) -> Result<Vec<TurnRow>, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let rows = client
            .query(
                "SELECT * FROM turns WHERE session_id = $1 ORDER BY started_at ASC",
                &[&session_id],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(rows.iter().map(row_to_turn).collect())
    }

    async fn update_status(
        &self,
        id: Uuid,
        status: &str,
        ended_at: Option<DateTime<Utc>>,
    ) -> Result<TurnRow, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;

        // Read current status and validate the FSM transition.
        let current_row = client
            .query_opt("SELECT status FROM turns WHERE id = $1", &[&id])
            .await
            .map_err(StoreError::from)?
            .ok_or_else(|| StoreError::NotFound {
                entity: "turn",
                id: id.to_string(),
            })?;
        let current_str: String = current_row.get("status");
        crate::turn_status::validate_turn_transition(&current_str, status)?;

        let row = client
            .query_opt(
                "UPDATE turns SET status = $1, ended_at = $2 \
                 WHERE id = $3 RETURNING *",
                &[&status, &ended_at, &id],
            )
            .await
            .map_err(StoreError::from)?
            .ok_or_else(|| StoreError::NotFound {
                entity: "turn",
                id: id.to_string(),
            })?;
        Ok(row_to_turn(&row))
    }

    async fn update_usage(
        &self,
        id: Uuid,
        prompt_tokens: i32,
        completion_tokens: i32,
    ) -> Result<TurnRow, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let row = client
            .query_opt(
                "UPDATE turns SET usage_prompt_tokens = $1, usage_completion_tokens = $2 \
                 WHERE id = $3 RETURNING *",
                &[&prompt_tokens, &completion_tokens, &id],
            )
            .await
            .map_err(StoreError::from)?
            .ok_or_else(|| StoreError::NotFound {
                entity: "turn",
                id: id.to_string(),
            })?;
        Ok(row_to_turn(&row))
    }

    async fn mark_stale_failed(&self, stale_secs: i64) -> Result<u64, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let cutoff = Utc::now() - chrono::Duration::seconds(stale_secs);
        let n = client
            .execute(
                "UPDATE turns SET status = 'failed', ended_at = NOW() \
                 WHERE status IN ('started','model_calling','tool_executing') \
                 AND started_at < $1",
                &[&cutoff],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(n)
    }
}

// ══════════════════════════════════════════════════════════════════════
// PgTranscriptRepo
// ══════════════════════════════════════════════════════════════════════

/// PostgreSQL-backed transcript repository.
#[derive(Clone)]
pub struct PgTranscriptRepo {
    pool: PgPool,
}

impl PgTranscriptRepo {
    /// Create a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TranscriptRepo for PgTranscriptRepo {
    async fn append(&self, item: NewTranscriptItem) -> Result<TranscriptItemRow, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let row = client
            .query_one(
                "INSERT INTO transcript_items (
                    id, session_id, turn_id, seq, kind, payload, created_at
                ) VALUES ($1,$2,$3,$4,$5,$6,$7)
                RETURNING *",
                &[
                    &item.id, &item.session_id, &item.turn_id, &item.seq,
                    &item.kind, &item.payload, &item.created_at,
                ],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(row_to_transcript_item(&row))
    }

    async fn list_by_session(
        &self,
        session_id: Uuid,
    ) -> Result<Vec<TranscriptItemRow>, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let rows = client
            .query(
                "SELECT * FROM transcript_items WHERE session_id = $1 ORDER BY seq ASC",
                &[&session_id],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(rows.iter().map(row_to_transcript_item).collect())
    }

    async fn delete_by_session(&self, session_id: Uuid) -> Result<usize, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let n = client
            .execute(
                "DELETE FROM transcript_items WHERE session_id = $1",
                &[&session_id],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(n as usize)
    }
}

// ══════════════════════════════════════════════════════════════════════
// PgJobRepo (Phase 3 -- stubbed)
// ══════════════════════════════════════════════════════════════════════

/// PostgreSQL-backed job repository.
#[derive(Clone)]
pub struct PgJobRepo {
    pool: PgPool,
}

impl PgJobRepo {
    /// Create a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl JobRepo for PgJobRepo {
    async fn create(&self, _job: NewJob) -> Result<JobRow, StoreError> {
        todo!("Phase 3: implement scheduler PG repos")
    }

    async fn find_by_id(&self, _id: Uuid) -> Result<JobRow, StoreError> {
        todo!("Phase 3: implement scheduler PG repos")
    }

    async fn list_enabled(&self) -> Result<Vec<JobRow>, StoreError> {
        todo!("Phase 3: implement scheduler PG repos")
    }

    async fn list_by_type(
        &self,
        _job_type: &str,
        _include_disabled: bool,
    ) -> Result<Vec<JobRow>, StoreError> {
        todo!("Phase 3: implement scheduler PG repos")
    }

    async fn update_job(
        &self,
        _id: Uuid,
        _enabled: bool,
        _due_at: Option<DateTime<Utc>>,
        _payload_kind: &str,
        _delivery_mode: &str,
        _payload: serde_json::Value,
        _updated_at: DateTime<Utc>,
        _last_run_at: Option<DateTime<Utc>>,
        _next_run_at: Option<DateTime<Utc>>,
    ) -> Result<JobRow, StoreError> {
        todo!("Phase 3: implement scheduler PG repos")
    }

    async fn delete(&self, _id: Uuid) -> Result<bool, StoreError> {
        todo!("Phase 3: implement scheduler PG repos")
    }

    async fn record_run(
        &self,
        _id: Uuid,
        _last_run_at: DateTime<Utc>,
        _next_run_at: Option<DateTime<Utc>>,
    ) -> Result<JobRow, StoreError> {
        todo!("Phase 3: implement scheduler PG repos")
    }

    async fn claim_due_jobs(
        &self,
        _job_type: &str,
        _now: DateTime<Utc>,
        _stale_before: DateTime<Utc>,
        _limit: i64,
    ) -> Result<Vec<JobRow>, StoreError> {
        todo!("Phase 3: implement scheduler PG repos")
    }

    async fn release_claim(&self, _id: Uuid) -> Result<(), StoreError> {
        todo!("Phase 3: implement scheduler PG repos")
    }
}

// ══════════════════════════════════════════════════════════════════════
// PgJobRunRepo (Phase 3 -- stubbed)
// ══════════════════════════════════════════════════════════════════════

/// PostgreSQL-backed durable job-run repository.
#[derive(Clone)]
pub struct PgJobRunRepo {
    pool: PgPool,
}

impl PgJobRunRepo {
    /// Create a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl JobRunRepo for PgJobRunRepo {
    async fn create(&self, _run: NewJobRun) -> Result<JobRunRow, StoreError> {
        todo!("Phase 3: implement scheduler PG repos")
    }

    async fn complete(
        &self,
        _id: Uuid,
        _status: &str,
        _output: Option<&str>,
        _finished_at: DateTime<Utc>,
    ) -> Result<JobRunRow, StoreError> {
        todo!("Phase 3: implement scheduler PG repos")
    }

    async fn list_by_job(
        &self,
        _job_id: Uuid,
        _limit: Option<i64>,
    ) -> Result<Vec<JobRunRow>, StoreError> {
        todo!("Phase 3: implement scheduler PG repos")
    }
}

// ══════════════════════════════════════════════════════════════════════
// PgApprovalRepo
// ══════════════════════════════════════════════════════════════════════

/// PostgreSQL-backed approval repository.
#[derive(Clone)]
pub struct PgApprovalRepo {
    pool: PgPool,
}

impl PgApprovalRepo {
    /// Create a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ApprovalRepo for PgApprovalRepo {
    async fn create(&self, a: NewApproval) -> Result<ApprovalRow, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let row = client
            .query_one(
                "INSERT INTO approvals (
                    id, subject_type, subject_id, reason, decision, decided_by,
                    decided_at, presented_payload, created_at, handle_ref, host_ref
                ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
                RETURNING *",
                &[
                    &a.id, &a.subject_type, &a.subject_id, &a.reason,
                    &None::<String>, &None::<String>, &None::<DateTime<Utc>>,
                    &a.presented_payload, &a.created_at, &a.handle_ref, &a.host_ref,
                ],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(row_to_approval(&row))
    }

    async fn find_by_id(&self, id: Uuid) -> Result<ApprovalRow, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let row = client
            .query_opt("SELECT * FROM approvals WHERE id = $1", &[&id])
            .await
            .map_err(StoreError::from)?
            .ok_or_else(|| StoreError::NotFound {
                entity: "approval",
                id: id.to_string(),
            })?;
        Ok(row_to_approval(&row))
    }

    async fn list(&self, pending_only: bool) -> Result<Vec<ApprovalRow>, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let rows = if pending_only {
            client
                .query(
                    "SELECT * FROM approvals WHERE decision IS NULL \
                     ORDER BY created_at DESC, id DESC",
                    &[],
                )
                .await
                .map_err(StoreError::from)?
        } else {
            client
                .query(
                    "SELECT * FROM approvals ORDER BY created_at DESC, id DESC",
                    &[],
                )
                .await
                .map_err(StoreError::from)?
        };
        Ok(rows.iter().map(row_to_approval).collect())
    }

    async fn decide(
        &self,
        id: Uuid,
        decision: &str,
        decided_by: &str,
        decided_at: DateTime<Utc>,
    ) -> Result<ApprovalRow, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let row = client
            .query_opt(
                "UPDATE approvals SET decision=$1, decided_by=$2, decided_at=$3 \
                 WHERE id=$4 RETURNING *",
                &[&decision, &decided_by, &decided_at, &id],
            )
            .await
            .map_err(StoreError::from)?
            .ok_or_else(|| StoreError::NotFound {
                entity: "approval",
                id: id.to_string(),
            })?;
        Ok(row_to_approval(&row))
    }

    async fn update_presented_payload(
        &self,
        id: Uuid,
        presented_payload: serde_json::Value,
    ) -> Result<ApprovalRow, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let row = client
            .query_opt(
                "UPDATE approvals SET presented_payload=$1 WHERE id=$2 RETURNING *",
                &[&presented_payload, &id],
            )
            .await
            .map_err(StoreError::from)?
            .ok_or_else(|| StoreError::NotFound {
                entity: "approval",
                id: id.to_string(),
            })?;
        Ok(row_to_approval(&row))
    }
}

// ══════════════════════════════════════════════════════════════════════
// PgToolApprovalPolicyRepo
// ══════════════════════════════════════════════════════════════════════

const TOOL_POLICY_SUBJECT_TYPE: &str = "tool_policy";

fn tool_policy_subject_id() -> Uuid {
    Uuid::nil()
}

/// PostgreSQL-backed tool approval policy repository.
#[derive(Clone)]
pub struct PgToolApprovalPolicyRepo {
    pool: PgPool,
}

impl PgToolApprovalPolicyRepo {
    /// Create a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ToolApprovalPolicyRepo for PgToolApprovalPolicyRepo {
    async fn list_policies(&self) -> Result<Vec<ToolApprovalPolicy>, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let rows = client
            .query(
                "SELECT * FROM approvals WHERE subject_type = $1 ORDER BY reason ASC",
                &[&TOOL_POLICY_SUBJECT_TYPE],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(rows
            .iter()
            .map(|r| {
                let a = row_to_approval(r);
                ToolApprovalPolicy {
                    tool_name: a.reason,
                    decision: a.decision.unwrap_or_default(),
                    decided_at: a.decided_at.unwrap_or(a.created_at),
                }
            })
            .collect())
    }

    async fn get_policy(&self, tool_name: &str) -> Result<Option<ToolApprovalPolicy>, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let row = client
            .query_opt(
                "SELECT * FROM approvals WHERE subject_type = $1 AND reason = $2 LIMIT 1",
                &[&TOOL_POLICY_SUBJECT_TYPE, &tool_name],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(row.map(|r| {
            let a = row_to_approval(&r);
            ToolApprovalPolicy {
                tool_name: a.reason,
                decision: a.decision.unwrap_or_default(),
                decided_at: a.decided_at.unwrap_or(a.created_at),
            }
        }))
    }

    async fn set_policy(
        &self,
        tool_name: &str,
        decision: &str,
    ) -> Result<ToolApprovalPolicy, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let now = Utc::now();
        // Delete any existing policy for this tool.
        client
            .execute(
                "DELETE FROM approvals WHERE subject_type = $1 AND reason = $2",
                &[&TOOL_POLICY_SUBJECT_TYPE, &tool_name],
            )
            .await
            .map_err(StoreError::from)?;
        // Insert new policy row.
        let id = Uuid::now_v7();
        let payload = serde_json::json!({"decision": decision});
        client
            .execute(
                "INSERT INTO approvals (
                    id, subject_type, subject_id, reason, decision, decided_by,
                    decided_at, presented_payload, created_at, handle_ref, host_ref
                ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
                &[
                    &id, &TOOL_POLICY_SUBJECT_TYPE, &tool_policy_subject_id(),
                    &tool_name, &decision, &"cli", &now,
                    &payload, &now, &None::<String>, &None::<String>,
                ],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(ToolApprovalPolicy {
            tool_name: tool_name.to_string(),
            decision: decision.to_string(),
            decided_at: now,
        })
    }

    async fn clear_policy(&self, tool_name: &str) -> Result<bool, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let n = client
            .execute(
                "DELETE FROM approvals WHERE subject_type = $1 AND reason = $2",
                &[&TOOL_POLICY_SUBJECT_TYPE, &tool_name],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(n > 0)
    }
}

// ══════════════════════════════════════════════════════════════════════
// PgToolExecutionRepo
// ══════════════════════════════════════════════════════════════════════

/// PostgreSQL-backed tool execution repository.
#[derive(Clone)]
pub struct PgToolExecutionRepo {
    pool: PgPool,
}

impl PgToolExecutionRepo {
    /// Create a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ToolExecutionRepo for PgToolExecutionRepo {
    async fn create(&self, e: NewToolExecution) -> Result<ToolExecutionRow, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let row = client
            .query_one(
                "INSERT INTO tool_executions (
                    id, tool_call_id, session_id, turn_id, tool_name, arguments,
                    status, result_summary, error_summary, started_at, ended_at,
                    approval_id, execution_mode
                ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
                RETURNING *",
                &[
                    &e.id, &e.tool_call_id, &e.session_id, &e.turn_id,
                    &e.tool_name, &e.arguments, &e.status,
                    &None::<String>, &None::<String>,
                    &e.started_at, &None::<DateTime<Utc>>,
                    &e.approval_id, &e.execution_mode,
                ],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(row_to_tool_execution(&row))
    }

    async fn find_by_id(&self, id: Uuid) -> Result<ToolExecutionRow, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let row = client
            .query_opt("SELECT * FROM tool_executions WHERE id = $1", &[&id])
            .await
            .map_err(StoreError::from)?
            .ok_or_else(|| StoreError::NotFound {
                entity: "tool_execution",
                id: id.to_string(),
            })?;
        Ok(row_to_tool_execution(&row))
    }

    async fn list_recent(&self, limit: i64) -> Result<Vec<ToolExecutionRow>, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let rows = client
            .query(
                "SELECT * FROM tool_executions ORDER BY started_at DESC LIMIT $1",
                &[&limit],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(rows.iter().map(row_to_tool_execution).collect())
    }

    async fn complete(
        &self,
        id: Uuid,
        status: &str,
        result_summary: Option<&str>,
        error_summary: Option<&str>,
        ended_at: DateTime<Utc>,
    ) -> Result<ToolExecutionRow, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let row = client
            .query_opt(
                "UPDATE tool_executions SET status=$1, result_summary=$2, error_summary=$3, \
                 ended_at=$4 WHERE id=$5 RETURNING *",
                &[&status, &result_summary, &error_summary, &ended_at, &id],
            )
            .await
            .map_err(StoreError::from)?
            .ok_or_else(|| StoreError::NotFound {
                entity: "tool_execution",
                id: id.to_string(),
            })?;
        Ok(row_to_tool_execution(&row))
    }
}

// ══════════════════════════════════════════════════════════════════════
// PgProcessHandleRepo
// ══════════════════════════════════════════════════════════════════════

/// PostgreSQL-backed process handle repository.
#[derive(Clone)]
pub struct PgProcessHandleRepo {
    pool: PgPool,
}

impl PgProcessHandleRepo {
    /// Create a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ProcessHandleRepo for PgProcessHandleRepo {
    async fn create(&self, h: NewProcessHandle) -> Result<ProcessHandleRow, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let row = client
            .query_one(
                "INSERT INTO process_handles (
                    process_id, tool_call_id, session_id, command, cwd,
                    status, exit_code, started_at, ended_at,
                    execution_mode, tool_execution_id
                ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
                RETURNING *",
                &[
                    &h.process_id, &h.tool_call_id, &h.session_id, &h.command, &h.cwd,
                    &h.status, &None::<i32>, &h.started_at, &None::<DateTime<Utc>>,
                    &h.execution_mode, &h.tool_execution_id,
                ],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(row_to_process_handle(&row))
    }

    async fn find_by_id(&self, process_id: Uuid) -> Result<ProcessHandleRow, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let row = client
            .query_opt(
                "SELECT * FROM process_handles WHERE process_id = $1",
                &[&process_id],
            )
            .await
            .map_err(StoreError::from)?
            .ok_or_else(|| StoreError::NotFound {
                entity: "process_handle",
                id: process_id.to_string(),
            })?;
        Ok(row_to_process_handle(&row))
    }

    async fn update_status(
        &self,
        process_id: Uuid,
        status: &str,
        exit_code: Option<i32>,
        ended_at: Option<DateTime<Utc>>,
    ) -> Result<ProcessHandleRow, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let row = client
            .query_opt(
                "UPDATE process_handles SET status=$1, exit_code=$2, ended_at=$3 \
                 WHERE process_id=$4 RETURNING *",
                &[&status, &exit_code, &ended_at, &process_id],
            )
            .await
            .map_err(StoreError::from)?
            .ok_or_else(|| StoreError::NotFound {
                entity: "process_handle",
                id: process_id.to_string(),
            })?;
        Ok(row_to_process_handle(&row))
    }

    async fn list_by_session(
        &self,
        session_id: Uuid,
    ) -> Result<Vec<ProcessHandleRow>, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let rows = client
            .query(
                "SELECT * FROM process_handles WHERE session_id = $1 ORDER BY started_at DESC",
                &[&session_id],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(rows.iter().map(row_to_process_handle).collect())
    }

    async fn list_active(&self) -> Result<Vec<ProcessHandleRow>, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let rows = client
            .query(
                "SELECT * FROM process_handles \
                 WHERE status IN ('running', 'backgrounded') \
                 ORDER BY started_at DESC",
                &[],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(rows.iter().map(row_to_process_handle).collect())
    }

    async fn find_by_tool_call_id(
        &self,
        tool_call_id: Uuid,
    ) -> Result<Option<ProcessHandleRow>, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let row = client
            .query_opt(
                "SELECT * FROM process_handles WHERE tool_call_id = $1",
                &[&tool_call_id],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(row.as_ref().map(row_to_process_handle))
    }

    async fn find_by_tool_execution_id(
        &self,
        tool_execution_id: Uuid,
    ) -> Result<Vec<ProcessHandleRow>, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let rows = client
            .query(
                "SELECT * FROM process_handles \
                 WHERE tool_execution_id = $1 ORDER BY started_at DESC",
                &[&tool_execution_id],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(rows.iter().map(row_to_process_handle).collect())
    }
}

// ══════════════════════════════════════════════════════════════════════
// PgDeviceRepo
// ══════════════════════════════════════════════════════════════════════

/// PostgreSQL-backed device pairing repository.
#[derive(Clone)]
pub struct PgDeviceRepo {
    pool: PgPool,
}

impl PgDeviceRepo {
    /// Create a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl DeviceRepo for PgDeviceRepo {
    async fn create_device(&self, d: NewPairedDevice) -> Result<PairedDeviceRow, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let row = client
            .query_one(
                "INSERT INTO paired_devices (
                    id, name, public_key, role, scopes, token_hash,
                    token_expires_at, paired_at, last_seen_at, created_at
                ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
                RETURNING *",
                &[
                    &d.id, &d.name, &d.public_key, &d.role, &d.scopes,
                    &d.token_hash, &d.token_expires_at, &d.paired_at,
                    &None::<DateTime<Utc>>, &d.created_at,
                ],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(row_to_paired_device(&row))
    }

    async fn find_device_by_id(&self, id: Uuid) -> Result<PairedDeviceRow, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let row = client
            .query_opt("SELECT * FROM paired_devices WHERE id = $1", &[&id])
            .await
            .map_err(StoreError::from)?
            .ok_or_else(|| StoreError::NotFound {
                entity: "paired_device",
                id: id.to_string(),
            })?;
        Ok(row_to_paired_device(&row))
    }

    async fn find_device_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<PairedDeviceRow>, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let row = client
            .query_opt(
                "SELECT * FROM paired_devices WHERE token_hash = $1",
                &[&token_hash],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(row.as_ref().map(row_to_paired_device))
    }

    async fn find_device_by_public_key(
        &self,
        public_key: &str,
    ) -> Result<Option<PairedDeviceRow>, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let row = client
            .query_opt(
                "SELECT * FROM paired_devices WHERE public_key = $1",
                &[&public_key],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(row.as_ref().map(row_to_paired_device))
    }

    async fn list_devices(&self) -> Result<Vec<PairedDeviceRow>, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let rows = client
            .query(
                "SELECT * FROM paired_devices ORDER BY paired_at ASC",
                &[],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(rows.iter().map(row_to_paired_device).collect())
    }

    async fn update_token(
        &self,
        id: Uuid,
        token_hash: &str,
        token_expires_at: DateTime<Utc>,
    ) -> Result<PairedDeviceRow, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let row = client
            .query_opt(
                "UPDATE paired_devices SET token_hash=$1, token_expires_at=$2 \
                 WHERE id=$3 RETURNING *",
                &[&token_hash, &token_expires_at, &id],
            )
            .await
            .map_err(StoreError::from)?
            .ok_or_else(|| StoreError::NotFound {
                entity: "paired_device",
                id: id.to_string(),
            })?;
        Ok(row_to_paired_device(&row))
    }

    async fn update_role(
        &self,
        id: Uuid,
        role: &str,
        scopes: serde_json::Value,
    ) -> Result<PairedDeviceRow, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let row = client
            .query_opt(
                "UPDATE paired_devices SET role=$1, scopes=$2 WHERE id=$3 RETURNING *",
                &[&role, &scopes, &id],
            )
            .await
            .map_err(StoreError::from)?
            .ok_or_else(|| StoreError::NotFound {
                entity: "paired_device",
                id: id.to_string(),
            })?;
        Ok(row_to_paired_device(&row))
    }

    async fn touch_last_seen(
        &self,
        id: Uuid,
        last_seen_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let n = client
            .execute(
                "UPDATE paired_devices SET last_seen_at=$1 WHERE id=$2",
                &[&last_seen_at, &id],
            )
            .await
            .map_err(StoreError::from)?;
        if n == 0 {
            return Err(StoreError::NotFound {
                entity: "paired_device",
                id: id.to_string(),
            });
        }
        Ok(())
    }

    async fn delete_device(&self, id: Uuid) -> Result<bool, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let n = client
            .execute("DELETE FROM paired_devices WHERE id = $1", &[&id])
            .await
            .map_err(StoreError::from)?;
        Ok(n > 0)
    }

    async fn create_pairing_request(
        &self,
        r: NewPairingRequest,
    ) -> Result<PairingRequestRow, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let row = client
            .query_one(
                "INSERT INTO pairing_requests (
                    id, device_name, public_key, challenge, created_at, expires_at
                ) VALUES ($1,$2,$3,$4,$5,$6)
                RETURNING *",
                &[
                    &r.id, &r.device_name, &r.public_key, &r.challenge,
                    &r.created_at, &r.expires_at,
                ],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(row_to_pairing_request(&row))
    }

    async fn take_pairing_request(
        &self,
        id: Uuid,
    ) -> Result<Option<PairingRequestRow>, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        // Atomically select-and-delete using a CTE.
        let row = client
            .query_opt(
                "WITH deleted AS (
                    DELETE FROM pairing_requests WHERE id = $1 RETURNING *
                ) SELECT * FROM deleted",
                &[&id],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(row.as_ref().map(row_to_pairing_request))
    }

    async fn delete_pairing_request(&self, id: Uuid) -> Result<bool, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let n = client
            .execute("DELETE FROM pairing_requests WHERE id = $1", &[&id])
            .await
            .map_err(StoreError::from)?;
        Ok(n > 0)
    }

    async fn list_pending_requests(&self) -> Result<Vec<PairingRequestRow>, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let now = Utc::now();
        let rows = client
            .query(
                "SELECT * FROM pairing_requests WHERE expires_at > $1 ORDER BY created_at ASC",
                &[&now],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(rows.iter().map(row_to_pairing_request).collect())
    }

    async fn prune_expired_requests(&self) -> Result<usize, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let now = Utc::now();
        let n = client
            .execute(
                "DELETE FROM pairing_requests WHERE expires_at <= $1",
                &[&now],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(n as usize)
    }
}

// ══════════════════════════════════════════════════════════════════════
// PgMemoryEmbeddingRepo
// ══════════════════════════════════════════════════════════════════════

/// PostgreSQL-backed memory embedding repository.
#[derive(Clone)]
pub struct PgMemoryEmbeddingRepo {
    pool: PgPool,
}

impl PgMemoryEmbeddingRepo {
    /// Create a new repository backed by the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Insert a chunk with keyword text only (no embedding column).
    /// Used when pgvector is unavailable.
    pub async fn upsert_keyword_only(
        &self,
        file_path: &str,
        chunk_index: i32,
        chunk_text: &str,
    ) -> Result<(), StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let id = Uuid::now_v7();
        let now = Utc::now();
        client
            .execute(
                "INSERT INTO memory_embeddings (id, file_path, chunk_index, chunk_text, created_at)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (file_path, chunk_index)
                 DO UPDATE SET chunk_text = EXCLUDED.chunk_text, created_at = EXCLUDED.created_at",
                &[&id, &file_path, &chunk_index, &chunk_text, &now],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(())
    }
}

#[async_trait]
impl MemoryEmbeddingRepo for PgMemoryEmbeddingRepo {
    async fn upsert_chunk(
        &self,
        file_path: &str,
        chunk_index: i32,
        chunk_text: &str,
        embedding: &[f32],
    ) -> Result<(), StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let id = Uuid::now_v7();
        let now = Utc::now();
        // Format the embedding as a pgvector literal: '[0.1,0.2,...]'
        let vec_str = format!(
            "[{}]",
            embedding
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        client
            .execute(
                "INSERT INTO memory_embeddings (id, file_path, chunk_index, chunk_text, embedding, created_at)
                 VALUES ($1, $2, $3, $4, $5::vector, $6)
                 ON CONFLICT (file_path, chunk_index)
                 DO UPDATE SET chunk_text = EXCLUDED.chunk_text, embedding = EXCLUDED.embedding, created_at = EXCLUDED.created_at",
                &[&id, &file_path, &chunk_index, &chunk_text, &vec_str, &now],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(())
    }

    async fn delete_by_file(&self, file_path: &str) -> Result<usize, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let n = client
            .execute(
                "DELETE FROM memory_embeddings WHERE file_path = $1",
                &[&file_path],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(n as usize)
    }

    async fn keyword_search(
        &self,
        query: &str,
        limit: i64,
    ) -> Result<Vec<KeywordSearchRow>, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let rows = client
            .query(
                "SELECT file_path, chunk_text,
                        ts_rank(to_tsvector('english', chunk_text), plainto_tsquery('english', $1)) AS score
                 FROM memory_embeddings
                 WHERE to_tsvector('english', chunk_text) @@ plainto_tsquery('english', $1)
                 ORDER BY score DESC
                 LIMIT $2",
                &[&query, &limit],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(rows
            .iter()
            .map(|r| KeywordSearchRow {
                file_path: r.get("file_path"),
                chunk_text: r.get("chunk_text"),
                score: r.get("score"),
            })
            .collect())
    }

    async fn vector_search(
        &self,
        embedding: &[f32],
        limit: i64,
    ) -> Result<Vec<VectorSearchRow>, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let vec_str = format!(
            "[{}]",
            embedding
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        let rows = client
            .query(
                "SELECT file_path, chunk_text,
                        1 - (embedding <=> $1::vector) AS score
                 FROM memory_embeddings
                 WHERE embedding IS NOT NULL
                 ORDER BY embedding <=> $1::vector ASC
                 LIMIT $2",
                &[&vec_str, &limit],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(rows
            .iter()
            .map(|r| VectorSearchRow {
                file_path: r.get("file_path"),
                chunk_text: r.get("chunk_text"),
                score: r.get("score"),
            })
            .collect())
    }

    async fn count(&self) -> Result<i64, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let row = client
            .query_one("SELECT COUNT(*) AS count FROM memory_embeddings", &[])
            .await
            .map_err(StoreError::from)?;
        Ok(row.get("count"))
    }

    async fn list_indexed_files(&self) -> Result<Vec<String>, StoreError> {
        let client = self.pool.get().await.map_err(StoreError::from)?;
        let rows = client
            .query(
                "SELECT DISTINCT file_path FROM memory_embeddings ORDER BY file_path ASC",
                &[],
            )
            .await
            .map_err(StoreError::from)?;
        Ok(rows.iter().map(|r| r.get("file_path")).collect())
    }
}
