//! Diesel-mapped row structs for insert and query.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::schema::*;
use diesel::sql_types::{Float8, Int4, Text, Timestamptz};

// ── Sessions ──────────────────────────────────────────────────────────

/// A session row as returned by queries.
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = sessions)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct SessionRow {
    pub id: Uuid,
    pub kind: String,
    pub status: String,
    pub workspace_root: Option<String>,
    pub channel_ref: Option<String>,
    pub requester_session_id: Option<Uuid>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_activity_at: DateTime<Utc>,
}

/// Insert payload for a new session.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = sessions)]
pub struct NewSession {
    pub id: Uuid,
    pub kind: String,
    pub status: String,
    pub workspace_root: Option<String>,
    pub channel_ref: Option<String>,
    pub requester_session_id: Option<Uuid>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_activity_at: DateTime<Utc>,
}

// ── Turns ─────────────────────────────────────────────────────────────

/// A turn row as returned by queries.
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = turns)]
#[diesel(check_for_backend(diesel::pg::Pg))]
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
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = turns)]
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
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = transcript_items)]
#[diesel(check_for_backend(diesel::pg::Pg))]
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
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = transcript_items)]
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
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = jobs)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct JobRow {
    pub id: Uuid,
    pub job_type: String,
    pub schedule: Option<String>,
    pub due_at: Option<DateTime<Utc>>,
    pub enabled: bool,
    pub last_run_at: Option<DateTime<Utc>>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Insert payload for a new job.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = jobs)]
pub struct NewJob {
    pub id: Uuid,
    pub job_type: String,
    pub schedule: Option<String>,
    pub due_at: Option<DateTime<Utc>>,
    pub enabled: bool,
    pub payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ── Job runs ────────────────────────────────────────────────────────────────

/// A durable job-run row.
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = job_runs)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct JobRunRow {
    pub id: Uuid,
    pub job_id: Uuid,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub status: String,
    pub output: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Insert payload for a durable job-run record.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = job_runs)]
pub struct NewJobRun {
    pub id: Uuid,
    pub job_id: Uuid,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub status: String,
    pub output: Option<String>,
    pub created_at: DateTime<Utc>,
}

// ── Approvals ─────────────────────────────────────────────────────────

/// An approval row.
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = approvals)]
#[diesel(check_for_backend(diesel::pg::Pg))]
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
}

/// Insert payload for a new approval.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = approvals)]
pub struct NewApproval {
    pub id: Uuid,
    pub subject_type: String,
    pub subject_id: Uuid,
    pub reason: String,
    pub presented_payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

// ── Tool executions ───────────────────────────────────────────────────

/// A tool execution row.
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = tool_executions)]
#[diesel(check_for_backend(diesel::pg::Pg))]
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
}

/// Insert payload for a new tool execution.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = tool_executions)]
pub struct NewToolExecution {
    pub id: Uuid,
    pub tool_call_id: Uuid,
    pub session_id: Uuid,
    pub turn_id: Uuid,
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub status: String,
    pub started_at: DateTime<Utc>,
}

// ── Device pairing ───────────────────────────────────────────────────────────

/// A paired device row.
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = paired_devices)]
#[diesel(check_for_backend(diesel::pg::Pg))]
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
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = paired_devices)]
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
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = pairing_requests)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct PairingRequestRow {
    pub id: Uuid,
    pub device_name: String,
    pub public_key: String,
    pub challenge: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// Insert payload for a new pairing request.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = pairing_requests)]
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
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = memory_embeddings)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct MemoryEmbeddingRow {
    pub id: Uuid,
    pub file_path: String,
    pub chunk_index: i32,
    pub chunk_text: String,
    pub created_at: DateTime<Utc>,
}

/// Result row from keyword search raw SQL.
#[derive(Debug, Clone, QueryableByName, Serialize, Deserialize)]
pub struct KeywordSearchRow {
    #[diesel(sql_type = Text)]
    pub file_path: String,
    #[diesel(sql_type = Text)]
    pub chunk_text: String,
    #[diesel(sql_type = Float8)]
    pub score: f64,
}

/// Result row from vector search raw SQL.
#[derive(Debug, Clone, QueryableByName, Serialize, Deserialize)]
pub struct VectorSearchRow {
    #[diesel(sql_type = Text)]
    pub file_path: String,
    #[diesel(sql_type = Text)]
    pub chunk_text: String,
    #[diesel(sql_type = Float8)]
    pub score: f64,
}

/// Result row for listing distinct indexed files.
#[derive(Debug, Clone, QueryableByName, Serialize, Deserialize)]
pub struct IndexedFileRow {
    #[diesel(sql_type = Text)]
    pub file_path: String,
}

/// Result row for raw COUNT(*) queries over memory embeddings.
#[derive(Debug, Clone, QueryableByName, Serialize, Deserialize)]
pub struct CountRow {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    pub count: i64,
}

/// Result row for loading a memory embedding row via raw SQL.
#[derive(Debug, Clone, QueryableByName, Serialize, Deserialize)]
pub struct MemoryEmbeddingByNameRow {
    #[diesel(sql_type = diesel::sql_types::Uuid)]
    pub id: Uuid,
    #[diesel(sql_type = Text)]
    pub file_path: String,
    #[diesel(sql_type = Int4)]
    pub chunk_index: i32,
    #[diesel(sql_type = Text)]
    pub chunk_text: String,
    #[diesel(sql_type = Timestamptz)]
    pub created_at: DateTime<Utc>,
}

// ── Channel deliveries ────────────────────────────────────────────────

/// A channel delivery row.
#[derive(Debug, Clone, Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = channel_deliveries)]
#[diesel(check_for_backend(diesel::pg::Pg))]
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
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = channel_deliveries)]
pub struct NewChannelDelivery {
    pub id: Uuid,
    pub channel: String,
    pub destination: String,
    pub source_session_id: Option<Uuid>,
    pub message_kind: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
}
