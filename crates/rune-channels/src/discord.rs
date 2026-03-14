//! Discord channel adapter.
//!
//! Uses the Discord REST API (v10) for both polling inbound messages and sending
//! outbound actions.  A background task periodically fetches new messages from
//! configured channels and feeds them into an internal mpsc queue consumed by
//! [`ChannelAdapter::receive`].

use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::Client;
use rune_core::ChannelId;
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use async_trait::async_trait;

use crate::{
    ChannelAdapter, ChannelError, ChannelMessage, DeliveryReceipt, InboundEvent, OutboundAction,
};

const DISCORD_API_BASE: &str = "https://discord.gg/api/v10";
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Discord REST-based channel adapter.
///
/// Spawns a background poller that fetches messages from the configured guild
/// channels and converts them into [`InboundEvent`]s.  Outbound actions are
/// dispatched directly via the Discord REST API.
pub struct DiscordAdapter {
    token: String,
    api_base: String,
    http: Client,
    rx: mpsc::Receiver<InboundEvent>,
    /// Retained only so the background task is not orphaned while the adapter
    /// is alive (dropping the sender lets the poller detect shutdown).
    _tx: mpsc::Sender<InboundEvent>,
}

impl DiscordAdapter {
    /// Create a new Discord adapter.
    ///
    /// * `token`      - Bot token (prefixed with `Bot ` automatically).
    /// * `guild_id`   - The Discord server (guild) to watch.
    /// * `channel_ids`- Specific text channels to poll.  If empty the adapter
    ///                   will only be able to send but not receive.
    pub fn new(
        token: impl Into<String>,
        guild_id: impl Into<String>,
        channel_ids: Vec<String>,
    ) -> Self {
        Self::with_api_base(token, guild_id, channel_ids, DISCORD_API_BASE)
    }

    fn with_api_base(
        token: impl Into<String>,
        guild_id: impl Into<String>,
        channel_ids: Vec<String>,
        api_base: impl Into<String>,
    ) -> Self {
        let token = token.into();
        let api_base = api_base.into().trim_end_matches('/').to_string();
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| Client::new());

        let (tx, rx) = mpsc::channel(256);

        // Spawn the background polling task.
        {
            let tx = tx.clone();
            let token = token.clone();
            let http = http.clone();
            let guild_id = guild_id.into();
            let poll_api_base = api_base.clone();
            tokio::spawn(async move {
                Self::poll_loop(tx, token, http, guild_id, channel_ids, poll_api_base).await;
            });
        }

        Self {
            token,
            api_base,
            http,
            rx,
            _tx: tx,
        }
    }

    /// Create an adapter pointing at a custom base URL (for testing).
    #[cfg(test)]
    pub fn with_base_url(token: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self::with_api_base(token, "test-guild", Vec::new(), base_url)
    }

    // ---------- background poller ----------

    async fn poll_loop(
        tx: mpsc::Sender<InboundEvent>,
        token: String,
        http: Client,
        _guild_id: String,
        channel_ids: Vec<String>,
        api_base: String,
    ) {
        // Track the last seen message id per channel so we only fetch new messages.
        let mut last_message_ids: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();

        loop {
            for channel_id in &channel_ids {
                let events = Self::fetch_new_messages(
                    &http,
                    &api_base,
                    &token,
                    channel_id,
                    last_message_ids.get(channel_id),
                )
                .await;

                match events {
                    Ok((messages, new_last_id)) => {
                        if let Some(id) = new_last_id {
                            last_message_ids.insert(channel_id.clone(), id);
                        }
                        for event in messages {
                            if tx.send(event).await.is_err() {
                                debug!("discord poller: receiver dropped, shutting down");
                                return;
                            }
                        }
                    }
                    Err(e) => {
                        warn!("discord poll error for channel {channel_id}: {e}");
                    }
                }
            }

            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    /// Fetch messages newer than `after` from a single channel.
    async fn fetch_new_messages(
        http: &Client,
        api_base: &str,
        token: &str,
        channel_id: &str,
        after: Option<&String>,
    ) -> Result<(Vec<InboundEvent>, Option<String>), String> {
        let mut url = format!("{api_base}/channels/{channel_id}/messages?limit=50");
        if let Some(after_id) = after {
            url.push_str(&format!("&after={after_id}"));
        }

        let resp = http
            .get(&url)
            .header("Authorization", format!("Bot {token}"))
            .send()
            .await
            .map_err(|e| format!("request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("HTTP {status}: {body}"));
        }

        let messages: Vec<DiscordMessage> = resp
            .json()
            .await
            .map_err(|e| format!("json parse failed: {e}"))?;

        let mut events = Vec::new();
        let mut newest_id: Option<String> = None;

        // Discord returns newest first, so iterate in reverse for chronological order.
        for msg in messages.iter().rev() {
            // Skip bot messages to avoid echo loops.
            if msg.author.bot.unwrap_or(false) {
                continue;
            }

            let timestamp = DateTime::parse_from_rfc3339(&msg.timestamp)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            events.push(InboundEvent::Message(ChannelMessage {
                channel_id: ChannelId::new(),
                raw_chat_id: msg.channel_id.clone(),
                sender: msg.author.username.clone(),
                content: msg.content.clone(),
                attachments: msg
                    .attachments
                    .iter()
                    .map(|a| rune_core::AttachmentRef {
                        name: a.filename.clone(),
                        mime_type: a.content_type.clone(),
                        size_bytes: Some(a.size),
                        url: Some(a.url.clone()),
                    })
                    .collect(),
                timestamp,
                provider_message_id: msg.id.clone(),
            }));

            // Track newest message id.
            match &newest_id {
                Some(current) if msg.id > *current => {
                    newest_id = Some(msg.id.clone());
                }
                None => {
                    newest_id = Some(msg.id.clone());
                }
                _ => {}
            }
        }

        Ok((events, newest_id))
    }

    // ---------- REST helpers ----------

    fn auth_header(&self) -> String {
        format!("Bot {}", self.token)
    }

    async fn rest_post(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, ChannelError> {
        let url = format!("{}{}", self.api_base, path);
        let resp = self
            .http
            .post(&url)
            .header("Authorization", self.auth_header())
            .json(body)
            .send()
            .await
            .map_err(|e| ChannelError::Provider {
                message: format!("discord REST error: {e}"),
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ChannelError::Provider {
                message: format!("discord API {status}: {text}"),
            });
        }

        resp.json().await.map_err(|e| ChannelError::Provider {
            message: format!("discord response parse error: {e}"),
        })
    }

    async fn rest_patch(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, ChannelError> {
        let url = format!("{}{}", self.api_base, path);
        let resp = self
            .http
            .patch(&url)
            .header("Authorization", self.auth_header())
            .json(body)
            .send()
            .await
            .map_err(|e| ChannelError::Provider {
                message: format!("discord REST error: {e}"),
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ChannelError::Provider {
                message: format!("discord API {status}: {text}"),
            });
        }

        resp.json().await.map_err(|e| ChannelError::Provider {
            message: format!("discord response parse error: {e}"),
        })
    }

    async fn rest_delete(&self, path: &str) -> Result<(), ChannelError> {
        let url = format!("{}{}", self.api_base, path);
        let resp = self
            .http
            .delete(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await
            .map_err(|e| ChannelError::Provider {
                message: format!("discord REST error: {e}"),
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ChannelError::Provider {
                message: format!("discord API {status}: {text}"),
            });
        }

        Ok(())
    }
}

#[async_trait]
impl ChannelAdapter for DiscordAdapter {
    async fn receive(&mut self) -> Result<InboundEvent, ChannelError> {
        self.rx.recv().await.ok_or(ChannelError::ConnectionLost {
            reason: "discord poller task exited".into(),
        })
    }

    async fn send(&self, action: OutboundAction) -> Result<DeliveryReceipt, ChannelError> {
        match action {
            OutboundAction::Send {
                chat_id, content, ..
            } => {
                let body = serde_json::json!({ "content": content });
                let resp = self
                    .rest_post(&format!("/channels/{chat_id}/messages"), &body)
                    .await?;
                let msg_id = resp["id"].as_str().unwrap_or("").to_string();
                Ok(DeliveryReceipt {
                    provider_message_id: msg_id,
                    delivered_at: Utc::now(),
                })
            }
            OutboundAction::Reply {
                chat_id,
                reply_to,
                content,
                ..
            } => {
                let body = serde_json::json!({
                    "content": content,
                    "message_reference": {
                        "message_id": reply_to,
                    },
                });
                let resp = self
                    .rest_post(&format!("/channels/{chat_id}/messages"), &body)
                    .await?;
                let msg_id = resp["id"].as_str().unwrap_or("").to_string();
                Ok(DeliveryReceipt {
                    provider_message_id: msg_id,
                    delivered_at: Utc::now(),
                })
            }
            OutboundAction::Edit {
                chat_id,
                message_id,
                new_content,
                ..
            } => {
                let body = serde_json::json!({ "content": new_content });
                let resp = self
                    .rest_patch(&format!("/channels/{chat_id}/messages/{message_id}"), &body)
                    .await?;
                let msg_id = resp["id"].as_str().unwrap_or("").to_string();
                Ok(DeliveryReceipt {
                    provider_message_id: msg_id,
                    delivered_at: Utc::now(),
                })
            }
            OutboundAction::React {
                message_id, emoji, ..
            } => {
                // Discord reaction: PUT /channels/{channel}/messages/{msg}/reactions/{emoji}/@me
                // The current OutboundAction::React shape lacks a chat_id, so we
                // cannot construct the full URL.  Return NotImplemented until the
                // type is extended.
                debug!(
                    "discord react: message_id={message_id} emoji={emoji} (chat_id not available in action shape)"
                );
                Err(ChannelError::NotImplemented)
            }
            OutboundAction::Delete {
                chat_id,
                message_id,
                ..
            } => {
                self.rest_delete(&format!("/channels/{chat_id}/messages/{message_id}"))
                    .await?;
                Ok(DeliveryReceipt {
                    provider_message_id: message_id,
                    delivered_at: Utc::now(),
                })
            }
            OutboundAction::SendTypingIndicator { chat_id, .. } => {
                let _ = self
                    .rest_post(
                        &format!("/channels/{chat_id}/typing"),
                        &serde_json::json!({}),
                    )
                    .await;
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
                // Map buttons to Discord action-row components.
                let components: Vec<serde_json::Value> = buttons
                    .iter()
                    .enumerate()
                    .map(|(_i, (label, data))| {
                        serde_json::json!({
                            "type": 2, // Button
                            "style": 1, // Primary
                            "label": label,
                            "custom_id": data,
                        })
                    })
                    .collect();

                let body = serde_json::json!({
                    "content": content,
                    "components": [{
                        "type": 1, // ActionRow
                        "components": components,
                    }],
                });

                let resp = self
                    .rest_post(&format!("/channels/{chat_id}/messages"), &body)
                    .await?;
                let msg_id = resp["id"].as_str().unwrap_or("").to_string();
                Ok(DeliveryReceipt {
                    provider_message_id: msg_id,
                    delivered_at: Utc::now(),
                })
            }
        }
    }
}

// ---------- Discord API response types ----------

#[derive(Debug, Deserialize)]
struct DiscordMessage {
    id: String,
    channel_id: String,
    content: String,
    timestamp: String,
    author: DiscordUser,
    #[serde(default)]
    attachments: Vec<DiscordAttachment>,
}

#[derive(Debug, Deserialize)]
struct DiscordUser {
    #[allow(dead_code)]
    id: String,
    username: String,
    #[serde(default)]
    bot: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct DiscordAttachment {
    #[allow(dead_code)]
    id: String,
    filename: String,
    content_type: Option<String>,
    size: u64,
    url: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChannelAdapter;
    use wiremock::matchers::{body_partial_json, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn discord_message_deser() {
        let json = serde_json::json!({
            "id": "123",
            "channel_id": "456",
            "content": "hello world",
            "timestamp": "2026-03-14T12:00:00+00:00",
            "author": {
                "id": "789",
                "username": "testuser",
                "bot": false,
            },
            "attachments": [],
        });

        let msg: DiscordMessage = serde_json::from_value(json).unwrap();
        assert_eq!(msg.id, "123");
        assert_eq!(msg.content, "hello world");
        assert_eq!(msg.author.username, "testuser");
        assert!(!msg.author.bot.unwrap_or(false));
    }

    #[test]
    fn discord_message_with_attachment_deser() {
        let json = serde_json::json!({
            "id": "100",
            "channel_id": "200",
            "content": "check this out",
            "timestamp": "2026-03-14T12:30:00+00:00",
            "author": {
                "id": "300",
                "username": "uploader",
            },
            "attachments": [{
                "id": "400",
                "filename": "image.png",
                "content_type": "image/png",
                "size": 2048,
                "url": "https://cdn.discordapp.com/attachments/200/400/image.png",
            }],
        });

        let msg: DiscordMessage = serde_json::from_value(json).unwrap();
        assert_eq!(msg.attachments.len(), 1);
        assert_eq!(msg.attachments[0].filename, "image.png");
        assert_eq!(msg.attachments[0].size, 2048);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn send_uses_configured_base_url() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/channels/channel-1/messages"))
            .and(header("authorization", "Bot discord-token"))
            .and(body_partial_json(serde_json::json!({
                "content": "ping"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "discord-msg-1"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let adapter = DiscordAdapter::with_base_url("discord-token", server.uri());
        let receipt = adapter
            .send(OutboundAction::Send {
                channel_id: ChannelId::new(),
                chat_id: "channel-1".into(),
                content: "ping".into(),
            })
            .await
            .expect("mocked discord send should succeed");

        assert_eq!(receipt.provider_message_id, "discord-msg-1");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reply_edit_delete_and_typing_use_discord_rest_routes() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/channels/channel-1/messages"))
            .and(header("authorization", "Bot discord-token"))
            .and(body_partial_json(serde_json::json!({
                "content": "reply text",
                "message_reference": {
                    "message_id": "source-1"
                }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "reply-1"
            })))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("PATCH"))
            .and(path("/channels/channel-1/messages/reply-1"))
            .and(header("authorization", "Bot discord-token"))
            .and(body_partial_json(serde_json::json!({
                "content": "edited text"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "reply-1"
            })))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("DELETE"))
            .and(path("/channels/channel-1/messages/reply-1"))
            .and(header("authorization", "Bot discord-token"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/channels/channel-1/typing"))
            .and(header("authorization", "Bot discord-token"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let adapter = DiscordAdapter::with_base_url("discord-token", server.uri());

        let reply = adapter
            .send(OutboundAction::Reply {
                channel_id: ChannelId::new(),
                chat_id: "channel-1".into(),
                reply_to: "source-1".into(),
                content: "reply text".into(),
            })
            .await
            .expect("reply should succeed");
        assert_eq!(reply.provider_message_id, "reply-1");

        let edit = adapter
            .send(OutboundAction::Edit {
                channel_id: ChannelId::new(),
                chat_id: "channel-1".into(),
                message_id: "reply-1".into(),
                new_content: "edited text".into(),
            })
            .await
            .expect("edit should succeed");
        assert_eq!(edit.provider_message_id, "reply-1");

        let delete = adapter
            .send(OutboundAction::Delete {
                channel_id: ChannelId::new(),
                chat_id: "channel-1".into(),
                message_id: "reply-1".into(),
            })
            .await
            .expect("delete should succeed");
        assert_eq!(delete.provider_message_id, "reply-1");

        let typing = adapter
            .send(OutboundAction::SendTypingIndicator {
                channel_id: ChannelId::new(),
                chat_id: "channel-1".into(),
            })
            .await
            .expect("typing should succeed even for empty receipt ids");
        assert!(typing.provider_message_id.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn inline_keyboard_maps_buttons_to_discord_components() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/channels/channel-1/messages"))
            .and(header("authorization", "Bot discord-token"))
            .and(body_partial_json(serde_json::json!({
                "content": "choose one",
                "components": [{
                    "type": 1,
                    "components": [
                        {
                            "type": 2,
                            "style": 1,
                            "label": "First",
                            "custom_id": "first"
                        },
                        {
                            "type": 2,
                            "style": 1,
                            "label": "Second",
                            "custom_id": "second"
                        }
                    ]
                }]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "keyboard-1"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let adapter = DiscordAdapter::with_base_url("discord-token", server.uri());
        let receipt = adapter
            .send(OutboundAction::SendInlineKeyboard {
                channel_id: ChannelId::new(),
                chat_id: "channel-1".into(),
                content: "choose one".into(),
                buttons: vec![
                    ("First".into(), "first".into()),
                    ("Second".into(), "second".into()),
                ],
            })
            .await
            .expect("inline keyboard send should succeed");

        assert_eq!(receipt.provider_message_id, "keyboard-1");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fetch_new_messages_filters_bot_messages_and_keeps_chronological_order() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/channels/channel-1/messages"))
            .and(query_param("limit", "50"))
            .and(query_param("after", "100"))
            .and(header("authorization", "Bot discord-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "id": "102",
                    "channel_id": "channel-1",
                    "content": "newest human",
                    "timestamp": "2026-03-14T12:02:00+00:00",
                    "author": {
                        "id": "u2",
                        "username": "bob",
                        "bot": false
                    },
                    "attachments": []
                },
                {
                    "id": "101",
                    "channel_id": "channel-1",
                    "content": "skip me",
                    "timestamp": "2026-03-14T12:01:30+00:00",
                    "author": {
                        "id": "b1",
                        "username": "bot",
                        "bot": true
                    },
                    "attachments": []
                },
                {
                    "id": "100",
                    "channel_id": "channel-1",
                    "content": "oldest human",
                    "timestamp": "2026-03-14T12:01:00+00:00",
                    "author": {
                        "id": "u1",
                        "username": "alice",
                        "bot": false
                    },
                    "attachments": [{
                        "id": "a1",
                        "filename": "report.txt",
                        "content_type": "text/plain",
                        "size": 128,
                        "url": "https://cdn.example/report.txt"
                    }]
                }
            ])))
            .expect(1)
            .mount(&server)
            .await;

        let http = Client::new();
        let after = "100".to_string();
        let (events, newest_id) = DiscordAdapter::fetch_new_messages(
            &http,
            &server.uri(),
            "discord-token",
            "channel-1",
            Some(&after),
        )
        .await
        .expect("fetch should succeed");

        assert_eq!(newest_id.as_deref(), Some("102"));
        assert_eq!(events.len(), 2);

        match &events[0] {
            InboundEvent::Message(msg) => {
                assert_eq!(msg.provider_message_id, "100");
                assert_eq!(msg.sender, "alice");
                assert_eq!(msg.content, "oldest human");
                assert_eq!(msg.attachments.len(), 1);
                assert_eq!(msg.attachments[0].name, "report.txt");
            }
            other => panic!("expected first event to be a message, got {other:?}"),
        }

        match &events[1] {
            InboundEvent::Message(msg) => {
                assert_eq!(msg.provider_message_id, "102");
                assert_eq!(msg.sender, "bob");
                assert_eq!(msg.content, "newest human");
            }
            other => panic!("expected second event to be a message, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn react_is_not_implemented_without_chat_id() {
        let adapter = DiscordAdapter::with_base_url("discord-token", "http://127.0.0.1:9");
        let err = adapter
            .send(OutboundAction::React {
                channel_id: ChannelId::new(),
                message_id: "123".into(),
                emoji: "👍".into(),
            })
            .await
            .expect_err("react should not be implemented yet");

        assert!(matches!(err, ChannelError::NotImplemented));
    }
}
