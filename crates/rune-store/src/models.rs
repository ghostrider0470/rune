//! Row structs for insert and query.
//!
//! These are plain data types usable by any backend. The Diesel derives
//! have been removed as part of the tokio-postgres migration.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// -- Sessions --

/// A session row as returned by queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRow {
    pub id: Uuid,
    pub kind: String,
    pub status: String,
    pub workspace_root: Option<String>,
    pub channel_ref: Option<String>,
    pub requester_session_id: Option<Uuid>,
    pub latest_turn_id: Option<Uuid>,
    pub runtime_profile: Option<String>,
    pub policy_profile: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_activity_at: DateTime<Utc>,
}

/// Insert payload for a new session.
#[derive(Debug, Clone)]
pub struct NewSession {
    pub id: Uuid,
    pub kind: String,
    pub status: String,
    pub workspace_root: Option<String>,
    pub channel_ref: Option<String>,
    pub requester_session_id: Option<Uuid>,
    pub latest_turn_id: Option<Uuid>,
    pub runtime_profile: Option<String>,
    pub policy_profile: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_activity_at: DateTime<Utc>,
}

// -- Turns --

/// A turn row as returned by queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub usage_cached_prompt_tokens: Option<i32>,
}

/// Insert payload for a new turn.
#[derive(Debug, Clone)]
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
    pub usage_cached_prompt_tokens: Option<i32>,
}

// -- Transcript items --

/// A transcript item row.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub struct NewTranscriptItem {
    pub id: Uuid,
    pub session_id: Uuid,
    pub turn_id: Option<Uuid>,
    pub seq: i32,
    pub kind: String,
    pub payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

// -- Jobs --

/// A job row.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub struct NewJob {
    pub id: Uuid,
    pub job_type: String,
    pub schedule: Option<String>,
    pub due_at: Option<DateTime<Utc>>,
    pub enabled: bool,
    pub next_run_at: Option<DateTime<Utc>>,
    pub payload_kind: String,
    pub delivery_mode: String,
    pub payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// -- Job runs --

/// A durable job-run row.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

// -- Approvals --

/// An approval row.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

// -- Tool executions --

/// A tool execution row.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

// -- Device pairing --

/// A paired device row.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub struct NewPairingRequest {
    pub id: Uuid,
    pub device_name: String,
    pub public_key: String,
    pub challenge: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

// -- Memory embeddings --

/// A persisted memory embedding chunk row (excluding the vector column).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEmbeddingRow {
    pub id: Uuid,
    pub file_path: String,
    pub chunk_index: i32,
    pub chunk_text: String,
    pub created_at: DateTime<Utc>,
}

/// Result row from keyword search raw SQL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeywordSearchRow {
    pub file_path: String,
    pub chunk_text: String,
    pub score: f64,
}

/// Result row from vector search raw SQL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorSearchRow {
    pub file_path: String,
    pub chunk_text: String,
    pub score: f64,
}

/// Result row for listing distinct indexed files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedFileRow {
    pub file_path: String,
}

/// Result row for raw COUNT(*) queries over memory embeddings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountRow {
    pub count: i64,
}

/// Result row for loading a memory embedding row via raw SQL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEmbeddingByNameRow {
    pub id: Uuid,
    pub file_path: String,
    pub chunk_index: i32,
    pub chunk_text: String,
    pub created_at: DateTime<Utc>,
}

// -- Memory facts (Mem0) --

/// A persisted semantic memory fact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryFact {
    pub id: Uuid,
    pub fact: String,
    pub category: String,
    pub source_session_id: Option<Uuid>,
    pub source_agent: Option<String>,
    pub trigger: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub access_count: i32,
}

/// An edge in the memory fact similarity graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryFactEdge {
    pub source: Uuid,
    pub target: Uuid,
    pub similarity: f64,
}

// -- Channel deliveries --

/// A channel delivery row.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub struct NewChannelDelivery {
    pub id: Uuid,
    pub channel: String,
    pub destination: String,
    pub source_session_id: Option<Uuid>,
    pub message_kind: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

// -- Process handles --

/// A durable process handle row as returned by queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
