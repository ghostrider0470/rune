//! Heartbeat runner: periodic check-ins that read HEARTBEAT.md and inject it
//! as a system prompt into a session. Suppresses no-op `HEARTBEAT_OK` responses.

use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

/// Persisted heartbeat configuration state.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HeartbeatState {
    /// Whether the heartbeat runner is enabled.
    pub enabled: bool,
    /// Interval in seconds between heartbeat checks.
    pub interval_secs: u64,
    /// Timestamp of the last heartbeat run (if any).
    pub last_run_at: Option<DateTime<Utc>>,
    /// Total number of heartbeat runs since creation.
    pub run_count: u64,
    /// Number of suppressed HEARTBEAT_OK responses.
    pub suppressed_count: u64,
}

impl Default for HeartbeatState {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_secs: 1800, // 30 minutes
            last_run_at: None,
            run_count: 0,
            suppressed_count: 0,
        }
    }
}

/// Result of a single heartbeat tick.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HeartbeatTickResult {
    /// Whether the heartbeat actually fired (vs skipped).
    pub fired: bool,
    /// The prompt that was injected (if any).
    pub prompt: Option<String>,
    /// Whether the response was suppressed as HEARTBEAT_OK.
    pub suppressed: bool,
    /// Timestamp of this tick.
    pub at: DateTime<Utc>,
}

/// The heartbeat runner manages periodic check-in state.
pub struct HeartbeatRunner {
    state: Arc<Mutex<HeartbeatState>>,
    workspace_root: PathBuf,
    state_file: Option<PathBuf>,
}

impl HeartbeatRunner {
    /// Create a new heartbeat runner for the given workspace.
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            state: Arc::new(Mutex::new(HeartbeatState::default())),
            workspace_root: workspace_root.into(),
            state_file: None,
        }
    }

    /// Create a heartbeat runner with pre-existing state (e.g. loaded from disk).
    pub fn with_state(workspace_root: impl Into<PathBuf>, state: HeartbeatState) -> Self {
        Self {
            state: Arc::new(Mutex::new(state)),
            workspace_root: workspace_root.into(),
            state_file: None,
        }
    }

    /// Create a heartbeat runner backed by a state file on disk.
    pub fn with_state_file(
        workspace_root: impl Into<PathBuf>,
        state_file: impl Into<PathBuf>,
    ) -> Self {
        let state_file = state_file.into();
        let state = load_state_file(&state_file).unwrap_or_default();
        Self {
            state: Arc::new(Mutex::new(state)),
            workspace_root: workspace_root.into(),
            state_file: Some(state_file),
        }
    }

    /// Enable the heartbeat runner.
    pub async fn enable(&self) {
        let mut state = self.state.lock().await;
        state.enabled = true;
        self.persist_state(&state);
        info!("heartbeat enabled");
    }

    /// Disable the heartbeat runner.
    pub async fn disable(&self) {
        let mut state = self.state.lock().await;
        state.enabled = false;
        self.persist_state(&state);
        info!("heartbeat disabled");
    }

    /// Set the heartbeat interval in seconds.
    pub async fn set_interval(&self, interval_secs: u64) {
        let mut state = self.state.lock().await;
        state.interval_secs = interval_secs;
        self.persist_state(&state);
        info!(interval_secs, "heartbeat interval updated");
    }

    /// Get the current heartbeat state.
    pub async fn status(&self) -> HeartbeatState {
        self.state.lock().await.clone()
    }

    /// Check if a heartbeat tick is due based on interval and last run.
    pub async fn is_due(&self) -> bool {
        let state = self.state.lock().await;
        if !state.enabled {
            return false;
        }
        match state.last_run_at {
            None => true,
            Some(last) => {
                let elapsed = Utc::now().signed_duration_since(last);
                elapsed.num_seconds() >= state.interval_secs as i64
            }
        }
    }

    /// Read HEARTBEAT.md from the workspace root.
    /// Returns `None` if the file doesn't exist or is empty.
    pub fn read_heartbeat_prompt(&self) -> Option<String> {
        let path = self.workspace_root.join("HEARTBEAT.md");
        match std::fs::read_to_string(&path) {
            Ok(content) if !content.trim().is_empty() => Some(content),
            Ok(_) => {
                debug!("HEARTBEAT.md is empty");
                None
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                debug!("HEARTBEAT.md not found");
                None
            }
            Err(e) => {
                warn!(error = %e, "failed to read HEARTBEAT.md");
                None
            }
        }
    }

    /// Execute a heartbeat tick. Returns the tick result.
    ///
    /// The caller is responsible for actually sending the prompt to a session
    /// and providing the response back via `record_response`.
    pub async fn tick(&self) -> HeartbeatTickResult {
        let prompt = self.read_heartbeat_prompt();
        let now = Utc::now();

        let mut state = self.state.lock().await;
        state.last_run_at = Some(now);
        state.run_count += 1;
        self.persist_state(&state);

        HeartbeatTickResult {
            fired: prompt.is_some(),
            prompt,
            suppressed: false,
            at: now,
        }
    }

    /// Check if a response should be suppressed (is a no-op HEARTBEAT_OK).
    pub fn should_suppress(response: &str) -> bool {
        let trimmed = response.trim();
        trimmed == "HEARTBEAT_OK"
            || trimmed.eq_ignore_ascii_case("heartbeat_ok")
            || trimmed.starts_with("HEARTBEAT_OK") && trimmed.len() < 20
    }

    /// Record that a response was suppressed.
    pub async fn record_suppression(&self) {
        let mut state = self.state.lock().await;
        state.suppressed_count += 1;
        self.persist_state(&state);
    }

    fn persist_state(&self, state: &HeartbeatState) {
        let Some(path) = &self.state_file else {
            return;
        };

        if let Some(parent) = path.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            warn!(path = %parent.display(), error = %error, "failed to create heartbeat state directory");
            return;
        }

        match serde_json::to_vec_pretty(state) {
            Ok(bytes) => {
                if let Err(error) = std::fs::write(path, bytes) {
                    warn!(path = %path.display(), error = %error, "failed to persist heartbeat state");
                }
            }
            Err(error) => {
                warn!(path = %path.display(), error = %error, "failed to serialize heartbeat state");
            }
        }
    }
}

fn load_state_file(path: &PathBuf) -> Option<HeartbeatState> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice::<HeartbeatState>(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn default_state_is_disabled() {
        let tmp = TempDir::new().unwrap();
        let runner = HeartbeatRunner::new(tmp.path());
        let status = runner.status().await;
        assert!(!status.enabled);
        assert_eq!(status.interval_secs, 1800);
        assert!(status.last_run_at.is_none());
    }

    #[tokio::test]
    async fn enable_disable_toggle() {
        let tmp = TempDir::new().unwrap();
        let runner = HeartbeatRunner::new(tmp.path());

        runner.enable().await;
        assert!(runner.status().await.enabled);

        runner.disable().await;
        assert!(!runner.status().await.enabled);
    }

    #[tokio::test]
    async fn set_interval() {
        let tmp = TempDir::new().unwrap();
        let runner = HeartbeatRunner::new(tmp.path());

        runner.set_interval(900).await;
        assert_eq!(runner.status().await.interval_secs, 900);
    }

    #[tokio::test]
    async fn is_due_when_never_run() {
        let tmp = TempDir::new().unwrap();
        let runner = HeartbeatRunner::new(tmp.path());

        // Disabled → not due
        assert!(!runner.is_due().await);

        // Enabled, never run → due
        runner.enable().await;
        assert!(runner.is_due().await);
    }

    #[tokio::test]
    async fn is_due_respects_interval() {
        let tmp = TempDir::new().unwrap();
        let state = HeartbeatState {
            enabled: true,
            interval_secs: 3600,
            last_run_at: Some(Utc::now()),
            run_count: 1,
            suppressed_count: 0,
        };
        let runner = HeartbeatRunner::with_state(tmp.path(), state);

        // Just ran → not due
        assert!(!runner.is_due().await);
    }

    #[tokio::test]
    async fn is_due_after_interval_elapsed() {
        let tmp = TempDir::new().unwrap();
        let state = HeartbeatState {
            enabled: true,
            interval_secs: 60,
            last_run_at: Some(Utc::now() - chrono::Duration::seconds(120)),
            run_count: 1,
            suppressed_count: 0,
        };
        let runner = HeartbeatRunner::with_state(tmp.path(), state);

        assert!(runner.is_due().await);
    }

    #[tokio::test]
    async fn read_heartbeat_prompt_missing_file() {
        let tmp = TempDir::new().unwrap();
        let runner = HeartbeatRunner::new(tmp.path());
        assert!(runner.read_heartbeat_prompt().is_none());
    }

    #[tokio::test]
    async fn read_heartbeat_prompt_with_content() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("HEARTBEAT.md"), "Check emails").unwrap();
        let runner = HeartbeatRunner::new(tmp.path());

        let prompt = runner.read_heartbeat_prompt().unwrap();
        assert_eq!(prompt, "Check emails");
    }

    #[tokio::test]
    async fn read_heartbeat_prompt_empty_file() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("HEARTBEAT.md"), "   \n  ").unwrap();
        let runner = HeartbeatRunner::new(tmp.path());
        assert!(runner.read_heartbeat_prompt().is_none());
    }

    #[tokio::test]
    async fn tick_increments_counters() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("HEARTBEAT.md"), "Check stuff").unwrap();
        let runner = HeartbeatRunner::new(tmp.path());

        let result = runner.tick().await;
        assert!(result.fired);
        assert_eq!(result.prompt.as_deref(), Some("Check stuff"));
        assert!(!result.suppressed);

        let status = runner.status().await;
        assert_eq!(status.run_count, 1);
        assert!(status.last_run_at.is_some());
    }

    #[tokio::test]
    async fn tick_without_heartbeat_file() {
        let tmp = TempDir::new().unwrap();
        let runner = HeartbeatRunner::new(tmp.path());

        let result = runner.tick().await;
        assert!(!result.fired);
        assert!(result.prompt.is_none());
    }

    #[test]
    fn should_suppress_heartbeat_ok() {
        assert!(HeartbeatRunner::should_suppress("HEARTBEAT_OK"));
        assert!(HeartbeatRunner::should_suppress("  HEARTBEAT_OK  "));
        assert!(HeartbeatRunner::should_suppress("heartbeat_ok"));
        assert!(!HeartbeatRunner::should_suppress(
            "HEARTBEAT_OK and then some longer text here"
        ));
        assert!(!HeartbeatRunner::should_suppress("Something else entirely"));
    }

    #[tokio::test]
    async fn record_suppression_increments() {
        let tmp = TempDir::new().unwrap();
        let runner = HeartbeatRunner::new(tmp.path());

        runner.record_suppression().await;
        runner.record_suppression().await;
        assert_eq!(runner.status().await.suppressed_count, 2);
    }

    #[tokio::test]
    async fn with_state_restores() {
        let tmp = TempDir::new().unwrap();
        let state = HeartbeatState {
            enabled: true,
            interval_secs: 900,
            last_run_at: Some(Utc::now()),
            run_count: 42,
            suppressed_count: 7,
        };
        let runner = HeartbeatRunner::with_state(tmp.path(), state.clone());
        let restored = runner.status().await;
        assert_eq!(restored.enabled, state.enabled);
        assert_eq!(restored.interval_secs, state.interval_secs);
        assert_eq!(restored.run_count, state.run_count);
        assert_eq!(restored.suppressed_count, state.suppressed_count);
    }

    #[tokio::test]
    async fn state_file_persists_across_runner_reloads() {
        let tmp = TempDir::new().unwrap();
        let state_file = tmp.path().join("state").join("heartbeat-state.json");

        let runner = HeartbeatRunner::with_state_file(tmp.path(), &state_file);
        runner.enable().await;
        runner.set_interval(900).await;
        runner.record_suppression().await;
        let tick = runner.tick().await;
        assert!(!tick.fired);

        let reloaded = HeartbeatRunner::with_state_file(tmp.path(), &state_file);
        let restored = reloaded.status().await;
        assert!(restored.enabled);
        assert_eq!(restored.interval_secs, 900);
        assert_eq!(restored.run_count, 1);
        assert_eq!(restored.suppressed_count, 1);
        assert!(restored.last_run_at.is_some());
    }
}
