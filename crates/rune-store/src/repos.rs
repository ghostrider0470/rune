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
}

// ── Job repository ────────────────────────────────────────────────────

/// Persistence contract for scheduled jobs.
#[async_trait]
pub trait JobRepo: Send + Sync {
    /// Insert a new job.
    async fn create(&self, job: NewJob) -> Result<JobRow, StoreError>;

    /// Find a job by ID.
    async fn find_by_id(&self, id: Uuid) -> Result<JobRow, StoreError>;

    /// List all enabled jobs.
    async fn list_enabled(&self) -> Result<Vec<JobRow>, StoreError>;

    /// Update last_run_at and next_run_at after a run.
    async fn record_run(
        &self,
        id: Uuid,
        last_run_at: chrono::DateTime<chrono::Utc>,
        next_run_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<JobRow, StoreError>;
}

// ── Approval repository ───────────────────────────────────────────────

/// Persistence contract for approval gates.
#[async_trait]
pub trait ApprovalRepo: Send + Sync {
    /// Insert a new approval request.
    async fn create(&self, approval: NewApproval) -> Result<ApprovalRow, StoreError>;

    /// Find an approval by ID.
    async fn find_by_id(&self, id: Uuid) -> Result<ApprovalRow, StoreError>;

    /// Record a decision on a pending approval.
    async fn decide(
        &self,
        id: Uuid,
        decision: &str,
        decided_by: &str,
        decided_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<ApprovalRow, StoreError>;
}

// ── Tool execution repository ─────────────────────────────────────────

/// Persistence contract for tool execution audit records.
#[async_trait]
pub trait ToolExecutionRepo: Send + Sync {
    /// Insert a new tool execution record.
    async fn create(&self, execution: NewToolExecution) -> Result<ToolExecutionRow, StoreError>;

    /// Find a tool execution by ID.
    async fn find_by_id(&self, id: Uuid) -> Result<ToolExecutionRow, StoreError>;

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
