//! Cosmos DB implementation of [`SessionRepo`](crate::repos::SessionRepo).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::cosmos::{parse_doc, pk, CosmosStore};
use crate::error::StoreError;
use crate::models::{NewSession, SessionRow};
use crate::repos::SessionRepo;

/// Cosmos document representation for a session.
#[derive(Debug, Serialize, Deserialize)]
struct SessionDoc {
    id: String,
    pk: String,
    #[serde(rename = "type")]
    doc_type: String,
    session_id: Uuid,
    kind: String,
    status: String,
    workspace_root: Option<String>,
    channel_ref: Option<String>,
    requester_session_id: Option<Uuid>,
    latest_turn_id: Option<Uuid>,
    runtime_profile: Option<String>,
    policy_profile: Option<String>,
    metadata: serde_json::Value,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    last_activity_at: DateTime<Utc>,
}

impl From<SessionDoc> for SessionRow {
    fn from(d: SessionDoc) -> Self {
        Self {
            id: d.session_id,
            kind: d.kind,
            status: d.status,
            workspace_root: d.workspace_root,
            channel_ref: d.channel_ref,
            requester_session_id: d.requester_session_id,
            latest_turn_id: d.latest_turn_id,
            runtime_profile: d.runtime_profile,
            policy_profile: d.policy_profile,
            metadata: d.metadata,
            created_at: d.created_at,
            updated_at: d.updated_at,
            last_activity_at: d.last_activity_at,
        }
    }
}

impl SessionDoc {
    fn from_new(s: NewSession) -> Self {
        let sid = s.id.to_string();
        Self {
            id: sid.clone(),
            pk: sid,
            doc_type: "session".to_string(),
            session_id: s.id,
            kind: s.kind,
            status: s.status,
            workspace_root: s.workspace_root,
            channel_ref: s.channel_ref,
            requester_session_id: s.requester_session_id,
            latest_turn_id: s.latest_turn_id,
            runtime_profile: s.runtime_profile,
            policy_profile: s.policy_profile,
            metadata: s.metadata,
            created_at: s.created_at,
            updated_at: s.updated_at,
            last_activity_at: s.last_activity_at,
        }
    }
}

fn session_row_to_doc(row: &SessionRow) -> SessionDoc {
    let sid = row.id.to_string();
    SessionDoc {
        id: sid.clone(),
        pk: sid,
        doc_type: "session".to_string(),
        session_id: row.id,
        kind: row.kind.clone(),
        status: row.status.clone(),
        workspace_root: row.workspace_root.clone(),
        channel_ref: row.channel_ref.clone(),
        requester_session_id: row.requester_session_id,
        latest_turn_id: row.latest_turn_id,
        runtime_profile: row.runtime_profile.clone(),
        policy_profile: row.policy_profile.clone(),
        metadata: row.metadata.clone(),
        created_at: row.created_at,
        updated_at: row.updated_at,
        last_activity_at: row.last_activity_at,
    }
}

#[async_trait]
impl SessionRepo for CosmosStore {
    async fn create(&self, session: NewSession) -> Result<SessionRow, StoreError> {
        let doc = SessionDoc::from_new(session);
        self.container()
            .upsert_item(pk(&doc.pk), &doc, None)
            .await?;
        Ok(SessionRow::from(doc))
    }

    async fn find_by_id(&self, id: Uuid) -> Result<SessionRow, StoreError> {
        let sid = id.to_string();
        let resp = self
            .container()
            .read_item::<serde_json::Value>(pk(&sid), &sid, None)
            .await
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("NotFound") || msg.contains("404") {
                    StoreError::NotFound {
                        entity: "session",
                        id: sid.clone(),
                    }
                } else {
                    StoreError::Database(msg)
                }
            })?;
        let doc: SessionDoc = parse_doc(resp.into_model().map_err(|e| StoreError::Database(e.to_string()))?)?;
        Ok(SessionRow::from(doc))
    }

    async fn list(&self, limit: i64, _offset: i64) -> Result<Vec<SessionRow>, StoreError> {
        // Cosmos TOP max is ~2 billion; cap to avoid overflow errors from callers passing i64::MAX.
        let capped = limit.min(100_000);
        let query = format!(
            "SELECT TOP {} * FROM c WHERE c.type = 'session' ORDER BY c.created_at DESC",
            capped
        );
        let docs: Vec<SessionDoc> = self.query_cross_partition(&query).await?;
        Ok(docs.into_iter().map(SessionRow::from).collect())
    }

    async fn find_by_channel_ref(
        &self,
        channel_ref: &str,
    ) -> Result<Option<SessionRow>, StoreError> {
        let query = format!(
            "SELECT * FROM c WHERE c.type = 'session' AND c.channel_ref = '{}' \
             AND c.status NOT IN ('completed', 'failed', 'cancelled') \
             ORDER BY c.created_at DESC",
            channel_ref.replace('\'', "''")
        );
        let docs: Vec<SessionDoc> = self.query_cross_partition(&query).await?;
        Ok(docs.into_iter().next().map(SessionRow::from))
    }

    async fn update_status(
        &self,
        id: Uuid,
        status: &str,
        updated_at: DateTime<Utc>,
    ) -> Result<SessionRow, StoreError> {
        let mut row = self.find_by_id(id).await?;

        // Validate FSM transition.
        let target: rune_core::SessionStatus = status
            .parse()
            .map_err(|e: rune_core::CoreError| StoreError::InvalidTransition(e.to_string()))?;
        let current: rune_core::SessionStatus = row
            .status
            .parse()
            .map_err(|e: rune_core::CoreError| StoreError::InvalidTransition(e.to_string()))?;
        if !current.can_transition_to(&target) {
            return Err(StoreError::InvalidTransition(format!(
                "{} -> {}",
                row.status, status
            )));
        }

        row.status = status.to_string();
        row.updated_at = updated_at;
        row.last_activity_at = updated_at;

        let doc = session_row_to_doc(&row);
        self.container()
            .upsert_item(pk(&doc.pk), &doc, None)
            .await?;
        Ok(row)
    }

    async fn update_metadata(
        &self,
        id: Uuid,
        metadata: serde_json::Value,
        updated_at: DateTime<Utc>,
    ) -> Result<SessionRow, StoreError> {
        let mut row = self.find_by_id(id).await?;
        row.metadata = metadata;
        row.updated_at = updated_at;
        row.last_activity_at = updated_at;

        let doc = session_row_to_doc(&row);
        self.container()
            .upsert_item(pk(&doc.pk), &doc, None)
            .await?;
        Ok(row)
    }

    async fn update_latest_turn(
        &self,
        id: Uuid,
        turn_id: Uuid,
        updated_at: DateTime<Utc>,
    ) -> Result<SessionRow, StoreError> {
        let mut row = self.find_by_id(id).await?;
        row.latest_turn_id = Some(turn_id);
        row.updated_at = updated_at;
        row.last_activity_at = updated_at;

        let doc = session_row_to_doc(&row);
        self.container()
            .upsert_item(pk(&doc.pk), &doc, None)
            .await?;
        Ok(row)
    }

    async fn delete(&self, id: Uuid) -> Result<bool, StoreError> {
        let sid = id.to_string();
        match self
            .container()
            .delete_item(pk(&sid), &sid, None)
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

    async fn list_active_channel_sessions(&self) -> Result<Vec<SessionRow>, StoreError> {
        let query =
            "SELECT * FROM c WHERE c.type = 'session' AND IS_DEFINED(c.channel_ref) \
             AND c.channel_ref != null \
             AND c.status NOT IN ('completed', 'failed', 'cancelled')";
        let docs: Vec<SessionDoc> = self.query_cross_partition(query).await?;
        Ok(docs.into_iter().map(SessionRow::from).collect())
    }

    async fn mark_stale_completed(&self, stale_secs: i64) -> Result<u64, StoreError> {
        let cutoff = Utc::now() - chrono::Duration::seconds(stale_secs);
        let cutoff_str = cutoff.to_rfc3339();
        let query = format!(
            "SELECT * FROM c WHERE c.type = 'session' AND c.status = 'running' \
             AND c.last_activity_at < '{}'",
            cutoff_str
        );
        let docs: Vec<SessionDoc> = self.query_cross_partition(&query).await?;
        let count = docs.len() as u64;
        let now = Utc::now();
        for doc in docs {
            let mut row = SessionRow::from(doc);
            row.status = "completed".to_string();
            row.updated_at = now;
            row.last_activity_at = now;
            let updated_doc = session_row_to_doc(&row);
            self.container()
                .upsert_item(pk(&updated_doc.pk), &updated_doc, None)
                .await?;
        }
        Ok(count)
    }
}
