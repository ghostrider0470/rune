//! PostgreSQL repository implementations using `diesel-async`.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use diesel::OptionalExtension;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Int4, Text};
use diesel_async::RunQueryDsl;
use uuid::Uuid;

use crate::error::StoreError;
use crate::models::*;
use crate::pool::PgPool;
use crate::repos::*;
use crate::schema::*;

const MEMORY_KEYWORD_SEARCH_SQL: &str = r#"SELECT file_path, chunk_text,
       ts_rank(to_tsvector('english', chunk_text),
               plainto_tsquery('english', $1))::float8 AS score
FROM memory_embeddings
WHERE to_tsvector('english', chunk_text) @@ plainto_tsquery('english', $1)
ORDER BY score DESC
LIMIT $2"#;

const MEMORY_VECTOR_SEARCH_SQL: &str = r#"SELECT file_path, chunk_text,
       1 - (embedding <=> $1::vector) AS score
FROM memory_embeddings
ORDER BY embedding <=> $1::vector
LIMIT $2"#;

const MEMORY_UPSERT_CHUNK_SQL: &str = r#"INSERT INTO memory_embeddings (file_path, chunk_index, chunk_text, embedding)
VALUES ($1, $2, $3, $4::vector)
ON CONFLICT (file_path, chunk_index)
DO UPDATE SET chunk_text = EXCLUDED.chunk_text,
              embedding  = EXCLUDED.embedding,
              created_at = now()"#;

const MEMORY_DELETE_FILE_CHUNKS_SQL: &str = "DELETE FROM memory_embeddings WHERE file_path = $1";

// ── helpers ──────────────────────────────────────────────────────────

fn pool_err(e: impl std::fmt::Display) -> StoreError {
    StoreError::Database(format!("pool error: {e}"))
}

fn not_found_or(e: diesel::result::Error, entity: &'static str, id: Uuid) -> StoreError {
    match e {
        diesel::result::Error::NotFound => StoreError::NotFound {
            entity,
            id: id.to_string(),
        },
        other => StoreError::from(other),
    }
}

// ── PgSessionRepo ───────────────────────────────────────────────────

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
    async fn create(&self, session: NewSession) -> Result<SessionRow, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        diesel::insert_into(sessions::table)
            .values(&session)
            .returning(SessionRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(StoreError::from)
    }

    async fn find_by_id(&self, id: Uuid) -> Result<SessionRow, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        sessions::table
            .find(id)
            .select(SessionRow::as_select())
            .first(&mut conn)
            .await
            .map_err(|e| not_found_or(e, "session", id))
    }

    async fn list(&self, limit: i64, offset: i64) -> Result<Vec<SessionRow>, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        sessions::table
            .select(SessionRow::as_select())
            .order(sessions::created_at.desc())
            .limit(limit)
            .offset(offset)
            .load(&mut conn)
            .await
            .map_err(StoreError::from)
    }

    async fn find_by_channel_ref(
        &self,
        channel_ref: &str,
    ) -> Result<Option<SessionRow>, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let terminal = vec!["completed", "failed", "cancelled"];
        sessions::table
            .filter(sessions::channel_ref.eq(channel_ref))
            .filter(sessions::status.ne_all(terminal))
            .select(SessionRow::as_select())
            .order(sessions::created_at.desc())
            .first(&mut conn)
            .await
            .optional()
            .map_err(StoreError::from)
    }

    async fn update_status(
        &self,
        id: Uuid,
        status: &str,
        updated_at: DateTime<Utc>,
    ) -> Result<SessionRow, StoreError> {
        let target: rune_core::SessionStatus = status
            .parse()
            .map_err(|e: rune_core::CoreError| StoreError::InvalidTransition(e.to_string()))?;

        let mut conn = self.pool.get().await.map_err(pool_err)?;

        // Read current status and validate the FSM transition.
        let current_str: String = sessions::table
            .find(id)
            .select(sessions::status)
            .first(&mut conn)
            .await
            .map_err(|e| not_found_or(e, "session", id))?;
        if let Ok(current) = current_str.parse::<rune_core::SessionStatus>() {
            current
                .transition(target)
                .map_err(|e| StoreError::InvalidTransition(e.to_string()))?;
        }

        diesel::update(sessions::table.find(id))
            .set((
                sessions::status.eq(status),
                sessions::updated_at.eq(updated_at),
                sessions::last_activity_at.eq(updated_at),
            ))
            .returning(SessionRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(|e| not_found_or(e, "session", id))
    }

    async fn update_metadata(
        &self,
        id: Uuid,
        metadata: serde_json::Value,
        updated_at: DateTime<Utc>,
    ) -> Result<SessionRow, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        diesel::update(sessions::table.find(id))
            .set((
                sessions::metadata.eq(metadata),
                sessions::updated_at.eq(updated_at),
                sessions::last_activity_at.eq(updated_at),
            ))
            .returning(SessionRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(|e| not_found_or(e, "session", id))
    }

    async fn update_latest_turn(
        &self,
        id: Uuid,
        turn_id: Uuid,
        updated_at: DateTime<Utc>,
    ) -> Result<SessionRow, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        diesel::update(sessions::table.find(id))
            .set((
                sessions::latest_turn_id.eq(Some(turn_id)),
                sessions::updated_at.eq(updated_at),
                sessions::last_activity_at.eq(updated_at),
            ))
            .returning(SessionRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(|e| not_found_or(e, "session", id))
    }

    async fn delete(&self, id: Uuid) -> Result<bool, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let affected = diesel::delete(sessions::table.find(id))
            .execute(&mut conn)
            .await
            .map_err(StoreError::from)?;
        Ok(affected > 0)
    }
}

// ── PgTurnRepo ──────────────────────────────────────────────────────

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
    async fn create(&self, turn: NewTurn) -> Result<TurnRow, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        diesel::insert_into(turns::table)
            .values(&turn)
            .returning(TurnRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(StoreError::from)
    }

    async fn find_by_id(&self, id: Uuid) -> Result<TurnRow, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        turns::table
            .find(id)
            .select(TurnRow::as_select())
            .first(&mut conn)
            .await
            .map_err(|e| not_found_or(e, "turn", id))
    }

    async fn list_by_session(&self, session_id: Uuid) -> Result<Vec<TurnRow>, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        turns::table
            .filter(turns::session_id.eq(session_id))
            .select(TurnRow::as_select())
            .order(turns::started_at.asc())
            .load(&mut conn)
            .await
            .map_err(StoreError::from)
    }

    async fn update_status(
        &self,
        id: Uuid,
        status: &str,
        ended_at: Option<DateTime<Utc>>,
    ) -> Result<TurnRow, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        if let Some(ended) = ended_at {
            diesel::update(turns::table.find(id))
                .set((turns::status.eq(status), turns::ended_at.eq(Some(ended))))
                .returning(TurnRow::as_returning())
                .get_result(&mut conn)
                .await
                .map_err(|e| not_found_or(e, "turn", id))
        } else {
            diesel::update(turns::table.find(id))
                .set(turns::status.eq(status))
                .returning(TurnRow::as_returning())
                .get_result(&mut conn)
                .await
                .map_err(|e| not_found_or(e, "turn", id))
        }
    }

    async fn update_usage(
        &self,
        id: Uuid,
        prompt_tokens: i32,
        completion_tokens: i32,
    ) -> Result<TurnRow, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        diesel::update(turns::table.find(id))
            .set((
                turns::usage_prompt_tokens.eq(Some(prompt_tokens)),
                turns::usage_completion_tokens.eq(Some(completion_tokens)),
            ))
            .returning(TurnRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(|e| not_found_or(e, "turn", id))
    }
}

// ── PgTranscriptRepo ────────────────────────────────────────────────

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
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        diesel::insert_into(transcript_items::table)
            .values(&item)
            .returning(TranscriptItemRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(StoreError::from)
    }

    async fn list_by_session(
        &self,
        session_id: Uuid,
    ) -> Result<Vec<TranscriptItemRow>, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        transcript_items::table
            .filter(transcript_items::session_id.eq(session_id))
            .select(TranscriptItemRow::as_select())
            .order(transcript_items::seq.asc())
            .load(&mut conn)
            .await
            .map_err(StoreError::from)
    }

    async fn delete_by_session(&self, session_id: Uuid) -> Result<usize, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let affected = diesel::delete(
            transcript_items::table.filter(transcript_items::session_id.eq(session_id)),
        )
        .execute(&mut conn)
        .await
        .map_err(StoreError::from)?;
        Ok(affected)
    }
}

// ── PgJobRepo ───────────────────────────────────────────────────────

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
    async fn create(&self, job: NewJob) -> Result<JobRow, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        diesel::insert_into(jobs::table)
            .values(&job)
            .returning(JobRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(StoreError::from)
    }

    async fn find_by_id(&self, id: Uuid) -> Result<JobRow, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        jobs::table
            .find(id)
            .select(JobRow::as_select())
            .first(&mut conn)
            .await
            .map_err(|e| not_found_or(e, "job", id))
    }

    async fn list_enabled(&self) -> Result<Vec<JobRow>, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        jobs::table
            .filter(jobs::enabled.eq(true))
            .select(JobRow::as_select())
            .order(jobs::created_at.asc())
            .load(&mut conn)
            .await
            .map_err(StoreError::from)
    }

    async fn list_by_type(
        &self,
        job_type: &str,
        include_disabled: bool,
    ) -> Result<Vec<JobRow>, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let mut query = jobs::table.filter(jobs::job_type.eq(job_type)).into_boxed();

        if !include_disabled {
            query = query.filter(jobs::enabled.eq(true));
        }

        query
            .select(JobRow::as_select())
            .order((jobs::due_at.asc().nulls_last(), jobs::created_at.asc()))
            .load(&mut conn)
            .await
            .map_err(StoreError::from)
    }

    async fn update_job(
        &self,
        id: Uuid,
        enabled: bool,
        due_at: Option<DateTime<Utc>>,
        payload_kind: &str,
        delivery_mode: &str,
        payload: serde_json::Value,
        updated_at: DateTime<Utc>,
        last_run_at: Option<DateTime<Utc>>,
        next_run_at: Option<DateTime<Utc>>,
    ) -> Result<JobRow, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        diesel::update(jobs::table.find(id))
            .set((
                jobs::enabled.eq(enabled),
                jobs::due_at.eq(due_at),
                jobs::payload_kind.eq(payload_kind),
                jobs::delivery_mode.eq(delivery_mode),
                jobs::payload.eq(payload),
                jobs::updated_at.eq(updated_at),
                jobs::last_run_at.eq(last_run_at),
                jobs::next_run_at.eq(next_run_at),
            ))
            .returning(JobRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(|e| not_found_or(e, "job", id))
    }

    async fn delete(&self, id: Uuid) -> Result<bool, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let deleted = diesel::delete(jobs::table.find(id))
            .execute(&mut conn)
            .await
            .map_err(StoreError::from)?;
        Ok(deleted > 0)
    }

    async fn record_run(
        &self,
        id: Uuid,
        last_run_at: DateTime<Utc>,
        next_run_at: Option<DateTime<Utc>>,
    ) -> Result<JobRow, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        diesel::update(jobs::table.find(id))
            .set((
                jobs::last_run_at.eq(Some(last_run_at)),
                jobs::next_run_at.eq(next_run_at),
                jobs::updated_at.eq(last_run_at),
            ))
            .returning(JobRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(|e| not_found_or(e, "job", id))
    }

    async fn claim_due_jobs(
        &self,
        job_type: &str,
        now: DateTime<Utc>,
        stale_before: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<JobRow>, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let due_col_sql = if job_type == "reminder" {
            "due_at"
        } else {
            "next_run_at"
        };
        // Atomic claim: UPDATE with subquery + FOR UPDATE SKIP LOCKED,
        // then SELECT the freshly claimed rows by matching claimed_at.
        let update_sql = format!(
            "UPDATE jobs SET claimed_at = $1 \
             WHERE id IN (\
                SELECT id FROM jobs \
                WHERE job_type = $2 AND enabled = true \
                  AND {due_col_sql} IS NOT NULL AND {due_col_sql} <= $1 \
                  AND (claimed_at IS NULL OR claimed_at < $3) \
                ORDER BY {due_col_sql} ASC \
                LIMIT $4 \
                FOR UPDATE SKIP LOCKED\
             )"
        );
        sql_query(update_sql)
            .bind::<diesel::sql_types::Timestamptz, _>(now)
            .bind::<Text, _>(job_type)
            .bind::<diesel::sql_types::Timestamptz, _>(stale_before)
            .bind::<BigInt, _>(limit)
            .execute(&mut conn)
            .await
            .map_err(StoreError::from)?;

        // Fetch the rows we just claimed.
        jobs::table
            .filter(jobs::job_type.eq(job_type))
            .filter(jobs::claimed_at.eq(Some(now)))
            .select(JobRow::as_select())
            .load(&mut conn)
            .await
            .map_err(StoreError::from)
    }

    async fn release_claim(&self, id: Uuid) -> Result<(), StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        diesel::update(jobs::table.find(id))
            .set(jobs::claimed_at.eq(None::<DateTime<Utc>>))
            .execute(&mut conn)
            .await
            .map_err(StoreError::from)?;
        Ok(())
    }
}

// ── PgJobRunRepo ────────────────────────────────────────────────────

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
    async fn create(&self, run: NewJobRun) -> Result<JobRunRow, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        diesel::insert_into(job_runs::table)
            .values(&run)
            .returning(JobRunRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(StoreError::from)
    }

    async fn complete(
        &self,
        id: Uuid,
        status: &str,
        output: Option<&str>,
        finished_at: DateTime<Utc>,
    ) -> Result<JobRunRow, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        diesel::update(job_runs::table.find(id))
            .set((
                job_runs::status.eq(status),
                job_runs::output.eq(output),
                job_runs::finished_at.eq(Some(finished_at)),
            ))
            .returning(JobRunRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(|e| not_found_or(e, "job_run", id))
    }

    async fn list_by_job(
        &self,
        job_id: Uuid,
        limit: Option<i64>,
    ) -> Result<Vec<JobRunRow>, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let mut query = job_runs::table
            .filter(job_runs::job_id.eq(job_id))
            .into_boxed();

        if let Some(limit) = limit {
            query = query.limit(limit);
        }

        query
            .select(JobRunRow::as_select())
            .order(job_runs::started_at.desc())
            .load(&mut conn)
            .await
            .map_err(StoreError::from)
    }
}

// ── PgApprovalRepo ──────────────────────────────────────────────────

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
    async fn create(&self, approval: NewApproval) -> Result<ApprovalRow, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        diesel::insert_into(approvals::table)
            .values(&approval)
            .returning(ApprovalRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(StoreError::from)
    }

    async fn find_by_id(&self, id: Uuid) -> Result<ApprovalRow, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        approvals::table
            .find(id)
            .select(ApprovalRow::as_select())
            .first(&mut conn)
            .await
            .map_err(|e| not_found_or(e, "approval", id))
    }

    async fn list(&self, pending_only: bool) -> Result<Vec<ApprovalRow>, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let mut query = approvals::table.into_boxed();
        if pending_only {
            query = query.filter(approvals::decision.is_null());
        }
        query
            .select(ApprovalRow::as_select())
            .order((approvals::created_at.desc(), approvals::id.desc()))
            .load(&mut conn)
            .await
            .map_err(StoreError::from)
    }

    async fn decide(
        &self,
        id: Uuid,
        decision: &str,
        decided_by: &str,
        decided_at: DateTime<Utc>,
    ) -> Result<ApprovalRow, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        diesel::update(approvals::table.find(id))
            .set((
                approvals::decision.eq(Some(decision)),
                approvals::decided_by.eq(Some(decided_by)),
                approvals::decided_at.eq(Some(decided_at)),
            ))
            .returning(ApprovalRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(|e| not_found_or(e, "approval", id))
    }

    async fn update_presented_payload(
        &self,
        id: Uuid,
        presented_payload: serde_json::Value,
    ) -> Result<ApprovalRow, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        diesel::update(approvals::table.find(id))
            .set(approvals::presented_payload.eq(presented_payload))
            .returning(ApprovalRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(|e| not_found_or(e, "approval", id))
    }
}

// ── PgToolApprovalPolicyRepo ─────────────────────────────────────

/// PostgreSQL-backed tool approval policy repository.
///
/// Stores tool-level allow-always / deny rules as rows in the `approvals`
/// table with `subject_type = "tool_policy"` and the tool name in `reason`.
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

/// Sentinel UUID used as `subject_id` for tool policy rows.
fn tool_policy_subject_id() -> Uuid {
    Uuid::nil()
}

const TOOL_POLICY_SUBJECT_TYPE: &str = "tool_policy";

#[async_trait]
impl ToolApprovalPolicyRepo for PgToolApprovalPolicyRepo {
    async fn list_policies(&self) -> Result<Vec<ToolApprovalPolicy>, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let rows: Vec<ApprovalRow> = approvals::table
            .filter(approvals::subject_type.eq(TOOL_POLICY_SUBJECT_TYPE))
            .select(ApprovalRow::as_select())
            .order(approvals::reason.asc())
            .load(&mut conn)
            .await
            .map_err(StoreError::from)?;

        Ok(rows
            .into_iter()
            .map(|r| ToolApprovalPolicy {
                tool_name: r.reason,
                decision: r.decision.unwrap_or_default(),
                decided_at: r.decided_at.unwrap_or(r.created_at),
            })
            .collect())
    }

    async fn get_policy(&self, tool_name: &str) -> Result<Option<ToolApprovalPolicy>, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let row: Option<ApprovalRow> = approvals::table
            .filter(approvals::subject_type.eq(TOOL_POLICY_SUBJECT_TYPE))
            .filter(approvals::reason.eq(tool_name))
            .select(ApprovalRow::as_select())
            .first(&mut conn)
            .await
            .optional()
            .map_err(StoreError::from)?;

        Ok(row.map(|r| ToolApprovalPolicy {
            tool_name: r.reason,
            decision: r.decision.unwrap_or_default(),
            decided_at: r.decided_at.unwrap_or(r.created_at),
        }))
    }

    async fn set_policy(
        &self,
        tool_name: &str,
        decision: &str,
    ) -> Result<ToolApprovalPolicy, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let now = Utc::now();

        // Delete any existing policy for this tool first (upsert semantics).
        diesel::delete(
            approvals::table
                .filter(approvals::subject_type.eq(TOOL_POLICY_SUBJECT_TYPE))
                .filter(approvals::reason.eq(tool_name)),
        )
        .execute(&mut conn)
        .await
        .map_err(StoreError::from)?;

        let new_row = NewApproval {
            id: Uuid::now_v7(),
            subject_type: TOOL_POLICY_SUBJECT_TYPE.to_string(),
            subject_id: tool_policy_subject_id(),
            reason: tool_name.to_string(),
            presented_payload: serde_json::json!({"decision": decision}),
            created_at: now,
            handle_ref: None,
            host_ref: None,
        };

        let row: ApprovalRow = diesel::insert_into(approvals::table)
            .values(&new_row)
            .returning(ApprovalRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(StoreError::from)?;

        // Update the decision fields.
        let row: ApprovalRow = diesel::update(approvals::table.find(row.id))
            .set((
                approvals::decision.eq(Some(decision)),
                approvals::decided_by.eq(Some("cli")),
                approvals::decided_at.eq(Some(now)),
            ))
            .returning(ApprovalRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(StoreError::from)?;

        Ok(ToolApprovalPolicy {
            tool_name: row.reason,
            decision: row.decision.unwrap_or_default(),
            decided_at: row.decided_at.unwrap_or(row.created_at),
        })
    }

    async fn clear_policy(&self, tool_name: &str) -> Result<bool, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let deleted = diesel::delete(
            approvals::table
                .filter(approvals::subject_type.eq(TOOL_POLICY_SUBJECT_TYPE))
                .filter(approvals::reason.eq(tool_name)),
        )
        .execute(&mut conn)
        .await
        .map_err(StoreError::from)?;

        Ok(deleted > 0)
    }
}

// ── PgToolExecutionRepo ─────────────────────────────────────────────

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
    async fn create(&self, execution: NewToolExecution) -> Result<ToolExecutionRow, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        diesel::insert_into(tool_executions::table)
            .values(&execution)
            .returning(ToolExecutionRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(StoreError::from)
    }

    async fn find_by_id(&self, id: Uuid) -> Result<ToolExecutionRow, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        tool_executions::table
            .find(id)
            .select(ToolExecutionRow::as_select())
            .first(&mut conn)
            .await
            .map_err(|e| not_found_or(e, "tool_execution", id))
    }

    async fn list_recent(&self, limit: i64) -> Result<Vec<ToolExecutionRow>, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        tool_executions::table
            .select(ToolExecutionRow::as_select())
            .order(tool_executions::started_at.desc())
            .limit(limit)
            .load(&mut conn)
            .await
            .map_err(StoreError::from)
    }

    async fn complete(
        &self,
        id: Uuid,
        status: &str,
        result_summary: Option<&str>,
        error_summary: Option<&str>,
        ended_at: DateTime<Utc>,
    ) -> Result<ToolExecutionRow, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        diesel::update(tool_executions::table.find(id))
            .set((
                tool_executions::status.eq(status),
                tool_executions::result_summary.eq(result_summary),
                tool_executions::error_summary.eq(error_summary),
                tool_executions::ended_at.eq(Some(ended_at)),
            ))
            .returning(ToolExecutionRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(|e| not_found_or(e, "tool_execution", id))
    }
}

// ── PgProcessHandleRepo ──────────────────────────────────────────────

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
    async fn create(&self, handle: NewProcessHandle) -> Result<ProcessHandleRow, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        diesel::insert_into(process_handles::table)
            .values(&handle)
            .returning(ProcessHandleRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(StoreError::from)
    }

    async fn find_by_id(&self, process_id: Uuid) -> Result<ProcessHandleRow, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        process_handles::table
            .find(process_id)
            .select(ProcessHandleRow::as_select())
            .first(&mut conn)
            .await
            .map_err(|e| not_found_or(e, "process_handle", process_id))
    }

    async fn update_status(
        &self,
        process_id: Uuid,
        status: &str,
        exit_code: Option<i32>,
        ended_at: Option<DateTime<Utc>>,
    ) -> Result<ProcessHandleRow, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        diesel::update(process_handles::table.find(process_id))
            .set((
                process_handles::status.eq(status),
                process_handles::exit_code.eq(exit_code),
                process_handles::ended_at.eq(ended_at),
            ))
            .returning(ProcessHandleRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(|e| not_found_or(e, "process_handle", process_id))
    }

    async fn list_by_session(&self, session_id: Uuid) -> Result<Vec<ProcessHandleRow>, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        process_handles::table
            .filter(process_handles::session_id.eq(session_id))
            .select(ProcessHandleRow::as_select())
            .order(process_handles::started_at.desc())
            .load(&mut conn)
            .await
            .map_err(StoreError::from)
    }

    async fn list_active(&self) -> Result<Vec<ProcessHandleRow>, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let active = vec!["running", "backgrounded"];
        process_handles::table
            .filter(process_handles::status.eq_any(active))
            .select(ProcessHandleRow::as_select())
            .order(process_handles::started_at.desc())
            .load(&mut conn)
            .await
            .map_err(StoreError::from)
    }
}

// ── PgDeviceRepo ────────────────────────────────────────────────────────────

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
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        sql_query(
            "INSERT INTO memory_embeddings (file_path, chunk_index, chunk_text) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (file_path, chunk_index) \
             DO UPDATE SET chunk_text = EXCLUDED.chunk_text, created_at = now()",
        )
        .bind::<Text, _>(file_path)
        .bind::<Int4, _>(chunk_index)
        .bind::<Text, _>(chunk_text)
        .execute(&mut conn)
        .await
        .map_err(StoreError::from)?;
        Ok(())
    }
}

fn format_pgvector_literal(embedding: &[f32]) -> String {
    let values = embedding
        .iter()
        .map(|value| {
            if value.is_finite() {
                value.to_string()
            } else {
                "0".to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("[{values}]")
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
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let embedding_literal = format_pgvector_literal(embedding);

        sql_query(MEMORY_UPSERT_CHUNK_SQL)
            .bind::<Text, _>(file_path)
            .bind::<Int4, _>(chunk_index)
            .bind::<Text, _>(chunk_text)
            .bind::<Text, _>(&embedding_literal)
            .execute(&mut conn)
            .await
            .map_err(StoreError::from)?;

        Ok(())
    }

    async fn delete_by_file(&self, file_path: &str) -> Result<usize, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let affected = sql_query(MEMORY_DELETE_FILE_CHUNKS_SQL)
            .bind::<Text, _>(file_path)
            .execute(&mut conn)
            .await
            .map_err(StoreError::from)?;
        Ok(affected)
    }

    async fn keyword_search(
        &self,
        query: &str,
        limit: i64,
    ) -> Result<Vec<KeywordSearchRow>, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        sql_query(MEMORY_KEYWORD_SEARCH_SQL)
            .bind::<Text, _>(query)
            .bind::<BigInt, _>(limit)
            .load::<KeywordSearchRow>(&mut conn)
            .await
            .map_err(StoreError::from)
    }

    async fn vector_search(
        &self,
        embedding: &[f32],
        limit: i64,
    ) -> Result<Vec<VectorSearchRow>, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let embedding_literal = format_pgvector_literal(embedding);

        sql_query(MEMORY_VECTOR_SEARCH_SQL)
            .bind::<Text, _>(&embedding_literal)
            .bind::<BigInt, _>(limit)
            .load::<VectorSearchRow>(&mut conn)
            .await
            .map_err(StoreError::from)
    }

    async fn count(&self) -> Result<i64, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let row = sql_query("SELECT COUNT(*) AS count FROM memory_embeddings")
            .get_result::<CountRow>(&mut conn)
            .await
            .map_err(StoreError::from)?;
        Ok(row.count)
    }

    async fn list_indexed_files(&self) -> Result<Vec<String>, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let rows =
            sql_query("SELECT DISTINCT file_path FROM memory_embeddings ORDER BY file_path ASC")
                .load::<IndexedFileRow>(&mut conn)
                .await
                .map_err(StoreError::from)?;
        Ok(rows.into_iter().map(|row| row.file_path).collect())
    }
}

#[async_trait]
impl DeviceRepo for PgDeviceRepo {
    async fn create_device(&self, device: NewPairedDevice) -> Result<PairedDeviceRow, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        diesel::insert_into(paired_devices::table)
            .values(&device)
            .returning(PairedDeviceRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(StoreError::from)
    }

    async fn find_device_by_id(&self, id: Uuid) -> Result<PairedDeviceRow, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        paired_devices::table
            .find(id)
            .select(PairedDeviceRow::as_select())
            .first(&mut conn)
            .await
            .map_err(|e| not_found_or(e, "paired_device", id))
    }

    async fn find_device_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<PairedDeviceRow>, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        paired_devices::table
            .filter(paired_devices::token_hash.eq(token_hash))
            .select(PairedDeviceRow::as_select())
            .first(&mut conn)
            .await
            .optional()
            .map_err(StoreError::from)
    }

    async fn find_device_by_public_key(
        &self,
        public_key: &str,
    ) -> Result<Option<PairedDeviceRow>, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        paired_devices::table
            .filter(paired_devices::public_key.eq(public_key))
            .select(PairedDeviceRow::as_select())
            .first(&mut conn)
            .await
            .optional()
            .map_err(StoreError::from)
    }

    async fn list_devices(&self) -> Result<Vec<PairedDeviceRow>, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        paired_devices::table
            .select(PairedDeviceRow::as_select())
            .order(paired_devices::paired_at.asc())
            .load(&mut conn)
            .await
            .map_err(StoreError::from)
    }

    async fn update_token(
        &self,
        id: Uuid,
        token_hash: &str,
        token_expires_at: DateTime<Utc>,
    ) -> Result<PairedDeviceRow, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        diesel::update(paired_devices::table.find(id))
            .set((
                paired_devices::token_hash.eq(token_hash),
                paired_devices::token_expires_at.eq(token_expires_at),
            ))
            .returning(PairedDeviceRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(|e| not_found_or(e, "paired_device", id))
    }

    async fn update_role(
        &self,
        id: Uuid,
        role: &str,
        scopes: serde_json::Value,
    ) -> Result<PairedDeviceRow, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        diesel::update(paired_devices::table.find(id))
            .set((
                paired_devices::role.eq(role),
                paired_devices::scopes.eq(scopes),
            ))
            .returning(PairedDeviceRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(|e| not_found_or(e, "paired_device", id))
    }

    async fn touch_last_seen(
        &self,
        id: Uuid,
        last_seen_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        diesel::update(paired_devices::table.find(id))
            .set(paired_devices::last_seen_at.eq(Some(last_seen_at)))
            .execute(&mut conn)
            .await
            .map_err(|e| not_found_or(e, "paired_device", id))?;
        Ok(())
    }

    async fn delete_device(&self, id: Uuid) -> Result<bool, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let deleted = diesel::delete(paired_devices::table.find(id))
            .execute(&mut conn)
            .await
            .map_err(StoreError::from)?;
        Ok(deleted > 0)
    }

    async fn create_pairing_request(
        &self,
        request: NewPairingRequest,
    ) -> Result<PairingRequestRow, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        diesel::insert_into(pairing_requests::table)
            .values(&request)
            .returning(PairingRequestRow::as_returning())
            .get_result(&mut conn)
            .await
            .map_err(StoreError::from)
    }

    async fn take_pairing_request(
        &self,
        id: Uuid,
    ) -> Result<Option<PairingRequestRow>, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        diesel::delete(pairing_requests::table.find(id))
            .returning(PairingRequestRow::as_returning())
            .get_result(&mut conn)
            .await
            .optional()
            .map_err(StoreError::from)
    }

    async fn delete_pairing_request(&self, id: Uuid) -> Result<bool, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let deleted = diesel::delete(pairing_requests::table.find(id))
            .execute(&mut conn)
            .await
            .map_err(StoreError::from)?;
        Ok(deleted > 0)
    }

    async fn list_pending_requests(&self) -> Result<Vec<PairingRequestRow>, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        pairing_requests::table
            .filter(pairing_requests::expires_at.gt(Utc::now()))
            .select(PairingRequestRow::as_select())
            .order(pairing_requests::created_at.asc())
            .load(&mut conn)
            .await
            .map_err(StoreError::from)
    }

    async fn prune_expired_requests(&self) -> Result<usize, StoreError> {
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let deleted = diesel::delete(
            pairing_requests::table.filter(pairing_requests::expires_at.le(Utc::now())),
        )
        .execute(&mut conn)
        .await
        .map_err(StoreError::from)?;
        Ok(deleted)
    }
}
