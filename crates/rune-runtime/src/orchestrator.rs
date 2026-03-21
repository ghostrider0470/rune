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
pub struct AgentEntry {
    pub agent_id: String,
    pub role: String,
    pub branch: String,
    pub worktree_path: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub github_issue: Option<String>,
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
        let data = std::fs::read_to_string(&path)
            .map_err(|e| OrchestratorError::Io(e.to_string()))?;
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
            std::fs::create_dir_all(parent)
                .map_err(|e| OrchestratorError::Io(e.to_string()))?;
        }
        let tmp = path.with_extension("json.tmp");
        let data = serde_json::to_string_pretty(self)
            .map_err(|e| OrchestratorError::Parse(e.to_string()))?;
        std::fs::write(&tmp, data).map_err(|e| OrchestratorError::Io(e.to_string()))?;
        std::fs::rename(&tmp, &path).map_err(|e| OrchestratorError::Io(e.to_string()))?;
        Ok(())
    }
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
            self.state.file_locks.insert(p.clone(), agent_id.to_string());
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
        assert!(!patterns_overlap("src/module_a/foo.rs", "src/module_b/bar.rs"));
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

        let loaded =
            OrchestratorState::load(workspace.path(), "test-project").unwrap().unwrap();
        assert_eq!(loaded.project, "test-project");
        assert_eq!(loaded.file_locks.get("src/*").unwrap(), "agent-1");
    }

    #[test]
    fn save_creates_agent_directory() {
        let workspace = tempfile::tempdir().unwrap();
        let state = make_state();
        state.save(workspace.path()).unwrap();

        let expected = workspace.path().join("agents/test-project/.orchestrator-state.json");
        assert!(expected.exists());
    }

    #[test]
    fn load_returns_none_for_missing_file() {
        let workspace = tempfile::tempdir().unwrap();
        let loaded = OrchestratorState::load(workspace.path(), "nonexistent").unwrap();
        assert!(loaded.is_none());
    }
}
