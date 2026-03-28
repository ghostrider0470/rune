#![doc = "Core domain types and protocol-safe primitives for Rune."]

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

macro_rules! id_newtype {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
        )]
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
// ToolCallId is a string wrapper (not UUID) because model providers return
// opaque string IDs like "call_abc123" that must be echoed back verbatim.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ToolCallId(String);

impl ToolCallId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7().to_string())
    }

    #[must_use]
    pub fn from_model(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// For backward compat with code that expects a UUID.
    /// Tries to parse as UUID; falls back to a deterministic UUID derived from the string bytes.
    #[must_use]
    pub fn into_uuid(self) -> Uuid {
        Uuid::parse_str(&self.0).unwrap_or_else(|_| {
            // Deterministic: take first 16 bytes of the string (padded) as a UUID
            let mut bytes = [0u8; 16];
            let src = self.0.as_bytes();
            for (i, b) in src.iter().take(16).enumerate() {
                bytes[i] = *b;
            }
            // Set version 4 and variant bits for a valid UUID
            bytes[6] = (bytes[6] & 0x0f) | 0x40;
            bytes[8] = (bytes[8] & 0x3f) | 0x80;
            Uuid::from_bytes(bytes)
        })
    }
}

impl Default for ToolCallId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ToolCallId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ToolCallId {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.to_owned()))
    }
}

impl From<ToolCallId> for Uuid {
    fn from(value: ToolCallId) -> Self {
        value.into_uuid()
    }
}

impl From<Uuid> for ToolCallId {
    fn from(value: Uuid) -> Self {
        Self(value.to_string())
    }
}
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
    External,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_file_id: Option<String>,
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
        tool_execution_id: Option<Uuid>,
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
    SystemInstruction {
        instruction: String,
        source: String,
    },
    ChannelDeliveryNote {
        channel: String,
        direction: String,
        summary: String,
    },
    CronHeartbeatNote {
        job_id: Option<uuid::Uuid>,
        trigger_kind: String,
        summary: String,
    },
}

/// What triggered a turn within a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerKind {
    UserMessage,
    CronJob,
    Heartbeat,
    SystemWake,
    SubagentRequest,
    Reminder,
}

impl TriggerKind {
    /// Convert to the canonical snake_case string stored in the database.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::UserMessage => "user_message",
            Self::CronJob => "cron_job",
            Self::Heartbeat => "heartbeat",
            Self::SystemWake => "system_wake",
            Self::SubagentRequest => "subagent_request",
            Self::Reminder => "reminder",
        }
    }
}

impl fmt::Display for TriggerKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TriggerKind {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "user_message" => Ok(Self::UserMessage),
            "cron_job" => Ok(Self::CronJob),
            "heartbeat" => Ok(Self::Heartbeat),
            "system_wake" => Ok(Self::SystemWake),
            "subagent_request" => Ok(Self::SubagentRequest),
            "reminder" => Ok(Self::Reminder),
            other => Err(CoreError::Validation {
                message: format!("unknown trigger kind: {other}"),
            }),
        }
    }
}

/// Semantic payload kind for durable scheduler jobs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchedulerPayloadKind {
    SystemEvent,
    AgentTurn,
    Reminder,
}

impl SchedulerPayloadKind {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SystemEvent => "system_event",
            Self::AgentTurn => "agent_turn",
            Self::Reminder => "reminder",
        }
    }
}

impl fmt::Display for SchedulerPayloadKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SchedulerPayloadKind {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "system_event" => Ok(Self::SystemEvent),
            "agent_turn" => Ok(Self::AgentTurn),
            "reminder" => Ok(Self::Reminder),
            other => Err(CoreError::Validation {
                message: format!("unknown scheduler payload kind: {other}"),
            }),
        }
    }
}

/// Delivery mode for scheduled work.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchedulerDeliveryMode {
    None,
    Announce,
    Webhook,
}

impl SchedulerDeliveryMode {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Announce => "announce",
            Self::Webhook => "webhook",
        }
    }
}

impl fmt::Display for SchedulerDeliveryMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SchedulerDeliveryMode {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "none" => Ok(Self::None),
            "announce" => Ok(Self::Announce),
            "webhook" => Ok(Self::Webhook),
            other => Err(CoreError::Validation {
                message: format!("unknown scheduler delivery mode: {other}"),
            }),
        }
    }
}

/// How a scheduled job run was triggered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchedulerRunTrigger {
    Due,
    Manual,
}

impl SchedulerRunTrigger {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Due => "due",
            Self::Manual => "manual",
        }
    }
}

impl fmt::Display for SchedulerRunTrigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SchedulerRunTrigger {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "due" => Ok(Self::Due),
            "manual" => Ok(Self::Manual),
            other => Err(CoreError::Validation {
                message: format!("unknown scheduler run trigger: {other}"),
            }),
        }
    }
}

/// Error returned when a session status transition is invalid.
#[derive(Debug, Error)]
#[error("invalid session transition: {from:?} -> {to:?}")]
pub struct TransitionError {
    pub from: SessionStatus,
    pub to: SessionStatus,
}

impl SessionStatus {
    /// Convert to the canonical snake_case string stored in the database.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Ready => "ready",
            Self::Running => "running",
            Self::WaitingForTool => "waiting_for_tool",
            Self::WaitingForApproval => "waiting_for_approval",
            Self::WaitingForSubagent => "waiting_for_subagent",
            Self::Suspended => "suspended",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Check whether transitioning from `self` to `target` is allowed.
    #[must_use]
    pub fn can_transition_to(&self, target: &SessionStatus) -> bool {
        matches!(
            (self, target),
            // Bootstrap
            (Self::Created, Self::Ready)
            // Ready → Running
            | (Self::Ready, Self::Running)
            // Running → Running (self-transition during multi-tool turns)
            | (Self::Running, Self::Running)
            // Running → terminal or waiting
            | (Self::Running, Self::WaitingForTool)
            | (Self::Running, Self::WaitingForApproval)
            | (Self::Running, Self::WaitingForSubagent)
            | (Self::Running, Self::Completed)
            | (Self::Running, Self::Failed)
            | (Self::Running, Self::Cancelled)
            // Waiting → back to running
            | (Self::WaitingForTool, Self::Running)
            | (Self::WaitingForApproval, Self::Running)
            | (Self::WaitingForSubagent, Self::Running)
            // Waiting → terminal (session can be cancelled, fail, or complete while waiting)
            | (Self::WaitingForTool, Self::Completed)
            | (Self::WaitingForTool, Self::Failed)
            | (Self::WaitingForTool, Self::Cancelled)
            | (Self::WaitingForApproval, Self::Completed)
            | (Self::WaitingForApproval, Self::Failed)
            | (Self::WaitingForApproval, Self::Cancelled)
            | (Self::WaitingForSubagent, Self::Completed)
            | (Self::WaitingForSubagent, Self::Failed)
            | (Self::WaitingForSubagent, Self::Cancelled)
            // Ready sessions may also be terminated explicitly without entering running.
            | (Self::Ready, Self::Completed)
            | (Self::Ready, Self::Failed)
            | (Self::Ready, Self::Cancelled)
            // Suspended ↔ any non-terminal
            | (Self::Suspended, Self::Ready)
            | (Self::Created, Self::Suspended)
            | (Self::Ready, Self::Suspended)
            | (Self::Running, Self::Suspended)
            | (Self::WaitingForTool, Self::Suspended)
            | (Self::WaitingForApproval, Self::Suspended)
            | (Self::WaitingForSubagent, Self::Suspended)
        )
    }

    /// Attempt the transition, returning the new status or an error.
    pub fn transition(self, target: SessionStatus) -> Result<SessionStatus, TransitionError> {
        if self.can_transition_to(&target) {
            Ok(target)
        } else {
            Err(TransitionError {
                from: self,
                to: target,
            })
        }
    }
}

impl fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SessionStatus {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "created" => Ok(Self::Created),
            "ready" => Ok(Self::Ready),
            "running" => Ok(Self::Running),
            "waiting_for_tool" => Ok(Self::WaitingForTool),
            "waiting_for_approval" => Ok(Self::WaitingForApproval),
            "waiting_for_subagent" => Ok(Self::WaitingForSubagent),
            "suspended" => Ok(Self::Suspended),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(CoreError::Validation {
                message: format!("unknown session status: {other}"),
            }),
        }
    }
}

// ── Agent templates ──────────────────────────────────────────────

/// Coarse category for grouping agent templates in listings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateCategory {
    Developer,
    Operator,
    Personal,
}

impl TemplateCategory {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Developer => "developer",
            Self::Operator => "operator",
            Self::Personal => "personal",
        }
    }
}

impl fmt::Display for TemplateCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TemplateCategory {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "developer" => Ok(Self::Developer),
            "operator" => Ok(Self::Operator),
            "personal" => Ok(Self::Personal),
            other => Err(CoreError::Validation {
                message: format!("unknown template category: {other}"),
            }),
        }
    }
}

/// A pre-built agent template that ships with the binary.
///
/// Only `Serialize` is derived — these are static definitions compiled into the
/// binary and never deserialized from external input.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AgentTemplate {
    /// URL-safe slug used as the `--template` value (e.g. `coding-agent`).
    pub slug: &'static str,
    /// Human-readable name.
    pub name: &'static str,
    /// One-line description shown in listings.
    pub description: &'static str,
    /// Category grouping for display.
    pub category: TemplateCategory,
    /// The agent mode this template activates.
    pub mode: &'static str,
    /// Spells (tool bundles) included in this template.
    pub spells: &'static [&'static str],
}

/// Return the set of agent templates that ship built-in with the binary.
///
/// These are static definitions — no I/O, no config resolution.  The gateway
/// may layer workspace-local templates on top when the full template system
/// lands.
#[must_use]
pub fn builtin_agent_templates() -> &'static [AgentTemplate] {
    &[
        AgentTemplate {
            slug: "coding-agent",
            name: "Coding Agent",
            description: "Implements features from issue descriptions",
            category: TemplateCategory::Developer,
            mode: "coder",
            spells: &["file-tools", "exec-tools", "git-tools", "test-runner"],
        },
        AgentTemplate {
            slug: "code-review-agent",
            name: "Code Review Agent",
            description: "Reviews PRs and suggests improvements",
            category: TemplateCategory::Developer,
            mode: "architect",
            spells: &["file-tools", "git-tools", "code-analysis"],
        },
        AgentTemplate {
            slug: "debug-agent",
            name: "Debug Agent",
            description: "Investigates failures and traces errors",
            category: TemplateCategory::Developer,
            mode: "debugger",
            spells: &["file-tools", "exec-tools", "log-search"],
        },
        AgentTemplate {
            slug: "devops-agent",
            name: "DevOps Agent",
            description: "Manages CI/CD, infrastructure, and releases",
            category: TemplateCategory::Developer,
            mode: "coder",
            spells: &["file-tools", "exec-tools", "deploy-tools"],
        },
        AgentTemplate {
            slug: "monitor-agent",
            name: "Monitor Agent",
            description: "Watches runtime health and alerts on anomalies",
            category: TemplateCategory::Operator,
            mode: "ask",
            spells: &["status-tools", "health-check"],
        },
        AgentTemplate {
            slug: "triage-agent",
            name: "Triage Agent",
            description: "Receives support messages, categorizes, and routes",
            category: TemplateCategory::Operator,
            mode: "architect",
            spells: &["channel-tools", "routing-tools"],
        },
        AgentTemplate {
            slug: "research-agent",
            name: "Research Agent",
            description: "Deep research with source attribution",
            category: TemplateCategory::Personal,
            mode: "architect",
            spells: &["browser-tools", "search-tools"],
        },
        AgentTemplate {
            slug: "daily-assistant",
            name: "Daily Assistant",
            description: "Manages reminders, daily notes, and proactive checks",
            category: TemplateCategory::Personal,
            mode: "orchestrator",
            spells: &["cron-tools", "heartbeat-tools"],
        },
    ]
}

/// Look up a built-in template by slug.
#[must_use]
pub fn builtin_template_by_slug(slug: &str) -> Option<&'static AgentTemplate> {
    builtin_agent_templates().iter().find(|t| t.slug == slug)
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
        T: Default
            + fmt::Display
            + FromStr
            + Serialize
            + for<'de> Deserialize<'de>
            + PartialEq
            + fmt::Debug,
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
    fn attachment_ref_serde_backfills_provider_file_id_for_legacy_payloads() {
        let attachment: AttachmentRef = serde_json::from_value(serde_json::json!({
            "name": "photo.jpg",
            "mime_type": "image/jpeg",
            "size_bytes": 42,
            "url": "https://example.com/photo.jpg"
        }))
        .unwrap();

        assert_eq!(attachment.provider_file_id, None);

        let value = serde_json::to_value(&attachment).unwrap();
        assert!(value.get("provider_file_id").is_none());
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
        assert_eq!(
            LifecycleState::from(SessionStatus::WaitingForTool),
            LifecycleState::Waiting
        );
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
        assert_eq!(
            LifecycleState::from(TurnStatus::Started),
            LifecycleState::Created
        );
        assert_eq!(
            LifecycleState::from(TurnStatus::ModelCalling),
            LifecycleState::Running
        );
        assert_eq!(
            LifecycleState::from(TurnStatus::ToolExecuting),
            LifecycleState::Running
        );
        assert_eq!(
            LifecycleState::from(TurnStatus::Completed),
            LifecycleState::Completed
        );
        assert_eq!(
            LifecycleState::from(TurnStatus::Failed),
            LifecycleState::Failed
        );
        assert_eq!(
            LifecycleState::from(TurnStatus::Cancelled),
            LifecycleState::Cancelled
        );
    }

    #[test]
    fn core_error_messages_are_operator_readable() {
        let err = CoreError::InvalidId {
            entity: "session",
            value: "bad-id".to_string(),
        };

        assert_eq!(err.to_string(), "invalid identifier for session: bad-id");
    }

    // ── SessionStatus FSM ──────────────────────────────────────────

    #[test]
    fn fsm_created_to_ready() {
        assert!(SessionStatus::Created.can_transition_to(&SessionStatus::Ready));
        assert!(
            SessionStatus::Created
                .transition(SessionStatus::Ready)
                .is_ok()
        );
    }

    #[test]
    fn fsm_ready_to_running() {
        assert!(SessionStatus::Ready.can_transition_to(&SessionStatus::Running));
    }

    #[test]
    fn fsm_running_to_waiting_variants() {
        for target in [
            SessionStatus::WaitingForTool,
            SessionStatus::WaitingForApproval,
            SessionStatus::WaitingForSubagent,
        ] {
            assert!(SessionStatus::Running.can_transition_to(&target));
        }
    }

    #[test]
    fn fsm_running_to_terminal() {
        for target in [
            SessionStatus::Completed,
            SessionStatus::Failed,
            SessionStatus::Cancelled,
        ] {
            assert!(SessionStatus::Running.can_transition_to(&target));
        }
    }

    #[test]
    fn fsm_waiting_back_to_running() {
        for from in [
            SessionStatus::WaitingForTool,
            SessionStatus::WaitingForApproval,
            SessionStatus::WaitingForSubagent,
        ] {
            assert!(from.can_transition_to(&SessionStatus::Running));
        }
    }

    #[test]
    fn fsm_waiting_to_terminal() {
        for from in [
            SessionStatus::WaitingForTool,
            SessionStatus::WaitingForApproval,
            SessionStatus::WaitingForSubagent,
        ] {
            assert!(from.can_transition_to(&SessionStatus::Completed));
            assert!(from.can_transition_to(&SessionStatus::Failed));
            assert!(from.can_transition_to(&SessionStatus::Cancelled));
        }
    }

    #[test]
    fn fsm_active_states_can_suspend() {
        for from in [
            SessionStatus::Created,
            SessionStatus::Ready,
            SessionStatus::Running,
            SessionStatus::WaitingForTool,
            SessionStatus::WaitingForApproval,
            SessionStatus::WaitingForSubagent,
        ] {
            assert!(from.can_transition_to(&SessionStatus::Suspended));
        }
    }

    #[test]
    fn fsm_terminal_states_cannot_suspend() {
        for from in [
            SessionStatus::Completed,
            SessionStatus::Failed,
            SessionStatus::Cancelled,
        ] {
            assert!(!from.can_transition_to(&SessionStatus::Suspended));
        }
    }

    #[test]
    fn fsm_suspended_to_ready() {
        assert!(SessionStatus::Suspended.can_transition_to(&SessionStatus::Ready));
    }

    #[test]
    fn fsm_rejects_invalid_transitions() {
        assert!(!SessionStatus::Created.can_transition_to(&SessionStatus::Running));
        assert!(!SessionStatus::Completed.can_transition_to(&SessionStatus::Running));
        assert!(!SessionStatus::Ready.can_transition_to(&SessionStatus::WaitingForApproval));
        assert!(
            SessionStatus::Created
                .transition(SessionStatus::Running)
                .is_err()
        );
    }

    // ── TriggerKind ──────────────────────────────────────────────

    #[test]
    fn trigger_kind_roundtrips_via_str() {
        for (s, expected) in [
            ("user_message", TriggerKind::UserMessage),
            ("cron_job", TriggerKind::CronJob),
            ("heartbeat", TriggerKind::Heartbeat),
            ("system_wake", TriggerKind::SystemWake),
            ("subagent_request", TriggerKind::SubagentRequest),
            ("reminder", TriggerKind::Reminder),
        ] {
            let parsed: TriggerKind = s.parse().unwrap();
            assert_eq!(parsed, expected);
            assert_eq!(parsed.as_str(), s);
            assert_eq!(parsed.to_string(), s);
        }
    }

    #[test]
    fn trigger_kind_serde_roundtrip() {
        let kind = TriggerKind::CronJob;
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, "\"cron_job\"");
        let back: TriggerKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, kind);
    }

    #[test]
    fn trigger_kind_rejects_unknown() {
        assert!("unknown_trigger".parse::<TriggerKind>().is_err());
    }

    #[test]
    fn scheduler_payload_kind_roundtrips_via_str() {
        for (s, expected) in [
            ("system_event", SchedulerPayloadKind::SystemEvent),
            ("agent_turn", SchedulerPayloadKind::AgentTurn),
            ("reminder", SchedulerPayloadKind::Reminder),
        ] {
            let parsed: SchedulerPayloadKind = s.parse().unwrap();
            assert_eq!(parsed, expected);
            assert_eq!(parsed.as_str(), s);
            assert_eq!(parsed.to_string(), s);
        }
    }

    #[test]
    fn scheduler_delivery_mode_roundtrips_via_str() {
        for (s, expected) in [
            ("none", SchedulerDeliveryMode::None),
            ("announce", SchedulerDeliveryMode::Announce),
            ("webhook", SchedulerDeliveryMode::Webhook),
        ] {
            let parsed: SchedulerDeliveryMode = s.parse().unwrap();
            assert_eq!(parsed, expected);
            assert_eq!(parsed.as_str(), s);
            assert_eq!(parsed.to_string(), s);
        }
    }

    #[test]
    fn scheduler_run_trigger_roundtrips_via_str() {
        for (s, expected) in [
            ("due", SchedulerRunTrigger::Due),
            ("manual", SchedulerRunTrigger::Manual),
        ] {
            let parsed: SchedulerRunTrigger = s.parse().unwrap();
            assert_eq!(parsed, expected);
            assert_eq!(parsed.as_str(), s);
            assert_eq!(parsed.to_string(), s);
        }
    }

    // ── Agent templates ──────────────────────────────────────────

    #[test]
    fn template_category_roundtrips_via_str() {
        for (s, expected) in [
            ("developer", TemplateCategory::Developer),
            ("operator", TemplateCategory::Operator),
            ("personal", TemplateCategory::Personal),
        ] {
            let parsed: TemplateCategory = s.parse().unwrap();
            assert_eq!(parsed, expected);
            assert_eq!(parsed.as_str(), s);
            assert_eq!(parsed.to_string(), s);
        }
    }

    #[test]
    fn template_category_serde_roundtrip() {
        let cat = TemplateCategory::Developer;
        let json = serde_json::to_string(&cat).unwrap();
        assert_eq!(json, "\"developer\"");
        let back: TemplateCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cat);
    }

    #[test]
    fn template_category_rejects_unknown() {
        assert!("unknown_category".parse::<TemplateCategory>().is_err());
    }

    #[test]
    fn builtin_templates_ship_at_least_four() {
        let templates = builtin_agent_templates();
        assert!(
            templates.len() >= 4,
            "issue #63 requires at least 4 built-in templates, got {}",
            templates.len()
        );
    }

    #[test]
    fn builtin_template_slugs_are_unique() {
        let templates = builtin_agent_templates();
        let mut slugs: Vec<&str> = templates.iter().map(|t| t.slug).collect();
        slugs.sort();
        slugs.dedup();
        assert_eq!(
            slugs.len(),
            templates.len(),
            "duplicate template slugs detected"
        );
    }

    #[test]
    fn builtin_template_lookup_by_slug() {
        assert!(builtin_template_by_slug("coding-agent").is_some());
        assert!(builtin_template_by_slug("nonexistent").is_none());
    }

    #[test]
    fn builtin_templates_have_all_three_categories() {
        let templates = builtin_agent_templates();
        let has_dev = templates
            .iter()
            .any(|t| t.category == TemplateCategory::Developer);
        let has_ops = templates
            .iter()
            .any(|t| t.category == TemplateCategory::Operator);
        let has_personal = templates
            .iter()
            .any(|t| t.category == TemplateCategory::Personal);
        assert!(has_dev, "missing developer templates");
        assert!(has_ops, "missing operator templates");
        assert!(has_personal, "missing personal templates");
    }

    #[test]
    fn agent_template_serde_roundtrip() {
        let template = &builtin_agent_templates()[0];
        let json = serde_json::to_value(template).unwrap();
        assert_eq!(json["slug"], template.slug);
        assert_eq!(json["name"], template.name);
        assert!(json["spells"].is_array());
    }
}
