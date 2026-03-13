//! Diesel-mapped row structs for insert and query.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::schema::*;

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
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_activity_at: DateTime<Utc>,
}

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
