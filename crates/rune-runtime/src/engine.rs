use std::sync::Arc;

use chrono::Utc;
use uuid::Uuid;

use rune_core::{SessionId, SessionKind, SessionStatus};
use rune_store::StoreError;
use rune_store::models::{NewSession, SessionRow};
use rune_store::repos::{SessionRepo, TranscriptRepo};

use crate::error::RuntimeError;
use crate::session_metadata::{set_project_id, set_session_mode};

/// Creates and manages session lifecycle. Persists state via store repo traits.
pub struct SessionEngine {
    session_repo: Arc<dyn SessionRepo>,
    transcript_repo: Option<Arc<dyn TranscriptRepo>>,
}

impl SessionEngine {
    pub fn new(session_repo: Arc<dyn SessionRepo>) -> Self {
        Self {
            session_repo,
            transcript_repo: None,
        }
    }

    /// Expose the underlying session repo for direct operations (e.g. stale cleanup).
    pub fn session_repo(&self) -> &Arc<dyn SessionRepo> {
        &self.session_repo
    }

    pub fn with_transcript_repo(mut self, transcript_repo: Arc<dyn TranscriptRepo>) -> Self {
        self.transcript_repo = Some(transcript_repo);
        self
    }

    /// Create a new session with the given kind, optional workspace root,
    /// optional parent (requester) session, and optional channel reference.
    pub async fn create_session(
        &self,
        kind: SessionKind,
        workspace_root: Option<String>,
    ) -> Result<SessionRow, RuntimeError> {
        self.create_session_full(kind, workspace_root, None, None, None, None)
            .await
    }

    /// Create a new session with full linkage options.
    ///
    /// `requester_session_id` links this session to a parent (for subagent/scheduled sessions).
    /// `channel_ref` associates the session with a specific channel context.
    pub async fn create_session_full(
        &self,
        kind: SessionKind,
        workspace_root: Option<String>,
        requester_session_id: Option<Uuid>,
        channel_ref: Option<String>,
        mode: Option<String>,
        project_id: Option<String>,
    ) -> Result<SessionRow, RuntimeError> {
        let id = SessionId::new();
        let now = Utc::now();

        let mut metadata = serde_json::json!({});
        if let Some(mode) = mode.as_deref() {
            metadata = set_session_mode(&metadata, mode);
        }
        if let Some(project_id) = project_id.as_deref() {
            metadata = set_project_id(&metadata, project_id);
        }

        let new_session = NewSession {
            id: id.into_uuid(),
            kind: serde_json::to_value(kind)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string(),
            status: SessionStatus::Created.as_str().to_string(),
            workspace_root,
            channel_ref,
            requester_session_id,
            latest_turn_id: None,
            runtime_profile: None,
            policy_profile: None,
            metadata,
            created_at: now,
            updated_at: now,
            last_activity_at: now,
        };

        let row = self.session_repo.create(new_session).await?;
        // Immediately transition Created → Ready so the session is usable.
        let row = self
            .session_repo
            .update_status(row.id, SessionStatus::Ready.as_str(), Utc::now())
            .await?;
        Ok(row)
    }

    /// Transition a session to Ready status. Idempotent: if already ready, returns Ok.
    pub async fn mark_ready(&self, session_id: Uuid) -> Result<SessionRow, RuntimeError> {
        let row = self.session_repo.find_by_id(session_id).await?;
        if row.status == "ready" {
            return Ok(row);
        }
        self.transition_session(session_id, "created", "ready")
            .await
    }

    /// Transition a session to Running status.
    ///
    /// Idempotent when already running, and valid from any waiting state when
    /// resuming execution after a tool/subagent/approval pause.
    pub async fn mark_running(&self, session_id: Uuid) -> Result<SessionRow, RuntimeError> {
        self.checked_transition(session_id, SessionStatus::Running)
            .await
    }

    /// Transition a session to Completed status.
    pub async fn mark_completed(&self, session_id: Uuid) -> Result<SessionRow, RuntimeError> {
        self.checked_transition(session_id, SessionStatus::Completed)
            .await
    }

    /// Transition a session to Failed status.
    pub async fn mark_failed(&self, session_id: Uuid) -> Result<SessionRow, RuntimeError> {
        self.checked_transition(session_id, SessionStatus::Failed)
            .await
    }

    /// Get a session by ID.
    pub async fn get_session(&self, session_id: Uuid) -> Result<SessionRow, RuntimeError> {
        self.session_repo
            .find_by_id(session_id)
            .await
            .map_err(|e| match e {
                StoreError::NotFound { .. } => {
                    RuntimeError::SessionNotFound(session_id.to_string())
                }
                other => RuntimeError::Store(other),
            })
    }

    /// Find the most recent active session by channel reference.
    pub async fn get_session_by_channel_ref(
        &self,
        channel_ref: &str,
    ) -> Result<Option<SessionRow>, RuntimeError> {
        self.session_repo
            .find_by_channel_ref(channel_ref)
            .await
            .map_err(RuntimeError::Store)
    }

    /// Merge a metadata patch into the current session metadata and persist it.
    pub async fn patch_metadata(
        &self,
        session_id: Uuid,
        patch: serde_json::Value,
    ) -> Result<SessionRow, RuntimeError> {
        let row = self.get_session(session_id).await?;
        let mut metadata = row.metadata;
        merge_json(&mut metadata, patch);
        self.session_repo
            .update_metadata(session_id, metadata, Utc::now())
            .await
            .map_err(RuntimeError::Store)
    }

    /// Delete a session and any transcript rows if transcript storage is attached.
    pub async fn delete_session(&self, session_id: Uuid) -> Result<(), RuntimeError> {
        self.get_session(session_id).await?;

        if let Some(transcript_repo) = &self.transcript_repo {
            transcript_repo.delete_by_session(session_id).await?;
        }

        let deleted = self.session_repo.delete(session_id).await?;
        if !deleted {
            return Err(RuntimeError::SessionNotFound(session_id.to_string()));
        }
        Ok(())
    }

    async fn checked_transition(
        &self,
        session_id: Uuid,
        target: SessionStatus,
    ) -> Result<SessionRow, RuntimeError> {
        let row = self.session_repo.find_by_id(session_id).await?;
        let current: SessionStatus =
            row.status
                .parse()
                .map_err(|_| RuntimeError::InvalidSessionState {
                    expected: target.as_str().to_string(),
                    actual: row.status.clone(),
                })?;
        if !current.can_transition_to(&target) {
            return Err(RuntimeError::InvalidSessionState {
                expected: target.as_str().to_string(),
                actual: row.status,
            });
        }
        let updated = self
            .session_repo
            .update_status(session_id, target.as_str(), Utc::now())
            .await?;
        Ok(updated)
    }

    async fn transition_session(
        &self,
        session_id: Uuid,
        expected_from: &str,
        to: &str,
    ) -> Result<SessionRow, RuntimeError> {
        let row = self.session_repo.find_by_id(session_id).await?;
        if row.status != expected_from {
            return Err(RuntimeError::InvalidSessionState {
                expected: expected_from.to_string(),
                actual: row.status,
            });
        }
        let updated = self
            .session_repo
            .update_status(session_id, to, Utc::now())
            .await?;
        Ok(updated)
    }
}

fn merge_json(target: &mut serde_json::Value, patch: serde_json::Value) {
    match (target, patch) {
        (serde_json::Value::Object(target_map), serde_json::Value::Object(patch_map)) => {
            for (key, value) in patch_map {
                match value {
                    serde_json::Value::Null => {
                        target_map.remove(&key);
                    }
                    other => {
                        merge_json(
                            target_map.entry(key).or_insert(serde_json::Value::Null),
                            other,
                        );
                    }
                }
            }
        }
        (target_slot, patch_value) => {
            *target_slot = patch_value;
        }
    }
}
