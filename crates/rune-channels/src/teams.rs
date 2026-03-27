use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use reqwest::Client;
use rune_core::{AttachmentRef, ChannelId};
use serde::Deserialize;

use crate::{
    ChannelAdapter, ChannelError, ChannelMessage, DeliveryReceipt, InboundEvent, OutboundAction,
};

const TEAMS_API_BASE: &str = "https://smba.trafficmanager.net/teams";

/// Microsoft Teams adapter using the Bot Framework connector API.
///
/// Current scope:
/// - send/reply direct messages through the Bot Framework REST API
/// - normalize inbound Activity payloads into [`InboundEvent::Message`]
/// - preserve adaptive card payloads and file/image attachments as `AttachmentRef`s
///
/// Auth is based on a Bot Framework bearer token supplied by configuration.
pub struct TeamsAdapter {
    bot_token: String,
    bot_app_id: Option<String>,
    api_base: String,
    client: Client,
    pending_events: Vec<InboundEvent>,
}

impl TeamsAdapter {
    pub fn new(bot_token: impl Into<String>, bot_app_id: Option<String>) -> Self {
        Self::with_api_base(bot_token, bot_app_id, TEAMS_API_BASE)
    }

    pub fn with_api_base(
        bot_token: impl Into<String>,
        bot_app_id: Option<String>,
        api_base: impl Into<String>,
    ) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            bot_token: bot_token.into(),
            bot_app_id,
            api_base: api_base.into().trim_end_matches('/').to_string(),
            client,
            pending_events: Vec::new(),
        }
    }

    pub fn queue_inbound_activity(&mut self, activity_json: &str) -> Result<(), ChannelError> {
        let activity: TeamsActivity =
            serde_json::from_str(activity_json).map_err(|e| ChannelError::Provider {
                message: format!("failed to parse Teams activity: {e}"),
            })?;

        if let Some(event) = Self::convert_activity(&activity) {
            self.pending_events.push(event);
        }

        Ok(())
    }

    fn convert_activity(activity: &TeamsActivity) -> Option<InboundEvent> {
        if activity.activity_type.as_deref() != Some("message") {
            return None;
        }

        let chat_id = activity
            .conversation
            .as_ref()
            .and_then(|c| c.id.clone())
            .unwrap_or_default();
        let provider_message_id = activity.id.clone().unwrap_or_default();
        let sender = activity
            .from
            .as_ref()
            .and_then(|f| f.name.clone().or_else(|| f.id.clone()))
            .unwrap_or_else(|| "unknown".into());
        let content = activity
            .text
            .clone()
            .or_else(|| activity.summary.clone())
            .unwrap_or_default();
        let attachments = activity
            .attachments
            .as_deref()
            .map(Self::extract_attachments)
            .unwrap_or_default();

        if content.is_empty() && attachments.is_empty() {
            return None;
        }

        Some(InboundEvent::Message(ChannelMessage {
            channel_id: ChannelId::new(),
            raw_chat_id: chat_id,
            sender,
            content,
            attachments,
            timestamp: Utc::now(),
            provider_message_id,
        }))
    }

    fn extract_attachments(attachments: &[TeamsAttachment]) -> Vec<AttachmentRef> {
        attachments
            .iter()
            .map(|attachment| AttachmentRef {
                name: attachment.name.clone().unwrap_or_else(|| {
                    attachment
                        .content_type
                        .clone()
                        .unwrap_or_else(|| "attachment".into())
                }),
                mime_type: attachment.content_type.clone(),
                size_bytes: None,
                url: attachment.content_url.clone().or_else(|| {
                    attachment
                        .content
                        .as_ref()
                        .map(|_| "teams-inline:adaptive-card".into())
                }),
            })
            .collect()
    }

    async fn post_message(
        &self,
        chat_id: &str,
        reply_to: Option<&str>,
        content: &str,
    ) -> Result<DeliveryReceipt, ChannelError> {
        let mut url = format!("{}/v3/conversations/{chat_id}/activities", self.api_base);
        if let Some(reply_to) = reply_to {
            url = format!("{url}/{reply_to}");
        }

        let body = serde_json::json!({
            "type": "message",
            "text": content,
            "from": self.bot_app_id.as_ref().map(|id| serde_json::json!({"id": id})).unwrap_or(serde_json::Value::Null),
        });

        let response = self
            .client
            .post(&url)
            .bearer_auth(&self.bot_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| ChannelError::Provider {
                message: format!("failed to send Teams message: {e}"),
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(ChannelError::Provider {
                message: format!("Teams API request failed with {status}: {text}"),
            });
        }

        let payload: TeamsSendResponse =
            response.json().await.map_err(|e| ChannelError::Provider {
                message: format!("failed to parse Teams send response: {e}"),
            })?;

        Ok(DeliveryReceipt {
            provider_message_id: payload.id.unwrap_or_default(),
            delivered_at: Utc::now(),
        })
    }
}

#[async_trait]
impl ChannelAdapter for TeamsAdapter {
    async fn receive(&mut self) -> Result<InboundEvent, ChannelError> {
        if let Some(event) = self.pending_events.pop() {
            return Ok(event);
        }

        Err(ChannelError::NotImplemented)
    }

    async fn send(&self, action: OutboundAction) -> Result<DeliveryReceipt, ChannelError> {
        match action {
            OutboundAction::Send {
                chat_id, content, ..
            } => self.post_message(&chat_id, None, &content).await,
            OutboundAction::Reply {
                chat_id,
                reply_to,
                content,
                ..
            } => self.post_message(&chat_id, Some(&reply_to), &content).await,
            _ => Err(ChannelError::NotImplemented),
        }
    }
}

#[derive(Debug, Deserialize)]
struct TeamsSendResponse {
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TeamsActivity {
    #[serde(rename = "type")]
    activity_type: Option<String>,
    id: Option<String>,
    text: Option<String>,
    summary: Option<String>,
    from: Option<TeamsAccount>,
    conversation: Option<TeamsConversation>,
    #[serde(default)]
    attachments: Option<Vec<TeamsAttachment>>,
}

#[derive(Debug, Deserialize)]
struct TeamsAccount {
    id: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TeamsConversation {
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TeamsAttachment {
    #[serde(rename = "contentType")]
    content_type: Option<String>,
    #[serde(rename = "contentUrl")]
    content_url: Option<String>,
    name: Option<String>,
    content: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune_core::ChannelId;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn teams_activity_converts_adaptive_card_and_file_attachments() {
        let activity = serde_json::json!({
            "type": "message",
            "id": "activity-1",
            "text": "hello from teams",
            "from": { "id": "user-1", "name": "Ada" },
            "conversation": { "id": "chat-42" },
            "attachments": [
                {
                    "contentType": "application/vnd.microsoft.card.adaptive",
                    "content": { "type": "AdaptiveCard", "version": "1.5" },
                    "name": "adaptive-card"
                },
                {
                    "contentType": "image/png",
                    "contentUrl": "https://files.example/image.png",
                    "name": "image.png"
                }
            ]
        });

        let activity: TeamsActivity = serde_json::from_value(activity).unwrap();
        let event = TeamsAdapter::convert_activity(&activity).unwrap();
        let InboundEvent::Message(message) = event else {
            panic!("expected message event");
        };

        assert_eq!(message.raw_chat_id, "chat-42");
        assert_eq!(message.sender, "Ada");
        assert_eq!(message.content, "hello from teams");
        assert_eq!(message.attachments.len(), 2);
        assert_eq!(
            message.attachments[0].url.as_deref(),
            Some("teams-inline:adaptive-card")
        );
        assert_eq!(
            message.attachments[1].url.as_deref(),
            Some("https://files.example/image.png")
        );
    }

    #[tokio::test]
    async fn teams_send_posts_bot_framework_message() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v3/conversations/chat-42/activities"))
            .and(header("authorization", "Bearer bot-token"))
            .and(body_json(serde_json::json!({
                "type": "message",
                "text": "hello",
                "from": {"id": "bot-app-id"}
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "activity-99"
            })))
            .mount(&server)
            .await;

        let adapter =
            TeamsAdapter::with_api_base("bot-token", Some("bot-app-id".into()), server.uri());

        let receipt = adapter
            .send(OutboundAction::Send {
                channel_id: ChannelId::new(),
                chat_id: "chat-42".into(),
                content: "hello".into(),
            })
            .await
            .unwrap();

        assert_eq!(receipt.provider_message_id, "activity-99");
    }
}
