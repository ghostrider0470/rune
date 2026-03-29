//! Cosmos DB implementation of [`ToolApprovalPolicyRepo`](crate::repos::ToolApprovalPolicyRepo).

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::cosmos::{CosmosStore, collect_query, parse_doc};
use crate::error::StoreError;
use crate::repos::{ToolApprovalPolicy, ToolApprovalPolicyRepo};
use azure_data_cosmos::PartitionKey;

const PK_GLOBAL: &str = "global";

/// Cosmos document representation for a tool approval policy.
#[derive(Debug, Serialize, Deserialize)]
struct ToolPolicyDoc {
    id: String,
    pk: String,
    #[serde(rename = "type")]
    doc_type: String,
    tool_name: String,
    decision: String,
    decided_at: chrono::DateTime<chrono::Utc>,
}

impl From<ToolPolicyDoc> for ToolApprovalPolicy {
    fn from(d: ToolPolicyDoc) -> Self {
        Self {
            tool_name: d.tool_name,
            decision: d.decision,
            decided_at: d.decided_at,
        }
    }
}

#[async_trait]
impl ToolApprovalPolicyRepo for CosmosStore {
    async fn list_policies(&self) -> Result<Vec<ToolApprovalPolicy>, StoreError> {
        let query = "SELECT * FROM c WHERE c.type = 'tool_policy'";
        let stream = self
            .container()
            .query_items::<serde_json::Value>(query, PartitionKey::from(PK_GLOBAL), None)
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let docs: Vec<ToolPolicyDoc> = collect_query(stream).await?;
        Ok(docs.into_iter().map(ToolApprovalPolicy::from).collect())
    }

    async fn get_policy(&self, tool_name: &str) -> Result<Option<ToolApprovalPolicy>, StoreError> {
        let item_id = format!("policy:{}", tool_name);
        match self
            .container()
            .read_item::<serde_json::Value>(PartitionKey::from(PK_GLOBAL), &item_id, None)
            .await
        {
            Ok(resp) => {
                let doc: ToolPolicyDoc = parse_doc(
                    resp.into_model()
                        .map_err(|e| StoreError::Database(e.to_string()))?,
                )?;
                Ok(Some(ToolApprovalPolicy::from(doc)))
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("NotFound") || msg.contains("404") {
                    Ok(None)
                } else {
                    Err(StoreError::Database(msg))
                }
            }
        }
    }

    async fn set_policy(
        &self,
        tool_name: &str,
        decision: &str,
    ) -> Result<ToolApprovalPolicy, StoreError> {
        let doc = ToolPolicyDoc {
            id: format!("policy:{}", tool_name),
            pk: PK_GLOBAL.to_string(),
            doc_type: "tool_policy".to_string(),
            tool_name: tool_name.to_string(),
            decision: decision.to_string(),
            decided_at: Utc::now(),
        };
        self.container()
            .upsert_item(PartitionKey::from(PK_GLOBAL), &doc, None)
            .await?;
        Ok(ToolApprovalPolicy::from(doc))
    }

    async fn clear_policy(&self, tool_name: &str) -> Result<bool, StoreError> {
        let item_id = format!("policy:{}", tool_name);
        match self
            .container()
            .delete_item(PartitionKey::from(PK_GLOBAL), &item_id, None)
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
}
