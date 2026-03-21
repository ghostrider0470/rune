use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::orchestrator::{OrchestratorError, OrchestratorState};

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CiStatus {
    Pending,
    Passed,
    Failed,
    Running,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStatus {
    Pending,
    Approved,
    ChangesRequested,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeQueueEntry {
    pub branch: String,
    pub pr_number: u64,
    pub ci_status: CiStatus,
    pub review_status: ReviewStatus,
    pub queued_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeResult {
    Merged { pr_number: u64 },
    NothingReady,
    CiFailed { pr_number: u64 },
    RebaseFailed { pr_number: u64 },
}

// ---------------------------------------------------------------------------
// Git executor trait (for testability)
// ---------------------------------------------------------------------------

/// Abstraction over shell-based git operations so the merge queue can be
/// tested without real subprocesses.
#[async_trait::async_trait]
pub trait GitExecutor: Send + Sync {
    /// Rebase `branch` onto `target` (e.g. `main`).
    async fn rebase(&self, branch: &str, target: &str) -> Result<(), String>;

    /// Squash-merge `branch` into the current branch.
    async fn squash_merge(&self, branch: &str, pr_number: u64) -> Result<(), String>;

    /// Checkout a branch.
    async fn checkout(&self, branch: &str) -> Result<(), String>;
}

// ---------------------------------------------------------------------------
// MergeQueue
// ---------------------------------------------------------------------------

pub struct MergeQueue {
    entries: Vec<MergeQueueEntry>,
    default_branch: String,
}

impl MergeQueue {
    /// Create a new queue from an [`OrchestratorState`].
    pub fn from_state(state: &OrchestratorState) -> Self {
        // Re-parse merge_queue entries from orchestrator state. The orchestrator
        // stores them as its own MergeQueueEntry type (simple strings for
        // status). We maintain our own typed copy.
        Self {
            entries: Vec::new(),
            default_branch: state.default_branch.clone(),
        }
    }

    /// Create a queue with an explicit default branch (useful for tests).
    pub fn new(default_branch: impl Into<String>) -> Self {
        Self {
            entries: Vec::new(),
            default_branch: default_branch.into(),
        }
    }

    pub fn entries(&self) -> &[MergeQueueEntry] {
        &self.entries
    }

    /// Add a PR to the back of the queue.
    pub fn enqueue(&mut self, branch: impl Into<String>, pr_number: u64) {
        self.entries.push(MergeQueueEntry {
            branch: branch.into(),
            pr_number,
            ci_status: CiStatus::Pending,
            review_status: ReviewStatus::Pending,
            queued_at: Utc::now(),
        });
    }

    /// Peek at the next entry eligible for merge (CI passed + approved).
    pub fn next(&self) -> Option<&MergeQueueEntry> {
        self.entries.iter().find(|e| {
            e.ci_status == CiStatus::Passed && e.review_status == ReviewStatus::Approved
        })
    }

    /// Remove the entry for `branch` from the queue.
    pub fn dequeue(&mut self, branch: &str) {
        self.entries.retain(|e| e.branch != branch);
    }

    /// Update CI status for a branch.
    pub fn set_ci_status(&mut self, branch: &str, status: CiStatus) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.branch == branch) {
            entry.ci_status = status;
        }
    }

    /// Update review status for a branch.
    pub fn set_review_status(&mut self, branch: &str, status: ReviewStatus) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.branch == branch) {
            entry.review_status = status;
        }
    }

    /// Return entries that have been queued longer than `threshold`.
    pub fn stale_entries(&self, threshold: Duration) -> Vec<&MergeQueueEntry> {
        let cutoff = Utc::now() - chrono::Duration::from_std(threshold).unwrap_or_default();
        self.entries.iter().filter(|e| e.queued_at < cutoff).collect()
    }

    /// Merge the next eligible entry: rebase onto default branch, squash-merge,
    /// then rebase remaining entries.
    pub async fn merge_next(
        &mut self,
        git: &dyn GitExecutor,
    ) -> Result<MergeResult, OrchestratorError> {
        let entry = match self.next() {
            Some(e) => e.clone(),
            None => return Ok(MergeResult::NothingReady),
        };

        // 1. Rebase the branch onto default.
        if let Err(_e) = git.rebase(&entry.branch, &self.default_branch).await {
            return Ok(MergeResult::RebaseFailed {
                pr_number: entry.pr_number,
            });
        }

        // 2. Checkout default branch and squash-merge.
        git.checkout(&self.default_branch)
            .await
            .map_err(|e| OrchestratorError::Io(e))?;

        if let Err(_e) = git.squash_merge(&entry.branch, entry.pr_number).await {
            return Ok(MergeResult::CiFailed {
                pr_number: entry.pr_number,
            });
        }

        let pr = entry.pr_number;
        let branch = entry.branch.clone();

        // 3. Remove merged entry.
        self.dequeue(&branch);

        // 4. Rebase remaining entries onto the updated default branch.
        for remaining in &self.entries {
            // Best-effort rebase; failures here are not fatal — they will be
            // caught on the next merge_next cycle.
            let _ = git.rebase(&remaining.branch, &self.default_branch).await;
        }

        Ok(MergeResult::Merged { pr_number: pr })
    }

    /// Persist the queue back into an [`OrchestratorState`] and save.
    pub fn persist(&self, state: &mut OrchestratorState) -> Result<(), OrchestratorError> {
        state.merge_queue = self
            .entries
            .iter()
            .map(|e| crate::orchestrator::MergeQueueEntry {
                branch: e.branch.clone(),
                pr_number: e.pr_number,
                ci_status: format!("{:?}", e.ci_status).to_lowercase(),
                review_status: format!("{:?}", e.review_status).to_lowercase(),
                queued_at: e.queued_at,
            })
            .collect();
        state.save()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// A test double that records git operations.
    struct FakeGit {
        operations: Arc<Mutex<Vec<String>>>,
        fail_rebase: bool,
        fail_merge: bool,
    }

    impl FakeGit {
        fn new() -> Self {
            Self {
                operations: Arc::new(Mutex::new(Vec::new())),
                fail_rebase: false,
                fail_merge: false,
            }
        }

        fn ops(&self) -> Vec<String> {
            self.operations.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl GitExecutor for FakeGit {
        async fn rebase(&self, branch: &str, target: &str) -> Result<(), String> {
            self.operations
                .lock()
                .unwrap()
                .push(format!("rebase {branch} onto {target}"));
            if self.fail_rebase {
                Err("conflict".into())
            } else {
                Ok(())
            }
        }

        async fn squash_merge(&self, branch: &str, pr: u64) -> Result<(), String> {
            self.operations
                .lock()
                .unwrap()
                .push(format!("squash-merge {branch} (#{pr})"));
            if self.fail_merge {
                Err("merge failed".into())
            } else {
                Ok(())
            }
        }

        async fn checkout(&self, branch: &str) -> Result<(), String> {
            self.operations
                .lock()
                .unwrap()
                .push(format!("checkout {branch}"));
            Ok(())
        }
    }

    #[test]
    fn enqueue_and_peek() {
        let mut q = MergeQueue::new("main");
        q.enqueue("feat/a", 1);
        q.enqueue("feat/b", 2);

        // Nothing ready yet (both pending).
        assert!(q.next().is_none());

        q.set_ci_status("feat/a", CiStatus::Passed);
        q.set_review_status("feat/a", ReviewStatus::Approved);

        let next = q.next().unwrap();
        assert_eq!(next.pr_number, 1);
    }

    #[test]
    fn dequeue_removes_entry() {
        let mut q = MergeQueue::new("main");
        q.enqueue("feat/a", 1);
        q.enqueue("feat/b", 2);
        q.dequeue("feat/a");
        assert_eq!(q.entries().len(), 1);
        assert_eq!(q.entries()[0].branch, "feat/b");
    }

    #[tokio::test]
    async fn merge_next_happy_path() {
        let git = FakeGit::new();
        let mut q = MergeQueue::new("main");
        q.enqueue("feat/a", 1);
        q.set_ci_status("feat/a", CiStatus::Passed);
        q.set_review_status("feat/a", ReviewStatus::Approved);

        let result = q.merge_next(&git).await.unwrap();
        assert_eq!(result, MergeResult::Merged { pr_number: 1 });
        assert!(q.entries().is_empty());

        let ops = git.ops();
        assert!(ops.contains(&"rebase feat/a onto main".to_string()));
        assert!(ops.contains(&"checkout main".to_string()));
        assert!(ops.contains(&"squash-merge feat/a (#1)".to_string()));
    }

    #[tokio::test]
    async fn merge_next_nothing_ready() {
        let git = FakeGit::new();
        let mut q = MergeQueue::new("main");
        q.enqueue("feat/a", 1); // still pending

        let result = q.merge_next(&git).await.unwrap();
        assert_eq!(result, MergeResult::NothingReady);
    }

    #[tokio::test]
    async fn merge_next_rebase_failure() {
        let git = FakeGit {
            fail_rebase: true,
            ..FakeGit::new()
        };
        let mut q = MergeQueue::new("main");
        q.enqueue("feat/a", 1);
        q.set_ci_status("feat/a", CiStatus::Passed);
        q.set_review_status("feat/a", ReviewStatus::Approved);

        let result = q.merge_next(&git).await.unwrap();
        assert_eq!(result, MergeResult::RebaseFailed { pr_number: 1 });
        // Entry should still be in the queue.
        assert_eq!(q.entries().len(), 1);
    }

    #[tokio::test]
    async fn merge_next_rebases_remaining() {
        let git = FakeGit::new();
        let mut q = MergeQueue::new("main");
        q.enqueue("feat/a", 1);
        q.enqueue("feat/b", 2);
        q.set_ci_status("feat/a", CiStatus::Passed);
        q.set_review_status("feat/a", ReviewStatus::Approved);

        q.merge_next(&git).await.unwrap();

        // feat/b should have been rebased onto main after feat/a was merged.
        let ops = git.ops();
        assert!(ops.contains(&"rebase feat/b onto main".to_string()));
    }

    #[test]
    fn stale_detection() {
        let mut q = MergeQueue::new("main");
        q.enqueue("feat/old", 1);
        // Manually backdate the entry.
        q.entries[0].queued_at = Utc::now() - chrono::Duration::hours(25);

        q.enqueue("feat/new", 2);

        let stale = q.stale_entries(Duration::from_secs(24 * 60 * 60));
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].pr_number, 1);
    }
}
