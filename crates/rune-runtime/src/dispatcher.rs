//! Main Agent message dispatch routing.
//!
//! Provides a lightweight classifier that the Main Agent uses (via its system
//! prompt and tool-call logic) to decide whether an inbound message should be
//! handled directly or routed to a project orchestrator subagent.
//!
//! The Main Agent IS a Rune session (e.g. Telegram-connected). Orchestrators
//! are subagent sessions, each with their own workspace pointing at a project
//! repo.  The dispatcher provides the *analysis* — the Main Agent acts on it
//! by calling `SessionEngine` to create/steer subagent sessions.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use uuid::Uuid;

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
// Registry — in-memory tracker of active orchestrator sessions
// ---------------------------------------------------------------------------

/// Tracks running orchestrator sessions keyed by project name.
///
/// This is purely in-memory runtime state — it does not persist to disk.
/// The Main Agent populates it on startup by scanning active subagent sessions
/// whose `channel_ref` matches `orchestrator:{project}`.
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

    /// Mark an orchestrator as idle. Returns `false` if not found.
    pub fn mark_idle(&mut self, project: &str) -> bool {
        if let Some(h) = self.entries.get_mut(project) {
            h.status = OrchestratorStatus::Idle;
            true
        } else {
            false
        }
    }

    /// Mark an orchestrator as stopped. Returns `false` if not found.
    pub fn mark_stopped(&mut self, project: &str) -> bool {
        if let Some(h) = self.entries.get_mut(project) {
            h.status = OrchestratorStatus::Stopped;
            true
        } else {
            false
        }
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
    "what", "how", "why", "when", "where", "who", "which", "status", "list", "show", "describe",
    "explain", "tell me", "help",
];

/// Words that signal dev work.
const DEV_KEYWORDS: &[&str] = &[
    "fix",
    "implement",
    "add",
    "refactor",
    "work on",
    "build",
    "create",
    "update",
    "remove",
    "delete",
    "migrate",
    "deploy",
    "write",
    "change",
    "modify",
    "debug",
    "patch",
    "upgrade",
    "rewrite",
];

// ---------------------------------------------------------------------------
// Message dispatcher
// ---------------------------------------------------------------------------

/// Analyzes inbound messages and decides how the Main Agent should handle them.
///
/// This is a lightweight helper — it does not own sessions or repos. The Main
/// Agent calls [`MessageDispatcher::analyze`] and then acts on the decision
/// using `SessionEngine` (to spawn subagent sessions) or `TurnExecutor` (to
/// inject messages into existing orchestrator sessions).
pub struct MessageDispatcher {
    orchestrators: OrchestratorRegistry,
}

impl MessageDispatcher {
    pub fn new() -> Self {
        Self {
            orchestrators: OrchestratorRegistry::new(),
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

    /// Classify an inbound message to decide how to handle it.
    ///
    /// When a `project` hint is provided (e.g. from channel context or an
    /// explicit project mention), the decision can be `RouteToOrchestrator`
    /// or `SpawnOrchestrator`. Without a project hint, dev-like messages
    /// return `HandleDirectly` since we cannot determine the target project.
    pub fn analyze(&self, content: &str, project_hint: Option<&str>) -> DispatchDecision {
        let lower = content.to_lowercase();

        // Questions or status queries → handle directly.
        if lower.trim_end().ends_with('?') || Self::matches_any(&lower, DIRECT_KEYWORDS) {
            return DispatchDecision::HandleDirectly;
        }

        // Dev-work signals → route to project orchestrator.
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

    fn matches_any(text: &str, keywords: &[&str]) -> bool {
        keywords.iter().any(|kw| text.contains(kw))
    }
}

impl Default for MessageDispatcher {
    fn default() -> Self {
        Self::new()
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

    #[test]
    fn registry_mark_idle_and_stopped() {
        let mut reg = OrchestratorRegistry::new();
        reg.insert(OrchestratorHandle {
            project_name: "p".into(),
            session_id: Uuid::now_v7(),
            status: OrchestratorStatus::Running,
            started_at: Utc::now(),
        });

        assert!(reg.mark_idle("p"));
        assert_eq!(reg.get("p").unwrap().status, OrchestratorStatus::Idle);

        assert!(reg.mark_stopped("p"));
        assert_eq!(reg.get("p").unwrap().status, OrchestratorStatus::Stopped);

        assert!(!reg.mark_idle("nope"));
        assert!(!reg.mark_stopped("nope"));
    }

    // -- DispatchDecision (analyze) -----------------------------------------

    #[test]
    fn analyze_question_handled_directly() {
        let d = MessageDispatcher::new();
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
        let d = MessageDispatcher::new();
        assert_eq!(
            d.analyze("fix the build?", Some("acme")),
            DispatchDecision::HandleDirectly,
        );
    }

    #[test]
    fn analyze_dev_command_spawns_when_no_orchestrator() {
        let d = MessageDispatcher::new();
        assert_eq!(
            d.analyze("fix the login bug", Some("acme")),
            DispatchDecision::SpawnOrchestrator {
                project: "acme".into()
            },
        );
    }

    #[test]
    fn analyze_dev_command_routes_when_orchestrator_exists() {
        let mut d = MessageDispatcher::new();
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
        let d = MessageDispatcher::new();
        assert_eq!(
            d.analyze("fix the login bug", None),
            DispatchDecision::HandleDirectly,
        );
    }

    #[test]
    fn analyze_greeting_handled_directly() {
        let d = MessageDispatcher::new();
        assert_eq!(
            d.analyze("hello there", Some("acme")),
            DispatchDecision::HandleDirectly,
        );
    }

    #[test]
    fn status_summary_matches_registry() {
        let mut d = MessageDispatcher::new();
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
}
