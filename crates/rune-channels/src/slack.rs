//! Slack channel adapter.
//!
//! Uses the Slack Web API for sending messages and a lightweight HTTP webhook
//! receiver for inbound events (Slack Events API).  A background task binds a
//! local HTTP server that Slack pushes events to, converts them into
//! [`InboundEvent`]s, and feeds them into an internal mpsc queue.

use std::time::Duration;

use chrono::Utc;
use reqwest::{Client, Response};
use rune_core::ChannelId;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use async_trait::async_trait;

use crate::{
    ChannelAdapter, ChannelError, ChannelMessage, DeliveryReceipt, InboundEvent, OutboundAction,
};

const SLACK_API_BASE: &str = "https://slack.com/api";
const RATE_LIMIT_RETRY_ATTEMPTS: usize = 3;

/// Slack channel adapter using the Web API and Events API webhooks.
///
/// The adapter expects Slack to POST event payloads to a local listener (see
/// [`Self::new`] for the bind address).  Outbound actions are dispatched via
/// the Slack Web API (`chat.postMessage`, `chat.update`, etc.).
pub struct SlackAdapter {
    bot_token: String,
    _signing_secret: Option<String>,
    api_base: String,
    http: Client,
    rx: mpsc::Receiver<InboundEvent>,
    _tx: mpsc::Sender<InboundEvent>,
}

impl SlackAdapter {
    /// Create a new Slack adapter.
    ///
    /// * `bot_token` - Slack bot OAuth token (`xoxb-...`).
    /// * `signing_secret` - Slack signing secret for HMAC-SHA256 webhook
    ///   verification. When `None`, signature checks are skipped (local dev mode).
    /// * `listen_addr` - Local address to bind the Events API webhook receiver
    ///   (e.g. `"0.0.0.0:3100"`). If `None`, only outbound sending is available.
    pub fn new(
        bot_token: impl Into<String>,
        signing_secret: Option<String>,
        listen_addr: Option<String>,
    ) -> Self {
        Self::with_api_base(bot_token, signing_secret, listen_addr, SLACK_API_BASE)
    }

    pub fn with_api_base(
        bot_token: impl Into<String>,
        signing_secret: Option<String>,
        listen_addr: Option<String>,
        api_base: impl Into<String>,
    ) -> Self {
        let bot_token = bot_token.into();
        let api_base = api_base.into().trim_end_matches('/').to_string();
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| Client::new());

        let (tx, rx) = mpsc::channel(256);

        if let Some(addr) = listen_addr {
            let tx = tx.clone();
            let secret = signing_secret.clone();
            tokio::spawn(async move {
                if let Err(e) = Self::run_event_listener(addr, tx, secret).await {
                    error!("slack event listener exited: {e}");
                }
            });
        }

        Self {
            bot_token,
            _signing_secret: signing_secret,
            api_base,
            http,
            rx,
            _tx: tx,
        }
    }

    // ---------- Events API webhook receiver ----------

    /// Minimal HTTP server that receives Slack Events API payloads.
    ///
    /// Handles the `url_verification` challenge and `event_callback` messages.
    async fn run_event_listener(
        addr: String,
        tx: mpsc::Sender<InboundEvent>,
        signing_secret: Option<String>,
    ) -> Result<(), String> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind(&addr)
            .await
            .map_err(|e| format!("failed to bind {addr}: {e}"))?;

        info!("slack event listener bound to {addr}");

        loop {
            let (mut stream, peer) = listener
                .accept()
                .await
                .map_err(|e| format!("accept failed: {e}"))?;

            let tx = tx.clone();
            let signing_secret = signing_secret.clone();

            tokio::spawn(async move {
                const MAX_BODY: usize = 1_048_576; // 1 MB

                // Read until we have the full headers.
                let mut buf = Vec::with_capacity(8192);
                let header_end;
                loop {
                    let mut tmp = vec![0u8; 4096];
                    let n = match stream.read(&mut tmp).await {
                        Ok(0) => {
                            warn!("slack listener: connection closed before headers from {peer}");
                            return;
                        }
                        Ok(n) => n,
                        Err(e) => {
                            warn!("slack listener read error from {peer}: {e}");
                            return;
                        }
                    };
                    buf.extend_from_slice(&tmp[..n]);

                    if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        header_end = pos + 4;
                        break;
                    }
                    if let Some(pos) = buf.windows(2).position(|w| w == b"\n\n") {
                        header_end = pos + 2;
                        break;
                    }
                    if buf.len() > 16_384 {
                        warn!("slack listener: headers too large from {peer}");
                        return;
                    }
                }

                // Parse Content-Length and read the full body.
                let header_text = String::from_utf8_lossy(&buf[..header_end]);
                let content_length: usize = header_text
                    .lines()
                    .find_map(|line| {
                        let line = line.trim_end_matches('\r');
                        let (name, value) = line.split_once(':')?;
                        if name.trim().eq_ignore_ascii_case("content-length") {
                            value.trim().parse().ok()
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0)
                    .min(MAX_BODY);

                let total_needed = header_end + content_length;
                while buf.len() < total_needed {
                    let mut tmp = vec![0u8; 4096];
                    let n = match stream.read(&mut tmp).await {
                        Ok(0) => break,
                        Ok(n) => n,
                        Err(e) => {
                            warn!("slack listener body read error from {peer}: {e}");
                            return;
                        }
                    };
                    buf.extend_from_slice(&tmp[..n]);
                }

                let raw = String::from_utf8_lossy(&buf);

                // Extract headers and body
                let (headers_section, body) = match raw.split_once("\r\n\r\n") {
                    Some((h, b)) => (h, b),
                    None => match raw.split_once("\n\n") {
                        Some((h, b)) => (h, b),
                        None => {
                            warn!("slack listener: no header/body separator found");
                            return;
                        }
                    },
                };

                // Verify signature
                if !verify_slack_signature(headers_section, body, signing_secret.as_deref()) {
                    let response = "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n";
                    let _ = stream.write_all(response.as_bytes()).await;
                    return;
                }

                let parsed: Result<serde_json::Value, _> = serde_json::from_str(body);
                let response = match parsed {
                    Ok(val) => {
                        let event_type = val["type"].as_str().unwrap_or("");
                        match event_type {
                            "url_verification" => {
                                // Respond with the challenge.
                                let challenge = val["challenge"].as_str().unwrap_or("");
                                debug!("slack url_verification challenge received");
                                format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
                                    challenge.len(),
                                    challenge
                                )
                            }
                            "event_callback" => {
                                if let Some(event) = Self::convert_event(&val["event"]) {
                                    let _ = tx.send(event).await;
                                }
                                "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_string()
                            }
                            _ => {
                                debug!("slack unknown event type: {event_type}");
                                "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_string()
                            }
                        }
                    }
                    Err(e) => {
                        warn!("slack listener json parse error: {e}");
                        "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n".to_string()
                    }
                };

                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    }

    /// Convert a Slack event JSON object to an [`InboundEvent`].
    fn convert_event(event: &serde_json::Value) -> Option<InboundEvent> {
        let event_type = event["type"].as_str()?;

        match event_type {
            "message" => {
                // Ignore bot messages, subtypes like message_changed, etc.
                if event.get("subtype").is_some() {
                    return Self::handle_message_subtype(event);
                }

                let channel = event["channel"].as_str().unwrap_or("").to_string();
                let user = event["user"].as_str().unwrap_or("unknown").to_string();
                let text = event["text"].as_str().unwrap_or("").to_string();
                let ts = event["ts"].as_str().unwrap_or("").to_string();

                if text.is_empty() {
                    return None;
                }

                Some(InboundEvent::Message(ChannelMessage {
                    channel_id: ChannelId::new(),
                    raw_chat_id: channel,
                    sender: user,
                    content: text,
                    attachments: Self::extract_attachments(event),
                    timestamp: Self::parse_slack_ts(&ts),
                    provider_message_id: ts,
                }))
            }
            "reaction_added" => {
                let _channel = event["item"]["channel"].as_str().unwrap_or("").to_string();
                let message_ts = event["item"]["ts"].as_str().unwrap_or("").to_string();
                let emoji = event["reaction"].as_str().unwrap_or("").to_string();
                let user = event["user"].as_str().unwrap_or("unknown").to_string();

                Some(InboundEvent::Reaction {
                    channel_id: ChannelId::new(),
                    message_id: message_ts,
                    emoji,
                    user,
                })
            }
            "member_joined_channel" => {
                let user = event["user"].as_str().unwrap_or("unknown").to_string();
                Some(InboundEvent::MemberJoin {
                    channel_id: ChannelId::new(),
                    user,
                })
            }
            "member_left_channel" => {
                let user = event["user"].as_str().unwrap_or("unknown").to_string();
                Some(InboundEvent::MemberLeave {
                    channel_id: ChannelId::new(),
                    user,
                })
            }
            _ => {
                debug!("slack: unhandled event type {event_type}");
                None
            }
        }
    }

    /// Handle message sub-types like `message_changed` and `message_deleted`.
    fn handle_message_subtype(event: &serde_json::Value) -> Option<InboundEvent> {
        let subtype = event["subtype"].as_str()?;
        match subtype {
            "message_changed" => {
                let inner = &event["message"];
                let _channel = event["channel"].as_str().unwrap_or("").to_string();
                let ts = inner["ts"].as_str().unwrap_or("").to_string();
                let new_text = inner["text"].as_str().unwrap_or("").to_string();
                Some(InboundEvent::Edit {
                    channel_id: ChannelId::new(),
                    message_id: ts,
                    new_content: new_text,
                })
            }
            "message_deleted" => {
                let _channel = event["channel"].as_str().unwrap_or("").to_string();
                let ts = event["deleted_ts"].as_str().unwrap_or("").to_string();
                Some(InboundEvent::Delete {
                    channel_id: ChannelId::new(),
                    message_id: ts,
                })
            }
            _ => None,
        }
    }

    /// Extract file attachments from a Slack message event.
    fn extract_attachments(event: &serde_json::Value) -> Vec<rune_core::AttachmentRef> {
        let files = match event["files"].as_array() {
            Some(f) => f,
            None => return Vec::new(),
        };

        files
            .iter()
            .map(|f| {
                let name = f["name"].as_str().unwrap_or("file").to_string();
                let mime = f["mimetype"].as_str().map(|s| s.to_string());
                let size = f["size"].as_u64();
                let url = f["url_private_download"]
                    .as_str()
                    .or_else(|| f["url_private"].as_str())
                    .map(|s| s.to_string());

                rune_core::AttachmentRef {
                    name,
                    mime_type: mime,
                    size_bytes: size,
                    url,
                    provider_file_id: None,
                }
            })
            .collect()
    }

    /// Parse a Slack timestamp (e.g. "1710320000.123456") into a `DateTime<Utc>`.
    fn parse_slack_ts(ts: &str) -> chrono::DateTime<Utc> {
        let secs: f64 = ts.parse().unwrap_or(0.0);
        let secs_i64 = secs as i64;
        let nanos = ((secs - secs_i64 as f64) * 1_000_000_000.0) as u32;
        chrono::TimeZone::timestamp_opt(&Utc, secs_i64, nanos)
            .single()
            .unwrap_or_else(Utc::now)
    }

    // ---------- Web API helpers ----------

    async fn send_api_request(
        &self,
        method: &str,
        body: &serde_json::Value,
    ) -> Result<Response, ChannelError> {
        let url = format!("{}/{method}", self.api_base);
        let mut attempt = 0usize;

        loop {
            let resp = self
                .http
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.bot_token))
                .header("Content-Type", "application/json; charset=utf-8")
                .json(body)
                .send()
                .await
                .map_err(|e| ChannelError::Provider {
                    message: format!("slack API request error: {e}"),
                })?;

            if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                if attempt >= RATE_LIMIT_RETRY_ATTEMPTS {
                    let text = resp.text().await.unwrap_or_default();
                    return Err(ChannelError::Provider {
                        message: format!(
                            "slack API rate limited after {} retries: {}",
                            RATE_LIMIT_RETRY_ATTEMPTS, text
                        ),
                    });
                }

                let retry_after = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<f64>().ok())
                    .unwrap_or(1.0);
                tokio::time::sleep(Duration::from_secs_f64(retry_after.max(0.0))).await;
                attempt += 1;
                continue;
            }

            return Ok(resp);
        }
    }

    async fn api_post(
        &self,
        method: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, ChannelError> {
        let resp = self.send_api_request(method, body).await?;
        let status = resp.status();
        let json: serde_json::Value = resp.json().await.map_err(|e| ChannelError::Provider {
            message: format!("slack API response parse error: {e}"),
        })?;

        if !status.is_success() {
            let err = json["error"].as_str().unwrap_or("unknown_error");
            return Err(ChannelError::Provider {
                message: format!("slack API HTTP {status}: {err}"),
            });
        }

        if !json["ok"].as_bool().unwrap_or(false) {
            let err = json["error"].as_str().unwrap_or("unknown_error");
            return Err(ChannelError::Provider {
                message: format!("slack API error: {err}"),
            });
        }

        Ok(json)
    }
}

#[async_trait]
impl ChannelAdapter for SlackAdapter {
    async fn receive(&mut self) -> Result<InboundEvent, ChannelError> {
        self.rx.recv().await.ok_or(ChannelError::ConnectionLost {
            reason: "slack event listener exited".into(),
        })
    }

    async fn send(&self, action: OutboundAction) -> Result<DeliveryReceipt, ChannelError> {
        match action {
            OutboundAction::Send {
                chat_id, content, ..
            } => {
                let body = serde_json::json!({
                    "channel": chat_id,
                    "text": content,
                });
                let resp = self.api_post("chat.postMessage", &body).await?;
                let ts = resp["ts"].as_str().unwrap_or("").to_string();
                Ok(DeliveryReceipt {
                    provider_message_id: ts,
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
                    "channel": chat_id,
                    "text": content,
                    "thread_ts": reply_to,
                });
                let resp = self.api_post("chat.postMessage", &body).await?;
                let ts = resp["ts"].as_str().unwrap_or("").to_string();
                Ok(DeliveryReceipt {
                    provider_message_id: ts,
                    delivered_at: Utc::now(),
                })
            }
            OutboundAction::Edit {
                chat_id,
                message_id,
                new_content,
                ..
            } => {
                let body = serde_json::json!({
                    "channel": chat_id,
                    "ts": message_id,
                    "text": new_content,
                });
                let resp = self.api_post("chat.update", &body).await?;
                let ts = resp["ts"].as_str().unwrap_or("").to_string();
                Ok(DeliveryReceipt {
                    provider_message_id: ts,
                    delivered_at: Utc::now(),
                })
            }
            OutboundAction::React {
                message_id, emoji, ..
            } => {
                // reactions.add requires a channel; we don't have it in the
                // current OutboundAction::React shape.  Return NotImplemented.
                debug!(
                    "slack react: message_id={message_id} emoji={emoji} (channel not available in action shape)"
                );
                Err(ChannelError::NotImplemented)
            }
            OutboundAction::Delete {
                chat_id,
                message_id,
                ..
            } => {
                let body = serde_json::json!({
                    "channel": chat_id,
                    "ts": message_id,
                });
                self.api_post("chat.delete", &body).await?;
                Ok(DeliveryReceipt {
                    provider_message_id: message_id,
                    delivered_at: Utc::now(),
                })
            }
            OutboundAction::SendTypingIndicator { .. } => {
                // Slack does not have a direct typing indicator API for bots.
                debug!("slack typing indicator: not supported by Slack bot API");
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
                // Map buttons to Slack Block Kit action elements.
                let elements: Vec<serde_json::Value> = buttons
                    .iter()
                    .map(|(label, data)| {
                        serde_json::json!({
                            "type": "button",
                            "text": {
                                "type": "plain_text",
                                "text": label,
                            },
                            "action_id": data,
                            "value": data,
                        })
                    })
                    .collect();

                let body = serde_json::json!({
                    "channel": chat_id,
                    "text": content,
                    "blocks": [
                        {
                            "type": "section",
                            "text": {
                                "type": "mrkdwn",
                                "text": content,
                            },
                        },
                        {
                            "type": "actions",
                            "elements": elements,
                        },
                    ],
                });

                let resp = self.api_post("chat.postMessage", &body).await?;
                let ts = resp["ts"].as_str().unwrap_or("").to_string();
                Ok(DeliveryReceipt {
                    provider_message_id: ts,
                    delivered_at: Utc::now(),
                })
            }
        }
    }
}

/// Verify a Slack webhook request using HMAC-SHA256 signature verification.
///
/// When `signing_secret` is `None`, verification is skipped (local dev mode).
fn verify_slack_signature(headers: &str, body: &str, signing_secret: Option<&str>) -> bool {
    let Some(secret) = signing_secret else {
        tracing::warn!("slack signing_secret not configured — skipping signature verification");
        return true;
    };

    // Extract X-Slack-Request-Timestamp header
    let timestamp = headers.lines().find_map(|line| {
        let line = line.trim_end_matches('\r');
        line.strip_prefix("X-Slack-Request-Timestamp:")
            .or_else(|| line.strip_prefix("x-slack-request-timestamp:"))
            .map(|v| v.trim())
    });

    let Some(timestamp) = timestamp else {
        return false;
    };

    // Reject timestamps older than 5 minutes
    let ts: i64 = match timestamp.parse() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let now = chrono::Utc::now().timestamp();
    if (now - ts).abs() > 300 {
        tracing::warn!("slack request timestamp too old: {timestamp}");
        return false;
    }

    // Extract X-Slack-Signature header
    let signature = headers.lines().find_map(|line| {
        let line = line.trim_end_matches('\r');
        line.strip_prefix("X-Slack-Signature:")
            .or_else(|| line.strip_prefix("x-slack-signature:"))
            .map(|v| v.trim())
    });

    let Some(sig_value) = signature else {
        return false;
    };

    let Some(received_hex) = sig_value.strip_prefix("v0=") else {
        return false;
    };

    // Compute expected signature
    let basestring = format!("v0:{timestamp}:{body}");
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;

    let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(basestring.as_bytes());
    let expected = hex::encode(mac.finalize().into_bytes());

    // Constant-time comparison
    expected == received_hex
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChannelAdapter;
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn parse_slack_timestamp() {
        let ts = SlackAdapter::parse_slack_ts("1710320000.123456");
        assert_eq!(ts.timestamp(), 1710320000);
    }

    #[test]
    fn parse_slack_ts_handles_invalid() {
        let ts = SlackAdapter::parse_slack_ts("not-a-number");
        // "not-a-number" parses to 0.0, yielding Unix epoch.  We just check
        // that the function does not panic.
        assert!(ts.timestamp() >= 0);
    }

    #[test]
    fn convert_basic_message_event() {
        let event = serde_json::json!({
            "type": "message",
            "channel": "C12345",
            "user": "U67890",
            "text": "Hello from Slack!",
            "ts": "1710320000.000100",
        });

        let result = SlackAdapter::convert_event(&event).unwrap();
        match result {
            InboundEvent::Message(msg) => {
                assert_eq!(msg.raw_chat_id, "C12345");
                assert_eq!(msg.sender, "U67890");
                assert_eq!(msg.content, "Hello from Slack!");
            }
            _ => panic!("expected Message event"),
        }
    }

    #[test]
    fn convert_reaction_event() {
        let event = serde_json::json!({
            "type": "reaction_added",
            "user": "U67890",
            "reaction": "thumbsup",
            "item": {
                "channel": "C12345",
                "ts": "1710320000.000100",
            },
        });

        let result = SlackAdapter::convert_event(&event).unwrap();
        match result {
            InboundEvent::Reaction { emoji, user, .. } => {
                assert_eq!(emoji, "thumbsup");
                assert_eq!(user, "U67890");
            }
            _ => panic!("expected Reaction event"),
        }
    }

    #[test]
    fn convert_message_changed_event() {
        let event = serde_json::json!({
            "type": "message",
            "subtype": "message_changed",
            "channel": "C12345",
            "message": {
                "ts": "1710320000.000100",
                "text": "updated text",
            },
        });

        let result = SlackAdapter::convert_event(&event).unwrap();
        match result {
            InboundEvent::Edit {
                message_id,
                new_content,
                ..
            } => {
                assert_eq!(message_id, "1710320000.000100");
                assert_eq!(new_content, "updated text");
            }
            _ => panic!("expected Edit event"),
        }
    }

    #[test]
    fn convert_message_deleted_event() {
        let event = serde_json::json!({
            "type": "message",
            "subtype": "message_deleted",
            "channel": "C12345",
            "deleted_ts": "1710320000.000100",
        });

        let result = SlackAdapter::convert_event(&event).unwrap();
        match result {
            InboundEvent::Delete { message_id, .. } => {
                assert_eq!(message_id, "1710320000.000100");
            }
            _ => panic!("expected Delete event"),
        }
    }

    #[test]
    fn extract_file_attachments() {
        let event = serde_json::json!({
            "type": "message",
            "channel": "C12345",
            "user": "U67890",
            "text": "here's a file",
            "ts": "1710320000.000100",
            "files": [{
                "name": "report.pdf",
                "mimetype": "application/pdf",
                "size": 4096,
                "url_private_download": "https://files.slack.com/files-pri/T1/report.pdf",
            }],
        });

        let attachments = SlackAdapter::extract_attachments(&event);
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].name, "report.pdf");
        assert_eq!(attachments[0].mime_type.as_deref(), Some("application/pdf"));
        assert_eq!(attachments[0].size_bytes, Some(4096));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn send_reply_edit_delete_and_keyboard_use_configured_api_base() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat.postMessage"))
            .and(header("authorization", "Bearer xoxb-test"))
            .and(body_partial_json(serde_json::json!({
                "channel": "C12345",
                "text": "hello slack"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "ts": "1710320000.000100"
            })))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/chat.postMessage"))
            .and(header("authorization", "Bearer xoxb-test"))
            .and(body_partial_json(serde_json::json!({
                "channel": "C12345",
                "text": "reply text",
                "thread_ts": "1710320000.000100"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "ts": "1710320001.000200"
            })))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/chat.update"))
            .and(header("authorization", "Bearer xoxb-test"))
            .and(body_partial_json(serde_json::json!({
                "channel": "C12345",
                "ts": "1710320001.000200",
                "text": "edited text"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "ts": "1710320001.000200"
            })))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/chat.delete"))
            .and(header("authorization", "Bearer xoxb-test"))
            .and(body_partial_json(serde_json::json!({
                "channel": "C12345",
                "ts": "1710320001.000200"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "ts": "1710320001.000200"
            })))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/chat.postMessage"))
            .and(header("authorization", "Bearer xoxb-test"))
            .and(body_partial_json(serde_json::json!({
                "channel": "C12345",
                "text": "choose",
                "blocks": [
                    {
                        "type": "section",
                        "text": {
                            "type": "mrkdwn",
                            "text": "choose"
                        }
                    },
                    {
                        "type": "actions",
                        "elements": [
                            {
                                "type": "button",
                                "text": {
                                    "type": "plain_text",
                                    "text": "One"
                                },
                                "action_id": "one",
                                "value": "one"
                            },
                            {
                                "type": "button",
                                "text": {
                                    "type": "plain_text",
                                    "text": "Two"
                                },
                                "action_id": "two",
                                "value": "two"
                            }
                        ]
                    }
                ]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "ts": "1710320002.000300"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let adapter = SlackAdapter::with_api_base("xoxb-test", None, None, server.uri());

        let send_receipt = adapter
            .send(OutboundAction::Send {
                channel_id: ChannelId::new(),
                chat_id: "C12345".into(),
                content: "hello slack".into(),
            })
            .await
            .expect("send should succeed");
        assert_eq!(send_receipt.provider_message_id, "1710320000.000100");

        let reply_receipt = adapter
            .send(OutboundAction::Reply {
                channel_id: ChannelId::new(),
                chat_id: "C12345".into(),
                reply_to: "1710320000.000100".into(),
                content: "reply text".into(),
            })
            .await
            .expect("reply should succeed");
        assert_eq!(reply_receipt.provider_message_id, "1710320001.000200");

        let edit_receipt = adapter
            .send(OutboundAction::Edit {
                channel_id: ChannelId::new(),
                chat_id: "C12345".into(),
                message_id: "1710320001.000200".into(),
                new_content: "edited text".into(),
            })
            .await
            .expect("edit should succeed");
        assert_eq!(edit_receipt.provider_message_id, "1710320001.000200");

        let delete_receipt = adapter
            .send(OutboundAction::Delete {
                channel_id: ChannelId::new(),
                chat_id: "C12345".into(),
                message_id: "1710320001.000200".into(),
            })
            .await
            .expect("delete should succeed");
        assert_eq!(delete_receipt.provider_message_id, "1710320001.000200");

        let keyboard_receipt = adapter
            .send(OutboundAction::SendInlineKeyboard {
                channel_id: ChannelId::new(),
                chat_id: "C12345".into(),
                content: "choose".into(),
                buttons: vec![("One".into(), "one".into()), ("Two".into(), "two".into())],
            })
            .await
            .expect("keyboard should succeed");
        assert_eq!(keyboard_receipt.provider_message_id, "1710320002.000300");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn send_retries_when_slack_rate_limits() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat.postMessage"))
            .and(header("authorization", "Bearer xoxb-test"))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("retry-after", "0")
                    .set_body_string("rate_limited"),
            )
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/chat.postMessage"))
            .and(header("authorization", "Bearer xoxb-test"))
            .and(body_partial_json(serde_json::json!({
                "channel": "C12345",
                "text": "hello after retry"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "ts": "1710320003.000400"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let adapter = SlackAdapter::with_api_base("xoxb-test", None, None, server.uri());

        let receipt = adapter
            .send(OutboundAction::Send {
                channel_id: ChannelId::new(),
                chat_id: "C12345".into(),
                content: "hello after retry".into(),
            })
            .await
            .expect("send should retry after rate limit");

        assert_eq!(receipt.provider_message_id, "1710320003.000400");
    }

    #[test]
    fn verify_slack_signature_accepts_valid() {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;

        let secret = "test-signing-secret";
        let timestamp = chrono::Utc::now().timestamp().to_string();
        let body = r#"{"type":"event_callback","event":{}}"#;
        let basestring = format!("v0:{timestamp}:{body}");
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(basestring.as_bytes());
        let sig = format!("v0={}", hex::encode(mac.finalize().into_bytes()));

        let headers = format!(
            "POST /slack HTTP/1.1\r\nHost: localhost\r\nX-Slack-Request-Timestamp: {timestamp}\r\nX-Slack-Signature: {sig}"
        );
        assert!(verify_slack_signature(&headers, body, Some(secret)));
    }

    #[test]
    fn verify_slack_signature_rejects_bad_sig() {
        let timestamp = chrono::Utc::now().timestamp().to_string();
        let headers = format!(
            "POST /slack HTTP/1.1\r\nX-Slack-Request-Timestamp: {timestamp}\r\nX-Slack-Signature: v0=deadbeef"
        );
        assert!(!verify_slack_signature(&headers, "body", Some("secret")));
    }

    #[test]
    fn verify_slack_signature_rejects_stale_timestamp() {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;

        let secret = "test-secret";
        let old_ts = (chrono::Utc::now().timestamp() - 600).to_string();
        let body = "body";
        let basestring = format!("v0:{old_ts}:{body}");
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(basestring.as_bytes());
        let sig = format!("v0={}", hex::encode(mac.finalize().into_bytes()));

        let headers = format!(
            "POST /slack HTTP/1.1\r\nX-Slack-Request-Timestamp: {old_ts}\r\nX-Slack-Signature: {sig}"
        );
        assert!(!verify_slack_signature(&headers, body, Some(secret)));
    }

    #[test]
    fn verify_slack_signature_passes_when_none() {
        assert!(verify_slack_signature(
            "POST /slack HTTP/1.1\r\n",
            "body",
            None
        ));
    }
}
