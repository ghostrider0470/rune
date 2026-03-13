use std::sync::Arc;

use chrono::Utc;
use uuid::Uuid;

use rune_core::{SessionId, SessionKind, SessionStatus};
use rune_store::StoreError;
use rune_store::models::{NewSession, SessionRow};
use rune_store::repos::SessionRepo;

use crate::error::RuntimeError;

/// Creates and manages session lifecycle. Persists state via store repo traits.
pub struct SessionEngine {
    session_repo: Arc<dyn SessionRepo>,
}

impl SessionEngine {
    pub fn new(session_repo: Arc<dyn SessionRepo>) -> Self {
        Self { session_repo }
    }

    /// Create a new session with the given kind and optional workspace root.
    pub async fn create_session(
        &self,
        kind: SessionKind,
        workspace_root: Option<String>,
    ) -> Result<SessionRow, RuntimeError> {
        let id = SessionId::new();
        let now = Utc::now();

        let new_session = NewSession {
            id: id.into_uuid(),
            kind: enum_label_session_kind(kind),
            status: enum_label_session_status(SessionStatus::Created),
            workspace_root,
            channel_ref: None,
            requester_session_id: None,
            created_at: now,
            updated_at: now,
            last_activity_at: now,
        };

        let row = self.session_repo.create(new_session).await?;
        Ok(row)
    }

    /// Transition a session to Ready status.
    pub async fn mark_ready(&self, session_id: Uuid) -> Result<SessionRow, RuntimeError> {
        self.transition_session(session_id, "created", "ready")
            .await
    }

    /// Transition a session to Running status.
    pub async fn mark_running(&self, session_id: Uuid) -> Result<SessionRow, RuntimeError> {
        self.transition_session(session_id, "ready", "running")
            .await
    }

    /// Transition a session to Completed status.
    pub async fn mark_completed(&self, session_id: Uuid) -> Result<SessionRow, RuntimeError> {
        // Running or waiting states can transition to completed
        let row = self.session_repo.find_by_id(session_id).await?;
        let valid_from = [
            "running",
            "waiting_for_tool",
            "waiting_for_approval",
            "waiting_for_subagent",
        ];
        if !valid_from.contains(&row.status.as_str()) {
            return Err(RuntimeError::InvalidSessionState {
                expected: "running|waiting_*".to_string(),
                actual: row.status,
            });
        }
        let updated = self
            .session_repo
            .update_status(session_id, "completed", Utc::now())
            .await?;
        Ok(updated)
    }

    /// Transition a session to Failed status.
    pub async fn mark_failed(&self, session_id: Uuid) -> Result<SessionRow, RuntimeError> {
        let updated = self
            .session_repo
            .update_status(session_id, "failed", Utc::now())
            .await?;
        Ok(updated)
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

fn enum_label_session_kind(kind: SessionKind) -> String {
    match kind {
        SessionKind::Direct => "direct",
        SessionKind::Channel => "channel",
        SessionKind::Scheduled => "scheduled",
        SessionKind::Subagent => "subagent",
    }
    .to_string()
}

fn enum_label_session_status(status: SessionStatus) -> String {
    match status {
        SessionStatus::Created => "created",
        SessionStatus::Ready => "ready",
        SessionStatus::Running => "running",
        SessionStatus::WaitingForTool => "waiting_for_tool",
        SessionStatus::WaitingForApproval => "waiting_for_approval",
        SessionStatus::WaitingForSubagent => "waiting_for_subagent",
        SessionStatus::Suspended => "suspended",
        SessionStatus::Completed => "completed",
        SessionStatus::Failed => "failed",
        SessionStatus::Cancelled => "cancelled",
    }
    .to_string()
}
