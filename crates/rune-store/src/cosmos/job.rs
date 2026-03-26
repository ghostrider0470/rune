//! Cosmos DB implementation of [`JobRepo`](crate::repos::JobRepo).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::cosmos::{collect_query, parse_doc, pk, CosmosStore};
use crate::error::StoreError;
use crate::models::{JobRow, NewJob};
use crate::repos::JobRepo;
use azure_data_cosmos::PartitionKey;

/// Cosmos document representation for a job.
#[derive(Debug, Serialize, Deserialize)]
struct JobDoc {
    id: String,
    pk: String,
    #[serde(rename = "type")]
    doc_type: String,
    job_id: Uuid,
    job_type: String,
    schedule: Option<String>,
    due_at: Option<DateTime<Utc>>,
    enabled: bool,
    last_run_at: Option<DateTime<Utc>>,
    next_run_at: Option<DateTime<Utc>>,
    payload_kind: String,
    delivery_mode: String,
    payload: serde_json::Value,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    claimed_at: Option<DateTime<Utc>>,
}

impl From<JobDoc> for JobRow {
    fn from(d: JobDoc) -> Self {
        Self {
            id: d.job_id,
            job_type: d.job_type,
            schedule: d.schedule,
            due_at: d.due_at,
            enabled: d.enabled,
            last_run_at: d.last_run_at,
            next_run_at: d.next_run_at,
            payload_kind: d.payload_kind,
            delivery_mode: d.delivery_mode,
            payload: d.payload,
            created_at: d.created_at,
            updated_at: d.updated_at,
            claimed_at: d.claimed_at,
        }
    }
}

impl JobDoc {
    fn from_new(j: NewJob) -> Self {
        Self {
            id: j.id.to_string(),
            pk: format!("job:{}", j.id),
            doc_type: "job".to_string(),
            job_id: j.id,
            job_type: j.job_type,
            schedule: j.schedule,
            due_at: j.due_at,
            enabled: j.enabled,
            last_run_at: None,
            next_run_at: j.next_run_at,
            payload_kind: j.payload_kind,
            delivery_mode: j.delivery_mode,
            payload: j.payload,
            created_at: j.created_at,
            updated_at: j.updated_at,
            claimed_at: None,
        }
    }
}

fn job_row_to_doc(row: &JobRow) -> JobDoc {
    JobDoc {
        id: row.id.to_string(),
        pk: format!("job:{}", row.id),
        doc_type: "job".to_string(),
        job_id: row.id,
        job_type: row.job_type.clone(),
        schedule: row.schedule.clone(),
        due_at: row.due_at,
        enabled: row.enabled,
        last_run_at: row.last_run_at,
        next_run_at: row.next_run_at,
        payload_kind: row.payload_kind.clone(),
        delivery_mode: row.delivery_mode.clone(),
        payload: row.payload.clone(),
        created_at: row.created_at,
        updated_at: row.updated_at,
        claimed_at: row.claimed_at,
    }
}

async fn read_job(store: &CosmosStore, id: Uuid) -> Result<JobDoc, StoreError> {
    let pk_val = format!("job:{}", id);
    let item_id = id.to_string();
    let resp = store
        .container()
        .read_item::<serde_json::Value>(pk(&pk_val), &item_id, None)
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("NotFound") || msg.contains("404") {
                StoreError::NotFound {
                    entity: "job",
                    id: item_id.clone(),
                }
            } else {
                StoreError::Database(msg)
            }
        })?;
    parse_doc(resp.into_model().map_err(|e| StoreError::Database(e.to_string()))?)
}

#[async_trait]
impl JobRepo for CosmosStore {
    async fn create(&self, job: NewJob) -> Result<JobRow, StoreError> {
        let doc = JobDoc::from_new(job);
        self.container()
            .upsert_item(pk(&doc.pk), &doc, None)
            .await?;
        Ok(JobRow::from(doc))
    }

    async fn find_by_id(&self, id: Uuid) -> Result<JobRow, StoreError> {
        let doc = read_job(self, id).await?;
        Ok(JobRow::from(doc))
    }

    async fn list_enabled(&self) -> Result<Vec<JobRow>, StoreError> {
        let query = "SELECT * FROM c WHERE c.type = 'job' AND c.enabled = true";
        let stream = self
            .container()
            .query_items::<serde_json::Value>(query, PartitionKey::EMPTY, None)
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let docs: Vec<JobDoc> = collect_query(stream).await?;
        Ok(docs.into_iter().map(JobRow::from).collect())
    }

    async fn list_by_type(
        &self,
        job_type: &str,
        include_disabled: bool,
    ) -> Result<Vec<JobRow>, StoreError> {
        let query = if include_disabled {
            format!(
                "SELECT * FROM c WHERE c.type = 'job' AND c.job_type = '{}'",
                job_type.replace('\'', "''")
            )
        } else {
            format!(
                "SELECT * FROM c WHERE c.type = 'job' AND c.job_type = '{}' AND c.enabled = true",
                job_type.replace('\'', "''")
            )
        };
        let stream = self
            .container()
            .query_items::<serde_json::Value>(&query, PartitionKey::EMPTY, None)
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let docs: Vec<JobDoc> = collect_query(stream).await?;
        Ok(docs.into_iter().map(JobRow::from).collect())
    }

    async fn update_job(
        &self,
        id: Uuid,
        enabled: bool,
        due_at: Option<DateTime<Utc>>,
        payload_kind: &str,
        delivery_mode: &str,
        payload: serde_json::Value,
        updated_at: DateTime<Utc>,
        last_run_at: Option<DateTime<Utc>>,
        next_run_at: Option<DateTime<Utc>>,
    ) -> Result<JobRow, StoreError> {
        let doc = read_job(self, id).await?;
        let mut row = JobRow::from(doc);
        row.enabled = enabled;
        row.due_at = due_at;
        row.payload_kind = payload_kind.to_string();
        row.delivery_mode = delivery_mode.to_string();
        row.payload = payload;
        row.updated_at = updated_at;
        row.last_run_at = last_run_at;
        row.next_run_at = next_run_at;

        let updated = job_row_to_doc(&row);
        self.container()
            .upsert_item(pk(&updated.pk), &updated, None)
            .await?;
        Ok(row)
    }

    async fn delete(&self, id: Uuid) -> Result<bool, StoreError> {
        let pk_val = format!("job:{}", id);
        let item_id = id.to_string();
        match self
            .container()
            .delete_item(pk(&pk_val), &item_id, None)
            .await
        {
            Ok(_) => Ok(true),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("NotFound") || msg.contains("404") {
                    Ok(false)
                } else {
                    Err(StoreError::Database(msg))
                }
            }
        }
    }

    async fn record_run(
        &self,
        id: Uuid,
        last_run_at: DateTime<Utc>,
        next_run_at: Option<DateTime<Utc>>,
    ) -> Result<JobRow, StoreError> {
        let doc = read_job(self, id).await?;
        let mut row = JobRow::from(doc);
        row.last_run_at = Some(last_run_at);
        row.next_run_at = next_run_at;
        row.updated_at = Utc::now();

        let updated = job_row_to_doc(&row);
        self.container()
            .upsert_item(pk(&updated.pk), &updated, None)
            .await?;
        Ok(row)
    }

    async fn claim_due_jobs(
        &self,
        job_type: &str,
        now: DateTime<Utc>,
        stale_before: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<JobRow>, StoreError> {
        let now_str = now.to_rfc3339();
        let stale_str = stale_before.to_rfc3339();
        // Cross-partition query for enabled jobs that are due.
        let query = format!(
            "SELECT * FROM c WHERE c.type = 'job' \
             AND c.enabled = true \
             AND c.job_type = '{}' \
             AND c.next_run_at <= '{}' \
             AND (NOT IS_DEFINED(c.claimed_at) OR c.claimed_at = null OR c.claimed_at < '{}')",
            job_type.replace('\'', "''"),
            now_str,
            stale_str
        );
        let stream = self
            .container()
            .query_items::<serde_json::Value>(&query, PartitionKey::EMPTY, None)
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let docs: Vec<JobDoc> = collect_query(stream).await?;

        let mut claimed = Vec::new();
        for doc in docs.into_iter().take(limit as usize) {
            let mut row = JobRow::from(doc);
            row.claimed_at = Some(now);
            let updated = job_row_to_doc(&row);
            self.container()
                .upsert_item(pk(&updated.pk), &updated, None)
                .await?;
            claimed.push(row);
        }
        Ok(claimed)
    }

    async fn release_claim(&self, id: Uuid) -> Result<(), StoreError> {
        let doc = read_job(self, id).await?;
        let mut row = JobRow::from(doc);
        row.claimed_at = None;
        let updated = job_row_to_doc(&row);
        self.container()
            .upsert_item(pk(&updated.pk), &updated, None)
            .await?;
        Ok(())
    }
}
