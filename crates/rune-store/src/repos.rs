//! Repository trait definitions for persistence abstraction.
//!
//! Concrete implementations using `diesel-async` will be added once
//! integration tests with embedded PostgreSQL are available.
//! These traits define the contract that `rune-runtime` and other
//! consumers depend on.

use async_trait::async_trait;
use uuid::Uuid;

use crate::error::StoreError;
use crate::models::*;

// ── Session repository ────────────────────────────────────────────────

/// Persistence contract for session records.
#[async_trait]
pub trait SessionRepo: Send + Sync {
    /// Insert a new session.
    async fn create(&self, session: NewSession) -> Result<SessionRow, StoreError>;

    /// Find a session by ID.
    async fn find_by_id(&self, id: Uuid) -> Result<SessionRow, StoreError>;

    /// List sessions, most recent first.
    async fn list(&self, limit: i64, offset: i64) -> Result<Vec<SessionRow>, StoreError>;

    /// Find the most recent non-terminal session by channel_ref.
    async fn find_by_channel_ref(
        &self,
        channel_ref: &str,
    ) -> Result<Option<SessionRow>, StoreError>;

    /// Update session status and last_activity_at.
    async fn update_status(
        &self,
        id: Uuid,
        status: &str,
        updated_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<SessionRow, StoreError>;

    /// Replace session metadata and update last_activity_at.
    async fn update_metadata(
        &self,
        id: Uuid,
        metadata: serde_json::Value,
        updated_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<SessionRow, StoreError>;

    /// Delete a session by ID. Returns true if a row was removed.
    async fn delete(&self, id: Uuid) -> Result<bool, StoreError>;
}

// ── Turn repository ───────────────────────────────────────────────────

/// Persistence contract for turn records.
#[async_trait]
pub trait TurnRepo: Send + Sync {
    /// Insert a new turn.
    async fn create(&self, turn: NewTurn) -> Result<TurnRow, StoreError>;

    /// Find a turn by ID.
    async fn find_by_id(&self, id: Uuid) -> Result<TurnRow, StoreError>;

    /// List turns for a session, ordered by started_at.
    async fn list_by_session(&self, session_id: Uuid) -> Result<Vec<TurnRow>, StoreError>;

    /// Update turn status and optional end time.
    async fn update_status(
        &self,
        id: Uuid,
        status: &str,
        ended_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<TurnRow, StoreError>;

    /// Persist token usage counters for a turn.
    async fn update_usage(
        &self,
        id: Uuid,
        prompt_tokens: i32,
        completion_tokens: i32,
    ) -> Result<TurnRow, StoreError>;
}

// ── Transcript repository ─────────────────────────────────────────────

/// Persistence contract for transcript items.
#[async_trait]
pub trait TranscriptRepo: Send + Sync {
    /// Append a transcript item.
    async fn append(&self, item: NewTranscriptItem) -> Result<TranscriptItemRow, StoreError>;

    /// List transcript items for a session in sequence order.
    async fn list_by_session(&self, session_id: Uuid)
    -> Result<Vec<TranscriptItemRow>, StoreError>;

    /// Delete all transcript items for a session. Returns the count removed.
    async fn delete_by_session(&self, session_id: Uuid) -> Result<usize, StoreError>;
}

// ── Job repository ────────────────────────────────────────────────────

/// Persistence contract for scheduled jobs.
#[async_trait]
#[allow(clippy::too_many_arguments)]
pub trait JobRepo: Send + Sync {
    /// Insert a new job.
    async fn create(&self, job: NewJob) -> Result<JobRow, StoreError>;

    /// Find a job by ID.
    async fn find_by_id(&self, id: Uuid) -> Result<JobRow, StoreError>;

    /// List all enabled jobs.
    async fn list_enabled(&self) -> Result<Vec<JobRow>, StoreError>;

    /// List jobs of a specific type, optionally including disabled rows.
    async fn list_by_type(
        &self,
        job_type: &str,
        include_disabled: bool,
    ) -> Result<Vec<JobRow>, StoreError>;

    /// Update the durable state of a job row.
    async fn update_job(
        &self,
        id: Uuid,
        enabled: bool,
        due_at: Option<chrono::DateTime<chrono::Utc>>,
        payload: serde_json::Value,
        updated_at: chrono::DateTime<chrono::Utc>,
        last_run_at: Option<chrono::DateTime<chrono::Utc>>,
        next_run_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<JobRow, StoreError>;

    /// Delete a job row. Returns true when a row was removed.
    async fn delete(&self, id: Uuid) -> Result<bool, StoreError>;

    /// Update last_run_at and next_run_at after a run.
    async fn record_run(
        &self,
        id: Uuid,
        last_run_at: chrono::DateTime<chrono::Utc>,
        next_run_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<JobRow, StoreError>;
}

// ── Job run repository ──────────────────────────────────────────────────────

/// Persistence contract for durable scheduled-job run history.
#[async_trait]
pub trait JobRunRepo: Send + Sync {
    /// Insert a new job-run row when execution starts.
    async fn create(&self, run: NewJobRun) -> Result<JobRunRow, StoreError>;

    /// Mark a job run complete and persist final status/output.
    async fn complete(
        &self,
        id: Uuid,
        status: &str,
        output: Option<&str>,
        finished_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<JobRunRow, StoreError>;

    /// List runs for a job, newest first.
    async fn list_by_job(
        &self,
        job_id: Uuid,
        limit: Option<i64>,
    ) -> Result<Vec<JobRunRow>, StoreError>;
}

// ── Approval repository ───────────────────────────────────────────────

/// Persistence contract for approval gates.
#[async_trait]
pub trait ApprovalRepo: Send + Sync {
    /// Insert a new approval request.
    async fn create(&self, approval: NewApproval) -> Result<ApprovalRow, StoreError>;

    /// Find an approval by ID.
    async fn find_by_id(&self, id: Uuid) -> Result<ApprovalRow, StoreError>;

    /// List approvals, optionally filtering to unresolved-only rows.
    async fn list(&self, pending_only: bool) -> Result<Vec<ApprovalRow>, StoreError>;

    /// Record a decision on a pending approval.
    async fn decide(
        &self,
        id: Uuid,
        decision: &str,
        decided_by: &str,
        decided_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<ApprovalRow, StoreError>;

    /// Replace the presented payload for an approval row.
    async fn update_presented_payload(
        &self,
        id: Uuid,
        presented_payload: serde_json::Value,
    ) -> Result<ApprovalRow, StoreError>;
}

// ── Tool approval policy repository ────────────────────────────────────

/// A persisted tool-level approval policy entry.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ToolApprovalPolicy {
    pub tool_name: String,
    pub decision: String,
    pub decided_at: chrono::DateTime<chrono::Utc>,
}

/// Persistence contract for tool-level approval policies (allow-always / deny).
///
/// These are stored as rows in the `approvals` table with
/// `subject_type = "tool_policy"` and the tool name in `reason`.
#[async_trait]
pub trait ToolApprovalPolicyRepo: Send + Sync {
    /// List all persisted tool policies.
    async fn list_policies(&self) -> Result<Vec<ToolApprovalPolicy>, StoreError>;

    /// Get the policy for a specific tool (if any).
    async fn get_policy(&self, tool_name: &str) -> Result<Option<ToolApprovalPolicy>, StoreError>;

    /// Upsert a policy for the given tool. Replaces any existing row.
    async fn set_policy(
        &self,
        tool_name: &str,
        decision: &str,
    ) -> Result<ToolApprovalPolicy, StoreError>;

    /// Remove the policy for a tool. Returns `true` if a row was deleted.
    async fn clear_policy(&self, tool_name: &str) -> Result<bool, StoreError>;
}

// ── Tool execution repository ─────────────────────────────────────────

/// Persistence contract for tool execution audit records.
#[async_trait]
pub trait ToolExecutionRepo: Send + Sync {
    /// Insert a new tool execution record.
    async fn create(&self, execution: NewToolExecution) -> Result<ToolExecutionRow, StoreError>;

    /// Find a tool execution by ID.
    async fn find_by_id(&self, id: Uuid) -> Result<ToolExecutionRow, StoreError>;

    /// List the most recent tool execution rows, newest first.
    async fn list_recent(&self, limit: i64) -> Result<Vec<ToolExecutionRow>, StoreError>;

    /// Update status and result after execution completes.
    async fn complete(
        &self,
        id: Uuid,
        status: &str,
        result_summary: Option<&str>,
        error_summary: Option<&str>,
        ended_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<ToolExecutionRow, StoreError>;
}
