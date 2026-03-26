//! Cosmos DB implementation of [`ProcessHandleRepo`](crate::repos::ProcessHandleRepo).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::cosmos::{collect_query, pk, CosmosStore};
use crate::error::StoreError;
use crate::models::{NewProcessHandle, ProcessHandleRow};
use crate::repos::ProcessHandleRepo;
use azure_data_cosmos::PartitionKey;

/// Cosmos document representation for a process handle.
#[derive(Debug, Serialize, Deserialize)]
struct ProcessHandleDoc {
    id: String,
    pk: String,
    #[serde(rename = "type")]
    doc_type: String,
    process_id: Uuid,
    tool_call_id: Uuid,
    session_id: Uuid,
    command: String,
    cwd: String,
    status: String,
    exit_code: Option<i32>,
    started_at: DateTime<Utc>,
    ended_at: Option<DateTime<Utc>>,
    execution_mode: Option<String>,
    tool_execution_id: Option<Uuid>,
}

impl From<ProcessHandleDoc> for ProcessHandleRow {
    fn from(d: ProcessHandleDoc) -> Self {
        Self {
            process_id: d.process_id,
            tool_call_id: d.tool_call_id,
            session_id: d.session_id,
            command: d.command,
            cwd: d.cwd,
            status: d.status,
            exit_code: d.exit_code,
            started_at: d.started_at,
            ended_at: d.ended_at,
            execution_mode: d.execution_mode,
            tool_execution_id: d.tool_execution_id,
        }
    }
}

impl ProcessHandleDoc {
    fn from_new(h: NewProcessHandle) -> Self {
        Self {
            id: h.process_id.to_string(),
            pk: h.session_id.to_string(),
            doc_type: "process_handle".to_string(),
            process_id: h.process_id,
            tool_call_id: h.tool_call_id,
            session_id: h.session_id,
            command: h.command,
            cwd: h.cwd,
            status: h.status,
            exit_code: None,
            started_at: h.started_at,
            ended_at: None,
            execution_mode: h.execution_mode,
            tool_execution_id: h.tool_execution_id,
        }
    }
}

fn process_row_to_doc(row: &ProcessHandleRow) -> ProcessHandleDoc {
    ProcessHandleDoc {
        id: row.process_id.to_string(),
        pk: row.session_id.to_string(),
        doc_type: "process_handle".to_string(),
        process_id: row.process_id,
        tool_call_id: row.tool_call_id,
        session_id: row.session_id,
        command: row.command.clone(),
        cwd: row.cwd.clone(),
        status: row.status.clone(),
        exit_code: row.exit_code,
        started_at: row.started_at,
        ended_at: row.ended_at,
        execution_mode: row.execution_mode.clone(),
        tool_execution_id: row.tool_execution_id,
    }
}

/// Read a process handle by ID. Cross-partition since we may not know session_id.
async fn read_process(store: &CosmosStore, process_id: Uuid) -> Result<ProcessHandleDoc, StoreError> {
    let query = format!(
        "SELECT * FROM c WHERE c.type = 'process_handle' AND c.process_id = '{}'",
        process_id
    );
    let stream = store
        .container()
        .query_items::<serde_json::Value>(&query, PartitionKey::EMPTY, None)
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let docs: Vec<ProcessHandleDoc> = collect_query(stream).await?;
    docs.into_iter().next().ok_or(StoreError::NotFound {
        entity: "process_handle",
        id: process_id.to_string(),
    })
}

#[async_trait]
impl ProcessHandleRepo for CosmosStore {
    async fn create(&self, handle: NewProcessHandle) -> Result<ProcessHandleRow, StoreError> {
        let doc = ProcessHandleDoc::from_new(handle);
        self.container()
            .upsert_item(pk(&doc.pk), &doc, None)
            .await?;
        Ok(ProcessHandleRow::from(doc))
    }

    async fn find_by_id(&self, process_id: Uuid) -> Result<ProcessHandleRow, StoreError> {
        let doc = read_process(self, process_id).await?;
        Ok(ProcessHandleRow::from(doc))
    }

    async fn update_status(
        &self,
        process_id: Uuid,
        status: &str,
        exit_code: Option<i32>,
        ended_at: Option<DateTime<Utc>>,
    ) -> Result<ProcessHandleRow, StoreError> {
        let doc = read_process(self, process_id).await?;
        let mut row = ProcessHandleRow::from(doc);
        row.status = status.to_string();
        row.exit_code = exit_code;
        row.ended_at = ended_at;

        let updated = process_row_to_doc(&row);
        self.container()
            .upsert_item(pk(&updated.pk), &updated, None)
            .await?;
        Ok(row)
    }

    async fn list_by_session(
        &self,
        session_id: Uuid,
    ) -> Result<Vec<ProcessHandleRow>, StoreError> {
        let pk_val = session_id.to_string();
        let query =
            "SELECT * FROM c WHERE c.type = 'process_handle' ORDER BY c.started_at DESC";
        let stream = self
            .container()
            .query_items::<serde_json::Value>(query, pk(&pk_val), None)
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let docs: Vec<ProcessHandleDoc> = collect_query(stream).await?;
        Ok(docs.into_iter().map(ProcessHandleRow::from).collect())
    }

    async fn list_active(&self) -> Result<Vec<ProcessHandleRow>, StoreError> {
        let query =
            "SELECT * FROM c WHERE c.type = 'process_handle' \
             AND c.status IN ('running', 'backgrounded')";
        let stream = self
            .container()
            .query_items::<serde_json::Value>(query, PartitionKey::EMPTY, None)
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let docs: Vec<ProcessHandleDoc> = collect_query(stream).await?;
        Ok(docs.into_iter().map(ProcessHandleRow::from).collect())
    }

    async fn find_by_tool_call_id(
        &self,
        tool_call_id: Uuid,
    ) -> Result<Option<ProcessHandleRow>, StoreError> {
        let query = format!(
            "SELECT * FROM c WHERE c.type = 'process_handle' AND c.tool_call_id = '{}'",
            tool_call_id
        );
        let stream = self
            .container()
            .query_items::<serde_json::Value>(&query, PartitionKey::EMPTY, None)
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let docs: Vec<ProcessHandleDoc> = collect_query(stream).await?;
        Ok(docs.into_iter().next().map(ProcessHandleRow::from))
    }

    async fn find_by_tool_execution_id(
        &self,
        tool_execution_id: Uuid,
    ) -> Result<Vec<ProcessHandleRow>, StoreError> {
        let query = format!(
            "SELECT * FROM c WHERE c.type = 'process_handle' AND c.tool_execution_id = '{}'",
            tool_execution_id
        );
        let stream = self
            .container()
            .query_items::<serde_json::Value>(&query, PartitionKey::EMPTY, None)
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let docs: Vec<ProcessHandleDoc> = collect_query(stream).await?;
        Ok(docs.into_iter().map(ProcessHandleRow::from).collect())
    }
}
