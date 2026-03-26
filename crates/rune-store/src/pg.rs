//! PostgreSQL repository implementations using `tokio-postgres`.
//!
//! All trait methods are stubbed with `todo!()` pending Phase 2 migration.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::error::StoreError;
use crate::models::*;
use crate::pool::PgPool;
use crate::repos::*;

// -- PgSessionRepo --

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
    async fn create(&self, _session: NewSession) -> Result<SessionRow, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn find_by_id(&self, _id: Uuid) -> Result<SessionRow, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn list(&self, _limit: i64, _offset: i64) -> Result<Vec<SessionRow>, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn find_by_channel_ref(
        &self,
        _channel_ref: &str,
    ) -> Result<Option<SessionRow>, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn update_status(
        &self,
        _id: Uuid,
        _status: &str,
        _updated_at: DateTime<Utc>,
    ) -> Result<SessionRow, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn update_metadata(
        &self,
        _id: Uuid,
        _metadata: serde_json::Value,
        _updated_at: DateTime<Utc>,
    ) -> Result<SessionRow, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn update_latest_turn(
        &self,
        _id: Uuid,
        _turn_id: Uuid,
        _updated_at: DateTime<Utc>,
    ) -> Result<SessionRow, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn delete(&self, _id: Uuid) -> Result<bool, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn list_active_channel_sessions(&self) -> Result<Vec<SessionRow>, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn mark_stale_completed(&self, _stale_secs: i64) -> Result<u64, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }
}

// -- PgTurnRepo --

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
    async fn create(&self, _turn: NewTurn) -> Result<TurnRow, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn find_by_id(&self, _id: Uuid) -> Result<TurnRow, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn list_by_session(&self, _session_id: Uuid) -> Result<Vec<TurnRow>, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn update_status(
        &self,
        _id: Uuid,
        _status: &str,
        _ended_at: Option<DateTime<Utc>>,
    ) -> Result<TurnRow, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn update_usage(
        &self,
        _id: Uuid,
        _prompt_tokens: i32,
        _completion_tokens: i32,
    ) -> Result<TurnRow, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn mark_stale_failed(&self, _stale_secs: i64) -> Result<u64, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }
}

// -- PgTranscriptRepo --

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
    async fn append(&self, _item: NewTranscriptItem) -> Result<TranscriptItemRow, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn list_by_session(
        &self,
        _session_id: Uuid,
    ) -> Result<Vec<TranscriptItemRow>, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn delete_by_session(&self, _session_id: Uuid) -> Result<usize, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }
}

// -- PgJobRepo --

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
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn find_by_id(&self, _id: Uuid) -> Result<JobRow, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn list_enabled(&self) -> Result<Vec<JobRow>, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn list_by_type(
        &self,
        _job_type: &str,
        _include_disabled: bool,
    ) -> Result<Vec<JobRow>, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
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
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn delete(&self, _id: Uuid) -> Result<bool, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn record_run(
        &self,
        _id: Uuid,
        _last_run_at: DateTime<Utc>,
        _next_run_at: Option<DateTime<Utc>>,
    ) -> Result<JobRow, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn claim_due_jobs(
        &self,
        _job_type: &str,
        _now: DateTime<Utc>,
        _stale_before: DateTime<Utc>,
        _limit: i64,
    ) -> Result<Vec<JobRow>, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn release_claim(&self, _id: Uuid) -> Result<(), StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }
}

// -- PgJobRunRepo --

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
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn complete(
        &self,
        _id: Uuid,
        _status: &str,
        _output: Option<&str>,
        _finished_at: DateTime<Utc>,
    ) -> Result<JobRunRow, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn list_by_job(
        &self,
        _job_id: Uuid,
        _limit: Option<i64>,
    ) -> Result<Vec<JobRunRow>, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }
}

// -- PgApprovalRepo --

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
    async fn create(&self, _approval: NewApproval) -> Result<ApprovalRow, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn find_by_id(&self, _id: Uuid) -> Result<ApprovalRow, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn list(&self, _pending_only: bool) -> Result<Vec<ApprovalRow>, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn decide(
        &self,
        _id: Uuid,
        _decision: &str,
        _decided_by: &str,
        _decided_at: DateTime<Utc>,
    ) -> Result<ApprovalRow, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn update_presented_payload(
        &self,
        _id: Uuid,
        _presented_payload: serde_json::Value,
    ) -> Result<ApprovalRow, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }
}

// -- PgToolApprovalPolicyRepo --

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
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn get_policy(&self, _tool_name: &str) -> Result<Option<ToolApprovalPolicy>, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn set_policy(
        &self,
        _tool_name: &str,
        _decision: &str,
    ) -> Result<ToolApprovalPolicy, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn clear_policy(&self, _tool_name: &str) -> Result<bool, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }
}

// -- PgToolExecutionRepo --

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
    async fn create(&self, _execution: NewToolExecution) -> Result<ToolExecutionRow, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn find_by_id(&self, _id: Uuid) -> Result<ToolExecutionRow, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn list_recent(&self, _limit: i64) -> Result<Vec<ToolExecutionRow>, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn complete(
        &self,
        _id: Uuid,
        _status: &str,
        _result_summary: Option<&str>,
        _error_summary: Option<&str>,
        _ended_at: DateTime<Utc>,
    ) -> Result<ToolExecutionRow, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }
}

// -- PgProcessHandleRepo --

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
    async fn create(&self, _handle: NewProcessHandle) -> Result<ProcessHandleRow, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn find_by_id(&self, _process_id: Uuid) -> Result<ProcessHandleRow, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn update_status(
        &self,
        _process_id: Uuid,
        _status: &str,
        _exit_code: Option<i32>,
        _ended_at: Option<DateTime<Utc>>,
    ) -> Result<ProcessHandleRow, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn list_by_session(
        &self,
        _session_id: Uuid,
    ) -> Result<Vec<ProcessHandleRow>, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn list_active(&self) -> Result<Vec<ProcessHandleRow>, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn find_by_tool_call_id(
        &self,
        _tool_call_id: Uuid,
    ) -> Result<Option<ProcessHandleRow>, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn find_by_tool_execution_id(
        &self,
        _tool_execution_id: Uuid,
    ) -> Result<Vec<ProcessHandleRow>, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }
}

// -- PgDeviceRepo --

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
        _file_path: &str,
        _chunk_index: i32,
        _chunk_text: &str,
    ) -> Result<(), StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }
}

#[async_trait]
impl MemoryEmbeddingRepo for PgMemoryEmbeddingRepo {
    async fn upsert_chunk(
        &self,
        _file_path: &str,
        _chunk_index: i32,
        _chunk_text: &str,
        _embedding: &[f32],
    ) -> Result<(), StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn delete_by_file(&self, _file_path: &str) -> Result<usize, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn keyword_search(
        &self,
        _query: &str,
        _limit: i64,
    ) -> Result<Vec<KeywordSearchRow>, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn vector_search(
        &self,
        _embedding: &[f32],
        _limit: i64,
    ) -> Result<Vec<VectorSearchRow>, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn count(&self) -> Result<i64, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn list_indexed_files(&self) -> Result<Vec<String>, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }
}

#[async_trait]
impl DeviceRepo for PgDeviceRepo {
    async fn create_device(&self, _device: NewPairedDevice) -> Result<PairedDeviceRow, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn find_device_by_id(&self, _id: Uuid) -> Result<PairedDeviceRow, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn find_device_by_token_hash(
        &self,
        _token_hash: &str,
    ) -> Result<Option<PairedDeviceRow>, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn find_device_by_public_key(
        &self,
        _public_key: &str,
    ) -> Result<Option<PairedDeviceRow>, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn list_devices(&self) -> Result<Vec<PairedDeviceRow>, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn update_token(
        &self,
        _id: Uuid,
        _token_hash: &str,
        _token_expires_at: DateTime<Utc>,
    ) -> Result<PairedDeviceRow, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn update_role(
        &self,
        _id: Uuid,
        _role: &str,
        _scopes: serde_json::Value,
    ) -> Result<PairedDeviceRow, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn touch_last_seen(
        &self,
        _id: Uuid,
        _last_seen_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn delete_device(&self, _id: Uuid) -> Result<bool, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn create_pairing_request(
        &self,
        _request: NewPairingRequest,
    ) -> Result<PairingRequestRow, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn take_pairing_request(
        &self,
        _id: Uuid,
    ) -> Result<Option<PairingRequestRow>, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn delete_pairing_request(&self, _id: Uuid) -> Result<bool, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn list_pending_requests(&self) -> Result<Vec<PairingRequestRow>, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }

    async fn prune_expired_requests(&self) -> Result<usize, StoreError> {
        todo!("Phase 2: rewrite with tokio-postgres")
    }
}
