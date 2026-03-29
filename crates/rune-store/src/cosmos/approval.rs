//! Cosmos DB implementation of [`ApprovalRepo`](crate::repos::ApprovalRepo).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::cosmos::{CosmosStore, collect_query, parse_doc};
use crate::error::StoreError;
use crate::models::{ApprovalRow, NewApproval};
use crate::repos::ApprovalRepo;
use azure_data_cosmos::PartitionKey;

const PK_GLOBAL: &str = "global";

/// Cosmos document representation for an approval.
#[derive(Debug, Serialize, Deserialize)]
struct ApprovalDoc {
    id: String,
    pk: String,
    #[serde(rename = "type")]
    doc_type: String,
    approval_id: Uuid,
    subject_type: String,
    subject_id: Uuid,
    reason: String,
    decision: Option<String>,
    decided_by: Option<String>,
    decided_at: Option<DateTime<Utc>>,
    presented_payload: serde_json::Value,
    created_at: DateTime<Utc>,
    handle_ref: Option<String>,
    host_ref: Option<String>,
}

impl From<ApprovalDoc> for ApprovalRow {
    fn from(d: ApprovalDoc) -> Self {
        Self {
            id: d.approval_id,
            subject_type: d.subject_type,
            subject_id: d.subject_id,
            reason: d.reason,
            decision: d.decision,
            decided_by: d.decided_by,
            decided_at: d.decided_at,
            presented_payload: d.presented_payload,
            created_at: d.created_at,
            handle_ref: d.handle_ref,
            host_ref: d.host_ref,
        }
    }
}

impl ApprovalDoc {
    fn from_new(a: NewApproval) -> Self {
        Self {
            id: a.id.to_string(),
            pk: PK_GLOBAL.to_string(),
            doc_type: "approval".to_string(),
            approval_id: a.id,
            subject_type: a.subject_type,
            subject_id: a.subject_id,
            reason: a.reason,
            decision: None,
            decided_by: None,
            decided_at: None,
            presented_payload: a.presented_payload,
            created_at: a.created_at,
            handle_ref: a.handle_ref,
            host_ref: a.host_ref,
        }
    }
}

fn approval_row_to_doc(row: &ApprovalRow) -> ApprovalDoc {
    ApprovalDoc {
        id: row.id.to_string(),
        pk: PK_GLOBAL.to_string(),
        doc_type: "approval".to_string(),
        approval_id: row.id,
        subject_type: row.subject_type.clone(),
        subject_id: row.subject_id,
        reason: row.reason.clone(),
        decision: row.decision.clone(),
        decided_by: row.decided_by.clone(),
        decided_at: row.decided_at,
        presented_payload: row.presented_payload.clone(),
        created_at: row.created_at,
        handle_ref: row.handle_ref.clone(),
        host_ref: row.host_ref.clone(),
    }
}

#[async_trait]
impl ApprovalRepo for CosmosStore {
    async fn create(&self, approval: NewApproval) -> Result<ApprovalRow, StoreError> {
        let doc = ApprovalDoc::from_new(approval);
        self.container()
            .upsert_item(PartitionKey::from(PK_GLOBAL), &doc, None)
            .await?;
        Ok(ApprovalRow::from(doc))
    }

    async fn find_by_id(&self, id: Uuid) -> Result<ApprovalRow, StoreError> {
        let item_id = id.to_string();
        let resp = self
            .container()
            .read_item::<serde_json::Value>(PartitionKey::from(PK_GLOBAL), &item_id, None)
            .await
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("NotFound") || msg.contains("404") {
                    StoreError::NotFound {
                        entity: "approval",
                        id: item_id.clone(),
                    }
                } else {
                    StoreError::Database(msg)
                }
            })?;
        let doc: ApprovalDoc = parse_doc(
            resp.into_model()
                .map_err(|e| StoreError::Database(e.to_string()))?,
        )?;
        Ok(ApprovalRow::from(doc))
    }

    async fn list(&self, pending_only: bool) -> Result<Vec<ApprovalRow>, StoreError> {
        let query = if pending_only {
            "SELECT * FROM c WHERE c.type = 'approval' AND (NOT IS_DEFINED(c.decision) OR c.decision = null) ORDER BY c.created_at DESC"
        } else {
            "SELECT * FROM c WHERE c.type = 'approval' ORDER BY c.created_at DESC"
        };
        let stream = self
            .container()
            .query_items::<serde_json::Value>(query, PartitionKey::from(PK_GLOBAL), None)
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let docs: Vec<ApprovalDoc> = collect_query(stream).await?;
        Ok(docs.into_iter().map(ApprovalRow::from).collect())
    }

    async fn decide(
        &self,
        id: Uuid,
        decision: &str,
        decided_by: &str,
        decided_at: DateTime<Utc>,
    ) -> Result<ApprovalRow, StoreError> {
        let mut row = self.find_by_id(id).await?;
        row.decision = Some(decision.to_string());
        row.decided_by = Some(decided_by.to_string());
        row.decided_at = Some(decided_at);

        let doc = approval_row_to_doc(&row);
        self.container()
            .upsert_item(PartitionKey::from(PK_GLOBAL), &doc, None)
            .await?;
        Ok(row)
    }

    async fn update_presented_payload(
        &self,
        id: Uuid,
        presented_payload: serde_json::Value,
    ) -> Result<ApprovalRow, StoreError> {
        let mut row = self.find_by_id(id).await?;
        row.presented_payload = presented_payload;

        let doc = approval_row_to_doc(&row);
        self.container()
            .upsert_item(PartitionKey::from(PK_GLOBAL), &doc, None)
            .await?;
        Ok(row)
    }
}
