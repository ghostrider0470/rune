use async_trait::async_trait;
use chrono::Utc;
use rune_store::StoreError;
use rune_store::models::{NewToolExecution, ToolExecutionRow};
use rune_store::repos::ToolExecutionRepo;
use tokio::sync::Mutex;
use uuid::Uuid;

pub struct InMemoryToolExecutionRepo {
    rows: Mutex<Vec<ToolExecutionRow>>,
}

impl Default for InMemoryToolExecutionRepo {
    fn default() -> Self {
        Self {
            rows: Mutex::new(Vec::new()),
        }
    }
}

impl InMemoryToolExecutionRepo {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ToolExecutionRepo for InMemoryToolExecutionRepo {
    async fn create(&self, execution: NewToolExecution) -> Result<ToolExecutionRow, StoreError> {
        let row = ToolExecutionRow {
            id: execution.id,
            tool_call_id: execution.tool_call_id,
            session_id: execution.session_id,
            turn_id: execution.turn_id,
            tool_name: execution.tool_name,
            arguments: execution.arguments,
            status: execution.status,
            result_summary: None,
            error_summary: None,
            started_at: execution.started_at,
            ended_at: None,
            approval_id: execution.approval_id,
            execution_mode: execution.execution_mode,
        };
        self.rows.lock().await.push(row.clone());
        Ok(row)
    }

    async fn find_by_id(&self, id: Uuid) -> Result<ToolExecutionRow, StoreError> {
        self.rows
            .lock()
            .await
            .iter()
            .find(|row| row.id == id)
            .cloned()
            .ok_or(StoreError::NotFound {
                entity: "tool_execution",
                id: id.to_string(),
            })
    }

    async fn list_recent(&self, limit: i64) -> Result<Vec<ToolExecutionRow>, StoreError> {
        let mut rows = self.rows.lock().await.clone();
        rows.sort_by_key(|row| std::cmp::Reverse(row.started_at));
        let capped = limit.max(0) as usize;
        rows.truncate(capped);
        Ok(rows)
    }

    async fn complete(
        &self,
        id: Uuid,
        status: &str,
        result_summary: Option<&str>,
        error_summary: Option<&str>,
        ended_at: chrono::DateTime<Utc>,
    ) -> Result<ToolExecutionRow, StoreError> {
        let mut rows = self.rows.lock().await;
        let row = rows
            .iter_mut()
            .find(|row| row.id == id)
            .ok_or(StoreError::NotFound {
                entity: "tool_execution",
                id: id.to_string(),
            })?;
        row.status = status.to_string();
        row.result_summary = result_summary.map(str::to_string);
        row.error_summary = error_summary.map(str::to_string);
        row.ended_at = Some(ended_at);
        Ok(row.clone())
    }
}
