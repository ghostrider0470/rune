//! Cosmos DB implementation of [`JobRunRepo`](crate::repos::JobRunRepo).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::cosmos::{CosmosStore, collect_query, pk};
use crate::error::StoreError;
use crate::models::{JobRunRow, NewJobRun};
use crate::repos::JobRunRepo;

/// Cosmos document representation for a job run.
#[derive(Debug, Serialize, Deserialize)]
struct JobRunDoc {
    id: String,
    pk: String,
    #[serde(rename = "type")]
    doc_type: String,
    run_id: Uuid,
    job_id: Uuid,
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
    trigger_kind: String,
    status: String,
    output: Option<String>,
    created_at: DateTime<Utc>,
}

impl From<JobRunDoc> for JobRunRow {
    fn from(d: JobRunDoc) -> Self {
        Self {
            id: d.run_id,
            job_id: d.job_id,
            started_at: d.started_at,
            finished_at: d.finished_at,
            trigger_kind: d.trigger_kind,
            status: d.status,
            output: d.output,
            created_at: d.created_at,
        }
    }
}

impl JobRunDoc {
    fn from_new(r: NewJobRun) -> Self {
        Self {
            id: r.id.to_string(),
            pk: format!("job:{}", r.job_id),
            doc_type: "job_run".to_string(),
            run_id: r.id,
            job_id: r.job_id,
            started_at: r.started_at,
            finished_at: r.finished_at,
            trigger_kind: r.trigger_kind,
            status: r.status,
            output: r.output,
            created_at: r.created_at,
        }
    }
}

fn job_run_row_to_doc(row: &JobRunRow) -> JobRunDoc {
    JobRunDoc {
        id: row.id.to_string(),
        pk: format!("job:{}", row.job_id),
        doc_type: "job_run".to_string(),
        run_id: row.id,
        job_id: row.job_id,
        started_at: row.started_at,
        finished_at: row.finished_at,
        trigger_kind: row.trigger_kind.clone(),
        status: row.status.clone(),
        output: row.output.clone(),
        created_at: row.created_at,
    }
}

/// Read a job run by ID. The run lives in `pk = job:{job_id}` but we may not know job_id,
/// so we use a cross-partition query.
async fn read_job_run(store: &CosmosStore, id: Uuid) -> Result<JobRunDoc, StoreError> {
    let query = format!(
        "SELECT * FROM c WHERE c.type = 'job_run' AND c.run_id = '{}'",
        id
    );
    let docs: Vec<JobRunDoc> = store.query_cross_partition(&query).await?;
    docs.into_iter().next().ok_or(StoreError::NotFound {
        entity: "job_run",
        id: id.to_string(),
    })
}

#[async_trait]
impl JobRunRepo for CosmosStore {
    async fn create(&self, run: NewJobRun) -> Result<JobRunRow, StoreError> {
        let doc = JobRunDoc::from_new(run);
        self.container()
            .upsert_item(pk(&doc.pk), &doc, None)
            .await?;
        Ok(JobRunRow::from(doc))
    }

    async fn complete(
        &self,
        id: Uuid,
        status: &str,
        output: Option<&str>,
        finished_at: DateTime<Utc>,
    ) -> Result<JobRunRow, StoreError> {
        let doc = read_job_run(self, id).await?;
        let mut row = JobRunRow::from(doc);
        row.status = status.to_string();
        row.output = output.map(|s| s.to_string());
        row.finished_at = Some(finished_at);

        let updated = job_run_row_to_doc(&row);
        self.container()
            .upsert_item(pk(&updated.pk), &updated, None)
            .await?;
        Ok(row)
    }

    async fn list_by_job(
        &self,
        job_id: Uuid,
        limit: Option<i64>,
    ) -> Result<Vec<JobRunRow>, StoreError> {
        let pk_val = format!("job:{}", job_id);
        let top_clause = match limit {
            Some(l) => format!("TOP {}", l),
            None => String::new(),
        };
        let query = format!(
            "SELECT {} * FROM c WHERE c.type = 'job_run' ORDER BY c.started_at DESC",
            top_clause
        );
        let stream = self
            .container()
            .query_items::<serde_json::Value>(&query, pk(&pk_val), None)
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let docs: Vec<JobRunDoc> = collect_query(stream).await?;
        Ok(docs.into_iter().map(JobRunRow::from).collect())
    }
}
