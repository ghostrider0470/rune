//! Diesel-mapped row structs for insert and query.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::schema::*;

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
