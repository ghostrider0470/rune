//! Cosmos DB implementation of [`TurnRepo`](crate::repos::TurnRepo).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::cosmos::{collect_query, pk, CosmosStore};
use crate::error::StoreError;
use crate::models::{NewTurn, TurnRow};
use crate::repos::TurnRepo;
use azure_data_cosmos::PartitionKey;

/// Cosmos document representation for a turn.
#[derive(Debug, Serialize, Deserialize)]
struct TurnDoc {
    id: String,
    pk: String,
    #[serde(rename = "type")]
    doc_type: String,
    turn_id: Uuid,
    session_id: Uuid,
    trigger_kind: String,
    status: String,
    model_ref: Option<String>,
    started_at: DateTime<Utc>,
    ended_at: Option<DateTime<Utc>>,
    usage_prompt_tokens: Option<i32>,
    usage_completion_tokens: Option<i32>,
}

impl From<TurnDoc> for TurnRow {
    fn from(d: TurnDoc) -> Self {
        Self {
            id: d.turn_id,
            session_id: d.session_id,
            trigger_kind: d.trigger_kind,
            status: d.status,
            model_ref: d.model_ref,
            started_at: d.started_at,
            ended_at: d.ended_at,
            usage_prompt_tokens: d.usage_prompt_tokens,
            usage_completion_tokens: d.usage_completion_tokens,
        }
    }
}

impl TurnDoc {
    fn from_new(t: NewTurn) -> Self {
        Self {
            id: t.id.to_string(),
            pk: t.session_id.to_string(),
            doc_type: "turn".to_string(),
            turn_id: t.id,
            session_id: t.session_id,
            trigger_kind: t.trigger_kind,
            status: t.status,
            model_ref: t.model_ref,
            started_at: t.started_at,
            ended_at: t.ended_at,
            usage_prompt_tokens: t.usage_prompt_tokens,
            usage_completion_tokens: t.usage_completion_tokens,
        }
    }
}

fn turn_row_to_doc(row: &TurnRow) -> TurnDoc {
    TurnDoc {
        id: row.id.to_string(),
        pk: row.session_id.to_string(),
        doc_type: "turn".to_string(),
        turn_id: row.id,
        session_id: row.session_id,
        trigger_kind: row.trigger_kind.clone(),
        status: row.status.clone(),
        model_ref: row.model_ref.clone(),
        started_at: row.started_at,
        ended_at: row.ended_at,
        usage_prompt_tokens: row.usage_prompt_tokens,
        usage_completion_tokens: row.usage_completion_tokens,
    }
}

/// Read a turn by ID. We need the session_id for partition key, so we do a cross-partition query.
async fn read_turn(store: &CosmosStore, id: Uuid) -> Result<TurnDoc, StoreError> {
    let query = format!(
        "SELECT * FROM c WHERE c.type = 'turn' AND c.turn_id = '{}'",
        id
    );
    let stream = store
        .container()
        .query_items::<serde_json::Value>(&query, PartitionKey::EMPTY, None)
        .map_err(|e| StoreError::Database(e.to_string()))?;
    let docs: Vec<TurnDoc> = collect_query(stream).await?;
    docs.into_iter().next().ok_or(StoreError::NotFound {
        entity: "turn",
        id: id.to_string(),
    })
}

#[async_trait]
impl TurnRepo for CosmosStore {
    async fn create(&self, turn: NewTurn) -> Result<TurnRow, StoreError> {
        let doc = TurnDoc::from_new(turn);
        self.container()
            .upsert_item(pk(&doc.pk), &doc, None)
            .await?;
        Ok(TurnRow::from(doc))
    }

    async fn find_by_id(&self, id: Uuid) -> Result<TurnRow, StoreError> {
        let doc = read_turn(self, id).await?;
        Ok(TurnRow::from(doc))
    }

    async fn list_by_session(&self, session_id: Uuid) -> Result<Vec<TurnRow>, StoreError> {
        let pk_val = session_id.to_string();
        let query = "SELECT * FROM c WHERE c.type = 'turn' ORDER BY c.started_at ASC";
        let stream = self
            .container()
            .query_items::<serde_json::Value>(query, pk(&pk_val), None)
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let docs: Vec<TurnDoc> = collect_query(stream).await?;
        Ok(docs.into_iter().map(TurnRow::from).collect())
    }

    async fn update_status(
        &self,
        id: Uuid,
        status: &str,
        ended_at: Option<DateTime<Utc>>,
    ) -> Result<TurnRow, StoreError> {
        let doc = read_turn(self, id).await?;
        let mut row = TurnRow::from(doc);
        row.status = status.to_string();
        row.ended_at = ended_at.or(row.ended_at);

        let updated = turn_row_to_doc(&row);
        self.container()
            .upsert_item(pk(&updated.pk), &updated, None)
            .await?;
        Ok(row)
    }

    async fn update_usage(
        &self,
        id: Uuid,
        prompt_tokens: i32,
        completion_tokens: i32,
    ) -> Result<TurnRow, StoreError> {
        let doc = read_turn(self, id).await?;
        let mut row = TurnRow::from(doc);
        row.usage_prompt_tokens = Some(prompt_tokens);
        row.usage_completion_tokens = Some(completion_tokens);

        let updated = turn_row_to_doc(&row);
        self.container()
            .upsert_item(pk(&updated.pk), &updated, None)
            .await?;
        Ok(row)
    }

    async fn mark_stale_failed(&self, stale_secs: i64) -> Result<u64, StoreError> {
        let cutoff = Utc::now() - chrono::Duration::seconds(stale_secs);
        let cutoff_str = cutoff.to_rfc3339();
        let query = format!(
            "SELECT * FROM c WHERE c.type = 'turn' \
             AND c.status IN ('started', 'model_calling', 'tool_executing') \
             AND c.started_at < '{}'",
            cutoff_str
        );
        let stream = self
            .container()
            .query_items::<serde_json::Value>(&query, PartitionKey::EMPTY, None)
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let docs: Vec<TurnDoc> = collect_query(stream).await?;
        let count = docs.len() as u64;
        let now = Utc::now();
        for doc in docs {
            let mut row = TurnRow::from(doc);
            row.status = "failed".to_string();
            row.ended_at = Some(now);
            let updated = turn_row_to_doc(&row);
            self.container()
                .upsert_item(pk(&updated.pk), &updated, None)
                .await?;
        }
        Ok(count)
    }
}
