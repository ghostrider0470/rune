//! Lane-based concurrency model for turn execution.
//!
//! Each turn is classified into a **lane** based on its session kind.
//! Lanes impose independent concurrency caps via tokio semaphores,
//! ensuring that (for example) a burst of subagent work cannot starve
//! interactive user sessions.

use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;

use rune_core::SessionKind;
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use tracing::debug;

// ── Lane classification ──────────────────────────────────────────────

/// Task classification that determines which concurrency lane a turn uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lane {
    /// Direct user sessions and channel sessions. Max 4 concurrent.
    Main,
    /// Subagent sessions. Max 8 concurrent.
    Subagent,
    /// Scheduled / cron jobs. Effectively uncapped (1024).
    Cron,
}

impl Lane {
    /// Map a `SessionKind` to its execution lane.
    pub fn from_session_kind(kind: &SessionKind) -> Self {
        match kind {
            SessionKind::Direct | SessionKind::Channel => Lane::Main,
            SessionKind::Subagent => Lane::Subagent,
            SessionKind::Scheduled => Lane::Cron,
        }
    }
}

impl fmt::Display for Lane {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Lane::Main => write!(f, "main"),
            Lane::Subagent => write!(f, "subagent"),
            Lane::Cron => write!(f, "cron"),
        }
    }
}

// ── Per-lane capacity defaults ───────────────────────────────────────

const MAIN_CAPACITY: usize = 4;
const SUBAGENT_CAPACITY: usize = 8;
const CRON_CAPACITY: usize = 1024;

// ── Internal: a single lane's semaphore + FIFO waiters ───────────────

/// Manages a semaphore with a fair FIFO waiting queue.
///
/// When a permit is not immediately available the caller is enqueued.
/// As permits are released the oldest waiter is woken first, preventing
/// starvation under sustained load.
struct LaneSemaphore {
    semaphore: Arc<Semaphore>,
    /// FIFO queue of waiters that could not acquire a permit immediately.
    waiters: Mutex<VecDeque<tokio::sync::oneshot::Sender<OwnedSemaphorePermit>>>,
    capacity: usize,
}

impl LaneSemaphore {
    fn new(capacity: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(capacity)),
            waiters: Mutex::new(VecDeque::new()),
            capacity,
        }
    }

    /// Acquire a permit, waiting in FIFO order if the lane is at capacity.
    async fn acquire(&self) -> OwnedSemaphorePermit {
        // Fast path: try to grab a permit without waiting.
        if let Ok(permit) = self.semaphore.clone().try_acquire_owned() {
            return permit;
        }

        // Slow path: enqueue ourselves and wait for a permit to be handed
        // to us when another task finishes.
        let (tx, rx) = tokio::sync::oneshot::channel();
        {
            let mut queue = self.waiters.lock().await;
            queue.push_back(tx);
        }

        // A permit may have become available after we enqueued but before
        // any releaser ran. Drain once proactively so the head waiter is not
        // left parked indefinitely until some unrelated future release.
        self.wake_next().await;

        // Wait for a permit to be delivered. If the sender is dropped (for
        // example because this waiter was cancelled and later skipped), fall
        // back to a direct semaphore acquire.
        match rx.await {
            Ok(permit) => permit,
            Err(_) => {
                // Defensive: channel was dropped without sending.
                // Fall back to blocking acquire on the semaphore.
                self.semaphore
                    .clone()
                    .acquire_owned()
                    .await
                    .expect("semaphore should not be closed")
            }
        }
    }

    /// Drain the oldest waiter when a permit becomes available.
    ///
    /// Called by `LanePermit::drop` (via `LaneQueue::release`) after
    /// returning a permit to the semaphore.
    async fn wake_next(&self) {
        let mut queue = self.waiters.lock().await;
        while let Some(tx) = queue.pop_front() {
            // Try to acquire a permit for this waiter.
            if let Ok(permit) = self.semaphore.clone().try_acquire_owned() {
                if tx.send(permit).is_ok() {
                    return;
                }
                // Receiver gone — permit returns to the semaphore via Drop,
                // try the next waiter.
            } else {
                // No permits available yet; re-enqueue at the front and stop.
                queue.push_front(tx);
                return;
            }
        }
    }

    /// Number of permits currently in use.
    fn active(&self) -> usize {
        self.capacity - self.semaphore.available_permits()
    }
}

// ── Public API ───────────────────────────────────────────────────────

/// Central lane-based concurrency controller.
///
/// Instantiate once and share (via `Arc`) across all turn executors.
/// When a turn begins, call [`acquire`] with the appropriate lane; the
/// returned [`LanePermit`] is held for the duration of the turn and
/// automatically released on drop.
pub struct LaneQueue {
    main: LaneSemaphore,
    subagent: LaneSemaphore,
    cron: LaneSemaphore,
}

impl LaneQueue {
    /// Create a `LaneQueue` with default capacities.
    pub fn new() -> Self {
        Self {
            main: LaneSemaphore::new(MAIN_CAPACITY),
            subagent: LaneSemaphore::new(SUBAGENT_CAPACITY),
            cron: LaneSemaphore::new(CRON_CAPACITY),
        }
    }

    /// Create a `LaneQueue` with custom per-lane capacities.
    pub fn with_capacities(main: usize, subagent: usize, cron: usize) -> Self {
        Self {
            main: LaneSemaphore::new(main),
            subagent: LaneSemaphore::new(subagent),
            cron: LaneSemaphore::new(cron),
        }
    }

    /// Acquire a permit for the given lane.
    ///
    /// This future resolves once a slot is available. Waiters are served
    /// in FIFO order within each lane.
    pub async fn acquire(self: &Arc<Self>, lane: Lane) -> LanePermit {
        let lane_sem = self.lane_semaphore(&lane);
        debug!(lane = %lane, "acquiring lane permit");
        let permit = lane_sem.acquire().await;
        debug!(lane = %lane, "lane permit acquired");
        LanePermit {
            _permit: permit,
            lane,
            queue: Arc::clone(self),
        }
    }

    /// Convenience: determine the lane and acquire in one step.
    pub async fn acquire_for_session(self: &Arc<Self>, kind: &SessionKind) -> LanePermit {
        let lane = Lane::from_session_kind(kind);
        self.acquire(lane).await
    }

    /// Current utilisation snapshot across all lanes.
    pub fn stats(&self) -> LaneStats {
        LaneStats {
            main_active: self.main.active(),
            main_capacity: self.main.capacity,
            subagent_active: self.subagent.active(),
            subagent_capacity: self.subagent.capacity,
            cron_active: self.cron.active(),
            cron_capacity: self.cron.capacity,
        }
    }

    fn lane_semaphore(&self, lane: &Lane) -> &LaneSemaphore {
        match lane {
            Lane::Main => &self.main,
            Lane::Subagent => &self.subagent,
            Lane::Cron => &self.cron,
        }
    }

    /// Called when a permit is dropped — wake the next FIFO waiter if any.
    async fn release(&self, lane: &Lane) {
        self.lane_semaphore(lane).wake_next().await;
    }
}

impl Default for LaneQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// A held lane permit. The turn holds this value for its entire lifetime;
/// dropping it releases the lane slot and wakes the next queued waiter.
pub struct LanePermit {
    _permit: OwnedSemaphorePermit,
    lane: Lane,
    queue: Arc<LaneQueue>,
}

impl LanePermit {
    /// Which lane this permit belongs to.
    pub fn lane(&self) -> Lane {
        self.lane
    }
}

impl Drop for LanePermit {
    fn drop(&mut self) {
        let queue = Arc::clone(&self.queue);
        let lane = self.lane;
        // Spawn the wake notification so it doesn't block the dropper.
        tokio::spawn(async move {
            queue.release(&lane).await;
        });
    }
}

/// Snapshot of lane utilisation returned by [`LaneQueue::stats`].
#[derive(Clone, Debug)]
pub struct LaneStats {
    pub main_active: usize,
    pub main_capacity: usize,
    pub subagent_active: usize,
    pub subagent_capacity: usize,
    pub cron_active: usize,
    pub cron_capacity: usize,
}

impl fmt::Display for LaneStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "main={}/{} subagent={}/{} cron={}/{}",
            self.main_active,
            self.main_capacity,
            self.subagent_active,
            self.subagent_capacity,
            self.cron_active,
            self.cron_capacity,
        )
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;
    use std::time::Duration;

    #[test]
    fn lane_from_session_kind_mapping() {
        assert_eq!(Lane::from_session_kind(&SessionKind::Direct), Lane::Main);
        assert_eq!(Lane::from_session_kind(&SessionKind::Channel), Lane::Main);
        assert_eq!(
            Lane::from_session_kind(&SessionKind::Subagent),
            Lane::Subagent
        );
        assert_eq!(Lane::from_session_kind(&SessionKind::Scheduled), Lane::Cron);
    }

    #[tokio::test]
    async fn acquire_and_release_permits() {
        let queue = Arc::new(LaneQueue::with_capacities(2, 2, 2));

        let p1 = queue.acquire(Lane::Main).await;
        let p2 = queue.acquire(Lane::Main).await;

        let stats = queue.stats();
        assert_eq!(stats.main_active, 2);

        drop(p1);
        // Give the spawned wake task a moment to run.
        tokio::time::sleep(Duration::from_millis(10)).await;

        let stats = queue.stats();
        assert_eq!(stats.main_active, 1);

        drop(p2);
        tokio::time::sleep(Duration::from_millis(10)).await;

        let stats = queue.stats();
        assert_eq!(stats.main_active, 0);
    }

    #[tokio::test]
    async fn fifo_ordering_under_contention() {
        let queue = Arc::new(LaneQueue::with_capacities(1, 1, 1));
        let observed = Arc::new(StdMutex::new(Vec::new()));

        // Grab the single permit so subsequent acquires must wait.
        let blocker = queue.acquire(Lane::Main).await;

        // Spawn two waiters in known order.
        let observed1 = Arc::clone(&observed);
        let q1 = Arc::clone(&queue);
        let h1 = tokio::spawn(async move {
            let _p = q1.acquire(Lane::Main).await;
            observed1.lock().unwrap().push("first");
        });

        // Let the first waiter enqueue before spawning the second.
        tokio::time::sleep(Duration::from_millis(10)).await;

        let observed2 = Arc::clone(&observed);
        let q2 = Arc::clone(&queue);
        let h2 = tokio::spawn(async move {
            let _p = q2.acquire(Lane::Main).await;
            observed2.lock().unwrap().push("second");
        });

        // Let both waiters enqueue.
        tokio::time::sleep(Duration::from_millis(20)).await;

        // Release the blocker — the first waiter should proceed, then the second.
        drop(blocker);

        h1.await.unwrap();
        h2.await.unwrap();

        let observed = observed.lock().unwrap().clone();
        assert_eq!(observed, vec!["first", "second"]);
    }

    #[tokio::test]
    async fn lanes_are_independent() {
        let queue = Arc::new(LaneQueue::with_capacities(1, 1, 1));

        // Saturate the main lane.
        let _main = queue.acquire(Lane::Main).await;

        // Subagent lane should still be available immediately.
        let sub =
            tokio::time::timeout(Duration::from_millis(50), queue.acquire(Lane::Subagent)).await;
        assert!(sub.is_ok(), "subagent lane should not be blocked by main");
    }

    #[tokio::test]
    async fn cancelled_waiter_does_not_block_next_waiter() {
        let queue = Arc::new(LaneQueue::with_capacities(1, 1, 1));

        let blocker = queue.acquire(Lane::Main).await;

        let cancelled = {
            let queue = Arc::clone(&queue);
            tokio::spawn(async move {
                let _permit = queue.acquire(Lane::Main).await;
            })
        };

        tokio::time::sleep(Duration::from_millis(10)).await;
        cancelled.abort();
        let _ = cancelled.await;

        let queue_for_second = Arc::clone(&queue);
        let second = tokio::spawn(async move {
            let _permit = queue_for_second.acquire(Lane::Main).await;
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        drop(blocker);

        tokio::time::timeout(Duration::from_millis(200), second)
            .await
            .expect("live waiter should not be blocked behind cancelled waiter")
            .expect("join should succeed");
    }

    #[tokio::test]
    async fn acquire_for_session_routes_scheduled_and_subagent_sessions_to_independent_lanes() {
        let queue = Arc::new(LaneQueue::with_capacities(1, 1, 1));

        let _scheduled = queue.acquire_for_session(&SessionKind::Scheduled).await;

        let subagent = tokio::time::timeout(
            Duration::from_millis(50),
            queue.acquire_for_session(&SessionKind::Subagent),
        )
        .await;
        assert!(
            subagent.is_ok(),
            "subagent lane should remain available while scheduled lane is saturated"
        );
    }

    #[tokio::test]
    async fn waiter_observes_released_permit_without_extra_release() {
        let queue = Arc::new(LaneQueue::with_capacities(1, 1, 1));
        let blocker = queue.acquire(Lane::Main).await;

        let queue_for_waiter = Arc::clone(&queue);
        let waiter = tokio::spawn(async move {
            let _permit = queue_for_waiter.acquire(Lane::Main).await;
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        drop(blocker);

        tokio::time::timeout(Duration::from_millis(200), waiter)
            .await
            .expect("waiter should be woken by the release")
            .expect("join should succeed");
    }

    #[tokio::test]
    async fn stats_reflect_utilisation() {
        let queue = Arc::new(LaneQueue::new());
        let stats = queue.stats();
        assert_eq!(stats.main_active, 0);
        assert_eq!(stats.main_capacity, 4);
        assert_eq!(stats.subagent_active, 0);
        assert_eq!(stats.subagent_capacity, 8);
        assert_eq!(stats.cron_active, 0);
        assert_eq!(stats.cron_capacity, 1024);
    }

    #[test]
    fn lane_display() {
        assert_eq!(Lane::Main.to_string(), "main");
        assert_eq!(Lane::Subagent.to_string(), "subagent");
        assert_eq!(Lane::Cron.to_string(), "cron");
    }

    #[test]
    fn stats_display() {
        let stats = LaneStats {
            main_active: 2,
            main_capacity: 4,
            subagent_active: 1,
            subagent_capacity: 8,
            cron_active: 0,
            cron_capacity: 1024,
        };
        assert_eq!(stats.to_string(), "main=2/4 subagent=1/8 cron=0/1024");
    }
}
