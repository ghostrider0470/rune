use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Durable metadata recorded for a background exec launch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessAuditRecord {
    pub process_id: String,
    pub tool_call_id: Uuid,
    pub tool_execution_id: Uuid,
    pub session_id: Option<Uuid>,
    pub turn_id: Option<Uuid>,
    pub tool_name: String,
    pub command: String,
    pub workdir: String,
    pub arguments: Value,
    pub status: String,
    pub result_summary: Option<String>,
    pub error_summary: Option<String>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
}

/// Request payload for a newly launched background process.
#[derive(Debug, Clone)]
pub struct NewProcessAudit {
    pub process_id: String,
    pub tool_call_id: Uuid,
    pub session_id: Option<Uuid>,
    pub turn_id: Option<Uuid>,
    pub tool_name: String,
    pub command: String,
    pub workdir: String,
    pub arguments: Value,
    pub started_at: DateTime<Utc>,
}

/// Completion payload for a background process.
#[derive(Debug, Clone)]
pub struct CompletedProcessAudit {
    pub process_id: String,
    pub status: String,
    pub result_summary: Option<String>,
    pub error_summary: Option<String>,
    pub ended_at: DateTime<Utc>,
}

/// Persistence/lookup contract for durable background-process metadata.
#[async_trait]
pub trait ProcessAuditStore: Send + Sync {
    async fn record_spawn(&self, spawn: NewProcessAudit) -> Result<ProcessAuditRecord, String>;
    async fn record_completion(
        &self,
        completion: CompletedProcessAudit,
    ) -> Result<ProcessAuditRecord, String>;
    async fn find(&self, process_id: &str) -> Result<Option<ProcessAuditRecord>, String>;
    async fn list_recent(&self, limit: usize) -> Result<Vec<ProcessAuditRecord>, String>;
}
