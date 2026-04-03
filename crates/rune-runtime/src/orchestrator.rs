use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Active,
    Completed,
    Failed,
    Stale,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalLease {
    pub goal_key: String,
    pub owner_agent_id: String,
    pub leased_at: DateTime<Utc>,
    pub lease_expires_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovered_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovered_from_agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalConflictRecord {
    pub goal_key: String,
    pub requested_by: String,
    pub active_owner: String,
    pub detected_at: DateTime<Utc>,
    pub resolution: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEntry {
    pub agent_id: String,
    pub role: String,
    pub branch: String,
    pub worktree_path: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub github_issue: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goal_key: Option<String>,
    pub file_locks: Vec<String>,
    pub started_at: DateTime<Utc>,
    pub status: AgentStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeQueueEntry {
    pub branch: String,
    pub pr_number: u64,
    pub ci_status: String,
    pub review_status: String,
    pub queued_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorState {
    pub project: String,
    pub repo: String,
    pub default_branch: String,
    pub build_command: String,
    pub test_command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lint_command: Option<String>,
    /// Path to the actual project repository on disk
    /// (e.g. `/home/user/Development/my-project`).
    pub repo_path: PathBuf,
    pub active_agents: Vec<AgentEntry>,
    #[serde(default)]
    pub goal_leases: Vec<GoalLease>,
    #[serde(default)]
    pub goal_conflicts: Vec<GoalConflictRecord>,
    /// Glob pattern -> agent_id that holds the lock.
    pub file_locks: HashMap<String, String>,
    pub merge_queue: Vec<MergeQueueEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_merge_to_default: Option<DateTime<Utc>>,
}

impl OrchestratorState {
    /// Path to the state file under the Main Agent's workspace:
    /// `{workspace}/agents/{project}/.orchestrator-state.json`
    pub fn state_path(workspace: &Path, project: &str) -> PathBuf {
        workspace
            .join("agents")
            .join(project)
            .join(".orchestrator-state.json")
    }

    /// Read state from disk. Returns `None` when the file does not exist.
    ///
    /// `workspace` is the Main Agent workspace root (e.g. `~/.rune/workspace`).
    pub fn load(workspace: &Path, project: &str) -> Result<Option<Self>, OrchestratorError> {
        let path = Self::state_path(workspace, project);
        if !path.exists() {
            return Ok(None);
        }
        let data =
            std::fs::read_to_string(&path).map_err(|e| OrchestratorError::Io(e.to_string()))?;
        let state: Self =
            serde_json::from_str(&data).map_err(|e| OrchestratorError::Parse(e.to_string()))?;
        Ok(Some(state))
    }

    /// Persist state to disk (atomic: write tmp then rename).
    ///
    /// Creates the `{workspace}/agents/{project}/` directory if it does not
    /// exist.
    pub fn save(&self, workspace: &Path) -> Result<(), OrchestratorError> {
        let path = Self::state_path(workspace, &self.project);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| OrchestratorError::Io(e.to_string()))?;
        }
        let tmp = path.with_extension("json.tmp");
        let data = serde_json::to_string_pretty(self)
            .map_err(|e| OrchestratorError::Parse(e.to_string()))?;
        std::fs::write(&tmp, data).map_err(|e| OrchestratorError::Io(e.to_string()))?;
        std::fs::rename(&tmp, &path).map_err(|e| OrchestratorError::Io(e.to_string()))?;
        Ok(())
    }
}


#[derive(Debug, Clone)]
pub enum GoalClaimOutcome {
    Claimed(GoalLease),
    DuplicateSuppressed(GoalConflictRecord),
    RecoveredStaleLease(GoalLease),
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum OrchestratorError {
    #[error("io error: {0}")]
    Io(String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("file lock conflict: {0}")]
    LockConflict(FileLockConflict),
}

#[derive(Debug, Clone)]
pub struct FileLockConflict {
    pub requested_pattern: String,
    pub held_pattern: String,
    pub held_by: String,
}

impl std::fmt::Display for FileLockConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "pattern \"{}\" overlaps with \"{}\" held by agent \"{}\"",
            self.requested_pattern, self.held_pattern, self.held_by,
        )
    }
}

// ---------------------------------------------------------------------------
// Glob-pattern helpers (simple, no external crate)
// ---------------------------------------------------------------------------

/// Check whether two glob patterns can match the same file path.
///
/// Supports `*` (one directory level) and `**` (any depth). This is a
/// conservative check: when in doubt it reports an overlap.
pub fn patterns_overlap(a: &str, b: &str) -> bool {
    // Normalise separators.
    let a = a.replace('\\', "/");
    let b = b.replace('\\', "/");

    // If either is a prefix of the other (ignoring trailing wildcard), overlap.
    if is_prefix_match(&a, &b) || is_prefix_match(&b, &a) {
        return true;
    }

    // Exact equality after normalisation.
    a == b
}

/// Returns true when `pattern` could match paths under `prefix` (or vice
/// versa). Handles `**` as "any number of directories" and `*` as "one
/// segment".
fn is_prefix_match(pattern: &str, candidate: &str) -> bool {
    let p_parts: Vec<&str> = pattern.split('/').collect();
    let c_parts: Vec<&str> = candidate.split('/').collect();

    match_parts(&p_parts, &c_parts)
}

fn match_parts(pattern: &[&str], candidate: &[&str]) -> bool {
    if pattern.is_empty() || candidate.is_empty() {
        // One exhausted — the other may still have remaining segments which
        // the exhausted side could match via wildcard.
        return true;
    }

    let p = pattern[0];
    let c = candidate[0];

    if p == "**" {
        // `**` matches zero or more segments.
        // Try consuming 0, 1, 2, … segments from candidate.
        for skip in 0..=candidate.len() {
            if match_parts(&pattern[1..], &candidate[skip..]) {
                return true;
            }
        }
        return false;
    }

    if c == "**" {
        // symmetric
        return match_parts(candidate, pattern);
    }

    if segment_matches(p, c) {
        match_parts(&pattern[1..], &candidate[1..])
    } else {
        false
    }
}

/// Simple single-segment match supporting `*` as "anything".
fn segment_matches(pattern: &str, candidate: &str) -> bool {
    if pattern == "*" || candidate == "*" {
        return true;
    }
    pattern == candidate
}

/// Check whether a concrete file path matches a glob pattern.
pub fn path_matches_pattern(pattern: &str, path: &str) -> bool {
    let pattern = pattern.replace('\\', "/");
    let path = path.replace('\\', "/");
    let p_parts: Vec<&str> = pattern.split('/').collect();
    let f_parts: Vec<&str> = path.split('/').collect();
    path_match_parts(&p_parts, &f_parts)
}

fn path_match_parts(pattern: &[&str], path: &[&str]) -> bool {
    match (pattern.first(), path.first()) {
        (None, None) => true,
        (Some(&"**"), _) => {
            // Match zero or more path segments.
            for skip in 0..=path.len() {
                if path_match_parts(&pattern[1..], &path[skip..]) {
                    return true;
                }
            }
            false
        }
        (Some(p), Some(f)) => {
            if segment_matches(p, f) {
                path_match_parts(&pattern[1..], &path[1..])
            } else {
                false
            }
        }
        _ => false,
    }
}

impl OrchestratorState {
    pub fn claim_goal(
        &mut self,
        agent_id: &str,
        goal_key: &str,
        lease_expires_at: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> GoalClaimOutcome {
        if let Some(existing) = self
            .goal_leases
            .iter_mut()
            .find(|lease| lease.goal_key == goal_key)
        {
            if existing.owner_agent_id == agent_id {
                existing.leased_at = now;
                existing.lease_expires_at = lease_expires_at;
                let claimed = existing.clone();
                self.attach_goal_to_agent(agent_id, goal_key);
                return GoalClaimOutcome::Claimed(claimed);
            }

            if existing.lease_expires_at > now {
                let conflict = GoalConflictRecord {
                    goal_key: goal_key.to_string(),
                    requested_by: agent_id.to_string(),
                    active_owner: existing.owner_agent_id.clone(),
                    detected_at: now,
                    resolution: "duplicate_suppressed".to_string(),
                };
                self.goal_conflicts.push(conflict.clone());
                return GoalClaimOutcome::DuplicateSuppressed(conflict);
            }

            let previous_owner = existing.owner_agent_id.clone();
            existing.owner_agent_id = agent_id.to_string();
            existing.leased_at = now;
            existing.lease_expires_at = lease_expires_at;
            existing.recovered_at = Some(now);
            existing.recovered_from_agent_id = Some(previous_owner.clone());
            let recovered = existing.clone();
            self.attach_goal_to_agent(agent_id, goal_key);
            self.detach_goal_from_agent(&previous_owner, goal_key);
            return GoalClaimOutcome::RecoveredStaleLease(recovered);
        }

        let lease = GoalLease {
            goal_key: goal_key.to_string(),
            owner_agent_id: agent_id.to_string(),
            leased_at: now,
            lease_expires_at,
            recovered_at: None,
            recovered_from_agent_id: None,
        };
        self.goal_leases.push(lease.clone());
        self.attach_goal_to_agent(agent_id, goal_key);
        GoalClaimOutcome::Claimed(lease)
    }

    pub fn release_goal(&mut self, agent_id: &str, goal_key: &str) {
        self.goal_leases
            .retain(|lease| !(lease.goal_key == goal_key && lease.owner_agent_id == agent_id));
        self.detach_goal_from_agent(agent_id, goal_key);
    }

    pub fn goal_lease(&self, goal_key: &str) -> Option<&GoalLease> {
        self.goal_leases
            .iter()
            .find(|lease| lease.goal_key == goal_key)
    }

    pub fn agent_goal_lease(&self, agent_id: &str) -> Option<&GoalLease> {
        self.goal_leases
            .iter()
            .find(|lease| lease.owner_agent_id == agent_id)
    }

    pub fn stale_goal_leases(&self, now: DateTime<Utc>) -> Vec<&GoalLease> {
        self.goal_leases
            .iter()
            .filter(|lease| lease.lease_expires_at <= now)
            .collect()
    }

    pub fn active_goal_leases(&self, now: DateTime<Utc>) -> Vec<&GoalLease> {
        self.goal_leases
            .iter()
            .filter(|lease| lease.lease_expires_at > now)
            .collect()
    }

    pub fn current_goal_owners(&self, now: DateTime<Utc>) -> HashMap<String, String> {
        self.active_goal_leases(now)
            .into_iter()
            .map(|lease| (lease.goal_key.clone(), lease.owner_agent_id.clone()))
            .collect()
    }

    pub fn expire_goal_leases(&mut self, now: DateTime<Utc>) -> Vec<GoalLease> {
        let mut expired = Vec::new();
        self.goal_leases.retain(|lease| {
            if lease.lease_expires_at > now {
                return true;
            }
            expired.push(lease.clone());
            false
        });

        for lease in &expired {
            self.detach_goal_from_agent(&lease.owner_agent_id, &lease.goal_key);
        }

        expired
    }

    pub fn active_goal_owner(&self, goal_key: &str, now: DateTime<Utc>) -> Option<&GoalLease> {
        self.goal_leases
            .iter()
            .find(|lease| lease.goal_key == goal_key && lease.lease_expires_at > now)
    }

    fn attach_goal_to_agent(&mut self, agent_id: &str, goal_key: &str) {
        if let Some(agent) = self
            .active_agents
            .iter_mut()
            .find(|agent| agent.agent_id == agent_id)
        {
            agent.goal_key = Some(goal_key.to_string());
        }
    }

    fn detach_goal_from_agent(&mut self, agent_id: &str, goal_key: &str) {
        if let Some(agent) = self
            .active_agents
            .iter_mut()
            .find(|agent| agent.agent_id == agent_id)
        {
            if agent.goal_key.as_deref() == Some(goal_key) {
                agent.goal_key = None;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// FileLockManager
// ---------------------------------------------------------------------------

pub struct FileLockManager {
    state: OrchestratorState,
}

impl FileLockManager {
    pub fn new(state: OrchestratorState) -> Self {
        Self { state }
    }

    /// Consume the manager and return the (possibly mutated) state.
    pub fn into_state(self) -> OrchestratorState {
        self.state
    }

    /// Borrow the inner state.
    pub fn state(&self) -> &OrchestratorState {
        &self.state
    }

    /// Check whether any of `paths` overlap with currently held locks.
    pub fn check_overlap(&self, paths: &[String]) -> Option<FileLockConflict> {
        for requested in paths {
            for (held_pattern, held_by) in &self.state.file_locks {
                if patterns_overlap(requested, held_pattern) {
                    return Some(FileLockConflict {
                        requested_pattern: requested.clone(),
                        held_pattern: held_pattern.clone(),
                        held_by: held_by.clone(),
                    });
                }
            }
        }
        None
    }

    /// Acquire locks for the given `paths` on behalf of `agent_id`.
    ///
    /// Fails with [`FileLockConflict`] if any requested pattern overlaps with
    /// a lock held by a *different* agent.
    pub fn acquire_locks(
        &mut self,
        agent_id: &str,
        paths: &[String],
    ) -> Result<(), FileLockConflict> {
        // Check for conflicts with other agents.
        for requested in paths {
            for (held_pattern, held_by) in &self.state.file_locks {
                if held_by == agent_id {
                    continue; // re-acquiring own lock is fine
                }
                if patterns_overlap(requested, held_pattern) {
                    return Err(FileLockConflict {
                        requested_pattern: requested.clone(),
                        held_pattern: held_pattern.clone(),
                        held_by: held_by.clone(),
                    });
                }
            }
        }

        // Insert locks.
        for p in paths {
            self.state
                .file_locks
                .insert(p.clone(), agent_id.to_string());
        }

        // Update the agent entry's lock list if present.
        if let Some(agent) = self
            .state
            .active_agents
            .iter_mut()
            .find(|a| a.agent_id == agent_id)
        {
            for p in paths {
                if !agent.file_locks.contains(p) {
                    agent.file_locks.push(p.clone());
                }
            }
        }

        Ok(())
    }

    /// Release all locks held by `agent_id`.
    pub fn release_locks(&mut self, agent_id: &str) {
        self.state.file_locks.retain(|_, v| v != agent_id);

        if let Some(agent) = self
            .state
            .active_agents
            .iter_mut()
            .find(|a| a.agent_id == agent_id)
        {
            agent.file_locks.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_state() -> OrchestratorState {
        OrchestratorState {
            project: "test-project".into(),
            repo: "org/repo".into(),
            default_branch: "main".into(),
            build_command: "cargo build".into(),
            test_command: "cargo test".into(),
            lint_command: None,
            repo_path: PathBuf::from("/home/user/Development/test-project"),
            active_agents: vec![],
            goal_leases: vec![],
            goal_conflicts: vec![],
            file_locks: HashMap::new(),
            merge_queue: vec![],
            last_merge_to_default: None,
        }
    }

    // -- pattern overlap tests --

    #[test]
    fn exact_same_pattern_overlaps() {
        assert!(patterns_overlap("src/foo.rs", "src/foo.rs"));
    }

    #[test]
    fn disjoint_patterns_do_not_overlap() {
        assert!(!patterns_overlap(
            "src/module_a/foo.rs",
            "src/module_b/bar.rs"
        ));
    }

    #[test]
    fn wildcard_overlaps_concrete() {
        assert!(patterns_overlap("src/module_a/*", "src/module_a/foo.rs"));
    }

    #[test]
    fn double_star_overlaps_nested() {
        assert!(patterns_overlap("src/**", "src/a/b/c.rs"));
    }

    #[test]
    fn non_overlapping_wildcards() {
        assert!(!patterns_overlap("src/module_a/*", "src/module_b/*"));
    }

    // -- path_matches_pattern tests --

    #[test]
    fn path_matches_exact() {
        assert!(path_matches_pattern("src/main.rs", "src/main.rs"));
    }

    #[test]
    fn path_matches_star() {
        assert!(path_matches_pattern("src/*", "src/main.rs"));
        assert!(!path_matches_pattern("src/*", "src/a/b.rs"));
    }

    #[test]
    fn path_matches_double_star() {
        assert!(path_matches_pattern("src/**", "src/a/b/c.rs"));
    }

    // -- FileLockManager tests --

    #[test]
    fn acquire_and_release() {
        let state = make_state();
        let mut mgr = FileLockManager::new(state);

        mgr.acquire_locks("agent-1", &["src/module_a/*".into()])
            .unwrap();
        assert_eq!(mgr.state().file_locks.len(), 1);

        mgr.release_locks("agent-1");
        assert!(mgr.state().file_locks.is_empty());
    }

    #[test]
    fn conflict_detected() {
        let state = make_state();
        let mut mgr = FileLockManager::new(state);

        mgr.acquire_locks("agent-1", &["src/module_a/*".into()])
            .unwrap();

        let err = mgr
            .acquire_locks("agent-2", &["src/module_a/foo.rs".into()])
            .unwrap_err();
        assert_eq!(err.held_by, "agent-1");
    }

    #[test]
    fn same_agent_can_reacquire() {
        let state = make_state();
        let mut mgr = FileLockManager::new(state);

        mgr.acquire_locks("agent-1", &["src/module_a/*".into()])
            .unwrap();
        // Same agent re-acquiring an overlapping pattern is fine.
        mgr.acquire_locks("agent-1", &["src/module_a/foo.rs".into()])
            .unwrap();
    }

    #[test]
    fn no_overlap_different_modules() {
        let state = make_state();
        let mut mgr = FileLockManager::new(state);

        mgr.acquire_locks("agent-1", &["src/module_a/*".into()])
            .unwrap();
        mgr.acquire_locks("agent-2", &["src/module_b/*".into()])
            .unwrap();

        assert_eq!(mgr.state().file_locks.len(), 2);
    }

    #[test]
    fn check_overlap_returns_conflict() {
        let state = make_state();
        let mut mgr = FileLockManager::new(state);

        mgr.acquire_locks("agent-1", &["src/**".into()]).unwrap();

        let conflict = mgr.check_overlap(&["src/module_a/foo.rs".into()]);
        assert!(conflict.is_some());
        assert_eq!(conflict.unwrap().held_by, "agent-1");
    }

    #[test]
    fn check_overlap_returns_none_when_no_conflict() {
        let state = make_state();
        let mut mgr = FileLockManager::new(state);

        mgr.acquire_locks("agent-1", &["src/module_a/*".into()])
            .unwrap();

        let conflict = mgr.check_overlap(&["tests/integration.rs".into()]);
        assert!(conflict.is_none());
    }

    // -- persistence round-trip --

    #[test]
    fn save_and_load_round_trip() {
        let workspace = tempfile::tempdir().unwrap();
        let mut state = make_state();
        state.file_locks.insert("src/*".into(), "agent-1".into());

        state.save(workspace.path()).unwrap();

        let loaded = OrchestratorState::load(workspace.path(), "test-project")
            .unwrap()
            .unwrap();
        assert_eq!(loaded.project, "test-project");
        assert_eq!(loaded.file_locks.get("src/*").unwrap(), "agent-1");
    }

    #[test]
    fn save_creates_agent_directory() {
        let workspace = tempfile::tempdir().unwrap();
        let state = make_state();
        state.save(workspace.path()).unwrap();

        let expected = workspace
            .path()
            .join("agents/test-project/.orchestrator-state.json");
        assert!(expected.exists());
    }

    #[test]
    fn claim_goal_suppresses_duplicate_when_lease_is_active() {
        let mut state = make_state();
        let now = Utc::now();
        state.active_agents.push(AgentEntry {
            agent_id: "agent-1".into(),
            role: "worker".into(),
            branch: "agent/rune/a1".into(),
            worktree_path: PathBuf::from("/tmp/a1"),
            github_issue: Some("779".into()),
            goal_key: None,
            file_locks: vec![],
            started_at: now,
            status: AgentStatus::Active,
        });
        state.active_agents.push(AgentEntry {
            agent_id: "agent-2".into(),
            role: "worker".into(),
            branch: "agent/rune/a2".into(),
            worktree_path: PathBuf::from("/tmp/a2"),
            github_issue: Some("779".into()),
            goal_key: None,
            file_locks: vec![],
            started_at: now,
            status: AgentStatus::Active,
        });

        let first = state.claim_goal(
            "agent-1",
            "issue-779",
            now + chrono::Duration::minutes(5),
            now,
        );
        let GoalClaimOutcome::Claimed(first_lease) = first else {
            panic!("expected initial goal claim")
        };
        assert_eq!(first_lease.owner_agent_id, "agent-1");

        let second = state.claim_goal(
            "agent-2",
            "issue-779",
            now + chrono::Duration::minutes(5),
            now,
        );
        let GoalClaimOutcome::DuplicateSuppressed(conflict) = second else {
            panic!("expected duplicate suppression")
        };
        assert_eq!(conflict.active_owner, "agent-1");
        assert_eq!(state.goal_conflicts.len(), 1);
        assert_eq!(state.active_agents[1].goal_key, None);
    }

    #[test]
    fn claim_goal_recovers_stale_lease_and_reassigns_owner() {
        let mut state = make_state();
        let now = Utc::now();
        state.active_agents.push(AgentEntry {
            agent_id: "agent-1".into(),
            role: "worker".into(),
            branch: "agent/rune/a1".into(),
            worktree_path: PathBuf::from("/tmp/a1"),
            github_issue: Some("779".into()),
            goal_key: None,
            file_locks: vec![],
            started_at: now,
            status: AgentStatus::Stale,
        });
        state.active_agents.push(AgentEntry {
            agent_id: "agent-2".into(),
            role: "worker".into(),
            branch: "agent/rune/a2".into(),
            worktree_path: PathBuf::from("/tmp/a2"),
            github_issue: Some("779".into()),
            goal_key: None,
            file_locks: vec![],
            started_at: now,
            status: AgentStatus::Active,
        });

        let GoalClaimOutcome::Claimed(initial_lease) = state.claim_goal(
            "agent-1",
            "issue-779",
            now + chrono::Duration::seconds(30),
            now,
        ) else {
            panic!("expected initial claim")
        };
        assert_eq!(initial_lease.owner_agent_id, "agent-1");

        let recovery_time = now + chrono::Duration::minutes(2);
        let recovered = state.claim_goal(
            "agent-2",
            "issue-779",
            recovery_time + chrono::Duration::minutes(5),
            recovery_time,
        );
        let GoalClaimOutcome::RecoveredStaleLease(lease) = recovered else {
            panic!("expected stale lease recovery")
        };
        assert_eq!(lease.owner_agent_id, "agent-2");
        assert_eq!(lease.recovered_from_agent_id.as_deref(), Some("agent-1"));
        assert_eq!(state.active_agents[0].goal_key, None);
        assert_eq!(state.active_agents[1].goal_key.as_deref(), Some("issue-779"));
    }

    #[test]
    fn save_and_load_preserves_goal_leases_and_conflicts() {
        let workspace = tempfile::tempdir().unwrap();
        let mut state = make_state();
        let now = Utc::now();
        state.goal_leases.push(GoalLease {
            goal_key: "issue-779".into(),
            owner_agent_id: "agent-1".into(),
            leased_at: now,
            lease_expires_at: now + chrono::Duration::minutes(5),
            recovered_at: Some(now + chrono::Duration::minutes(1)),
            recovered_from_agent_id: Some("agent-0".into()),
        });
        state.goal_conflicts.push(GoalConflictRecord {
            goal_key: "issue-779".into(),
            requested_by: "agent-2".into(),
            active_owner: "agent-1".into(),
            detected_at: now,
            resolution: "duplicate_suppressed".into(),
        });

        state.save(workspace.path()).unwrap();
        let loaded = OrchestratorState::load(workspace.path(), "test-project")
            .unwrap()
            .unwrap();
        assert_eq!(loaded.goal_leases.len(), 1);
        assert_eq!(loaded.goal_conflicts.len(), 1);
        assert_eq!(loaded.goal_leases[0].goal_key, "issue-779");
        assert_eq!(loaded.goal_conflicts[0].resolution, "duplicate_suppressed");
    }

    #[test]
    fn active_goal_owner_filters_expired_leases() {
        let mut state = make_state();
        let now = Utc::now();
        state.goal_leases.push(GoalLease {
            goal_key: "issue-778".into(),
            owner_agent_id: "agent-1".into(),
            leased_at: now - chrono::Duration::minutes(10),
            lease_expires_at: now - chrono::Duration::minutes(5),
            recovered_at: None,
            recovered_from_agent_id: None,
        });
        state.goal_leases.push(GoalLease {
            goal_key: "issue-779".into(),
            owner_agent_id: "agent-2".into(),
            leased_at: now,
            lease_expires_at: now + chrono::Duration::minutes(5),
            recovered_at: None,
            recovered_from_agent_id: None,
        });

        assert!(state.active_goal_owner("issue-778", now).is_none());
        let lease = state
            .active_goal_owner("issue-779", now)
            .expect("active owner should be visible");
        assert_eq!(lease.owner_agent_id, "agent-2");
    }

    #[test]
    fn expire_goal_leases_releases_agent_goal_keys() {
        let mut state = make_state();
        let now = Utc::now();
        state.active_agents.push(AgentEntry {
            agent_id: "agent-1".into(),
            role: "worker".into(),
            branch: "agent/rune/a1".into(),
            worktree_path: PathBuf::from("/tmp/a1"),
            github_issue: Some("778".into()),
            goal_key: Some("issue-778".into()),
            file_locks: vec![],
            started_at: now - chrono::Duration::minutes(10),
            status: AgentStatus::Stale,
        });
        state.goal_leases.push(GoalLease {
            goal_key: "issue-778".into(),
            owner_agent_id: "agent-1".into(),
            leased_at: now - chrono::Duration::minutes(10),
            lease_expires_at: now - chrono::Duration::minutes(1),
            recovered_at: None,
            recovered_from_agent_id: None,
        });

        let expired = state.expire_goal_leases(now);
        assert_eq!(expired.len(), 1);
        assert!(state.goal_leases.is_empty());
        assert_eq!(state.active_agents[0].goal_key, None);
    }

    #[test]
    fn save_and_load_preserves_active_agent_goal_assignment() {
        let workspace = tempfile::tempdir().unwrap();
        let mut state = make_state();
        let now = Utc::now();
        state.active_agents.push(AgentEntry {
            agent_id: "agent-1".into(),
            role: "worker".into(),
            branch: "agent/rune/a1".into(),
            worktree_path: PathBuf::from("/tmp/a1"),
            github_issue: Some("778".into()),
            goal_key: Some("issue-778".into()),
            file_locks: vec!["src/**".into()],
            started_at: now,
            status: AgentStatus::Active,
        });

        state.save(workspace.path()).unwrap();
        let loaded = OrchestratorState::load(workspace.path(), "test-project")
            .unwrap()
            .unwrap();
        assert_eq!(loaded.active_agents.len(), 1);
        assert_eq!(loaded.active_agents[0].goal_key.as_deref(), Some("issue-778"));
    }

    #[test]
    fn goal_lease_helpers_surface_goal_and_agent_ownership() {
        let mut state = make_state();
        let now = Utc::now();
        state.active_agents.push(AgentEntry {
            agent_id: "agent-1".into(),
            role: "worker".into(),
            branch: "agent/rune/a1".into(),
            worktree_path: PathBuf::from("/tmp/a1"),
            github_issue: Some("766".into()),
            goal_key: None,
            file_locks: vec![],
            started_at: now,
            status: AgentStatus::Active,
        });

        let GoalClaimOutcome::Claimed(lease) = state.claim_goal(
            "agent-1",
            "issue-766",
            now + chrono::Duration::minutes(5),
            now,
        ) else {
            panic!("expected claim")
        };

        let by_goal = state.goal_lease("issue-766").expect("lease by goal");
        assert_eq!(by_goal.owner_agent_id, "agent-1");
        let by_agent = state.agent_goal_lease("agent-1").expect("lease by agent");
        assert_eq!(by_agent.goal_key, "issue-766");
        assert_eq!(by_agent.leased_at, lease.leased_at);
        assert!(state.goal_lease("issue-999").is_none());
        assert!(state.agent_goal_lease("agent-2").is_none());
    }


    #[test]
    fn active_goal_leases_only_include_unexpired_entries() {
        let mut state = make_state();
        let now = Utc::now();
        state.goal_leases.push(GoalLease {
            goal_key: "issue-stale".into(),
            owner_agent_id: "agent-1".into(),
            leased_at: now - chrono::Duration::minutes(10),
            lease_expires_at: now - chrono::Duration::minutes(1),
            recovered_at: None,
            recovered_from_agent_id: None,
        });
        state.goal_leases.push(GoalLease {
            goal_key: "issue-active".into(),
            owner_agent_id: "agent-2".into(),
            leased_at: now,
            lease_expires_at: now + chrono::Duration::minutes(5),
            recovered_at: None,
            recovered_from_agent_id: None,
        });

        let active = state.active_goal_leases(now);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].goal_key, "issue-active");
    }

    #[test]
    fn current_goal_owners_returns_goal_to_agent_map_for_active_leases() {
        let mut state = make_state();
        let now = Utc::now();
        state.goal_leases.push(GoalLease {
            goal_key: "issue-stale".into(),
            owner_agent_id: "agent-1".into(),
            leased_at: now - chrono::Duration::minutes(10),
            lease_expires_at: now - chrono::Duration::minutes(1),
            recovered_at: None,
            recovered_from_agent_id: None,
        });
        state.goal_leases.push(GoalLease {
            goal_key: "issue-766".into(),
            owner_agent_id: "agent-2".into(),
            leased_at: now,
            lease_expires_at: now + chrono::Duration::minutes(5),
            recovered_at: None,
            recovered_from_agent_id: None,
        });
        state.goal_leases.push(GoalLease {
            goal_key: "issue-765".into(),
            owner_agent_id: "agent-3".into(),
            leased_at: now,
            lease_expires_at: now + chrono::Duration::minutes(10),
            recovered_at: None,
            recovered_from_agent_id: None,
        });

        let owners = state.current_goal_owners(now);
        assert_eq!(owners.len(), 2);
        assert_eq!(owners.get("issue-766").map(String::as_str), Some("agent-2"));
        assert_eq!(owners.get("issue-765").map(String::as_str), Some("agent-3"));
        assert!(!owners.contains_key("issue-stale"));
    }

    #[test]
    fn stale_goal_leases_lists_only_expired_entries_without_mutating_state() {
        let mut state = make_state();
        let now = Utc::now();
        state.goal_leases.push(GoalLease {
            goal_key: "issue-stale".into(),
            owner_agent_id: "agent-1".into(),
            leased_at: now - chrono::Duration::minutes(10),
            lease_expires_at: now - chrono::Duration::minutes(1),
            recovered_at: None,
            recovered_from_agent_id: None,
        });
        state.goal_leases.push(GoalLease {
            goal_key: "issue-active".into(),
            owner_agent_id: "agent-2".into(),
            leased_at: now,
            lease_expires_at: now + chrono::Duration::minutes(5),
            recovered_at: None,
            recovered_from_agent_id: None,
        });

        let stale = state.stale_goal_leases(now);
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].goal_key, "issue-stale");
        assert_eq!(state.goal_leases.len(), 2);
    }

    #[test]
    fn load_returns_none_for_missing_file() {
        let workspace = tempfile::tempdir().unwrap();
        let loaded = OrchestratorState::load(workspace.path(), "nonexistent").unwrap();
        assert!(loaded.is_none());
    }
}
