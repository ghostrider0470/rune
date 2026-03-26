//! Cosmos DB implementation of [`ToolExecutionRepo`](crate::repos::ToolExecutionRepo).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::cosmos::{collect_query, pk, CosmosStore};
use crate::error::StoreError;
use crate::models::{NewToolExecution, ToolExecutionRow};
use crate::repos::ToolExecutionRepo;
use azure_data_cosmos::PartitionKey;

/// Cosmos document representation for a tool execution.
#[derive(Debug, Serialize, Deserialize)]
struct ToolExecutionDoc {
    id: String,
    pk: String,
    #[serde(rename = "type")]
    doc_type: String,
    execution_id: Uuid,
    tool_call_id: Uuid,
    session_id: Uuid,
    turn_id: Uuid,
    tool_name: String,
    arguments: serde_json::Value,
    status: String,
    result_summary: Option<String>,
    error_summary: Option<String>,
    started_at: DateTime<Utc>,
    ended_at: Option<DateTime<Utc>>,
    approval_id: Option<Uuid>,
    execution_mode: Option<String>,
}

impl From<ToolExecutionDoc> for ToolExecutionRow {
    fn from(d: ToolExecutionDoc) -> Self {
        Self {
            id: d.execution_id,
            tool_call_id: d.tool_call_id,
            session_id: d.session_id,
            turn_id: d.turn_id,
            tool_name: d.tool_name,
            arguments: d.arguments,
            status: d.status,
            result_summary: d.result_summary,
            error_summary: d.error_summary,
            started_at: d.started_at,
            ended_at: d.ended_at,
            approval_id: d.approval_id,
            execution_mode: d.execution_mode,
        }
    }
}

impl ToolExecutionDoc {
    fn from_new(e: NewToolExecution) -> Self {
        Self {
            id: e.id.to_string(),
            pk: e.session_id.to_string(),
            doc_type: "tool_execution".to_string(),
            execution_id: e.id,
            tool_call_id: e.tool_call_id,
            session_id: e.session_id,
            turn_id: e.turn_id,
            tool_name: e.tool_name,
            arguments: e.arguments,
            status: e.status,
            result_summary: None,
            error_summary: None,
            started_at: e.started_at,
            ended_at: None,
            approval_id: e.approval_id,
            execution_mode: e.execution_mode,
        }
    }
}

fn tool_exec_row_to_doc(row: &ToolExecutionRow) -> ToolExecutionDoc {
    ToolExecutionDoc {
        id: row.id.to_string(),
        pk: row.session_id.to_string(),
        doc_type: "tool_execution".to_string(),
        execution_id: row.id,
        tool_call_id: row.tool_call_id,
        session_id: row.session_id,
        turn_id: row.turn_id,
        tool_name: row.tool_name.clone(),
        arguments: row.arguments.clone(),
        status: row.status.clone(),
        result_summary: row.result_summary.clone(),
        error_summary: row.error_summary.clone(),
        started_at: row.started_at,
        ended_at: row.ended_at,
        approval_id: row.approval_id,
        execution_mode: row.execution_mode.clone(),
    }
}

/// Read a tool execution by ID. Cross-partition since we may not know session_id.
async fn read_tool_execution(store: &CosmosStore, id: Uuid) -> Result<ToolExecutionDoc, StoreError> {
    let query = format!(
        "SELECT * FROM c WHERE c.type = 'tool_execution' AND c.execution_id = '{}'",
        id
    );
    let stream = store
        .container()
        .query_items::<serde_json::Value>(&query, PartitionKey::EMPTY, None)
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let docs: Vec<ToolExecutionDoc> = collect_query(stream).await?;
    docs.into_iter().next().ok_or(StoreError::NotFound {
        entity: "tool_execution",
        id: id.to_string(),
    })
}

#[async_trait]
impl ToolExecutionRepo for CosmosStore {
    async fn create(&self, execution: NewToolExecution) -> Result<ToolExecutionRow, StoreError> {
        let doc = ToolExecutionDoc::from_new(execution);
        self.container()
            .upsert_item(pk(&doc.pk), &doc, None)
            .await?;
        Ok(ToolExecutionRow::from(doc))
    }

    async fn find_by_id(&self, id: Uuid) -> Result<ToolExecutionRow, StoreError> {
        let doc = read_tool_execution(self, id).await?;
        Ok(ToolExecutionRow::from(doc))
    }

    async fn list_recent(&self, limit: i64) -> Result<Vec<ToolExecutionRow>, StoreError> {
        let query = format!(
            "SELECT TOP {} * FROM c WHERE c.type = 'tool_execution' \
             ORDER BY c.started_at DESC",
            limit
        );
        let stream = self
            .container()
            .query_items::<serde_json::Value>(&query, PartitionKey::EMPTY, None)
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let docs: Vec<ToolExecutionDoc> = collect_query(stream).await?;
        Ok(docs.into_iter().map(ToolExecutionRow::from).collect())
    }

    async fn complete(
        &self,
        id: Uuid,
        status: &str,
        result_summary: Option<&str>,
        error_summary: Option<&str>,
        ended_at: DateTime<Utc>,
    ) -> Result<ToolExecutionRow, StoreError> {
        let doc = read_tool_execution(self, id).await?;
        let mut row = ToolExecutionRow::from(doc);
        row.status = status.to_string();
        row.result_summary = result_summary.map(|s| s.to_string());
        row.error_summary = error_summary.map(|s| s.to_string());
        row.ended_at = Some(ended_at);

        let updated = tool_exec_row_to_doc(&row);
        self.container()
            .upsert_item(pk(&updated.pk), &updated, None)
            .await?;
        Ok(row)
    }
}
