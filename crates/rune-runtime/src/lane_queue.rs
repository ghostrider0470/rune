//! Lane-based concurrency model for turn execution.
//!
//! Each turn is classified into a **lane** based on its session kind.
//! Lanes impose independent concurrency caps via tokio semaphores,
//! ensuring that (for example) a burst of subagent work cannot starve
//! interactive user sessions.

use std::collections::{HashMap, VecDeque};
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
    /// High-priority control/comms traffic that should preempt background work.
    Priority,
    /// Subagent sessions. Max 8 concurrent.
    Subagent,
    /// Scheduled / cron jobs. Effectively uncapped (1024).
    Cron,
    /// Heartbeat/system checks that should bypass normal scheduled contention.
    Heartbeat,
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
            Lane::Priority => write!(f, "priority"),
            Lane::Subagent => write!(f, "subagent"),
            Lane::Cron => write!(f, "cron"),
            Lane::Heartbeat => write!(f, "heartbeat"),
        }
    }
}

// ── Per-lane capacity defaults ───────────────────────────────────────

const MAIN_CAPACITY: usize = 4;
const PRIORITY_CAPACITY: usize = 16;
const SUBAGENT_CAPACITY: usize = 8;
const CRON_CAPACITY: usize = 1024;
const HEARTBEAT_CAPACITY: usize = 1024;
const DEFAULT_GLOBAL_TOOL_CAPACITY: usize = 32;
const DEFAULT_PROJECT_TOOL_CAPACITY: usize = 4;

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
            Err(_) => self
                .semaphore
                .clone()
                .acquire_owned()
                .await
                .expect("semaphore should not be closed"),
        }
    }

    /// Drain the oldest waiter when a permit becomes available.
    async fn wake_next(&self) {
        let mut queue = self.waiters.lock().await;
        while let Some(tx) = queue.pop_front() {
            if let Ok(permit) = self.semaphore.clone().try_acquire_owned() {
                if tx.send(permit).is_ok() {
                    return;
                }
            } else {
                queue.push_front(tx);
                return;
            }
        }
    }

    /// Number of permits currently in use.
    fn active(&self) -> usize {
        self.capacity - self.semaphore.available_permits()
    }

    fn queued(&self) -> usize {
        self.waiters
            .try_lock()
            .map(|queue| queue.iter().filter(|waiter| !waiter.is_closed()).count())
            .unwrap_or(0)
    }
}

/// Central lane-based concurrency controller.
pub struct LaneQueue {
    main: LaneSemaphore,
    priority: LaneSemaphore,
    subagent: LaneSemaphore,
    cron: LaneSemaphore,
    heartbeat: LaneSemaphore,
    tool_limits: ToolConcurrencyQueue,
}

impl LaneQueue {
    /// Create a `LaneQueue` with default capacities.
    pub fn new() -> Self {
        Self {
            main: LaneSemaphore::new(MAIN_CAPACITY),
            priority: LaneSemaphore::new(PRIORITY_CAPACITY),
            subagent: LaneSemaphore::new(SUBAGENT_CAPACITY),
            cron: LaneSemaphore::new(CRON_CAPACITY),
            heartbeat: LaneSemaphore::new(HEARTBEAT_CAPACITY),
            tool_limits: ToolConcurrencyQueue::new(
                DEFAULT_GLOBAL_TOOL_CAPACITY,
                DEFAULT_PROJECT_TOOL_CAPACITY,
            ),
        }
    }

    /// Create a `LaneQueue` with custom per-lane capacities.
    pub fn with_capacities(main: usize, subagent: usize, cron: usize) -> Self {
        Self::with_all_capacities(main, PRIORITY_CAPACITY, subagent, cron, HEARTBEAT_CAPACITY)
    }

    /// Create a queue with custom capacities for every execution lane.
    pub fn with_all_capacities(
        main: usize,
        priority: usize,
        subagent: usize,
        cron: usize,
        heartbeat: usize,
    ) -> Self {
        Self {
            main: LaneSemaphore::new(main),
            priority: LaneSemaphore::new(priority),
            subagent: LaneSemaphore::new(subagent),
            cron: LaneSemaphore::new(cron),
            heartbeat: LaneSemaphore::new(heartbeat),
            tool_limits: ToolConcurrencyQueue::new(
                DEFAULT_GLOBAL_TOOL_CAPACITY,
                DEFAULT_PROJECT_TOOL_CAPACITY,
            ),
        }
    }

    /// Create a queue with custom lane caps and tool concurrency limits.
    pub fn with_limits(
        main: usize,
        subagent: usize,
        cron: usize,
        global_tool_capacity: usize,
        project_tool_capacity: usize,
    ) -> Self {
        Self::with_all_limits(
            main,
            PRIORITY_CAPACITY,
            subagent,
            cron,
            HEARTBEAT_CAPACITY,
            global_tool_capacity,
            project_tool_capacity,
        )
    }

    /// Create a queue with custom capacities for every lane and tool concurrency limits.
    pub fn with_all_limits(
        main: usize,
        priority: usize,
        subagent: usize,
        cron: usize,
        heartbeat: usize,
        global_tool_capacity: usize,
        project_tool_capacity: usize,
    ) -> Self {
        Self {
            main: LaneSemaphore::new(main),
            priority: LaneSemaphore::new(priority),
            subagent: LaneSemaphore::new(subagent),
            cron: LaneSemaphore::new(cron),
            heartbeat: LaneSemaphore::new(heartbeat),
            tool_limits: ToolConcurrencyQueue::new(global_tool_capacity, project_tool_capacity),
        }
    }

    /// Acquire a concurrency permit for a tool invocation.
    pub async fn acquire_tool(self: &Arc<Self>, project_key: Option<&str>) -> ToolPermit {
        let project_key = project_key.unwrap_or("__default").to_string();
        let permit = self.tool_limits.acquire(project_key.clone()).await;
        ToolPermit {
            _global_permit: permit.global_permit,
            _project_permit: permit.project_permit,
            project_key,
            queue: Arc::clone(self),
        }
    }

    /// Acquire a permit for the given lane.
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
        let tool_active = self.tool_limits.active();
        let tool_capacity = self.tool_limits.global_capacity();
        let tool_queued = self.tool_limits.queued();
        let project_tool_capacity = self.tool_limits.project_capacity();

        LaneStats {
            main_active: self.main.active(),
            main_capacity: self.main.capacity,
            main_queued: self.main.queued(),
            priority_active: self.priority.active(),
            priority_capacity: self.priority.capacity,
            priority_queued: self.priority.queued(),
            subagent_active: self.subagent.active(),
            subagent_capacity: self.subagent.capacity,
            subagent_queued: self.subagent.queued(),
            cron_active: self.cron.active(),
            cron_capacity: self.cron.capacity,
            cron_queued: self.cron.queued(),
            heartbeat_active: self.heartbeat.active(),
            heartbeat_capacity: self.heartbeat.capacity,
            heartbeat_queued: self.heartbeat.queued(),
            tool_active,
            tool_capacity,
            tool_queued,
            project_tool_capacity,
        }
    }

    fn lane_semaphore(&self, lane: &Lane) -> &LaneSemaphore {
        match lane {
            Lane::Main => &self.main,
            Lane::Priority => &self.priority,
            Lane::Subagent => &self.subagent,
            Lane::Cron => &self.cron,
            Lane::Heartbeat => &self.heartbeat,
        }
    }

    async fn release(&self, lane: &Lane) {
        self.lane_semaphore(lane).wake_next().await;
    }
}

struct ToolPermitInner {
    global_permit: OwnedSemaphorePermit,
    project_permit: OwnedSemaphorePermit,
}

struct ToolConcurrencyQueue {
    global: LaneSemaphore,
    project_capacity: usize,
    projects: Mutex<HashMap<String, Arc<LaneSemaphore>>>,
}

impl ToolConcurrencyQueue {
    fn new(global_capacity: usize, project_capacity: usize) -> Self {
        Self {
            global: LaneSemaphore::new(global_capacity.max(1)),
            project_capacity: project_capacity.max(1),
            projects: Mutex::new(HashMap::new()),
        }
    }

    async fn acquire(&self, project_key: String) -> ToolPermitInner {
        let global_permit = self.global.acquire().await;
        let project_semaphore = self.project_semaphore(&project_key).await;
        let project_permit = project_semaphore.acquire().await;
        ToolPermitInner {
            global_permit,
            project_permit,
        }
    }

    async fn project_semaphore(&self, project_key: &str) -> Arc<LaneSemaphore> {
        let mut projects = self.projects.lock().await;
        Arc::clone(
            projects
                .entry(project_key.to_string())
                .or_insert_with(|| Arc::new(LaneSemaphore::new(self.project_capacity))),
        )
    }

    fn active(&self) -> usize {
        self.global.active()
    }

    fn global_capacity(&self) -> usize {
        self.global.capacity
    }

    fn queued(&self) -> usize {
        self.global.queued()
    }

    fn project_capacity(&self) -> usize {
        self.project_capacity
    }

    async fn release(&self, project_key: &str) {
        let project = {
            let projects = self.projects.lock().await;
            projects.get(project_key).cloned()
        };
        if let Some(project) = project {
            project.wake_next().await;
        }
        self.global.wake_next().await;
    }
}

impl Default for LaneQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// A held lane permit.
pub struct LanePermit {
    _permit: OwnedSemaphorePermit,
    lane: Lane,
    queue: Arc<LaneQueue>,
}

/// A held concurrency permit for a tool invocation.
pub struct ToolPermit {
    _global_permit: OwnedSemaphorePermit,
    _project_permit: OwnedSemaphorePermit,
    project_key: String,
    queue: Arc<LaneQueue>,
}

impl LanePermit {
    pub fn lane(&self) -> Lane {
        self.lane
    }
}

impl Drop for LanePermit {
    fn drop(&mut self) {
        let queue = Arc::clone(&self.queue);
        let lane = self.lane;
        tokio::spawn(async move {
            queue.release(&lane).await;
        });
    }
}

impl Drop for ToolPermit {
    fn drop(&mut self) {
        let queue = Arc::clone(&self.queue);
        let project_key = self.project_key.clone();
        tokio::spawn(async move {
            queue.tool_limits.release(&project_key).await;
        });
    }
}

/// Snapshot of lane utilisation returned by [`LaneQueue::stats`].
#[derive(Clone, Debug)]
pub struct LaneStats {
    pub main_active: usize,
    pub main_capacity: usize,
    pub main_queued: usize,
    pub priority_active: usize,
    pub priority_capacity: usize,
    pub priority_queued: usize,
    pub subagent_active: usize,
    pub subagent_capacity: usize,
    pub subagent_queued: usize,
    pub cron_active: usize,
    pub cron_capacity: usize,
    pub cron_queued: usize,
    pub heartbeat_active: usize,
    pub heartbeat_capacity: usize,
    pub heartbeat_queued: usize,
    pub tool_active: usize,
    pub tool_capacity: usize,
    pub tool_queued: usize,
    pub project_tool_capacity: usize,
}

impl fmt::Display for LaneStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "main={}/{} queued={} priority={}/{} queued={} subagent={}/{} queued={} cron={}/{} queued={} heartbeat={}/{} queued={} tools={}/{} queued={} per_project={}",
            self.main_active,
            self.main_capacity,
            self.main_queued,
            self.priority_active,
            self.priority_capacity,
            self.priority_queued,
            self.subagent_active,
            self.subagent_capacity,
            self.subagent_queued,
            self.cron_active,
            self.cron_capacity,
            self.cron_queued,
            self.heartbeat_active,
            self.heartbeat_capacity,
            self.heartbeat_queued,
            self.tool_active,
            self.tool_capacity,
            self.tool_queued,
            self.project_tool_capacity,
        )
    }
}

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

        let blocker = queue.acquire(Lane::Main).await;

        let observed1 = Arc::clone(&observed);
        let q1 = Arc::clone(&queue);
        let h1 = tokio::spawn(async move {
            let _p = q1.acquire(Lane::Main).await;
            observed1.lock().unwrap().push("first");
        });

        tokio::time::sleep(Duration::from_millis(10)).await;

        let observed2 = Arc::clone(&observed);
        let q2 = Arc::clone(&queue);
        let h2 = tokio::spawn(async move {
            let _p = q2.acquire(Lane::Main).await;
            observed2.lock().unwrap().push("second");
        });

        tokio::time::sleep(Duration::from_millis(20)).await;
        drop(blocker);

        h1.await.unwrap();
        h2.await.unwrap();

        let observed = observed.lock().unwrap().clone();
        assert_eq!(observed, vec!["first", "second"]);
    }

    #[tokio::test]
    async fn priority_lane_is_independent_from_cron_lane() {
        let queue = Arc::new(LaneQueue::with_capacities(1, 1, 1));
        let _cron = queue.acquire(Lane::Cron).await;

        let priority =
            tokio::time::timeout(Duration::from_millis(50), queue.acquire(Lane::Priority)).await;
        assert!(
            priority.is_ok(),
            "priority lane should not be blocked by cron saturation"
        );
    }

    #[tokio::test]
    async fn heartbeat_lane_is_independent_from_priority_lane() {
        let queue = Arc::new(LaneQueue::with_capacities(1, 1, 1));
        let _priority = queue.acquire(Lane::Priority).await;

        let heartbeat =
            tokio::time::timeout(Duration::from_millis(50), queue.acquire(Lane::Heartbeat)).await;
        assert!(
            heartbeat.is_ok(),
            "heartbeat lane should remain immediate under priority load"
        );
    }

    #[tokio::test]
    async fn stats_reflect_priority_utilisation() {
        let queue = Arc::new(LaneQueue::with_capacities(1, 1, 1));
        let _priority = queue.acquire(Lane::Priority).await;

        let stats = queue.stats();
        assert_eq!(stats.priority_active, 1);
        assert_eq!(stats.priority_capacity, 16);
    }

    #[tokio::test]
    async fn lanes_are_independent() {
        let queue = Arc::new(LaneQueue::with_capacities(1, 1, 1));
        let _main = queue.acquire(Lane::Main).await;

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
    async fn tool_limits_are_project_scoped() {
        let queue = Arc::new(LaneQueue::with_limits(1, 1, 1, 8, 1));

        let _first = queue.acquire_tool(Some("alpha")).await;

        let queue_for_other = Arc::clone(&queue);
        let other_project = tokio::time::timeout(
            Duration::from_millis(50),
            queue_for_other.acquire_tool(Some("beta")),
        )
        .await;
        assert!(
            other_project.is_ok(),
            "different project should not be blocked"
        );
    }

    #[tokio::test]
    async fn tool_limits_block_same_project_until_release() {
        let queue = Arc::new(LaneQueue::with_limits(1, 1, 1, 8, 1));
        let blocker = queue.acquire_tool(Some("alpha")).await;

        let queue_for_waiter = Arc::clone(&queue);
        let waiter = tokio::spawn(async move {
            let _permit = queue_for_waiter.acquire_tool(Some("alpha")).await;
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        drop(blocker);

        tokio::time::timeout(Duration::from_millis(200), waiter)
            .await
            .expect("same-project waiter should be released")
            .expect("join should succeed");
    }

    #[tokio::test]
    async fn stats_ignore_cancelled_lane_waiters() {
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
        tokio::time::sleep(Duration::from_millis(10)).await;

        let stats = queue.stats();
        assert_eq!(stats.main_active, 1);
        assert_eq!(stats.main_queued, 0);

        drop(blocker);
    }

    #[tokio::test]
    async fn stats_ignore_cancelled_tool_waiters() {
        let queue = Arc::new(LaneQueue::with_limits(1, 1, 1, 1, 1));
        let blocker = queue.acquire_tool(Some("alpha")).await;

        let cancelled = {
            let queue = Arc::clone(&queue);
            tokio::spawn(async move {
                let _permit = queue.acquire_tool(Some("beta")).await;
            })
        };

        tokio::time::sleep(Duration::from_millis(10)).await;
        cancelled.abort();
        let _ = cancelled.await;
        tokio::time::sleep(Duration::from_millis(10)).await;

        let stats = queue.stats();
        assert_eq!(stats.tool_active, 1);
        assert_eq!(stats.tool_queued, 0);

        drop(blocker);
    }

    #[tokio::test]
    async fn stats_reflect_utilisation() {
        let queue = Arc::new(LaneQueue::new());
        let stats = queue.stats();
        assert_eq!(stats.main_active, 0);
        assert_eq!(stats.main_capacity, 4);
        assert_eq!(stats.main_queued, 0);
        assert_eq!(stats.priority_active, 0);
        assert_eq!(stats.priority_capacity, 16);
        assert_eq!(stats.priority_queued, 0);
        assert_eq!(stats.subagent_active, 0);
        assert_eq!(stats.subagent_capacity, 8);
        assert_eq!(stats.subagent_queued, 0);
        assert_eq!(stats.cron_active, 0);
        assert_eq!(stats.cron_capacity, 1024);
        assert_eq!(stats.cron_queued, 0);
        assert_eq!(stats.heartbeat_active, 0);
        assert_eq!(stats.heartbeat_capacity, 1024);
        assert_eq!(stats.heartbeat_queued, 0);
        assert_eq!(stats.tool_active, 0);
        assert_eq!(stats.tool_capacity, 32);
        assert_eq!(stats.tool_queued, 0);
        assert_eq!(stats.project_tool_capacity, 4);
    }

    #[test]
    fn lane_display() {
        assert_eq!(Lane::Main.to_string(), "main");
        assert_eq!(Lane::Priority.to_string(), "priority");
        assert_eq!(Lane::Subagent.to_string(), "subagent");
        assert_eq!(Lane::Cron.to_string(), "cron");
        assert_eq!(Lane::Heartbeat.to_string(), "heartbeat");
    }

    #[test]
    fn stats_display() {
        let stats = LaneStats {
            main_active: 2,
            main_capacity: 4,
            main_queued: 3,
            priority_active: 1,
            priority_capacity: 16,
            priority_queued: 2,
            subagent_active: 1,
            subagent_capacity: 8,
            subagent_queued: 1,
            cron_active: 0,
            cron_capacity: 1024,
            cron_queued: 0,
            heartbeat_active: 1,
            heartbeat_capacity: 1024,
            heartbeat_queued: 4,
            tool_active: 0,
            tool_capacity: 32,
            tool_queued: 5,
            project_tool_capacity: 4,
        };
        assert_eq!(
            stats.to_string(),
            "main=2/4 queued=3 priority=1/16 queued=2 subagent=1/8 queued=1 cron=0/1024 queued=0 heartbeat=1/1024 queued=4 tools=0/32 queued=5 per_project=4"
        );
    }
}
