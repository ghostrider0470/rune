//! Granular, typed runtime event families for WebSocket streaming.
//!
//! Each event family (turn, tool, approval, process) is a self-contained enum
//! whose variants map 1:1 to dotted event-kind strings on the wire (e.g.
//! `turn.started`, `tool.completed`). The unified [`RuntimeEvent`] wrapper
//! provides dispatch helpers and converts into the existing [`SessionEvent`]
//! broadcast type so that the subscribe/filter machinery in `ws.rs` works
//! unchanged.

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::state::SessionEvent;

// ── Shared primitives ────────────────────────────────────────────────────────

/// Token usage summary attached to turn completion events.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UsageSummary {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

// ── Turn events ──────────────────────────────────────────────────────────────

/// Lifecycle events for a single model turn within a session.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TurnEvent {
    Started {
        session_id: Uuid,
        turn_id: Uuid,
        trigger: String,
    },
    Progressed {
        session_id: Uuid,
        turn_id: Uuid,
        status: String,
    },
    Completed {
        session_id: Uuid,
        turn_id: Uuid,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<UsageSummary>,
    },
    Failed {
        session_id: Uuid,
        turn_id: Uuid,
        error: String,
    },
}

impl TurnEvent {
    /// Dotted event-kind string for the `EventFrame.event` field.
    pub fn event_kind(&self) -> &'static str {
        match self {
            Self::Started { .. } => "turn.started",
            Self::Progressed { .. } => "turn.progressed",
            Self::Completed { .. } => "turn.completed",
            Self::Failed { .. } => "turn.failed",
        }
    }

    pub fn session_id(&self) -> Uuid {
        match self {
            Self::Started { session_id, .. }
            | Self::Progressed { session_id, .. }
            | Self::Completed { session_id, .. }
            | Self::Failed { session_id, .. } => *session_id,
        }
    }

    pub fn state_changed(&self) -> bool {
        match self {
            Self::Started { .. } | Self::Completed { .. } | Self::Failed { .. } => true,
            Self::Progressed { .. } => false,
        }
    }
}

// ── Tool events ──────────────────────────────────────────────────────────────

/// Lifecycle events for tool invocations within a turn.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolEvent {
    Invoked {
        session_id: Uuid,
        turn_id: Uuid,
        tool_call_id: Uuid,
        tool_name: String,
    },
    ApprovalRequired {
        session_id: Uuid,
        turn_id: Uuid,
        tool_call_id: Uuid,
        tool_name: String,
        approval_id: Uuid,
    },
    Running {
        session_id: Uuid,
        turn_id: Uuid,
        tool_call_id: Uuid,
        tool_name: String,
    },
    Completed {
        session_id: Uuid,
        turn_id: Uuid,
        tool_call_id: Uuid,
        tool_name: String,
    },
    Failed {
        session_id: Uuid,
        turn_id: Uuid,
        tool_call_id: Uuid,
        tool_name: String,
        error: String,
    },
}

impl ToolEvent {
    pub fn event_kind(&self) -> &'static str {
        match self {
            Self::Invoked { .. } => "tool.invoked",
            Self::ApprovalRequired { .. } => "tool.approval_required",
            Self::Running { .. } => "tool.running",
            Self::Completed { .. } => "tool.completed",
            Self::Failed { .. } => "tool.failed",
        }
    }

    pub fn session_id(&self) -> Uuid {
        match self {
            Self::Invoked { session_id, .. }
            | Self::ApprovalRequired { session_id, .. }
            | Self::Running { session_id, .. }
            | Self::Completed { session_id, .. }
            | Self::Failed { session_id, .. } => *session_id,
        }
    }

    pub fn state_changed(&self) -> bool {
        match self {
            Self::Invoked { .. }
            | Self::ApprovalRequired { .. }
            | Self::Completed { .. }
            | Self::Failed { .. } => true,
            Self::Running { .. } => false,
        }
    }
}

// ── Approval events ──────────────────────────────────────────────────────────

/// Lifecycle events for operator approval flows.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ApprovalEvent {
    Created {
        session_id: Uuid,
        approval_id: Uuid,
        summary: String,
    },
    Resolved {
        session_id: Uuid,
        approval_id: Uuid,
        decision: String,
    },
    Expired {
        session_id: Uuid,
        approval_id: Uuid,
    },
}

impl ApprovalEvent {
    pub fn event_kind(&self) -> &'static str {
        match self {
            Self::Created { .. } => "approval.created",
            Self::Resolved { .. } => "approval.resolved",
            Self::Expired { .. } => "approval.expired",
        }
    }

    pub fn session_id(&self) -> Uuid {
        match self {
            Self::Created { session_id, .. }
            | Self::Resolved { session_id, .. }
            | Self::Expired { session_id, .. } => *session_id,
        }
    }

    pub fn state_changed(&self) -> bool {
        // All approval lifecycle transitions change visible state.
        true
    }
}

// ── Process events ───────────────────────────────────────────────────────────

/// Lifecycle events for background processes managed by the runtime.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProcessEvent {
    Started {
        session_id: Uuid,
        process_id: String,
        command: String,
    },
    Output {
        session_id: Uuid,
        process_id: String,
        stream: String,
        data: String,
    },
    Backgrounded {
        session_id: Uuid,
        process_id: String,
    },
    Exited {
        session_id: Uuid,
        process_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
    },
}

impl ProcessEvent {
    pub fn event_kind(&self) -> &'static str {
        match self {
            Self::Started { .. } => "process.started",
            Self::Output { .. } => "process.output",
            Self::Backgrounded { .. } => "process.backgrounded",
            Self::Exited { .. } => "process.exited",
        }
    }

    pub fn session_id(&self) -> Uuid {
        match self {
            Self::Started { session_id, .. }
            | Self::Output { session_id, .. }
            | Self::Backgrounded { session_id, .. }
            | Self::Exited { session_id, .. } => *session_id,
        }
    }

    pub fn state_changed(&self) -> bool {
        match self {
            Self::Started { .. } | Self::Backgrounded { .. } | Self::Exited { .. } => true,
            Self::Output { .. } => false,
        }
    }
}

// ── Unified envelope ─────────────────────────────────────────────────────────

/// Unified wrapper for all runtime event families.
///
/// On the Rust side this gives a single dispatch point; on the wire the inner
/// event is serialised directly into [`SessionEvent::payload`] while
/// [`RuntimeEvent::event_kind`] becomes the `EventFrame.event` string.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "family", content = "event", rename_all = "snake_case")]
pub enum RuntimeEvent {
    Turn(TurnEvent),
    Tool(ToolEvent),
    Approval(ApprovalEvent),
    Process(ProcessEvent),
}

impl RuntimeEvent {
    /// Dotted event-kind string (e.g. `"turn.started"`).
    pub fn event_kind(&self) -> &'static str {
        match self {
            Self::Turn(e) => e.event_kind(),
            Self::Tool(e) => e.event_kind(),
            Self::Approval(e) => e.event_kind(),
            Self::Process(e) => e.event_kind(),
        }
    }

    /// The event family prefix (e.g. `"turn"`, `"tool"`).
    pub fn family(&self) -> &'static str {
        match self {
            Self::Turn(_) => "turn",
            Self::Tool(_) => "tool",
            Self::Approval(_) => "approval",
            Self::Process(_) => "process",
        }
    }

    /// Session UUID that owns this event.
    pub fn session_id(&self) -> Uuid {
        match self {
            Self::Turn(e) => e.session_id(),
            Self::Tool(e) => e.session_id(),
            Self::Approval(e) => e.session_id(),
            Self::Process(e) => e.session_id(),
        }
    }

    /// Whether this event should bump the connection-visible state version.
    pub fn state_changed(&self) -> bool {
        match self {
            Self::Turn(e) => e.state_changed(),
            Self::Tool(e) => e.state_changed(),
            Self::Approval(e) => e.state_changed(),
            Self::Process(e) => e.state_changed(),
        }
    }

    /// Serialise the inner event (without the `RuntimeEvent` wrapper) to JSON.
    pub fn to_payload(&self) -> serde_json::Value {
        match self {
            Self::Turn(e) => serde_json::to_value(e).unwrap_or_default(),
            Self::Tool(e) => serde_json::to_value(e).unwrap_or_default(),
            Self::Approval(e) => serde_json::to_value(e).unwrap_or_default(),
            Self::Process(e) => serde_json::to_value(e).unwrap_or_default(),
        }
    }
}

impl From<RuntimeEvent> for SessionEvent {
    fn from(event: RuntimeEvent) -> Self {
        SessionEvent {
            session_id: event.session_id().to_string(),
            kind: event.event_kind().to_string(),
            payload: event.to_payload(),
            state_changed: event.state_changed(),
        }
    }
}

// ── Broadcasting helper ──────────────────────────────────────────────────────

/// Broadcast a typed runtime event on the session event bus.
///
/// Converts the [`RuntimeEvent`] into a [`SessionEvent`] and sends it through
/// the existing broadcast channel, returning the number of active receivers.
pub fn broadcast_runtime_event(
    tx: &broadcast::Sender<SessionEvent>,
    event: RuntimeEvent,
) -> Result<usize, broadcast::error::SendError<SessionEvent>> {
    tx.send(SessionEvent::from(event))
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_session_id() -> Uuid {
        Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap()
    }

    fn sample_turn_id() -> Uuid {
        Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap()
    }

    fn sample_tool_call_id() -> Uuid {
        Uuid::parse_str("00000000-0000-0000-0000-000000000003").unwrap()
    }

    fn sample_approval_id() -> Uuid {
        Uuid::parse_str("00000000-0000-0000-0000-000000000004").unwrap()
    }

    // ── TurnEvent ────────────────────────────────────────────────────────

    #[test]
    fn turn_started_serialises_with_kind_tag() {
        let event = TurnEvent::Started {
            session_id: sample_session_id(),
            turn_id: sample_turn_id(),
            trigger: "user_message".to_string(),
        };
        let val = serde_json::to_value(&event).unwrap();
        assert_eq!(val["kind"], "started");
        assert_eq!(val["trigger"], "user_message");
    }

    #[test]
    fn turn_completed_serialises_usage_when_present() {
        let event = TurnEvent::Completed {
            session_id: sample_session_id(),
            turn_id: sample_turn_id(),
            usage: Some(UsageSummary {
                prompt_tokens: 100,
                completion_tokens: 50,
            }),
        };
        let val = serde_json::to_value(&event).unwrap();
        assert_eq!(val["usage"]["prompt_tokens"], 100);
        assert_eq!(val["usage"]["completion_tokens"], 50);
    }

    #[test]
    fn turn_completed_omits_usage_when_none() {
        let event = TurnEvent::Completed {
            session_id: sample_session_id(),
            turn_id: sample_turn_id(),
            usage: None,
        };
        let val = serde_json::to_value(&event).unwrap();
        assert!(val.get("usage").is_none());
    }

    #[test]
    fn turn_event_kinds_are_dotted() {
        let started = TurnEvent::Started {
            session_id: sample_session_id(),
            turn_id: sample_turn_id(),
            trigger: "api".to_string(),
        };
        assert_eq!(started.event_kind(), "turn.started");

        let progressed = TurnEvent::Progressed {
            session_id: sample_session_id(),
            turn_id: sample_turn_id(),
            status: "model_calling".to_string(),
        };
        assert_eq!(progressed.event_kind(), "turn.progressed");

        let completed = TurnEvent::Completed {
            session_id: sample_session_id(),
            turn_id: sample_turn_id(),
            usage: None,
        };
        assert_eq!(completed.event_kind(), "turn.completed");

        let failed = TurnEvent::Failed {
            session_id: sample_session_id(),
            turn_id: sample_turn_id(),
            error: "timeout".to_string(),
        };
        assert_eq!(failed.event_kind(), "turn.failed");
    }

    #[test]
    fn turn_state_changed_flags() {
        assert!(TurnEvent::Started {
            session_id: sample_session_id(),
            turn_id: sample_turn_id(),
            trigger: "api".to_string(),
        }
        .state_changed());

        assert!(!TurnEvent::Progressed {
            session_id: sample_session_id(),
            turn_id: sample_turn_id(),
            status: "model_calling".to_string(),
        }
        .state_changed());

        assert!(TurnEvent::Completed {
            session_id: sample_session_id(),
            turn_id: sample_turn_id(),
            usage: None,
        }
        .state_changed());

        assert!(TurnEvent::Failed {
            session_id: sample_session_id(),
            turn_id: sample_turn_id(),
            error: "oops".to_string(),
        }
        .state_changed());
    }

    // ── ToolEvent ────────────────────────────────────────────────────────

    #[test]
    fn tool_event_kinds_are_dotted() {
        assert_eq!(
            ToolEvent::Invoked {
                session_id: sample_session_id(),
                turn_id: sample_turn_id(),
                tool_call_id: sample_tool_call_id(),
                tool_name: "read".to_string(),
            }
            .event_kind(),
            "tool.invoked"
        );
        assert_eq!(
            ToolEvent::ApprovalRequired {
                session_id: sample_session_id(),
                turn_id: sample_turn_id(),
                tool_call_id: sample_tool_call_id(),
                tool_name: "bash".to_string(),
                approval_id: sample_approval_id(),
            }
            .event_kind(),
            "tool.approval_required"
        );
        assert_eq!(
            ToolEvent::Running {
                session_id: sample_session_id(),
                turn_id: sample_turn_id(),
                tool_call_id: sample_tool_call_id(),
                tool_name: "read".to_string(),
            }
            .event_kind(),
            "tool.running"
        );
        assert_eq!(
            ToolEvent::Completed {
                session_id: sample_session_id(),
                turn_id: sample_turn_id(),
                tool_call_id: sample_tool_call_id(),
                tool_name: "read".to_string(),
            }
            .event_kind(),
            "tool.completed"
        );
        assert_eq!(
            ToolEvent::Failed {
                session_id: sample_session_id(),
                turn_id: sample_turn_id(),
                tool_call_id: sample_tool_call_id(),
                tool_name: "bash".to_string(),
                error: "permission denied".to_string(),
            }
            .event_kind(),
            "tool.failed"
        );
    }

    #[test]
    fn tool_failed_serialises_error() {
        let event = ToolEvent::Failed {
            session_id: sample_session_id(),
            turn_id: sample_turn_id(),
            tool_call_id: sample_tool_call_id(),
            tool_name: "bash".to_string(),
            error: "command failed".to_string(),
        };
        let val = serde_json::to_value(&event).unwrap();
        assert_eq!(val["error"], "command failed");
        assert_eq!(val["tool_name"], "bash");
    }

    // ── ApprovalEvent ────────────────────────────────────────────────────

    #[test]
    fn approval_event_kinds_are_dotted() {
        assert_eq!(
            ApprovalEvent::Created {
                session_id: sample_session_id(),
                approval_id: sample_approval_id(),
                summary: "run rm -rf /".to_string(),
            }
            .event_kind(),
            "approval.created"
        );
        assert_eq!(
            ApprovalEvent::Resolved {
                session_id: sample_session_id(),
                approval_id: sample_approval_id(),
                decision: "allow_once".to_string(),
            }
            .event_kind(),
            "approval.resolved"
        );
        assert_eq!(
            ApprovalEvent::Expired {
                session_id: sample_session_id(),
                approval_id: sample_approval_id(),
            }
            .event_kind(),
            "approval.expired"
        );
    }

    #[test]
    fn approval_events_always_change_state() {
        assert!(ApprovalEvent::Created {
            session_id: sample_session_id(),
            approval_id: sample_approval_id(),
            summary: "test".to_string(),
        }
        .state_changed());

        assert!(ApprovalEvent::Resolved {
            session_id: sample_session_id(),
            approval_id: sample_approval_id(),
            decision: "deny".to_string(),
        }
        .state_changed());

        assert!(ApprovalEvent::Expired {
            session_id: sample_session_id(),
            approval_id: sample_approval_id(),
        }
        .state_changed());
    }

    // ── ProcessEvent ─────────────────────────────────────────────────────

    #[test]
    fn process_event_kinds_are_dotted() {
        assert_eq!(
            ProcessEvent::Started {
                session_id: sample_session_id(),
                process_id: "p-1".to_string(),
                command: "cargo test".to_string(),
            }
            .event_kind(),
            "process.started"
        );
        assert_eq!(
            ProcessEvent::Output {
                session_id: sample_session_id(),
                process_id: "p-1".to_string(),
                stream: "stdout".to_string(),
                data: "ok".to_string(),
            }
            .event_kind(),
            "process.output"
        );
        assert_eq!(
            ProcessEvent::Backgrounded {
                session_id: sample_session_id(),
                process_id: "p-1".to_string(),
            }
            .event_kind(),
            "process.backgrounded"
        );
        assert_eq!(
            ProcessEvent::Exited {
                session_id: sample_session_id(),
                process_id: "p-1".to_string(),
                exit_code: Some(0),
            }
            .event_kind(),
            "process.exited"
        );
    }

    #[test]
    fn process_output_does_not_change_state() {
        assert!(!ProcessEvent::Output {
            session_id: sample_session_id(),
            process_id: "p-1".to_string(),
            stream: "stderr".to_string(),
            data: "warning".to_string(),
        }
        .state_changed());
    }

    #[test]
    fn process_exited_omits_exit_code_when_none() {
        let event = ProcessEvent::Exited {
            session_id: sample_session_id(),
            process_id: "p-1".to_string(),
            exit_code: None,
        };
        let val = serde_json::to_value(&event).unwrap();
        assert!(val.get("exit_code").is_none());
    }

    // ── RuntimeEvent ─────────────────────────────────────────────────────

    #[test]
    fn runtime_event_delegates_to_inner_event_kind() {
        let event = RuntimeEvent::Turn(TurnEvent::Started {
            session_id: sample_session_id(),
            turn_id: sample_turn_id(),
            trigger: "api".to_string(),
        });
        assert_eq!(event.event_kind(), "turn.started");
        assert_eq!(event.family(), "turn");
    }

    #[test]
    fn runtime_event_delegates_session_id() {
        let sid = sample_session_id();
        let event = RuntimeEvent::Tool(ToolEvent::Invoked {
            session_id: sid,
            turn_id: sample_turn_id(),
            tool_call_id: sample_tool_call_id(),
            tool_name: "read".to_string(),
        });
        assert_eq!(event.session_id(), sid);
    }

    #[test]
    fn runtime_event_converts_to_session_event() {
        let event = RuntimeEvent::Turn(TurnEvent::Completed {
            session_id: sample_session_id(),
            turn_id: sample_turn_id(),
            usage: Some(UsageSummary {
                prompt_tokens: 200,
                completion_tokens: 80,
            }),
        });
        let se: SessionEvent = event.into();
        assert_eq!(se.session_id, sample_session_id().to_string());
        assert_eq!(se.kind, "turn.completed");
        assert!(se.state_changed);
        assert_eq!(se.payload["kind"], "completed");
        assert_eq!(se.payload["usage"]["prompt_tokens"], 200);
    }

    #[test]
    fn runtime_event_payload_is_inner_event_only() {
        let event = RuntimeEvent::Approval(ApprovalEvent::Created {
            session_id: sample_session_id(),
            approval_id: sample_approval_id(),
            summary: "execute dangerous command".to_string(),
        });
        let payload = event.to_payload();
        // Payload should be the inner ApprovalEvent, not wrapped in RuntimeEvent.
        assert_eq!(payload["kind"], "created");
        assert_eq!(payload["summary"], "execute dangerous command");
        assert!(payload.get("family").is_none());
    }

    #[test]
    fn runtime_event_roundtrips_through_serde() {
        let event = RuntimeEvent::Process(ProcessEvent::Exited {
            session_id: sample_session_id(),
            process_id: "bg-42".to_string(),
            exit_code: Some(1),
        });
        let json = serde_json::to_string(&event).unwrap();
        let restored: RuntimeEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.event_kind(), "process.exited");
        assert_eq!(restored.family(), "process");
    }

    // ── broadcast_runtime_event ──────────────────────────────────────────

    #[test]
    fn broadcast_sends_converted_session_event() {
        let (tx, mut rx) = broadcast::channel(8);
        let event = RuntimeEvent::Turn(TurnEvent::Failed {
            session_id: sample_session_id(),
            turn_id: sample_turn_id(),
            error: "model error".to_string(),
        });

        let receivers = broadcast_runtime_event(&tx, event).expect("send should succeed");
        assert_eq!(receivers, 1);

        let se = rx.try_recv().expect("should receive event");
        assert_eq!(se.kind, "turn.failed");
        assert_eq!(se.payload["error"], "model error");
        assert!(se.state_changed);
    }

    // ── Serde roundtrip for all families ─────────────────────────────────

    #[test]
    fn all_event_families_roundtrip() {
        let events = vec![
            RuntimeEvent::Turn(TurnEvent::Progressed {
                session_id: sample_session_id(),
                turn_id: sample_turn_id(),
                status: "model_calling".to_string(),
            }),
            RuntimeEvent::Tool(ToolEvent::Running {
                session_id: sample_session_id(),
                turn_id: sample_turn_id(),
                tool_call_id: sample_tool_call_id(),
                tool_name: "write".to_string(),
            }),
            RuntimeEvent::Approval(ApprovalEvent::Resolved {
                session_id: sample_session_id(),
                approval_id: sample_approval_id(),
                decision: "deny".to_string(),
            }),
            RuntimeEvent::Process(ProcessEvent::Started {
                session_id: sample_session_id(),
                process_id: "proc-1".to_string(),
                command: "npm start".to_string(),
            }),
        ];

        for event in &events {
            let json = serde_json::to_string(event).unwrap();
            let restored: RuntimeEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(event.event_kind(), restored.event_kind());
            assert_eq!(event.family(), restored.family());
        }
    }
}
