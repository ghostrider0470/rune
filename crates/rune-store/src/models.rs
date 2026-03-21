//! Row structs for insert and query.
//!
//! When the `postgres` feature is active, Diesel derives are included.
//! Otherwise the structs are plain data types usable by any backend.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg(feature = "postgres")]
use diesel::prelude::*;

#[cfg(feature = "postgres")]
use crate::schema::*;
#[cfg(feature = "postgres")]
use diesel::sql_types::{Float8, Int4, Text, Timestamptz};

// ── Sessions ──────────────────────────────────────────────────────────

/// A session row as returned by queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(Queryable, Selectable))]
#[cfg_attr(feature = "postgres", diesel(table_name = sessions))]
#[cfg_attr(feature = "postgres", diesel(check_for_backend(diesel::pg::Pg)))]
pub struct SessionRow {
    pub id: Uuid,
    pub kind: String,
    pub status: String,
    pub workspace_root: Option<String>,
    pub channel_ref: Option<String>,
    pub requester_session_id: Option<Uuid>,
    pub latest_turn_id: Option<Uuid>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_activity_at: DateTime<Utc>,
}

/// Insert payload for a new session.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "postgres", derive(Insertable))]
#[cfg_attr(feature = "postgres", diesel(table_name = sessions))]
pub struct NewSession {
    pub id: Uuid,
    pub kind: String,
    pub status: String,
    pub workspace_root: Option<String>,
    pub channel_ref: Option<String>,
    pub requester_session_id: Option<Uuid>,
    pub latest_turn_id: Option<Uuid>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_activity_at: DateTime<Utc>,
}

// ── Turns ─────────────────────────────────────────────────────────────

/// A turn row as returned by queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(Queryable, Selectable))]
#[cfg_attr(feature = "postgres", diesel(table_name = turns))]
#[cfg_attr(feature = "postgres", diesel(check_for_backend(diesel::pg::Pg)))]
pub struct TurnRow {
    pub id: Uuid,
    pub session_id: Uuid,
    pub trigger_kind: String,
    pub status: String,
    pub model_ref: Option<String>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub usage_prompt_tokens: Option<i32>,
    pub usage_completion_tokens: Option<i32>,
}

/// Insert payload for a new turn.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "postgres", derive(Insertable))]
#[cfg_attr(feature = "postgres", diesel(table_name = turns))]
pub struct NewTurn {
    pub id: Uuid,
    pub session_id: Uuid,
    pub trigger_kind: String,
    pub status: String,
    pub model_ref: Option<String>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub usage_prompt_tokens: Option<i32>,
    pub usage_completion_tokens: Option<i32>,
}

// ── Transcript items ──────────────────────────────────────────────────

/// A transcript item row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(Queryable, Selectable))]
#[cfg_attr(feature = "postgres", diesel(table_name = transcript_items))]
#[cfg_attr(feature = "postgres", diesel(check_for_backend(diesel::pg::Pg)))]
pub struct TranscriptItemRow {
    pub id: Uuid,
    pub session_id: Uuid,
    pub turn_id: Option<Uuid>,
    pub seq: i32,
    pub kind: String,
    pub payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

/// Insert payload for a new transcript item.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "postgres", derive(Insertable))]
#[cfg_attr(feature = "postgres", diesel(table_name = transcript_items))]
pub struct NewTranscriptItem {
    pub id: Uuid,
    pub session_id: Uuid,
    pub turn_id: Option<Uuid>,
    pub seq: i32,
    pub kind: String,
    pub payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

// ── Jobs ──────────────────────────────────────────────────────────────

/// A job row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(Queryable, Selectable))]
#[cfg_attr(feature = "postgres", diesel(table_name = jobs))]
#[cfg_attr(feature = "postgres", diesel(check_for_backend(diesel::pg::Pg)))]
pub struct JobRow {
    pub id: Uuid,
    pub job_type: String,
    pub schedule: Option<String>,
    pub due_at: Option<DateTime<Utc>>,
    pub enabled: bool,
    pub last_run_at: Option<DateTime<Utc>>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub payload_kind: String,
    pub delivery_mode: String,
    pub payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Set when a supervisor claims this job for execution; NULL = unclaimed.
    pub claimed_at: Option<DateTime<Utc>>,
}

/// Insert payload for a new job.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "postgres", derive(Insertable))]
#[cfg_attr(feature = "postgres", diesel(table_name = jobs))]
pub struct NewJob {
    pub id: Uuid,
    pub job_type: String,
    pub schedule: Option<String>,
    pub due_at: Option<DateTime<Utc>>,
    pub enabled: bool,
    pub payload_kind: String,
    pub delivery_mode: String,
    pub payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ── Job runs ────────────────────────────────────────────────────────────────

/// A durable job-run row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(Queryable, Selectable))]
#[cfg_attr(feature = "postgres", diesel(table_name = job_runs))]
#[cfg_attr(feature = "postgres", diesel(check_for_backend(diesel::pg::Pg)))]
pub struct JobRunRow {
    pub id: Uuid,
    pub job_id: Uuid,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub trigger_kind: String,
    pub status: String,
    pub output: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Insert payload for a durable job-run record.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "postgres", derive(Insertable))]
#[cfg_attr(feature = "postgres", diesel(table_name = job_runs))]
pub struct NewJobRun {
    pub id: Uuid,
    pub job_id: Uuid,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub trigger_kind: String,
    pub status: String,
    pub output: Option<String>,
    pub created_at: DateTime<Utc>,
}

// ── Approvals ─────────────────────────────────────────────────────────

/// An approval row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(Queryable, Selectable))]
#[cfg_attr(feature = "postgres", diesel(table_name = approvals))]
#[cfg_attr(feature = "postgres", diesel(check_for_backend(diesel::pg::Pg)))]
pub struct ApprovalRow {
    pub id: Uuid,
    pub subject_type: String,
    pub subject_id: Uuid,
    pub reason: String,
    pub decision: Option<String>,
    pub decided_by: Option<String>,
    pub decided_at: Option<DateTime<Utc>>,
    pub presented_payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub handle_ref: Option<String>,
    pub host_ref: Option<String>,
}

/// Insert payload for a new approval.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "postgres", derive(Insertable))]
#[cfg_attr(feature = "postgres", diesel(table_name = approvals))]
pub struct NewApproval {
    pub id: Uuid,
    pub subject_type: String,
    pub subject_id: Uuid,
    pub reason: String,
    pub presented_payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub handle_ref: Option<String>,
    pub host_ref: Option<String>,
}

// ── Tool executions ───────────────────────────────────────────────────

/// A tool execution row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(Queryable, Selectable))]
#[cfg_attr(feature = "postgres", diesel(table_name = tool_executions))]
#[cfg_attr(feature = "postgres", diesel(check_for_backend(diesel::pg::Pg)))]
pub struct ToolExecutionRow {
    pub id: Uuid,
    pub tool_call_id: Uuid,
    pub session_id: Uuid,
    pub turn_id: Uuid,
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub status: String,
    pub result_summary: Option<String>,
    pub error_summary: Option<String>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub approval_id: Option<Uuid>,
    pub execution_mode: Option<String>,
}

/// Insert payload for a new tool execution.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "postgres", derive(Insertable))]
#[cfg_attr(feature = "postgres", diesel(table_name = tool_executions))]
pub struct NewToolExecution {
    pub id: Uuid,
    pub tool_call_id: Uuid,
    pub session_id: Uuid,
    pub turn_id: Uuid,
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub approval_id: Option<Uuid>,
    pub execution_mode: Option<String>,
}

// ── Device pairing ───────────────────────────────────────────────────────────

/// A paired device row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(Queryable, Selectable))]
#[cfg_attr(feature = "postgres", diesel(table_name = paired_devices))]
#[cfg_attr(feature = "postgres", diesel(check_for_backend(diesel::pg::Pg)))]
pub struct PairedDeviceRow {
    pub id: Uuid,
    pub name: String,
    pub public_key: String,
    pub role: String,
    pub scopes: serde_json::Value,
    pub token_hash: String,
    pub token_expires_at: DateTime<Utc>,
    pub paired_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Insert payload for a new paired device.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "postgres", derive(Insertable))]
#[cfg_attr(feature = "postgres", diesel(table_name = paired_devices))]
pub struct NewPairedDevice {
    pub id: Uuid,
    pub name: String,
    pub public_key: String,
    pub role: String,
    pub scopes: serde_json::Value,
    pub token_hash: String,
    pub token_expires_at: DateTime<Utc>,
    pub paired_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

/// A pairing request row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(Queryable, Selectable))]
#[cfg_attr(feature = "postgres", diesel(table_name = pairing_requests))]
#[cfg_attr(feature = "postgres", diesel(check_for_backend(diesel::pg::Pg)))]
pub struct PairingRequestRow {
    pub id: Uuid,
    pub device_name: String,
    pub public_key: String,
    pub challenge: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// Insert payload for a new pairing request.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "postgres", derive(Insertable))]
#[cfg_attr(feature = "postgres", diesel(table_name = pairing_requests))]
pub struct NewPairingRequest {
    pub id: Uuid,
    pub device_name: String,
    pub public_key: String,
    pub challenge: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

// ── Memory embeddings ─────────────────────────────────────────────────

/// A persisted memory embedding chunk row (excluding the vector column).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(Queryable, Selectable))]
#[cfg_attr(feature = "postgres", diesel(table_name = memory_embeddings))]
#[cfg_attr(feature = "postgres", diesel(check_for_backend(diesel::pg::Pg)))]
pub struct MemoryEmbeddingRow {
    pub id: Uuid,
    pub file_path: String,
    pub chunk_index: i32,
    pub chunk_text: String,
    pub created_at: DateTime<Utc>,
}

/// Result row from keyword search raw SQL.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(QueryableByName))]
pub struct KeywordSearchRow {
    #[cfg_attr(feature = "postgres", diesel(sql_type = Text))]
    pub file_path: String,
    #[cfg_attr(feature = "postgres", diesel(sql_type = Text))]
    pub chunk_text: String,
    #[cfg_attr(feature = "postgres", diesel(sql_type = Float8))]
    pub score: f64,
}

/// Result row from vector search raw SQL.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(QueryableByName))]
pub struct VectorSearchRow {
    #[cfg_attr(feature = "postgres", diesel(sql_type = Text))]
    pub file_path: String,
    #[cfg_attr(feature = "postgres", diesel(sql_type = Text))]
    pub chunk_text: String,
    #[cfg_attr(feature = "postgres", diesel(sql_type = Float8))]
    pub score: f64,
}

/// Result row for listing distinct indexed files.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(QueryableByName))]
pub struct IndexedFileRow {
    #[cfg_attr(feature = "postgres", diesel(sql_type = Text))]
    pub file_path: String,
}

/// Result row for raw COUNT(*) queries over memory embeddings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(QueryableByName))]
pub struct CountRow {
    #[cfg_attr(feature = "postgres", diesel(sql_type = diesel::sql_types::BigInt))]
    pub count: i64,
}

/// Result row for loading a memory embedding row via raw SQL.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(QueryableByName))]
pub struct MemoryEmbeddingByNameRow {
    #[cfg_attr(feature = "postgres", diesel(sql_type = diesel::sql_types::Uuid))]
    pub id: Uuid,
    #[cfg_attr(feature = "postgres", diesel(sql_type = Text))]
    pub file_path: String,
    #[cfg_attr(feature = "postgres", diesel(sql_type = Int4))]
    pub chunk_index: i32,
    #[cfg_attr(feature = "postgres", diesel(sql_type = Text))]
    pub chunk_text: String,
    #[cfg_attr(feature = "postgres", diesel(sql_type = Timestamptz))]
    pub created_at: DateTime<Utc>,
}

// ── Channel deliveries ────────────────────────────────────────────────

/// A channel delivery row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(Queryable, Selectable))]
#[cfg_attr(feature = "postgres", diesel(table_name = channel_deliveries))]
#[cfg_attr(feature = "postgres", diesel(check_for_backend(diesel::pg::Pg)))]
pub struct ChannelDeliveryRow {
    pub id: Uuid,
    pub channel: String,
    pub destination: String,
    pub source_session_id: Option<Uuid>,
    pub message_kind: String,
    pub provider_message_id: Option<String>,
    pub attempt_count: i32,
    pub status: String,
    pub sent_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Insert payload for a new channel delivery.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "postgres", derive(Insertable))]
#[cfg_attr(feature = "postgres", diesel(table_name = channel_deliveries))]
pub struct NewChannelDelivery {
    pub id: Uuid,
    pub channel: String,
    pub destination: String,
    pub source_session_id: Option<Uuid>,
    pub message_kind: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

// ── Process handles ─────────────────────────────────────────────────────

/// A durable process handle row as returned by queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(Queryable, Selectable))]
#[cfg_attr(feature = "postgres", diesel(table_name = process_handles))]
#[cfg_attr(feature = "postgres", diesel(check_for_backend(diesel::pg::Pg)))]
pub struct ProcessHandleRow {
    pub process_id: Uuid,
    pub tool_call_id: Uuid,
    pub session_id: Uuid,
    pub command: String,
    pub cwd: String,
    pub status: String,
    pub exit_code: Option<i32>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub execution_mode: Option<String>,
    pub tool_execution_id: Option<Uuid>,
}

/// Insert payload for a new process handle.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "postgres", derive(Insertable))]
#[cfg_attr(feature = "postgres", diesel(table_name = process_handles))]
pub struct NewProcessHandle {
    pub process_id: Uuid,
    pub tool_call_id: Uuid,
    pub session_id: Uuid,
    pub command: String,
    pub cwd: String,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub execution_mode: Option<String>,
    pub tool_execution_id: Option<Uuid>,
}
