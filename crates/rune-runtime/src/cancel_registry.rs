use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{Notify, RwLock};
use uuid::Uuid;

#[derive(Debug, Default)]
pub struct TurnCancelRegistry {
    entries: RwLock<HashMap<Uuid, Arc<TurnCancellationInner>>>,
}

#[derive(Debug)]
struct TurnCancellationInner {
    notify: Notify,
    cancelled: std::sync::atomic::AtomicBool,
}

impl TurnCancellationInner {
    fn new() -> Self {
        Self {
            notify: Notify::new(),
            cancelled: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Release);
        self.notify.notify_waiters();
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::Acquire)
    }
}

impl TurnCancelRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register(self: &Arc<Self>, turn_id: Uuid) -> (TurnCancelGuard, TurnCancellation) {
        let inner = Arc::new(TurnCancellationInner::new());
        self.entries.write().await.insert(turn_id, inner.clone());
        (
            TurnCancelGuard {
                registry: Arc::clone(self),
                turn_id,
            },
            TurnCancellation { inner },
        )
    }

    pub async fn cancel(&self, turn_id: Uuid) -> bool {
        let inner = self.entries.read().await.get(&turn_id).cloned();
        if let Some(inner) = inner {
            inner.cancel();
            true
        } else {
            false
        }
    }

    pub async fn is_registered(&self, turn_id: Uuid) -> bool {
        self.entries.read().await.contains_key(&turn_id)
    }

    async fn unregister(&self, turn_id: Uuid) {
        self.entries.write().await.remove(&turn_id);
    }
}

pub struct TurnCancelGuard {
    registry: Arc<TurnCancelRegistry>,
    turn_id: Uuid,
}

impl Drop for TurnCancelGuard {
    fn drop(&mut self) {
        let registry = Arc::clone(&self.registry);
        let turn_id = self.turn_id;
        tokio::spawn(async move {
            registry.unregister(turn_id).await;
        });
    }
}

#[derive(Clone, Debug)]
pub struct TurnCancellation {
    inner: Arc<TurnCancellationInner>,
}

impl TurnCancellation {
    pub async fn cancelled(&self) {
        if self.inner.is_cancelled() {
            return;
        }
        self.inner.notify.notified().await;
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn cancel_notifies_registered_turn() {
        let registry = Arc::new(TurnCancelRegistry::new());
        let turn_id = Uuid::new_v4();
        let (_guard, signal) = registry.register(turn_id).await;

        assert!(registry.cancel(turn_id).await);
        tokio::time::timeout(Duration::from_millis(200), signal.cancelled())
            .await
            .expect("signal should fire");
    }

    #[tokio::test]
    async fn dropped_guard_unregisters_turn() {
        let registry = Arc::new(TurnCancelRegistry::new());
        let turn_id = Uuid::new_v4();
        let (guard, _signal) = registry.register(turn_id).await;
        assert!(registry.is_registered(turn_id).await);
        drop(guard);
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!registry.is_registered(turn_id).await);
        assert!(!registry.cancel(turn_id).await);
    }
}
