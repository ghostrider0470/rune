//! Cosmos DB implementation of [`TranscriptRepo`](crate::repos::TranscriptRepo).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::cosmos::{CosmosStore, collect_query, pk};
use crate::error::StoreError;
use crate::models::{NewTranscriptItem, TranscriptItemRow};
use crate::repos::TranscriptRepo;

/// Cosmos document representation for a transcript item.
#[derive(Debug, Serialize, Deserialize)]
struct TranscriptDoc {
    id: String,
    pk: String,
    #[serde(rename = "type")]
    doc_type: String,
    item_id: Uuid,
    session_id: Uuid,
    turn_id: Option<Uuid>,
    seq: i32,
    kind: String,
    payload: serde_json::Value,
    created_at: DateTime<Utc>,
}

impl From<TranscriptDoc> for TranscriptItemRow {
    fn from(d: TranscriptDoc) -> Self {
        Self {
            id: d.item_id,
            session_id: d.session_id,
            turn_id: d.turn_id,
            seq: d.seq,
            kind: d.kind,
            payload: d.payload,
            created_at: d.created_at,
        }
    }
}

impl TranscriptDoc {
    fn from_new(item: NewTranscriptItem) -> Self {
        Self {
            id: item.id.to_string(),
            pk: item.session_id.to_string(),
            doc_type: "transcript_item".to_string(),
            item_id: item.id,
            session_id: item.session_id,
            turn_id: item.turn_id,
            seq: item.seq,
            kind: item.kind,
            payload: item.payload,
            created_at: item.created_at,
        }
    }
}

#[async_trait]
impl TranscriptRepo for CosmosStore {
    async fn append(&self, item: NewTranscriptItem) -> Result<TranscriptItemRow, StoreError> {
        let doc = TranscriptDoc::from_new(item);
        self.container()
            .upsert_item(pk(&doc.pk), &doc, None)
            .await?;
        Ok(TranscriptItemRow::from(doc))
    }

    async fn list_by_session(
        &self,
        session_id: Uuid,
    ) -> Result<Vec<TranscriptItemRow>, StoreError> {
        let pk_val = session_id.to_string();
        let query = "SELECT * FROM c WHERE c.type = 'transcript_item' ORDER BY c.seq ASC";
        let stream = self
            .container()
            .query_items::<serde_json::Value>(query, pk(&pk_val), None)
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let docs: Vec<TranscriptDoc> = collect_query(stream).await?;
        Ok(docs.into_iter().map(TranscriptItemRow::from).collect())
    }

    async fn delete_by_session(&self, session_id: Uuid) -> Result<usize, StoreError> {
        let pk_val = session_id.to_string();
        let query = "SELECT c.id FROM c WHERE c.type = 'transcript_item'";
        let stream = self
            .container()
            .query_items::<serde_json::Value>(query, pk(&pk_val), None)
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let ids: Vec<serde_json::Value> = collect_query(stream).await?;
        let mut count = 0usize;
        for val in &ids {
            if let Some(doc_id) = val.get("id").and_then(|v| v.as_str()) {
                self.container()
                    .delete_item(pk(&pk_val), doc_id, None)
                    .await?;
                count += 1;
            }
        }
        Ok(count)
    }
}
