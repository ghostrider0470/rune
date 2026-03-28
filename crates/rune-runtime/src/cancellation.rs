use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::error::RuntimeError;

#[derive(Default)]
pub struct TurnCancellationRegistry {
    inner: Arc<Mutex<HashMap<Uuid, Arc<CancellationState>>>>,
}

impl TurnCancellationRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register(&self, session_id: Uuid, turn_id: Uuid) -> TurnCancellationHandle {
        let state = Arc::new(CancellationState::new(turn_id));
        let mut inner = self.inner.lock().await;
        inner.insert(session_id, state.clone());
        TurnCancellationHandle {
            session_id,
            state,
            registry: Arc::clone(&self.inner),
        }
    }

    pub async fn cancel(&self, session_id: Uuid, reason: impl Into<String>) -> bool {
        let state = {
            let inner = self.inner.lock().await;
            inner.get(&session_id).cloned()
        };

        if let Some(state) = state {
            state.cancel(reason.into()).await;
            true
        } else {
            false
        }
    }

    pub async fn current_turn_id(&self, session_id: Uuid) -> Option<Uuid> {
        let inner = self.inner.lock().await;
        inner.get(&session_id).map(|state| state.turn_id)
    }
}

pub struct TurnCancellationHandle {
    session_id: Uuid,
    state: Arc<CancellationState>,
    registry: Arc<Mutex<HashMap<Uuid, Arc<CancellationState>>>>,
}

impl TurnCancellationHandle {
    pub async fn checkpoint(&self) -> Result<(), RuntimeError> {
        self.state.checkpoint().await
    }
}

impl Drop for TurnCancellationHandle {
    fn drop(&mut self) {
        let registry = Arc::clone(&self.registry);
        let session_id = self.session_id;
        tokio::spawn(async move {
            let mut inner = registry.lock().await;
            inner.remove(&session_id);
        });
    }
}

struct CancellationState {
    turn_id: Uuid,
    cancelled: Mutex<Option<CancellationRecord>>,
}

impl CancellationState {
    fn new(turn_id: Uuid) -> Self {
        Self {
            turn_id,
            cancelled: Mutex::new(None),
        }
    }

    async fn cancel(&self, reason: String) {
        let mut cancelled = self.cancelled.lock().await;
        if cancelled.is_none() {
            *cancelled = Some(CancellationRecord {
                reason,
                cancelled_at: Utc::now(),
            });
        }
    }

    async fn checkpoint(&self) -> Result<(), RuntimeError> {
        let cancelled = self.cancelled.lock().await;
        if let Some(record) = cancelled.as_ref() {
            return Err(RuntimeError::Aborted(format!(
                "cancelled at {}: {}",
                record.cancelled_at.to_rfc3339(),
                record.reason
            )));
        }
        Ok(())
    }
}

struct CancellationRecord {
    reason: String,
    cancelled_at: DateTime<Utc>,
}
