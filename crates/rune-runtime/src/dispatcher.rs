//! Main Agent message dispatch routing.
//!
//! Analyzes inbound messages and decides whether to handle them directly
//! (questions, status queries) or route them to a project orchestrator
//! (dev work, fixes, implementations). The Main Agent stays responsive
//! because dev work runs in orchestrator subagent sessions.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use rune_core::{SessionKind, SessionStatus};
use rune_store::models::SessionRow;
use rune_store::repos::SessionRepo;

use crate::error::RuntimeError;

// ---------------------------------------------------------------------------
// Orchestrator handle & status
// ---------------------------------------------------------------------------

/// Lifecycle state of an orchestrator session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrchestratorStatus {
    Running,
    Idle,
    Stopped,
}

/// A handle to a running (or stopped) orchestrator subagent session.
#[derive(Clone, Debug)]
pub struct OrchestratorHandle {
    pub project_name: String,
    pub session_id: Uuid,
    pub status: OrchestratorStatus,
    pub started_at: DateTime<Utc>,
}

/// Compact status view returned by [`MessageDispatcher::status_summary`].
#[derive(Clone, Debug)]
pub struct OrchestratorSummary {
    pub project_name: String,
    pub session_id: Uuid,
    pub status: OrchestratorStatus,
    pub started_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// Tracks running orchestrator sessions keyed by project name.
#[derive(Debug, Default)]
pub struct OrchestratorRegistry {
    entries: HashMap<String, OrchestratorHandle>,
}

impl OrchestratorRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register (or replace) the orchestrator for a project.
    pub fn insert(&mut self, handle: OrchestratorHandle) {
        self.entries.insert(handle.project_name.clone(), handle);
    }

    /// Look up the orchestrator for a project.
    pub fn get(&self, project: &str) -> Option<&OrchestratorHandle> {
        self.entries.get(project)
    }

    /// Look up the orchestrator for a project (mutable).
    pub fn get_mut(&mut self, project: &str) -> Option<&mut OrchestratorHandle> {
        self.entries.get_mut(project)
    }

    /// Remove an orchestrator entry.
    pub fn remove(&mut self, project: &str) -> Option<OrchestratorHandle> {
        self.entries.remove(project)
    }

    /// Return handles for all tracked orchestrators.
    pub fn all(&self) -> impl Iterator<Item = &OrchestratorHandle> {
        self.entries.values()
    }

    /// Check whether a project already has an orchestrator.
    pub fn contains(&self, project: &str) -> bool {
        self.entries.contains_key(project)
    }

    /// Number of tracked orchestrators.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Dispatch decision
// ---------------------------------------------------------------------------

/// What should happen with an inbound message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DispatchDecision {
    /// Answer directly — questions, greetings, status queries.
    HandleDirectly,
    /// Forward to an existing orchestrator session for a known project.
    RouteToOrchestrator { project: String },
    /// No orchestrator exists for this project yet — spawn one first.
    SpawnOrchestrator { project: String },
}

// ---------------------------------------------------------------------------
// Keyword sets used by the classifier
// ---------------------------------------------------------------------------

/// Words that signal the message is a question / status query.
const DIRECT_KEYWORDS: &[&str] = &[
    "what", "how", "why", "when", "where", "who", "which", "status", "list",
    "show", "describe", "explain", "tell me", "help",
];

/// Words that signal dev work.
const DEV_KEYWORDS: &[&str] = &[
    "fix", "implement", "add", "refactor", "work on", "build", "create",
    "update", "remove", "delete", "migrate", "deploy", "write", "change",
    "modify", "debug", "patch", "upgrade", "rewrite",
];

// ---------------------------------------------------------------------------
// Message dispatcher
// ---------------------------------------------------------------------------

/// Routes inbound messages: handle directly or delegate to an orchestrator.
pub struct MessageDispatcher {
    orchestrators: OrchestratorRegistry,
    session_repo: Arc<dyn SessionRepo>,
}

impl MessageDispatcher {
    pub fn new(session_repo: Arc<dyn SessionRepo>) -> Self {
        Self {
            orchestrators: OrchestratorRegistry::new(),
            session_repo,
        }
    }

    /// Access the underlying orchestrator registry.
    pub fn registry(&self) -> &OrchestratorRegistry {
        &self.orchestrators
    }

    /// Mutable access to the orchestrator registry.
    pub fn registry_mut(&mut self) -> &mut OrchestratorRegistry {
        &mut self.orchestrators
    }

    // -------------------------------------------------------------------
    // Classification
    // -------------------------------------------------------------------

    /// Classify an inbound message to decide how to handle it.
    ///
    /// When a `project` hint is provided (e.g. from a channel or prior
    /// context), the decision can be `RouteToOrchestrator` or
    /// `SpawnOrchestrator`. Without a project hint, dev-like messages still
    /// return `HandleDirectly` since we cannot determine the target project.
    pub fn analyze(&self, content: &str, project_hint: Option<&str>) -> DispatchDecision {
        let lower = content.to_lowercase();

        // If the message ends with '?' or starts with a direct keyword, handle directly.
        if lower.trim_end().ends_with('?') || Self::matches_any(&lower, DIRECT_KEYWORDS) {
            return DispatchDecision::HandleDirectly;
        }

        // Check for dev-work signals.
        if Self::matches_any(&lower, DEV_KEYWORDS) {
            if let Some(project) = project_hint {
                let project = project.to_string();
                if self.orchestrators.contains(&project) {
                    return DispatchDecision::RouteToOrchestrator { project };
                } else {
                    return DispatchDecision::SpawnOrchestrator { project };
                }
            }
        }

        DispatchDecision::HandleDirectly
    }

    /// Simple keyword matcher — checks if any keyword appears in the text.
    fn matches_any(text: &str, keywords: &[&str]) -> bool {
        keywords.iter().any(|kw| text.contains(kw))
    }

    // -------------------------------------------------------------------
    // Orchestrator lifecycle
    // -------------------------------------------------------------------

    /// Spawn a new orchestrator subagent session for the given project.
    ///
    /// Creates a `Subagent` session linked to `parent_session_id`, registers
    /// it in the orchestrator registry, and returns the new session id.
    pub async fn spawn_orchestrator(
        &mut self,
        project: &str,
        parent_session_id: Uuid,
        workspace_root: Option<String>,
    ) -> Result<Uuid, RuntimeError> {
        use rune_store::models::NewSession;

        let session_id = Uuid::now_v7();
        let now = Utc::now();

        let new_session = NewSession {
            id: session_id,
            kind: serde_json::to_value(SessionKind::Subagent)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string(),
            status: SessionStatus::Created.as_str().to_string(),
            workspace_root,
            channel_ref: Some(format!("orchestrator:{project}")),
            requester_session_id: Some(parent_session_id),
            latest_turn_id: None,
            metadata: serde_json::json!({ "role": "orchestrator", "project": project }),
            created_at: now,
            updated_at: now,
            last_activity_at: now,
        };

        let row = self.session_repo.create(new_session).await?;
        let _row = self
            .session_repo
            .update_status(row.id, SessionStatus::Ready.as_str(), Utc::now())
            .await?;

        let handle = OrchestratorHandle {
            project_name: project.to_string(),
            session_id,
            status: OrchestratorStatus::Running,
            started_at: now,
        };
        self.orchestrators.insert(handle);

        tracing::info!(project, %session_id, "spawned orchestrator session");
        Ok(session_id)
    }

    /// Route a message to the orchestrator for the given project.
    ///
    /// Returns the orchestrator's session row so the caller can inject the
    /// message via `TurnExecutor`.  Returns an error if the orchestrator is
    /// not found or has stopped.
    pub async fn route_to_orchestrator(
        &self,
        project: &str,
    ) -> Result<SessionRow, RuntimeError> {
        let handle = self
            .orchestrators
            .get(project)
            .ok_or_else(|| RuntimeError::SessionNotFound(format!("orchestrator:{project}")))?;

        if handle.status == OrchestratorStatus::Stopped {
            return Err(RuntimeError::InvalidSessionState {
                expected: "running or idle".to_string(),
                actual: "stopped".to_string(),
            });
        }

        let row = self
            .session_repo
            .find_by_id(handle.session_id)
            .await
            .map_err(|_| {
                RuntimeError::SessionNotFound(format!(
                    "orchestrator session {} for project {project}",
                    handle.session_id
                ))
            })?;

        Ok(row)
    }

    /// Mark an orchestrator as stopped and update the registry.
    pub fn stop_orchestrator(&mut self, project: &str) -> bool {
        if let Some(handle) = self.orchestrators.get_mut(project) {
            handle.status = OrchestratorStatus::Stopped;
            true
        } else {
            false
        }
    }

    /// Mark an orchestrator as idle.
    pub fn idle_orchestrator(&mut self, project: &str) -> bool {
        if let Some(handle) = self.orchestrators.get_mut(project) {
            handle.status = OrchestratorStatus::Idle;
            true
        } else {
            false
        }
    }

    // -------------------------------------------------------------------
    // Reporting
    // -------------------------------------------------------------------

    /// Return a summary of all tracked orchestrators.
    pub fn status_summary(&self) -> Vec<OrchestratorSummary> {
        self.orchestrators
            .all()
            .map(|h| OrchestratorSummary {
                project_name: h.project_name.clone(),
                session_id: h.session_id,
                status: h.status,
                started_at: h.started_at,
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- OrchestratorRegistry -----------------------------------------------

    #[test]
    fn registry_insert_and_lookup() {
        let mut reg = OrchestratorRegistry::new();
        assert!(reg.is_empty());

        let handle = OrchestratorHandle {
            project_name: "acme".into(),
            session_id: Uuid::now_v7(),
            status: OrchestratorStatus::Running,
            started_at: Utc::now(),
        };
        reg.insert(handle.clone());

        assert_eq!(reg.len(), 1);
        assert!(reg.contains("acme"));
        assert!(!reg.contains("other"));

        let got = reg.get("acme").unwrap();
        assert_eq!(got.session_id, handle.session_id);
    }

    #[test]
    fn registry_remove() {
        let mut reg = OrchestratorRegistry::new();
        reg.insert(OrchestratorHandle {
            project_name: "proj".into(),
            session_id: Uuid::now_v7(),
            status: OrchestratorStatus::Idle,
            started_at: Utc::now(),
        });

        assert!(reg.remove("proj").is_some());
        assert!(reg.is_empty());
        assert!(reg.remove("proj").is_none());
    }

    // -- DispatchDecision (analyze) -----------------------------------------

    /// Helper: build a dispatcher with no session repo (analysis is sync and
    /// does not hit the store).
    fn test_dispatcher() -> MessageDispatcher {
        // We need a SessionRepo for construction but analyze() is sync.
        // Use an in-memory store stub.
        use std::sync::Arc;
        let repo: Arc<dyn SessionRepo> = Arc::new(StubSessionRepo);
        MessageDispatcher::new(repo)
    }

    #[test]
    fn analyze_question_handled_directly() {
        let d = test_dispatcher();
        assert_eq!(
            d.analyze("what is the build status?", Some("acme")),
            DispatchDecision::HandleDirectly,
        );
        assert_eq!(
            d.analyze("how does the auth module work", Some("acme")),
            DispatchDecision::HandleDirectly,
        );
    }

    #[test]
    fn analyze_trailing_question_mark_handled_directly() {
        let d = test_dispatcher();
        assert_eq!(
            d.analyze("fix the build?", Some("acme")),
            DispatchDecision::HandleDirectly,
        );
    }

    #[test]
    fn analyze_dev_command_spawns_when_no_orchestrator() {
        let d = test_dispatcher();
        assert_eq!(
            d.analyze("fix the login bug", Some("acme")),
            DispatchDecision::SpawnOrchestrator {
                project: "acme".into()
            },
        );
    }

    #[test]
    fn analyze_dev_command_routes_when_orchestrator_exists() {
        let mut d = test_dispatcher();
        d.orchestrators.insert(OrchestratorHandle {
            project_name: "acme".into(),
            session_id: Uuid::now_v7(),
            status: OrchestratorStatus::Running,
            started_at: Utc::now(),
        });

        assert_eq!(
            d.analyze("implement the new API endpoint", Some("acme")),
            DispatchDecision::RouteToOrchestrator {
                project: "acme".into()
            },
        );
    }

    #[test]
    fn analyze_dev_command_without_project_hint_falls_back() {
        let d = test_dispatcher();
        // No project hint ⇒ we cannot route, so handle directly.
        assert_eq!(
            d.analyze("fix the login bug", None),
            DispatchDecision::HandleDirectly,
        );
    }

    #[test]
    fn analyze_greeting_handled_directly() {
        let d = test_dispatcher();
        assert_eq!(
            d.analyze("hello there", Some("acme")),
            DispatchDecision::HandleDirectly,
        );
    }

    #[test]
    fn status_summary_matches_registry() {
        let mut d = test_dispatcher();
        let id = Uuid::now_v7();
        d.orchestrators.insert(OrchestratorHandle {
            project_name: "proj".into(),
            session_id: id,
            status: OrchestratorStatus::Idle,
            started_at: Utc::now(),
        });

        let summaries = d.status_summary();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].project_name, "proj");
        assert_eq!(summaries[0].session_id, id);
        assert_eq!(summaries[0].status, OrchestratorStatus::Idle);
    }

    #[test]
    fn stop_and_idle_orchestrator() {
        let mut d = test_dispatcher();
        d.orchestrators.insert(OrchestratorHandle {
            project_name: "p".into(),
            session_id: Uuid::now_v7(),
            status: OrchestratorStatus::Running,
            started_at: Utc::now(),
        });

        assert!(d.idle_orchestrator("p"));
        assert_eq!(d.orchestrators.get("p").unwrap().status, OrchestratorStatus::Idle);

        assert!(d.stop_orchestrator("p"));
        assert_eq!(d.orchestrators.get("p").unwrap().status, OrchestratorStatus::Stopped);

        // Unknown project returns false.
        assert!(!d.stop_orchestrator("nope"));
    }

    // -- Stub SessionRepo (satisfies the trait, unused in sync tests) -------

    struct StubSessionRepo;

    #[async_trait::async_trait]
    impl SessionRepo for StubSessionRepo {
        async fn create(
            &self,
            _new: rune_store::models::NewSession,
        ) -> Result<SessionRow, rune_store::StoreError> {
            unimplemented!()
        }
        async fn find_by_id(&self, _id: Uuid) -> Result<SessionRow, rune_store::StoreError> {
            unimplemented!()
        }
        async fn find_by_channel_ref(
            &self,
            _cr: &str,
        ) -> Result<Option<SessionRow>, rune_store::StoreError> {
            unimplemented!()
        }
        async fn update_status(
            &self,
            _id: Uuid,
            _status: &str,
            _now: DateTime<Utc>,
        ) -> Result<SessionRow, rune_store::StoreError> {
            unimplemented!()
        }
        async fn update_metadata(
            &self,
            _id: Uuid,
            _meta: serde_json::Value,
            _now: DateTime<Utc>,
        ) -> Result<SessionRow, rune_store::StoreError> {
            unimplemented!()
        }
        async fn update_latest_turn(
            &self,
            _id: Uuid,
            _turn_id: Uuid,
            _now: DateTime<Utc>,
        ) -> Result<SessionRow, rune_store::StoreError> {
            unimplemented!()
        }
        async fn delete(&self, _id: Uuid) -> Result<bool, rune_store::StoreError> {
            unimplemented!()
        }
        async fn list(
            &self,
            _limit: i64,
            _offset: i64,
        ) -> Result<Vec<SessionRow>, rune_store::StoreError> {
            unimplemented!()
        }
    }
}
