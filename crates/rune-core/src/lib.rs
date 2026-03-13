#![doc = "Core domain types and protocol-safe primitives for Rune."]

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

macro_rules! id_newtype {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            #[must_use]
            pub const fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            #[must_use]
            pub const fn as_uuid(&self) -> &Uuid {
                &self.0
            }

            #[must_use]
            pub fn into_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl From<Uuid> for $name {
            fn from(value: Uuid) -> Self {
                Self::from_uuid(value)
            }
        }

        impl From<$name> for Uuid {
            fn from(value: $name) -> Self {
                value.into_uuid()
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(s).map(Self)
            }
        }
    };
}

id_newtype!(SessionId);
id_newtype!(TurnId);
id_newtype!(ToolCallId);
id_newtype!(JobId);
id_newtype!(ApprovalId);
id_newtype!(ChannelId);

/// High-level lifecycle state for a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Created,
    Ready,
    Running,
    WaitingForTool,
    WaitingForApproval,
    WaitingForSubagent,
    Suspended,
    Completed,
    Failed,
    Cancelled,
}

/// Origin/shape classification for a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    Direct,
    Channel,
    Scheduled,
    Subagent,
}

/// Lifecycle state for an individual turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnStatus {
    Started,
    ModelCalling,
    ToolExecuting,
    Completed,
    Failed,
    Cancelled,
}

/// Operator decision for an approval request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    AllowOnce,
    AllowAlways,
    Deny,
}

/// Coarse capability bucket for built-in tools.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCategory {
    FileRead,
    FileWrite,
    ProcessExec,
    ProcessBackground,
    SessionControl,
    MemoryAccess,
    SchedulerControl,
}

/// A normalized cross-channel message representation used by adapters and runtime.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedMessage {
    pub channel_id: Option<ChannelId>,
    pub sender_id: String,
    pub sender_display_name: Option<String>,
    pub message_id: Option<String>,
    pub reply_to_message_id: Option<String>,
    pub content: String,
    pub attachments: Vec<AttachmentRef>,
    pub metadata: serde_json::Value,
}

impl NormalizedMessage {
    #[must_use]
    pub fn new(sender_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            channel_id: None,
            sender_id: sender_id.into(),
            sender_display_name: None,
            message_id: None,
            reply_to_message_id: None,
            content: content.into(),
            attachments: Vec::new(),
            metadata: serde_json::Value::Null,
        }
    }
}

/// Minimal attachment metadata preserved in normalized messages and transcript items.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentRef {
    pub name: String,
    pub mime_type: Option<String>,
    pub size_bytes: Option<u64>,
    pub url: Option<String>,
}

/// Transcript entries persisted during session execution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TranscriptItem {
    UserMessage {
        message: NormalizedMessage,
    },
    AssistantMessage {
        content: String,
    },
    ToolRequest {
        tool_call_id: ToolCallId,
        tool_name: String,
        arguments: serde_json::Value,
    },
    ToolResult {
        tool_call_id: ToolCallId,
        output: String,
        is_error: bool,
    },
    ApprovalRequest {
        approval_id: ApprovalId,
        summary: String,
        command: Option<String>,
    },
    ApprovalResponse {
        approval_id: ApprovalId,
        decision: ApprovalDecision,
        note: Option<String>,
    },
    StatusNote {
        status: SessionStatus,
        note: String,
    },
    SubagentResult {
        session_id: SessionId,
        summary: String,
    },
}

/// Typed core-domain failures that should remain transport-agnostic.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("invalid identifier for {entity}: {value}")]
    InvalidId { entity: &'static str, value: String },
    #[error("invalid state transition for {entity}: {from:?} -> {to:?}")]
    InvalidStateTransition {
        entity: &'static str,
        from: LifecycleState,
        to: LifecycleState,
    },
    #[error("invalid transcript item: {reason}")]
    InvalidTranscriptItem { reason: String },
    #[error("validation error: {message}")]
    Validation { message: String },
}

/// A coarse, transport-neutral lifecycle state view used for cross-entity error reporting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleState {
    Created,
    Ready,
    Running,
    Waiting,
    Suspended,
    Completed,
    Failed,
    Cancelled,
}

impl From<SessionStatus> for LifecycleState {
    fn from(value: SessionStatus) -> Self {
        match value {
            SessionStatus::Created => Self::Created,
            SessionStatus::Ready => Self::Ready,
            SessionStatus::Running => Self::Running,
            SessionStatus::WaitingForTool
            | SessionStatus::WaitingForApproval
            | SessionStatus::WaitingForSubagent => Self::Waiting,
            SessionStatus::Suspended => Self::Suspended,
            SessionStatus::Completed => Self::Completed,
            SessionStatus::Failed => Self::Failed,
            SessionStatus::Cancelled => Self::Cancelled,
        }
    }
}

impl From<TurnStatus> for LifecycleState {
    fn from(value: TurnStatus) -> Self {
        match value {
            TurnStatus::Started => Self::Created,
            TurnStatus::ModelCalling | TurnStatus::ToolExecuting => Self::Running,
            TurnStatus::Completed => Self::Completed,
            TurnStatus::Failed => Self::Failed,
            TurnStatus::Cancelled => Self::Cancelled,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip_id<T>()
    where
        T: Default + fmt::Display + FromStr + Serialize + for<'de> Deserialize<'de> + PartialEq + fmt::Debug,
        <T as FromStr>::Err: fmt::Debug,
    {
        let id = T::default();
        let rendered = id.to_string();
        let reparsed = rendered.parse::<T>().expect("id should parse");
        assert_eq!(id, reparsed);

        let json = serde_json::to_string(&id).expect("id should serialize");
        let decoded: T = serde_json::from_str(&json).expect("id should deserialize");
        assert_eq!(id, decoded);
    }

    #[test]
    fn ids_roundtrip_via_display_parse_and_serde() {
        roundtrip_id::<SessionId>();
        roundtrip_id::<TurnId>();
        roundtrip_id::<ToolCallId>();
        roundtrip_id::<JobId>();
        roundtrip_id::<ApprovalId>();
        roundtrip_id::<ChannelId>();
    }

    #[test]
    fn session_status_serialization_uses_snake_case() {
        let value = serde_json::to_string(&SessionStatus::WaitingForApproval).unwrap();
        assert_eq!(value, "\"waiting_for_approval\"");
    }

    #[test]
    fn turn_status_serialization_uses_snake_case() {
        let value = serde_json::to_string(&TurnStatus::ToolExecuting).unwrap();
        assert_eq!(value, "\"tool_executing\"");
    }

    #[test]
    fn approval_decision_serialization_uses_snake_case() {
        let value = serde_json::to_string(&ApprovalDecision::AllowAlways).unwrap();
        assert_eq!(value, "\"allow_always\"");
    }

    #[test]
    fn tool_category_serialization_uses_snake_case() {
        let value = serde_json::to_string(&ToolCategory::ProcessBackground).unwrap();
        assert_eq!(value, "\"process_background\"");
    }

    #[test]
    fn normalized_message_constructor_sets_safe_defaults() {
        let message = NormalizedMessage::new("user-1", "hello");
        assert_eq!(message.sender_id, "user-1");
        assert_eq!(message.content, "hello");
        assert!(message.channel_id.is_none());
        assert!(message.attachments.is_empty());
        assert_eq!(message.metadata, serde_json::Value::Null);
    }

    #[test]
    fn transcript_item_roundtrips_with_tagged_shape() {
        let item = TranscriptItem::ToolRequest {
            tool_call_id: ToolCallId::new(),
            tool_name: "read".to_string(),
            arguments: serde_json::json!({ "path": "/tmp/test.txt" }),
        };

        let json = serde_json::to_value(&item).unwrap();
        assert_eq!(json["kind"], "tool_request");

        let restored: TranscriptItem = serde_json::from_value(json).unwrap();
        assert_eq!(item, restored);
    }

    #[test]
    fn lifecycle_state_maps_waiting_session_states() {
        assert_eq!(LifecycleState::from(SessionStatus::WaitingForTool), LifecycleState::Waiting);
        assert_eq!(
            LifecycleState::from(SessionStatus::WaitingForApproval),
            LifecycleState::Waiting
        );
        assert_eq!(
            LifecycleState::from(SessionStatus::WaitingForSubagent),
            LifecycleState::Waiting
        );
    }

    #[test]
    fn lifecycle_state_maps_turn_states() {
        assert_eq!(LifecycleState::from(TurnStatus::Started), LifecycleState::Created);
        assert_eq!(LifecycleState::from(TurnStatus::ModelCalling), LifecycleState::Running);
        assert_eq!(LifecycleState::from(TurnStatus::ToolExecuting), LifecycleState::Running);
        assert_eq!(LifecycleState::from(TurnStatus::Completed), LifecycleState::Completed);
        assert_eq!(LifecycleState::from(TurnStatus::Failed), LifecycleState::Failed);
        assert_eq!(LifecycleState::from(TurnStatus::Cancelled), LifecycleState::Cancelled);
    }

    #[test]
    fn core_error_messages_are_operator_readable() {
        let err = CoreError::InvalidId {
            entity: "session",
            value: "bad-id".to_string(),
        };

        assert_eq!(err.to_string(), "invalid identifier for session: bad-id");
    }
}
