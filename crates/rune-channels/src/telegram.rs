//! Telegram Bot API channel adapter.
//!
//! Implements long-polling for inbound messages and HTTP Bot API for outbound actions.
//! Supports voice, audio, video_note, photo, and document media with file download.

use std::time::Duration;

use chrono::{TimeZone, Utc};
use reqwest::Client;
use reqwest::multipart::{Form, Part};
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
    fn build_client(timeout: Duration) -> Client {
        Client::builder()
            .timeout(timeout)
            .build()
            .unwrap_or_else(|_| Client::new())
    }

    /// Create a new Telegram adapter with the given bot token.
    pub fn new(token: impl Into<String>) -> Self {
        let token = token.into();
        let base_url = format!("https://api.telegram.org/bot{token}");
        Self {
            client: Self::build_client(Duration::from_secs(35)),
            base_url,
            last_update_id: None,
            pending_events: Vec::new(),
        }
    }

    /// Create with a custom base URL (for testing).
    pub fn with_base_url(_token: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            client: Self::build_client(Duration::from_secs(2)),
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
            // Use text, or caption (for media messages with captions), or empty
            let text = msg
                .text
                .clone()
                .or_else(|| msg.caption.clone())
                .unwrap_or_default();

            let has_media = msg.document.is_some()
                || msg.photo.is_some()
                || msg.voice.is_some()
                || msg.audio.is_some()
                || msg.video_note.is_some()
                || msg.video.is_some();

            if text.is_empty() && !has_media {
                return None;
            }

            let attachments = Self::extract_attachments(msg);

            let timestamp = Utc
                .timestamp_opt(msg.date, 0)
                .single()
                .unwrap_or_else(Utc::now);

            Some(InboundEvent::Message(ChannelMessage {
                channel_id: ChannelId::new(),
                raw_chat_id: msg.chat.id.to_string(),
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
        } else if let Some(cb) = &update.callback_query {
            let data = cb.data.clone().unwrap_or_default();
            let chat_id = cb
                .message
                .as_ref()
                .map(|m| m.chat.id.to_string())
                .unwrap_or_default();
            let message_id = cb
                .message
                .as_ref()
                .map(|m| m.message_id.to_string())
                .unwrap_or_default();

            Some(InboundEvent::Message(ChannelMessage {
                channel_id: ChannelId::new(),
                raw_chat_id: chat_id,
                sender: cb
                    .from
                    .username
                    .clone()
                    .unwrap_or_else(|| cb.from.id.to_string()),
                content: data,
                attachments: Vec::new(),
                timestamp: Utc::now(),
                provider_message_id: message_id,
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
                url: Some(format!("telegram-file:{}", doc.file_id)),
            });
        }

        if let Some(photos) = &msg.photo {
            if let Some(largest) = photos.last() {
                attachments.push(AttachmentRef {
                    name: format!("photo_{}.jpg", largest.file_id),
                    mime_type: Some("image/jpeg".into()),
                    size_bytes: largest.file_size.map(|s| s as u64),
                    url: Some(format!("telegram-file:{}", largest.file_id)),
                });
            }
        }

        if let Some(voice) = &msg.voice {
            attachments.push(AttachmentRef {
                name: "voice.ogg".into(),
                mime_type: Some(
                    voice
                        .mime_type
                        .clone()
                        .unwrap_or_else(|| "audio/ogg".into()),
                ),
                size_bytes: voice.file_size.map(|s| s as u64),
                url: Some(format!("telegram-file:{}", voice.file_id)),
            });
        }

        if let Some(audio) = &msg.audio {
            attachments.push(AttachmentRef {
                name: "audio.mp3".into(),
                mime_type: Some(
                    audio
                        .mime_type
                        .clone()
                        .unwrap_or_else(|| "audio/mpeg".into()),
                ),
                size_bytes: audio.file_size.map(|s| s as u64),
                url: Some(format!("telegram-file:{}", audio.file_id)),
            });
        }

        if let Some(video_note) = &msg.video_note {
            attachments.push(AttachmentRef {
                name: "video_note.mp4".into(),
                mime_type: Some("video/mp4".into()),
                size_bytes: video_note.file_size.map(|s| s as u64),
                url: Some(format!("telegram-file:{}", video_note.file_id)),
            });
        }

        if let Some(video) = &msg.video {
            attachments.push(AttachmentRef {
                name: "video.mp4".into(),
                mime_type: Some(
                    video
                        .mime_type
                        .clone()
                        .unwrap_or_else(|| "video/mp4".into()),
                ),
                size_bytes: video.file_size.map(|s| s as u64),
                url: Some(format!("telegram-file:{}", video.file_id)),
            });
        }

        attachments
    }

    /// Download a file from Telegram servers by file_id.
    /// Returns (bytes, file_path) on success.
    pub async fn download_file(&self, file_id: &str) -> Result<(Vec<u8>, String), ChannelError> {
        // Step 1: getFile to get the file_path
        let url = format!("{}/getFile", self.base_url);
        let params = serde_json::json!({ "file_id": file_id });
        let resp = self
            .client
            .post(&url)
            .json(&params)
            .send()
            .await
            .map_err(|e| ChannelError::Provider {
                message: format!("getFile failed: {e}"),
            })?;
        let body: serde_json::Value = resp.json().await.map_err(|e| ChannelError::Provider {
            message: format!("getFile parse failed: {e}"),
        })?;
        let file_path = body
            .get("result")
            .and_then(|r| r.get("file_path"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| ChannelError::Provider {
                message: "getFile: no file_path in response".into(),
            })?
            .to_string();

        // Step 2: download the actual file bytes
        let download_url = self.base_url.replace("/bot", "/file/bot");
        let download_url = format!("{download_url}/{file_path}");
        let file_resp = self
            .client
            .get(&download_url)
            .send()
            .await
            .map_err(|e| ChannelError::Provider {
                message: format!("file download failed: {e}"),
            })?;
        let bytes = file_resp
            .bytes()
            .await
            .map_err(|e| ChannelError::Provider {
                message: format!("file read failed: {e}"),
            })?;

        Ok((bytes.to_vec(), file_path))
    }



    /// Send synthesized audio back to Telegram as either a voice note bubble or audio file.
    pub async fn send_audio_bytes(
        &self,
        chat_id: &str,
        audio: &[u8],
        reply_to: Option<&str>,
        as_voice: bool,
        caption: Option<&str>,
    ) -> Result<DeliveryReceipt, ChannelError> {
        let endpoint = if as_voice { "sendVoice" } else { "sendAudio" };
        let url = format!("{}/{}", self.base_url, endpoint);

        let filename = if as_voice { "voice.mp3" } else { "audio.mp3" };
        let field_name = if as_voice { "voice" } else { "audio" };
        let file_part = Part::bytes(audio.to_vec())
            .file_name(filename.to_string())
            .mime_str("audio/mpeg")
            .map_err(|e| ChannelError::Provider {
                message: format!("failed to build audio multipart: {e}"),
            })?;

        let mut form = Form::new()
            .text("chat_id", chat_id.to_string())
            .part(field_name.to_string(), file_part);

        if let Some(reply_id) = reply_to {
            if let Ok(id) = reply_id.parse::<i64>() {
                form = form.text(
                    "reply_parameters",
                    serde_json::json!({ "message_id": id }).to_string(),
                );
            }
        }

        if let Some(caption) = caption.filter(|c| !c.is_empty()) {
            form = form.text("caption", caption.to_string());
        }

        let response = self
            .client
            .post(&url)
            .multipart(form)
            .send()
            .await
            .map_err(|e| ChannelError::Provider {
                message: format!("failed to send audio: {e}"),
            })?;

        let body: TelegramResponse<TelegramMessage> =
            response.json().await.map_err(|e| ChannelError::Provider {
                message: format!("failed to parse audio send response: {e}"),
            })?;

        if !body.ok {
            return Err(ChannelError::Provider {
                message: body.description.unwrap_or_else(|| "audio send failed".into()),
            });
        }

        let msg = body.result.ok_or_else(|| ChannelError::Provider {
            message: "no message in audio send response".into(),
        })?;

        Ok(DeliveryReceipt {
            provider_message_id: msg.message_id.to_string(),
            delivered_at: Utc::now(),
        })
    }

    /// Telegram's maximum message length.
    const MAX_MESSAGE_LEN: usize = 4096;

    /// Split text into chunks that fit within Telegram's message limit.
    /// Tries to split at paragraph breaks, then sentence boundaries, then hard-cuts.
    fn split_message(text: &str) -> Vec<&str> {
        if text.len() <= Self::MAX_MESSAGE_LEN {
            return vec![text];
        }

        let mut chunks = Vec::new();
        let mut remaining = text;

        while remaining.len() > Self::MAX_MESSAGE_LEN {
            let window = &remaining[..Self::MAX_MESSAGE_LEN];

            // Try paragraph break (double newline)
            let split_at = window
                .rfind("\n\n")
                .filter(|&pos| pos > 0)
                // Try single newline
                .or_else(|| window.rfind('\n').filter(|&pos| pos > 0))
                // Try sentence boundary (". " to avoid splitting on decimals)
                .or_else(|| window.rfind(". ").map(|pos| pos + 1).filter(|&pos| pos > 0))
                // Hard split at limit
                .unwrap_or(Self::MAX_MESSAGE_LEN);

            chunks.push(remaining[..split_at].trim_end());
            remaining = remaining[split_at..].trim_start();
        }

        if !remaining.is_empty() {
            chunks.push(remaining);
        }

        chunks
    }

    /// Send a message via Telegram Bot API, splitting if over 4096 chars.
    async fn send_message(
        &self,
        chat_id: &str,
        text: &str,
        reply_to: Option<&str>,
    ) -> Result<DeliveryReceipt, ChannelError> {
        let chunks = Self::split_message(text);
        let mut last_receipt = None;

        for (i, chunk) in chunks.iter().enumerate() {
            // Only the first chunk gets the reply_to
            let chunk_reply_to = if i == 0 { reply_to } else { None };
            let receipt = self.send_single_message(chat_id, chunk, chunk_reply_to).await?;
            last_receipt = Some(receipt);

            // Small delay between chunks to avoid rate limits
            if i + 1 < chunks.len() {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }

        last_receipt.ok_or_else(|| ChannelError::Provider {
            message: "no chunks to send".into(),
        })
    }

    /// Send a single message chunk via Telegram Bot API.
    async fn send_single_message(
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

        // If Markdown parsing failed, retry without parse_mode.
        let body = if !body.ok
            && body
                .description
                .as_deref()
                .is_some_and(|d| d.contains("parse entities") || d.contains("can't parse"))
        {
            tracing::warn!("Markdown parse failed, retrying without parse_mode");
            params.as_object_mut().unwrap().remove("parse_mode");
            let retry_resp = self
                .client
                .post(&url)
                .json(&params)
                .send()
                .await
                .map_err(|e| ChannelError::Provider {
                    message: format!("failed to send message (retry): {e}"),
                })?;
            retry_resp
                .json::<TelegramResponse<TelegramMessage>>()
                .await
                .map_err(|e| ChannelError::Provider {
                    message: format!("failed to parse retry response: {e}"),
                })?
        } else {
            body
        };

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
            if let Some(event) = self.pending_events.pop() {
                return Ok(event);
            }

            let updates = self.poll_updates().await?;

            for update in &updates {
                if let Some(event) = Self::convert_update(update) {
                    self.pending_events.push(event);
                }
            }

            self.pending_events.reverse();
        }
    }

    async fn send(&self, action: OutboundAction) -> Result<DeliveryReceipt, ChannelError> {
        match action {
            OutboundAction::Send {
                chat_id, content, ..
            } => self.send_message(&chat_id, &content, None).await,
            OutboundAction::Reply {
                chat_id,
                reply_to,
                content,
                ..
            } => self.send_message(&chat_id, &content, Some(&reply_to)).await,
            OutboundAction::Edit {
                chat_id,
                message_id,
                new_content,
                ..
            } => {
                let url = format!("{}/editMessageText", self.base_url);
                let params = serde_json::json!({
                    "chat_id": chat_id,
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
                debug!("reaction requested but Telegram Bot API support is limited");
                Ok(DeliveryReceipt {
                    provider_message_id: String::new(),
                    delivered_at: Utc::now(),
                })
            }
            OutboundAction::Delete {
                chat_id,
                message_id,
                ..
            } => {
                let url = format!("{}/deleteMessage", self.base_url);
                let params = serde_json::json!({
                    "chat_id": chat_id,
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
            OutboundAction::SendTypingIndicator { chat_id, .. } => {
                let url = format!("{}/sendChatAction", self.base_url);
                let params = serde_json::json!({
                    "chat_id": chat_id,
                    "action": "typing",
                });

                let _ = self.client.post(&url).json(&params).send().await;

                Ok(DeliveryReceipt {
                    provider_message_id: String::new(),
                    delivered_at: Utc::now(),
                })
            }
            OutboundAction::SendInlineKeyboard {
                chat_id,
                content,
                buttons,
                ..
            } => {
                let keyboard_buttons: Vec<serde_json::Value> = buttons
                    .iter()
                    .map(|(label, data)| {
                        serde_json::json!([{
                            "text": label,
                            "callback_data": data,
                        }])
                    })
                    .collect();

                let url = format!("{}/sendMessage", self.base_url);
                let params = serde_json::json!({
                    "chat_id": chat_id,
                    "text": content,
                    "reply_markup": {
                        "inline_keyboard": keyboard_buttons,
                    },
                });

                let response = self
                    .client
                    .post(&url)
                    .json(&params)
                    .send()
                    .await
                    .map_err(|e| ChannelError::Provider {
                        message: format!("failed to send inline keyboard: {e}"),
                    })?;

                let body: TelegramResponse<TelegramMessage> =
                    response.json().await.map_err(|e| ChannelError::Provider {
                        message: format!("failed to parse keyboard response: {e}"),
                    })?;

                Ok(DeliveryReceipt {
                    provider_message_id: body
                        .result
                        .map(|m| m.message_id.to_string())
                        .unwrap_or_default(),
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
    callback_query: Option<TelegramCallbackQuery>,
}

#[derive(Debug, Deserialize)]
struct TelegramCallbackQuery {
    #[allow(dead_code)]
    id: String,
    from: TelegramUser,
    message: Option<TelegramMessage>,
    data: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TelegramMessage {
    message_id: i64,
    chat: TelegramChat,
    date: i64,
    text: Option<String>,
    caption: Option<String>,
    from: Option<TelegramUser>,
    document: Option<TelegramDocument>,
    photo: Option<Vec<TelegramPhotoSize>>,
    voice: Option<TelegramFileRef>,
    audio: Option<TelegramFileRef>,
    video_note: Option<TelegramFileRef>,
    video: Option<TelegramFileRef>,
}

#[derive(Debug, Deserialize)]
struct TelegramChat {
    id: i64,
}

#[derive(Debug, Deserialize)]
struct TelegramUser {
    id: i64,
    username: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TelegramDocument {
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

/// Generic Telegram file reference (voice, audio, video_note, video).
#[derive(Debug, Deserialize)]
struct TelegramFileRef {
    file_id: String,
    #[allow(dead_code)]
    file_unique_id: Option<String>,
    mime_type: Option<String>,
    file_size: Option<i32>,
    #[allow(dead_code)]
    duration: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_msg(text: Option<&str>, caption: Option<&str>) -> TelegramMessage {
        TelegramMessage {
            message_id: 42,
            chat: TelegramChat { id: 999 },
            date: 1710320000,
            text: text.map(String::from),
            caption: caption.map(String::from),
            from: Some(TelegramUser {
                id: 123,
                username: Some("testuser".into()),
            }),
            document: None,
            photo: None,
            voice: None,
            audio: None,
            video_note: None,
            video: None,
        }
    }

    #[test]
    fn convert_text_message_update() {
        let update = TelegramUpdate {
            update_id: 1,
            message: Some(make_msg(Some("hello"), None)),
            edited_message: None,
            callback_query: None,
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
    fn convert_voice_message_produces_event() {
        let mut msg = make_msg(None, None);
        msg.voice = Some(TelegramFileRef {
            file_id: "voice123".into(),
            file_unique_id: None,
            mime_type: Some("audio/ogg".into()),
            file_size: Some(4096),
            duration: Some(5),
        });
        let update = TelegramUpdate {
            update_id: 10,
            message: Some(msg),
            edited_message: None,
            callback_query: None,
        };
        let event = TelegramAdapter::convert_update(&update).unwrap();
        match event {
            InboundEvent::Message(m) => {
                assert!(m.content.is_empty());
                assert_eq!(m.attachments.len(), 1);
                assert_eq!(m.attachments[0].name, "voice.ogg");
                assert_eq!(
                    m.attachments[0].url.as_deref(),
                    Some("telegram-file:voice123")
                );
            }
            _ => panic!("expected Message event"),
        }
    }

    #[test]
    fn caption_used_as_content_for_media() {
        let mut msg = make_msg(None, Some("check this out"));
        msg.photo = Some(vec![TelegramPhotoSize {
            file_id: "photo456".into(),
            file_size: Some(1024),
        }]);
        let update = TelegramUpdate {
            update_id: 11,
            message: Some(msg),
            edited_message: None,
            callback_query: None,
        };
        let event = TelegramAdapter::convert_update(&update).unwrap();
        match event {
            InboundEvent::Message(m) => {
                assert_eq!(m.content, "check this out");
                assert_eq!(m.attachments.len(), 1);
            }
            _ => panic!("expected Message event"),
        }
    }

    #[test]
    fn convert_edit_update() {
        let mut msg = make_msg(Some("edited text"), None);
        msg.message_id = 43;
        let update = TelegramUpdate {
            update_id: 2,
            message: None,
            edited_message: Some(msg),
            callback_query: None,
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
            message: Some(make_msg(None, None)),
            edited_message: None,
            callback_query: None,
        };
        assert!(TelegramAdapter::convert_update(&update).is_none());
    }

    #[test]
    fn extract_document_attachment() {
        let mut msg = make_msg(Some("here's a file"), None);
        msg.document = Some(TelegramDocument {
            file_id: "abc123".into(),
            file_name: Some("report.pdf".into()),
            mime_type: Some("application/pdf".into()),
            file_size: Some(1024),
        });

        let attachments = TelegramAdapter::extract_attachments(&msg);
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].name, "report.pdf");
        assert_eq!(attachments[0].mime_type.as_deref(), Some("application/pdf"));
        assert!(attachments[0].url.as_ref().unwrap().starts_with("telegram-file:"));
    }

    #[test]
    fn adapter_with_custom_base_url() {
        let adapter = TelegramAdapter::with_base_url("token", "http://localhost:8080");
        assert_eq!(adapter.base_url, "http://localhost:8080");
    }

    #[test]
    fn split_message_short_text() {
        let chunks = TelegramAdapter::split_message("hello world");
        assert_eq!(chunks, vec!["hello world"]);
    }

    #[test]
    fn split_message_at_paragraph_boundary() {
        let para1 = "a".repeat(3000);
        let para2 = "b".repeat(2000);
        let text = format!("{para1}\n\n{para2}");
        let chunks = TelegramAdapter::split_message(&text);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], para1);
        assert_eq!(chunks[1], para2);
    }

    #[test]
    fn split_message_at_newline() {
        let line1 = "a".repeat(3000);
        let line2 = "b".repeat(2000);
        let text = format!("{line1}\n{line2}");
        let chunks = TelegramAdapter::split_message(&text);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], line1);
        assert_eq!(chunks[1], line2);
    }

    #[test]
    fn split_message_hard_cut() {
        let text = "a".repeat(5000);
        let chunks = TelegramAdapter::split_message(&text);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 4096);
        assert_eq!(chunks[1].len(), 904);
    }

    #[test]
    fn split_message_exactly_at_limit() {
        let text = "a".repeat(4096);
        let chunks = TelegramAdapter::split_message(&text);
        assert_eq!(chunks.len(), 1);
    }
}
