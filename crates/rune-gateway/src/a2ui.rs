#![allow(dead_code)]

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rune_core::ToolCategory;
use rune_tools::{ToolCall, ToolDefinition, ToolError, ToolExecutor, ToolResult};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tokio::sync::broadcast;

use crate::state::SessionEvent;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct A2uiComponent {
    #[serde(rename = "type")]
    pub component_type: String,
    pub id: String,
    #[serde(flatten)]
    pub props: Map<String, Value>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum A2uiTarget {
    #[default]
    Inline,
    Panel,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum A2uiEvent {
    Push {
        session_id: String,
        component: A2uiComponent,
        target: A2uiTarget,
        timestamp: DateTime<Utc>,
    },
    Remove {
        session_id: String,
        component_id: String,
        timestamp: DateTime<Utc>,
    },
    Reset {
        session_id: String,
        timestamp: DateTime<Utc>,
    },
    FormSubmit {
        session_id: String,
        callback_id: String,
        data: Map<String, Value>,
        timestamp: DateTime<Utc>,
    },
    Action {
        session_id: String,
        component_id: String,
        action_target: String,
        timestamp: DateTime<Utc>,
    },
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct A2uiPushParams {
    pub session_id: String,
    pub component: A2uiComponent,
    #[serde(default)]
    pub target: A2uiTarget,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct A2uiFormSubmitParams {
    pub session_id: String,
    pub callback_id: String,
    #[serde(default)]
    pub data: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct A2uiActionParams {
    pub session_id: String,
    pub component_id: String,
    pub action_target: String,
}

pub struct A2uiTool {
    event_tx: broadcast::Sender<SessionEvent>,
}

impl A2uiTool {
    #[must_use]
    pub fn new(event_tx: broadcast::Sender<SessionEvent>) -> Self {
        Self { event_tx }
    }
}

pub fn a2ui_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: "a2ui_push".to_string(),
        description: "Push a declarative UI component to the admin UI for a session.".to_string(),
        parameters: json!({
            "type": "object",
            "required": ["session_id", "component"],
            "properties": {
                "session_id": {
                    "type": "string",
                    "description": "Session that should receive the UI component"
                },
                "component": {
                    "type": "object",
                    "required": ["type", "id"],
                    "properties": {
                        "type": {
                            "type": "string",
                            "description": "Declarative component type such as card, table, list, form, chart, kv, progress, or code"
                        },
                        "id": {
                            "type": "string",
                            "description": "Stable component id used for updates"
                        }
                    },
                    "additionalProperties": true
                },
                "target": {
                    "type": "string",
                    "enum": ["inline", "panel"],
                    "description": "Where the component should render"
                }
            }
        }),
        category: ToolCategory::SessionControl,
        requires_approval: false,
    }
}

pub fn broadcast_a2ui_event(
    event_tx: &broadcast::Sender<SessionEvent>,
    event: &A2uiEvent,
) -> Result<usize, String> {
    let session_id = event.session_id().to_string();
    let payload = serde_json::to_value(event).map_err(|err| err.to_string())?;
    event_tx
        .send(SessionEvent {
            session_id,
            kind: "a2ui".to_string(),
            payload,
            state_changed: false,
        })
        .map_err(|err| err.to_string())
}

impl A2uiEvent {
    #[must_use]
    pub fn session_id(&self) -> &str {
        match self {
            Self::Push { session_id, .. }
            | Self::Remove { session_id, .. }
            | Self::Reset { session_id, .. }
            | Self::FormSubmit { session_id, .. }
            | Self::Action { session_id, .. } => session_id,
        }
    }
}

#[async_trait]
impl ToolExecutor for A2uiTool {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let params: A2uiPushParams =
            serde_json::from_value(call.arguments).map_err(|err| ToolError::InvalidArguments {
                tool: call.tool_name.clone(),
                reason: err.to_string(),
            })?;
        let event = A2uiEvent::Push {
            session_id: params.session_id,
            component: params.component,
            target: params.target,
            timestamp: Utc::now(),
        };
        broadcast_a2ui_event(&self.event_tx, &event).map_err(|err| {
            ToolError::ExecutionFailed(format!("failed to broadcast A2UI event: {err}"))
        })?;

        Ok(ToolResult {
            tool_call_id: call.tool_call_id,
            output: serde_json::to_string(&json!({
                "ok": true,
                "event": "a2ui",
                "action": "push",
                "session_id": event.session_id(),
            }))
            .map_err(|err| ToolError::ExecutionFailed(err.to_string()))?,
            is_error: false,
            tool_execution_id: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune_core::ToolCallId;

    fn sample_component() -> A2uiComponent {
        A2uiComponent {
            component_type: "card".to_string(),
            id: "deploy-card".to_string(),
            props: Map::from_iter([("title".to_string(), Value::String("Deploy".to_string()))]),
        }
    }

    #[test]
    fn a2ui_tool_definition_exposes_expected_schema() {
        let definition = a2ui_tool_definition();
        assert_eq!(definition.name, "a2ui_push");
        assert_eq!(definition.category, ToolCategory::SessionControl);
        assert_eq!(
            definition.parameters["required"],
            json!(["session_id", "component"])
        );
    }

    #[tokio::test]
    async fn broadcast_a2ui_event_sends_session_event() {
        let (tx, mut rx) = broadcast::channel(8);
        let event = A2uiEvent::Push {
            session_id: "sess-123".to_string(),
            component: sample_component(),
            target: A2uiTarget::Panel,
            timestamp: Utc::now(),
        };

        broadcast_a2ui_event(&tx, &event).expect("broadcast should succeed");

        let session_event = rx.recv().await.expect("session event should be received");
        assert_eq!(session_event.session_id, "sess-123");
        assert_eq!(session_event.kind, "a2ui");
        assert_eq!(session_event.payload["action"], "push");
        assert_eq!(session_event.payload["component"]["id"], "deploy-card");
        assert_eq!(session_event.payload["target"], "panel");
    }

    #[tokio::test]
    async fn a2ui_tool_emits_push_event() {
        let (tx, mut rx) = broadcast::channel(8);
        let tool = A2uiTool::new(tx);
        let call = ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: "a2ui_push".to_string(),
            arguments: json!({
                "session_id": "sess-456",
                "target": "inline",
                "component": {
                    "type": "card",
                    "id": "summary-card",
                    "title": "Summary"
                }
            }),
        };

        let result = tool.execute(call).await.expect("tool should succeed");
        assert!(!result.is_error);

        let session_event = rx.recv().await.expect("session event should be received");
        assert_eq!(session_event.session_id, "sess-456");
        assert_eq!(session_event.payload["component"]["type"], "card");
        assert_eq!(session_event.payload["component"]["id"], "summary-card");
    }
}
