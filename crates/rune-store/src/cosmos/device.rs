//! Cosmos DB implementation of [`DeviceRepo`](crate::repos::DeviceRepo).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::cosmos::{CosmosStore, collect_query, parse_doc};
use crate::error::StoreError;
use crate::models::{NewPairedDevice, NewPairingRequest, PairedDeviceRow, PairingRequestRow};
use crate::repos::DeviceRepo;
use azure_data_cosmos::PartitionKey;

const PK_GLOBAL: &str = "global";

// ── Device document ──────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
struct DeviceDoc {
    id: String,
    pk: String,
    #[serde(rename = "type")]
    doc_type: String,
    device_id: Uuid,
    name: String,
    public_key: String,
    role: String,
    scopes: serde_json::Value,
    token_hash: String,
    token_expires_at: DateTime<Utc>,
    paired_at: DateTime<Utc>,
    last_seen_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl From<DeviceDoc> for PairedDeviceRow {
    fn from(d: DeviceDoc) -> Self {
        Self {
            id: d.device_id,
            name: d.name,
            public_key: d.public_key,
            role: d.role,
            scopes: d.scopes,
            token_hash: d.token_hash,
            token_expires_at: d.token_expires_at,
            paired_at: d.paired_at,
            last_seen_at: d.last_seen_at,
            created_at: d.created_at,
        }
    }
}

impl DeviceDoc {
    fn from_new(d: NewPairedDevice) -> Self {
        Self {
            id: d.id.to_string(),
            pk: PK_GLOBAL.to_string(),
            doc_type: "device".to_string(),
            device_id: d.id,
            name: d.name,
            public_key: d.public_key,
            role: d.role,
            scopes: d.scopes,
            token_hash: d.token_hash,
            token_expires_at: d.token_expires_at,
            paired_at: d.paired_at,
            last_seen_at: None,
            created_at: d.created_at,
        }
    }
}

fn device_row_to_doc(row: &PairedDeviceRow) -> DeviceDoc {
    DeviceDoc {
        id: row.id.to_string(),
        pk: PK_GLOBAL.to_string(),
        doc_type: "device".to_string(),
        device_id: row.id,
        name: row.name.clone(),
        public_key: row.public_key.clone(),
        role: row.role.clone(),
        scopes: row.scopes.clone(),
        token_hash: row.token_hash.clone(),
        token_expires_at: row.token_expires_at,
        paired_at: row.paired_at,
        last_seen_at: row.last_seen_at,
        created_at: row.created_at,
    }
}

// ── Pairing request document ─────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
struct PairingRequestDoc {
    id: String,
    pk: String,
    #[serde(rename = "type")]
    doc_type: String,
    request_id: Uuid,
    device_name: String,
    public_key: String,
    challenge: String,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

impl From<PairingRequestDoc> for PairingRequestRow {
    fn from(d: PairingRequestDoc) -> Self {
        Self {
            id: d.request_id,
            device_name: d.device_name,
            public_key: d.public_key,
            challenge: d.challenge,
            created_at: d.created_at,
            expires_at: d.expires_at,
        }
    }
}

impl PairingRequestDoc {
    fn from_new(r: NewPairingRequest) -> Self {
        Self {
            id: format!("pr:{}", r.id),
            pk: PK_GLOBAL.to_string(),
            doc_type: "pairing_request".to_string(),
            request_id: r.id,
            device_name: r.device_name,
            public_key: r.public_key,
            challenge: r.challenge,
            created_at: r.created_at,
            expires_at: r.expires_at,
        }
    }
}

#[async_trait]
impl DeviceRepo for CosmosStore {
    // ── Device operations ────────────────────────────────────────────

    async fn create_device(&self, device: NewPairedDevice) -> Result<PairedDeviceRow, StoreError> {
        let doc = DeviceDoc::from_new(device);
        self.container()
            .upsert_item(PartitionKey::from(PK_GLOBAL), &doc, None)
            .await?;
        Ok(PairedDeviceRow::from(doc))
    }

    async fn find_device_by_id(&self, id: Uuid) -> Result<PairedDeviceRow, StoreError> {
        let item_id = id.to_string();
        let resp = self
            .container()
            .read_item::<serde_json::Value>(PartitionKey::from(PK_GLOBAL), &item_id, None)
            .await
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("NotFound") || msg.contains("404") {
                    StoreError::NotFound {
                        entity: "device",
                        id: item_id.clone(),
                    }
                } else {
                    StoreError::Database(msg)
                }
            })?;
        let doc: DeviceDoc = parse_doc(
            resp.into_model()
                .map_err(|e| StoreError::Database(e.to_string()))?,
        )?;
        Ok(PairedDeviceRow::from(doc))
    }

    async fn find_device_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<PairedDeviceRow>, StoreError> {
        let query = format!(
            "SELECT * FROM c WHERE c.type = 'device' AND c.token_hash = '{}'",
            token_hash.replace('\'', "''")
        );
        let stream = self
            .container()
            .query_items::<serde_json::Value>(&query, PartitionKey::from(PK_GLOBAL), None)
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let docs: Vec<DeviceDoc> = collect_query(stream).await?;
        Ok(docs.into_iter().next().map(PairedDeviceRow::from))
    }

    async fn find_device_by_public_key(
        &self,
        public_key: &str,
    ) -> Result<Option<PairedDeviceRow>, StoreError> {
        let query = format!(
            "SELECT * FROM c WHERE c.type = 'device' AND c.public_key = '{}'",
            public_key.replace('\'', "''")
        );
        let stream = self
            .container()
            .query_items::<serde_json::Value>(&query, PartitionKey::from(PK_GLOBAL), None)
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let docs: Vec<DeviceDoc> = collect_query(stream).await?;
        Ok(docs.into_iter().next().map(PairedDeviceRow::from))
    }

    async fn list_devices(&self) -> Result<Vec<PairedDeviceRow>, StoreError> {
        let query = "SELECT * FROM c WHERE c.type = 'device' ORDER BY c.created_at DESC";
        let stream = self
            .container()
            .query_items::<serde_json::Value>(query, PartitionKey::from(PK_GLOBAL), None)
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let docs: Vec<DeviceDoc> = collect_query(stream).await?;
        Ok(docs.into_iter().map(PairedDeviceRow::from).collect())
    }

    async fn update_token(
        &self,
        id: Uuid,
        token_hash: &str,
        token_expires_at: DateTime<Utc>,
    ) -> Result<PairedDeviceRow, StoreError> {
        let mut row = self.find_device_by_id(id).await?;
        row.token_hash = token_hash.to_string();
        row.token_expires_at = token_expires_at;

        let doc = device_row_to_doc(&row);
        self.container()
            .upsert_item(PartitionKey::from(PK_GLOBAL), &doc, None)
            .await?;
        Ok(row)
    }

    async fn update_role(
        &self,
        id: Uuid,
        role: &str,
        scopes: serde_json::Value,
    ) -> Result<PairedDeviceRow, StoreError> {
        let mut row = self.find_device_by_id(id).await?;
        row.role = role.to_string();
        row.scopes = scopes;

        let doc = device_row_to_doc(&row);
        self.container()
            .upsert_item(PartitionKey::from(PK_GLOBAL), &doc, None)
            .await?;
        Ok(row)
    }

    async fn touch_last_seen(
        &self,
        id: Uuid,
        last_seen_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let mut row = self.find_device_by_id(id).await?;
        row.last_seen_at = Some(last_seen_at);

        let doc = device_row_to_doc(&row);
        self.container()
            .upsert_item(PartitionKey::from(PK_GLOBAL), &doc, None)
            .await?;
        Ok(())
    }

    async fn delete_device(&self, id: Uuid) -> Result<bool, StoreError> {
        let item_id = id.to_string();
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

    // ── Pairing request operations ───────────────────────────────────

    async fn create_pairing_request(
        &self,
        request: NewPairingRequest,
    ) -> Result<PairingRequestRow, StoreError> {
        let doc = PairingRequestDoc::from_new(request);
        self.container()
            .upsert_item(PartitionKey::from(PK_GLOBAL), &doc, None)
            .await?;
        Ok(PairingRequestRow::from(doc))
    }

    async fn take_pairing_request(
        &self,
        id: Uuid,
    ) -> Result<Option<PairingRequestRow>, StoreError> {
        let item_id = format!("pr:{}", id);
        match self
            .container()
            .read_item::<serde_json::Value>(PartitionKey::from(PK_GLOBAL), &item_id, None)
            .await
        {
            Ok(resp) => {
                let doc: PairingRequestDoc = parse_doc(
                    resp.into_model()
                        .map_err(|e| StoreError::Database(e.to_string()))?,
                )?;
                let row = PairingRequestRow::from(doc);
                // Delete the document after reading (consume on use).
                let _ = self
                    .container()
                    .delete_item(PartitionKey::from(PK_GLOBAL), &item_id, None)
                    .await;
                Ok(Some(row))
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

    async fn delete_pairing_request(&self, id: Uuid) -> Result<bool, StoreError> {
        let item_id = format!("pr:{}", id);
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

    async fn list_pending_requests(&self) -> Result<Vec<PairingRequestRow>, StoreError> {
        let now_str = Utc::now().to_rfc3339();
        let query = format!(
            "SELECT * FROM c WHERE c.type = 'pairing_request' AND c.expires_at > '{}'",
            now_str
        );
        let stream = self
            .container()
            .query_items::<serde_json::Value>(&query, PartitionKey::from(PK_GLOBAL), None)
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let docs: Vec<PairingRequestDoc> = collect_query(stream).await?;
        Ok(docs.into_iter().map(PairingRequestRow::from).collect())
    }

    async fn prune_expired_requests(&self) -> Result<usize, StoreError> {
        let now_str = Utc::now().to_rfc3339();
        let query = format!(
            "SELECT c.id FROM c WHERE c.type = 'pairing_request' AND c.expires_at <= '{}'",
            now_str
        );
        let stream = self
            .container()
            .query_items::<serde_json::Value>(&query, PartitionKey::from(PK_GLOBAL), None)
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let ids: Vec<serde_json::Value> = collect_query(stream).await?;
        let mut count = 0usize;
        for val in &ids {
            if let Some(doc_id) = val.get("id").and_then(|v| v.as_str()) {
                self.container()
                    .delete_item(PartitionKey::from(PK_GLOBAL), doc_id, None)
                    .await?;
                count += 1;
            }
        }
        Ok(count)
    }
}
