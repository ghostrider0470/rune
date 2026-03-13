//! Telegram Bot API channel adapter.
//!
//! Implements long-polling for inbound messages and HTTP Bot API for outbound actions.

use chrono::{TimeZone, Utc};
use reqwest::Client;
use rune_core::{AttachmentRef, ChannelId};
use serde::Deserialize;
use tracing::debug;

use async_trait::async_trait;

use crate::{
    ChannelAdapter, ChannelError, ChannelMessage, DeliveryReceipt, InboundEvent, OutboundAction,
};

/// Telegram Bot API adapter using long-polling for updates.
pub struct TelegramAdapter {
    client: Client,
    base_url: String,
    last_update_id: Option<i64>,
    pending_events: Vec<InboundEvent>,
}

impl TelegramAdapter {
    /// Create a new Telegram adapter with the given bot token.
    pub fn new(token: impl Into<String>) -> Self {
        let token = token.into();
        let base_url = format!("https://api.telegram.org/bot{token}");
        Self {
            client: Client::new(),
            base_url,
            last_update_id: None,
            pending_events: Vec::new(),
        }
    }

    /// Create with a custom base URL (for testing).
    pub fn with_base_url(_token: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.into(),
            last_update_id: None,
            pending_events: Vec::new(),
        }
    }

    /// Poll Telegram for new updates via getUpdates.
    async fn poll_updates(&mut self) -> Result<Vec<TelegramUpdate>, ChannelError> {
        let mut params = serde_json::json!({
            "timeout": 30,
            "allowed_updates": ["message", "edited_message", "callback_query"],
        });

        if let Some(offset) = self.last_update_id {
            params["offset"] = serde_json::json!(offset + 1);
        }

        let url = format!("{}/getUpdates", self.base_url);

        let response = self
            .client
            .post(&url)
            .json(&params)
            .send()
            .await
            .map_err(|e| ChannelError::Provider {
                message: format!("failed to poll updates: {e}"),
            })?;

        let body: TelegramResponse<Vec<TelegramUpdate>> =
            response.json().await.map_err(|e| ChannelError::Provider {
                message: format!("failed to parse updates response: {e}"),
            })?;

        if !body.ok {
            return Err(ChannelError::Provider {
                message: body
                    .description
                    .unwrap_or_else(|| "unknown Telegram error".into()),
            });
        }

        let updates = body.result.unwrap_or_default();

        if let Some(last) = updates.last() {
            self.last_update_id = Some(last.update_id);
        }

        Ok(updates)
    }

    /// Convert a Telegram update into an InboundEvent.
    fn convert_update(update: &TelegramUpdate) -> Option<InboundEvent> {
        if let Some(msg) = &update.message {
            let text = msg.text.clone().unwrap_or_default();
            if text.is_empty() && msg.document.is_none() && msg.photo.is_none() {
                return None;
            }

            let attachments = Self::extract_attachments(msg);

            let timestamp = Utc
                .timestamp_opt(msg.date, 0)
                .single()
                .unwrap_or_else(Utc::now);

            Some(InboundEvent::Message(ChannelMessage {
                channel_id: ChannelId::new(),
                sender: msg
                    .from
                    .as_ref()
                    .map(|u| u.username.clone().unwrap_or_else(|| u.id.to_string()))
                    .unwrap_or_else(|| "unknown".into()),
                content: text,
                attachments,
                timestamp,
                provider_message_id: msg.message_id.to_string(),
            }))
        } else {
            update
                .edited_message
                .as_ref()
                .map(|edited| InboundEvent::Edit {
                    channel_id: ChannelId::new(),
                    message_id: edited.message_id.to_string(),
                    new_content: edited.text.clone().unwrap_or_default(),
                })
        }
    }

    fn extract_attachments(msg: &TelegramMessage) -> Vec<AttachmentRef> {
        let mut attachments = Vec::new();

        if let Some(doc) = &msg.document {
            attachments.push(AttachmentRef {
                name: doc.file_name.clone().unwrap_or_else(|| "document".into()),
                mime_type: doc.mime_type.clone(),
                size_bytes: doc.file_size.map(|s| s as u64),
                url: None,
            });
        }

        if let Some(photos) = &msg.photo {
            if let Some(largest) = photos.last() {
                attachments.push(AttachmentRef {
                    name: format!("photo_{}.jpg", largest.file_id),
                    mime_type: Some("image/jpeg".into()),
                    size_bytes: largest.file_size.map(|s| s as u64),
                    url: None,
                });
            }
        }

        attachments
    }

    /// Send a message via Telegram Bot API.
    async fn send_message(
        &self,
        chat_id: &str,
        text: &str,
        reply_to: Option<&str>,
    ) -> Result<DeliveryReceipt, ChannelError> {
        let mut params = serde_json::json!({
            "chat_id": chat_id,
            "text": text,
            "parse_mode": "Markdown",
        });

        if let Some(reply_id) = reply_to {
            if let Ok(id) = reply_id.parse::<i64>() {
                params["reply_parameters"] = serde_json::json!({
                    "message_id": id,
                });
            }
        }

        let url = format!("{}/sendMessage", self.base_url);

        let response = self
            .client
            .post(&url)
            .json(&params)
            .send()
            .await
            .map_err(|e| ChannelError::Provider {
                message: format!("failed to send message: {e}"),
            })?;

        let body: TelegramResponse<TelegramMessage> =
            response.json().await.map_err(|e| ChannelError::Provider {
                message: format!("failed to parse send response: {e}"),
            })?;

        if !body.ok {
            return Err(ChannelError::Provider {
                message: body.description.unwrap_or_else(|| "send failed".into()),
            });
        }

        let msg = body.result.ok_or_else(|| ChannelError::Provider {
            message: "no message in send response".into(),
        })?;

        Ok(DeliveryReceipt {
            provider_message_id: msg.message_id.to_string(),
            delivered_at: Utc::now(),
        })
    }
}

#[async_trait]
impl ChannelAdapter for TelegramAdapter {
    async fn receive(&mut self) -> Result<InboundEvent, ChannelError> {
        loop {
            // Return from buffer first
            if let Some(event) = self.pending_events.pop() {
                return Ok(event);
            }

            // Long-poll for new updates (blocks up to `timeout` seconds)
            let updates = self.poll_updates().await?;

            for update in &updates {
                if let Some(event) = Self::convert_update(update) {
                    self.pending_events.push(event);
                }
            }

            // Reverse so we pop from the front (oldest first)
            self.pending_events.reverse();

            // If no actionable events, just loop back and poll again
        }
    }

    async fn send(&self, action: OutboundAction) -> Result<DeliveryReceipt, ChannelError> {
        match action {
            OutboundAction::Send {
                channel_id,
                content,
            } => {
                self.send_message(&channel_id.to_string(), &content, None)
                    .await
            }
            OutboundAction::Reply {
                channel_id,
                reply_to,
                content,
            } => {
                self.send_message(&channel_id.to_string(), &content, Some(&reply_to))
                    .await
            }
            OutboundAction::Edit {
                channel_id,
                message_id,
                new_content,
            } => {
                let url = format!("{}/editMessageText", self.base_url);
                let params = serde_json::json!({
                    "chat_id": channel_id.to_string(),
                    "message_id": message_id.parse::<i64>().unwrap_or(0),
                    "text": new_content,
                });

                let response = self
                    .client
                    .post(&url)
                    .json(&params)
                    .send()
                    .await
                    .map_err(|e| ChannelError::Provider {
                        message: format!("failed to edit message: {e}"),
                    })?;

                let body: TelegramResponse<TelegramMessage> =
                    response.json().await.map_err(|e| ChannelError::Provider {
                        message: format!("failed to parse edit response: {e}"),
                    })?;

                Ok(DeliveryReceipt {
                    provider_message_id: body
                        .result
                        .map(|m| m.message_id.to_string())
                        .unwrap_or_default(),
                    delivered_at: Utc::now(),
                })
            }
            OutboundAction::React { .. } => {
                // Telegram Bot API has limited reaction support
                debug!("reaction requested but Telegram Bot API support is limited");
                Ok(DeliveryReceipt {
                    provider_message_id: String::new(),
                    delivered_at: Utc::now(),
                })
            }
            OutboundAction::Delete {
                channel_id,
                message_id,
            } => {
                let url = format!("{}/deleteMessage", self.base_url);
                let params = serde_json::json!({
                    "chat_id": channel_id.to_string(),
                    "message_id": message_id.parse::<i64>().unwrap_or(0),
                });

                self.client
                    .post(&url)
                    .json(&params)
                    .send()
                    .await
                    .map_err(|e| ChannelError::Provider {
                        message: format!("failed to delete message: {e}"),
                    })?;

                Ok(DeliveryReceipt {
                    provider_message_id: message_id,
                    delivered_at: Utc::now(),
                })
            }
        }
    }
}

// ---- Telegram Bot API types ----

#[derive(Debug, Deserialize)]
struct TelegramResponse<T> {
    ok: bool,
    result: Option<T>,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TelegramUpdate {
    update_id: i64,
    message: Option<TelegramMessage>,
    edited_message: Option<TelegramMessage>,
}

#[derive(Debug, Deserialize)]
struct TelegramMessage {
    message_id: i64,
    date: i64,
    text: Option<String>,
    from: Option<TelegramUser>,
    document: Option<TelegramDocument>,
    photo: Option<Vec<TelegramPhotoSize>>,
}

#[derive(Debug, Deserialize)]
struct TelegramUser {
    id: i64,
    username: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TelegramDocument {
    #[allow(dead_code)]
    file_id: String,
    file_name: Option<String>,
    mime_type: Option<String>,
    file_size: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct TelegramPhotoSize {
    file_id: String,
    file_size: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convert_text_message_update() {
        let update = TelegramUpdate {
            update_id: 1,
            message: Some(TelegramMessage {
                message_id: 42,
                date: 1710320000,
                text: Some("hello".into()),
                from: Some(TelegramUser {
                    id: 123,
                    username: Some("testuser".into()),
                }),
                document: None,
                photo: None,
            }),
            edited_message: None,
        };

        let event = TelegramAdapter::convert_update(&update).unwrap();
        match event {
            InboundEvent::Message(msg) => {
                assert_eq!(msg.content, "hello");
                assert_eq!(msg.sender, "testuser");
                assert_eq!(msg.provider_message_id, "42");
            }
            _ => panic!("expected Message event"),
        }
    }

    #[test]
    fn convert_edit_update() {
        let update = TelegramUpdate {
            update_id: 2,
            message: None,
            edited_message: Some(TelegramMessage {
                message_id: 43,
                date: 1710320100,
                text: Some("edited text".into()),
                from: None,
                document: None,
                photo: None,
            }),
        };

        let event = TelegramAdapter::convert_update(&update).unwrap();
        match event {
            InboundEvent::Edit {
                message_id,
                new_content,
                ..
            } => {
                assert_eq!(message_id, "43");
                assert_eq!(new_content, "edited text");
            }
            _ => panic!("expected Edit event"),
        }
    }

    #[test]
    fn convert_empty_message_returns_none() {
        let update = TelegramUpdate {
            update_id: 3,
            message: Some(TelegramMessage {
                message_id: 44,
                date: 1710320200,
                text: None,
                from: None,
                document: None,
                photo: None,
            }),
            edited_message: None,
        };

        assert!(TelegramAdapter::convert_update(&update).is_none());
    }

    #[test]
    fn extract_document_attachment() {
        let msg = TelegramMessage {
            message_id: 45,
            date: 1710320300,
            text: Some("here's a file".into()),
            from: None,
            document: Some(TelegramDocument {
                file_id: "abc123".into(),
                file_name: Some("report.pdf".into()),
                mime_type: Some("application/pdf".into()),
                file_size: Some(1024),
            }),
            photo: None,
        };

        let attachments = TelegramAdapter::extract_attachments(&msg);
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].name, "report.pdf");
        assert_eq!(attachments[0].mime_type.as_deref(), Some("application/pdf"));
    }

    #[test]
    fn adapter_with_custom_base_url() {
        let adapter = TelegramAdapter::with_base_url("token", "http://localhost:8080");
        assert_eq!(adapter.base_url, "http://localhost:8080");
    }
}
