//! WhatsApp Cloud API channel adapter.
//!
//! Sends messages through the Meta WhatsApp Business Cloud API and receives
//! inbound messages via a webhook HTTP receiver.  The adapter spawns a
//! background task that listens on a configurable TCP address for incoming
//! webhook POSTs from Meta, converts them into [`InboundEvent`]s, and pushes
//! them into an mpsc queue.

use std::time::Duration;

use chrono::Utc;
use reqwest::Client;
use rune_core::ChannelId;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use async_trait::async_trait;

use crate::{
    ChannelAdapter, ChannelError, ChannelMessage, DeliveryReceipt, InboundEvent, OutboundAction,
};

const GRAPH_API_BASE: &str = "https://graph.facebook.com/v17.0";

/// WhatsApp Cloud API adapter.
///
/// Outbound messages are sent via the Graph API.  Inbound messages arrive as
/// webhook notifications from Meta which the adapter processes through a
/// lightweight HTTP listener.
pub struct WhatsAppAdapter {
    access_token: String,
    phone_number_id: String,
    #[allow(dead_code)]
    verify_token: String,
    graph_api_base: String,
    http: Client,
    rx: mpsc::Receiver<InboundEvent>,
    _tx: mpsc::Sender<InboundEvent>,
}

impl WhatsAppAdapter {
    /// Create a new WhatsApp adapter.
    ///
    /// * `access_token`     - Meta permanent access token.
    /// * `phone_number_id`  - Phone number ID from the WhatsApp Business dashboard.
    /// * `verify_token`     - Token used by Meta to verify the webhook endpoint.
    /// * `listen_addr`      - Local address to bind the webhook receiver
    ///                         (e.g. `"0.0.0.0:3200"`).  If `None`, only outbound
    ///                         sending is available.
    pub fn new(
        access_token: impl Into<String>,
        phone_number_id: impl Into<String>,
        verify_token: impl Into<String>,
        listen_addr: Option<String>,
    ) -> Self {
        Self::with_graph_api_base(
            access_token,
            phone_number_id,
            verify_token,
            listen_addr,
            GRAPH_API_BASE,
        )
    }

    /// Create a new WhatsApp adapter targeting a custom Graph API base URL.
    pub fn with_graph_api_base(
        access_token: impl Into<String>,
        phone_number_id: impl Into<String>,
        verify_token: impl Into<String>,
        listen_addr: Option<String>,
        graph_api_base: impl Into<String>,
    ) -> Self {
        let access_token = access_token.into();
        let phone_number_id = phone_number_id.into();
        let verify_token = verify_token.into();
        let graph_api_base = graph_api_base.into().trim_end_matches('/').to_string();

        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| Client::new());

        let (tx, rx) = mpsc::channel(256);

        if let Some(addr) = listen_addr {
            let tx = tx.clone();
            let vt = verify_token.clone();
            tokio::spawn(async move {
                if let Err(e) = Self::run_webhook_listener(addr, tx, vt).await {
                    error!("whatsapp webhook listener exited: {e}");
                }
            });
        }

        Self {
            access_token,
            phone_number_id,
            verify_token,
            graph_api_base,
            http,
            rx,
            _tx: tx,
        }
    }

    // ---------- Webhook receiver ----------

    /// Minimal HTTP server that handles Meta webhook verification and event
    /// delivery.
    async fn run_webhook_listener(
        addr: String,
        tx: mpsc::Sender<InboundEvent>,
        verify_token: String,
    ) -> Result<(), String> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind(&addr)
            .await
            .map_err(|e| format!("failed to bind {addr}: {e}"))?;

        info!("whatsapp webhook listener bound to {addr}");

        loop {
            let (mut stream, peer) = listener
                .accept()
                .await
                .map_err(|e| format!("accept failed: {e}"))?;

            let tx = tx.clone();
            let verify_token = verify_token.clone();

            tokio::spawn(async move {
                let mut buf = vec![0u8; 65536];
                let n = match stream.read(&mut buf).await {
                    Ok(n) => n,
                    Err(e) => {
                        warn!("whatsapp webhook read error from {peer}: {e}");
                        return;
                    }
                };
                let raw = String::from_utf8_lossy(&buf[..n]).to_string();

                // Parse the HTTP request line to determine GET vs POST.
                let first_line = raw.lines().next().unwrap_or("");
                let response = if first_line.starts_with("GET") {
                    // Webhook verification request.
                    Self::handle_verification(&raw, &verify_token)
                } else {
                    // Webhook event notification.
                    let body = raw
                        .split("\r\n\r\n")
                        .nth(1)
                        .or_else(|| raw.split("\n\n").nth(1))
                        .unwrap_or("");

                    match serde_json::from_str::<serde_json::Value>(body) {
                        Ok(payload) => {
                            let events = Self::extract_events(&payload);
                            for event in events {
                                let _ = tx.send(event).await;
                            }
                            "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_string()
                        }
                        Err(e) => {
                            warn!("whatsapp webhook json parse error: {e}");
                            "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n".to_string()
                        }
                    }
                };

                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    }

    /// Handle the Meta webhook verification GET request.
    ///
    /// Expects query parameters: `hub.mode=subscribe`, `hub.verify_token`,
    /// `hub.challenge`.
    fn handle_verification(raw_request: &str, verify_token: &str) -> String {
        // Parse query string from the request line.
        let first_line = raw_request.lines().next().unwrap_or("");
        let parts: Vec<&str> = first_line.split_whitespace().collect();
        let path = parts.get(1).unwrap_or(&"/");

        let query = path.split('?').nth(1).unwrap_or("");
        let params: std::collections::HashMap<String, String> = query
            .split('&')
            .filter_map(|pair| {
                let mut kv = pair.splitn(2, '=');
                let k = kv.next()?.to_string();
                let v = kv.next().unwrap_or("").to_string();
                Some((k, v))
            })
            .collect();

        let mode = params.get("hub.mode").map(|s| s.as_str()).unwrap_or("");
        let token = params
            .get("hub.verify_token")
            .map(|s| s.as_str())
            .unwrap_or("");
        let challenge = params
            .get("hub.challenge")
            .map(|s| s.as_str())
            .unwrap_or("");

        if mode == "subscribe" && token == verify_token {
            debug!("whatsapp webhook verification succeeded");
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
                challenge.len(),
                challenge
            )
        } else {
            warn!("whatsapp webhook verification failed (bad token or mode)");
            "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n".to_string()
        }
    }

    /// Extract inbound events from a WhatsApp Cloud API webhook payload.
    ///
    /// The payload shape is:
    /// ```json
    /// {
    ///   "object": "whatsapp_business_account",
    ///   "entry": [{
    ///     "changes": [{
    ///       "value": {
    ///         "messages": [{ ... }],
    ///         "statuses": [{ ... }]
    ///       }
    ///     }]
    ///   }]
    /// }
    /// ```
    fn extract_events(payload: &serde_json::Value) -> Vec<InboundEvent> {
        let mut events = Vec::new();

        let entries = match payload["entry"].as_array() {
            Some(e) => e,
            None => return events,
        };

        for entry in entries {
            let changes = match entry["changes"].as_array() {
                Some(c) => c,
                None => continue,
            };

            for change in changes {
                let value = &change["value"];
                let messages = value["messages"].as_array();

                if let Some(msgs) = messages {
                    for msg in msgs {
                        if let Some(event) = Self::convert_message(msg) {
                            events.push(event);
                        }
                    }
                }
            }
        }

        events
    }

    /// Convert a single WhatsApp message object to an [`InboundEvent`].
    fn convert_message(msg: &serde_json::Value) -> Option<InboundEvent> {
        let msg_type = msg["type"].as_str()?;
        let from = msg["from"].as_str().unwrap_or("unknown").to_string();
        let msg_id = msg["id"].as_str().unwrap_or("").to_string();
        let ts_str = msg["timestamp"].as_str().unwrap_or("0");
        let ts_secs: i64 = ts_str.parse().unwrap_or(0);
        let timestamp = chrono::TimeZone::timestamp_opt(&Utc, ts_secs, 0)
            .single()
            .unwrap_or_else(Utc::now);

        match msg_type {
            "text" => {
                let body = msg["text"]["body"].as_str().unwrap_or("").to_string();
                if body.is_empty() {
                    return None;
                }
                Some(InboundEvent::Message(ChannelMessage {
                    channel_id: ChannelId::new(),
                    raw_chat_id: from.clone(),
                    sender: from,
                    content: body,
                    attachments: Vec::new(),
                    timestamp,
                    provider_message_id: msg_id,
                }))
            }
            "image" | "document" | "audio" | "video" => {
                let media = &msg[msg_type];
                let caption = media["caption"]
                    .as_str()
                    .or_else(|| msg["text"]["body"].as_str())
                    .unwrap_or("")
                    .to_string();
                let mime = media["mime_type"].as_str().map(|s| s.to_string());
                let media_id = media["id"].as_str().unwrap_or("").to_string();

                let attachment = rune_core::AttachmentRef {
                    name: format!("{msg_type}_{media_id}"),
                    mime_type: mime,
                    size_bytes: None,
                    url: None, // Media URLs require a separate download call.
                };

                Some(InboundEvent::Message(ChannelMessage {
                    channel_id: ChannelId::new(),
                    raw_chat_id: from.clone(),
                    sender: from,
                    content: caption,
                    attachments: vec![attachment],
                    timestamp,
                    provider_message_id: msg_id,
                }))
            }
            "reaction" => {
                let emoji = msg["reaction"]["emoji"].as_str().unwrap_or("").to_string();
                let reacted_msg_id = msg["reaction"]["message_id"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                Some(InboundEvent::Reaction {
                    channel_id: ChannelId::new(),
                    message_id: reacted_msg_id,
                    emoji,
                    user: from,
                })
            }
            _ => {
                debug!("whatsapp: unhandled message type {msg_type}");
                None
            }
        }
    }

    // ---------- Cloud API helpers ----------

    async fn graph_post(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, ChannelError> {
        let url = format!("{}{path}", self.graph_api_base);
        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| ChannelError::Provider {
                message: format!("whatsapp API request error: {e}"),
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ChannelError::Provider {
                message: format!("whatsapp API {status}: {text}"),
            });
        }

        resp.json().await.map_err(|e| ChannelError::Provider {
            message: format!("whatsapp API response parse error: {e}"),
        })
    }

    /// Send a text message to a WhatsApp number.
    async fn send_text(
        &self,
        to: &str,
        body: &str,
        reply_to: Option<&str>,
    ) -> Result<DeliveryReceipt, ChannelError> {
        let mut payload = serde_json::json!({
            "messaging_product": "whatsapp",
            "to": to,
            "type": "text",
            "text": {
                "body": body,
            },
        });

        if let Some(msg_id) = reply_to {
            payload["context"] = serde_json::json!({
                "message_id": msg_id,
            });
        }

        let path = format!("/{}/messages", self.phone_number_id);
        let resp = self.graph_post(&path, &payload).await?;

        let msg_id = resp["messages"][0]["id"].as_str().unwrap_or("").to_string();

        Ok(DeliveryReceipt {
            provider_message_id: msg_id,
            delivered_at: Utc::now(),
        })
    }

    /// Send an interactive button message.
    async fn send_buttons(
        &self,
        to: &str,
        body_text: &str,
        buttons: &[(String, String)],
    ) -> Result<DeliveryReceipt, ChannelError> {
        // WhatsApp interactive buttons (max 3).
        let button_items: Vec<serde_json::Value> = buttons
            .iter()
            .take(3)
            .map(|(label, id)| {
                serde_json::json!({
                    "type": "reply",
                    "reply": {
                        "id": id,
                        "title": label,
                    },
                })
            })
            .collect();

        let payload = serde_json::json!({
            "messaging_product": "whatsapp",
            "to": to,
            "type": "interactive",
            "interactive": {
                "type": "button",
                "body": {
                    "text": body_text,
                },
                "action": {
                    "buttons": button_items,
                },
            },
        });

        let path = format!("/{}/messages", self.phone_number_id);
        let resp = self.graph_post(&path, &payload).await?;

        let msg_id = resp["messages"][0]["id"].as_str().unwrap_or("").to_string();

        Ok(DeliveryReceipt {
            provider_message_id: msg_id,
            delivered_at: Utc::now(),
        })
    }

    /// Mark a message as read.
    #[allow(dead_code)]
    async fn mark_read(&self, message_id: &str) -> Result<(), ChannelError> {
        let payload = serde_json::json!({
            "messaging_product": "whatsapp",
            "status": "read",
            "message_id": message_id,
        });

        let path = format!("/{}/messages", self.phone_number_id);
        let _ = self.graph_post(&path, &payload).await;
        Ok(())
    }
}

#[async_trait]
impl ChannelAdapter for WhatsAppAdapter {
    async fn receive(&mut self) -> Result<InboundEvent, ChannelError> {
        self.rx.recv().await.ok_or(ChannelError::ConnectionLost {
            reason: "whatsapp webhook listener exited".into(),
        })
    }

    async fn send(&self, action: OutboundAction) -> Result<DeliveryReceipt, ChannelError> {
        match action {
            OutboundAction::Send {
                chat_id, content, ..
            } => self.send_text(&chat_id, &content, None).await,
            OutboundAction::Reply {
                chat_id,
                reply_to,
                content,
                ..
            } => self.send_text(&chat_id, &content, Some(&reply_to)).await,
            OutboundAction::Edit { .. } => {
                // WhatsApp Cloud API does not support editing sent messages.
                debug!("whatsapp: message editing not supported by Cloud API");
                Err(ChannelError::NotImplemented)
            }
            OutboundAction::React {
                message_id, emoji, ..
            } => {
                // WhatsApp supports reactions via the messages endpoint, but
                // the current OutboundAction::React shape lacks a recipient
                // phone number.  Return NotImplemented until the type is
                // extended.
                debug!(
                    "whatsapp react: message_id={message_id} emoji={emoji} (recipient not available in action shape)"
                );
                Err(ChannelError::NotImplemented)
            }
            OutboundAction::Delete { .. } => {
                debug!("whatsapp: message deletion not supported by Cloud API");
                Err(ChannelError::NotImplemented)
            }
            OutboundAction::SendTypingIndicator { .. } => {
                // WhatsApp does not have a typing indicator API.
                debug!("whatsapp typing indicator: not supported");
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
            } => self.send_buttons(&chat_id, &content, &buttons).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChannelAdapter;
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn convert_text_message() {
        let msg = serde_json::json!({
            "from": "15551234567",
            "id": "wamid.abc123",
            "timestamp": "1710320000",
            "type": "text",
            "text": {
                "body": "Hello from WhatsApp!",
            },
        });

        let event = WhatsAppAdapter::convert_message(&msg).unwrap();
        match event {
            InboundEvent::Message(m) => {
                assert_eq!(m.sender, "15551234567");
                assert_eq!(m.content, "Hello from WhatsApp!");
                assert_eq!(m.provider_message_id, "wamid.abc123");
            }
            _ => panic!("expected Message event"),
        }
    }

    #[test]
    fn convert_image_message() {
        let msg = serde_json::json!({
            "from": "15551234567",
            "id": "wamid.img456",
            "timestamp": "1710320100",
            "type": "image",
            "image": {
                "id": "media123",
                "mime_type": "image/jpeg",
                "caption": "Check this out",
            },
        });

        let event = WhatsAppAdapter::convert_message(&msg).unwrap();
        match event {
            InboundEvent::Message(m) => {
                assert_eq!(m.content, "Check this out");
                assert_eq!(m.attachments.len(), 1);
                assert_eq!(m.attachments[0].mime_type.as_deref(), Some("image/jpeg"));
            }
            _ => panic!("expected Message event"),
        }
    }

    #[test]
    fn convert_reaction_message() {
        let msg = serde_json::json!({
            "from": "15551234567",
            "id": "wamid.react789",
            "timestamp": "1710320200",
            "type": "reaction",
            "reaction": {
                "message_id": "wamid.original123",
                "emoji": "\u{1f44d}",
            },
        });

        let event = WhatsAppAdapter::convert_message(&msg).unwrap();
        match event {
            InboundEvent::Reaction {
                message_id,
                emoji,
                user,
                ..
            } => {
                assert_eq!(message_id, "wamid.original123");
                assert_eq!(user, "15551234567");
                assert!(!emoji.is_empty());
            }
            _ => panic!("expected Reaction event"),
        }
    }

    #[test]
    fn extract_events_from_webhook_payload() {
        let payload = serde_json::json!({
            "object": "whatsapp_business_account",
            "entry": [{
                "id": "BIZ_ID",
                "changes": [{
                    "value": {
                        "messaging_product": "whatsapp",
                        "metadata": {
                            "display_phone_number": "15550001111",
                            "phone_number_id": "PH_ID",
                        },
                        "messages": [{
                            "from": "15559998888",
                            "id": "wamid.test1",
                            "timestamp": "1710320000",
                            "type": "text",
                            "text": { "body": "webhook test" },
                        }],
                    },
                    "field": "messages",
                }],
            }],
        });

        let events = WhatsAppAdapter::extract_events(&payload);
        assert_eq!(events.len(), 1);
        match &events[0] {
            InboundEvent::Message(m) => {
                assert_eq!(m.content, "webhook test");
                assert_eq!(m.sender, "15559998888");
            }
            _ => panic!("expected Message event"),
        }
    }

    #[test]
    fn webhook_verification_success() {
        let request = "GET /webhook?hub.mode=subscribe&hub.verify_token=mytoken&hub.challenge=abc123 HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let response = WhatsAppAdapter::handle_verification(request, "mytoken");
        assert!(response.contains("200 OK"));
        assert!(response.contains("abc123"));
    }

    #[test]
    fn webhook_verification_failure() {
        let request = "GET /webhook?hub.mode=subscribe&hub.verify_token=wrong&hub.challenge=abc123 HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let response = WhatsAppAdapter::handle_verification(request, "mytoken");
        assert!(response.contains("403 Forbidden"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn send_text_uses_configurable_graph_api_base() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v17.0/phone-1/messages"))
            .and(header("authorization", "Bearer wa-token"))
            .and(body_partial_json(serde_json::json!({
                "messaging_product": "whatsapp",
                "to": "15551234567",
                "type": "text",
                "text": {
                    "body": "ping"
                }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "messages": [{ "id": "wamid.mocked123" }]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let adapter = WhatsAppAdapter::with_graph_api_base(
            "wa-token",
            "phone-1",
            "verify-me",
            None,
            format!("{}/v17.0", server.uri()),
        );

        let receipt = adapter
            .send(OutboundAction::Send {
                channel_id: ChannelId::new(),
                chat_id: "15551234567".into(),
                content: "ping".into(),
            })
            .await
            .expect("mocked whatsapp send should succeed");

        assert_eq!(receipt.provider_message_id, "wamid.mocked123");
    }
}
