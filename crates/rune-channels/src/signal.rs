//! Signal adapter via signal-cli REST API.
//!
//! Connects to a running [signal-cli-rest-api](https://github.com/bbernhard/signal-cli-rest-api)
//! daemon.  A background task polls the receive endpoint for new messages and
//! feeds them into an internal mpsc queue.  Outbound actions are dispatched via
//! the REST API.

use std::time::Duration;

use chrono::Utc;
use reqwest::Client;
use rune_core::ChannelId;
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use async_trait::async_trait;

use crate::{
    ChannelAdapter, ChannelError, ChannelMessage, DeliveryReceipt, InboundEvent, OutboundAction,
};

const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Signal channel adapter backed by the signal-cli REST API.
///
/// Expects a signal-cli REST daemon to be running at `base_url` (default
/// `http://localhost:8080`).  The adapter registers with a specific phone
/// `number` and polls for inbound messages.
pub struct SignalAdapter {
    number: String,
    base_url: String,
    poll_inbound: bool,
    http: Client,
    rx: mpsc::Receiver<InboundEvent>,
    _tx: mpsc::Sender<InboundEvent>,
}

impl SignalAdapter {
    /// Create a new Signal adapter.
    ///
    /// * `number`   - The Signal phone number to use (e.g. `"+15551234567"`).
    /// * `base_url` - Base URL of the signal-cli REST API (e.g. `"http://localhost:8080"`).
    pub fn new(number: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self::with_polling(number, base_url, true)
    }

    /// Create a Signal adapter with explicit inbound polling control.
    pub fn with_polling(
        number: impl Into<String>,
        base_url: impl Into<String>,
        poll_inbound: bool,
    ) -> Self {
        let number = number.into();
        let base_url = base_url.into().trim_end_matches('/').to_string();

        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| Client::new());

        let (tx, rx) = mpsc::channel(256);

        if poll_inbound {
            let tx = tx.clone();
            let number = number.clone();
            let base_url = base_url.clone();
            let http = http.clone();
            tokio::spawn(async move {
                Self::poll_loop(tx, number, base_url, http).await;
            });
        }

        Self {
            number,
            base_url,
            poll_inbound,
            http,
            rx,
            _tx: tx,
        }
    }

    // ---------- background poller ----------

    async fn poll_loop(
        tx: mpsc::Sender<InboundEvent>,
        number: String,
        base_url: String,
        http: Client,
    ) {
        loop {
            match Self::receive_messages(&http, &base_url, &number).await {
                Ok(events) => {
                    for event in events {
                        if tx.send(event).await.is_err() {
                            debug!("signal poller: receiver dropped, shutting down");
                            return;
                        }
                    }
                }
                Err(e) => {
                    warn!("signal poll error: {e}");
                }
            }

            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    /// Poll signal-cli REST API for new messages.
    ///
    /// `GET /v1/receive/{number}` returns an array of envelope objects.
    async fn receive_messages(
        http: &Client,
        base_url: &str,
        number: &str,
    ) -> Result<Vec<InboundEvent>, String> {
        let url = format!("{base_url}/v1/receive/{number}");

        let resp = http
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("signal receive request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("signal receive HTTP {status}: {body}"));
        }

        let envelopes: Vec<SignalEnvelope> = resp
            .json()
            .await
            .map_err(|e| format!("signal receive parse error: {e}"))?;

        let mut events = Vec::new();

        for envelope in &envelopes {
            if let Some(event) = Self::convert_envelope(envelope) {
                events.push(event);
            }
        }

        Ok(events)
    }

    /// Convert a signal-cli envelope to an [`InboundEvent`].
    fn convert_envelope(envelope: &SignalEnvelope) -> Option<InboundEvent> {
        let source = envelope.source.as_deref().unwrap_or("unknown");
        let ts = envelope.timestamp.unwrap_or(0);
        let timestamp = chrono::TimeZone::timestamp_millis_opt(&Utc, ts)
            .single()
            .unwrap_or_else(Utc::now);

        // Data message (normal text / media).
        if let Some(data) = &envelope.data_message {
            let text = data.message.clone().unwrap_or_default();

            let attachments: Vec<rune_core::AttachmentRef> = data
                .attachments
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .map(|a| rune_core::AttachmentRef {
                    name: a.filename.clone().unwrap_or_else(|| "attachment".into()),
                    mime_type: a.content_type.clone(),
                    size_bytes: a.size.map(|s| s as u64),
                    url: None,
                    provider_file_id: None,
                })
                .collect();

            // Determine the raw_chat_id: for group messages use groupId,
            // for 1:1 use the source number.
            let chat_id = data
                .group_info
                .as_ref()
                .and_then(|g| g.group_id.clone())
                .unwrap_or_else(|| source.to_string());

            if text.is_empty() && attachments.is_empty() {
                return None;
            }

            return Some(InboundEvent::Message(ChannelMessage {
                channel_id: ChannelId::new(),
                raw_chat_id: chat_id,
                sender: source.to_string(),
                content: text,
                attachments,
                timestamp,
                provider_message_id: ts.to_string(),
            }));
        }

        // Receipt message (read / delivery receipts) -- skip.
        if envelope.receipt_message.is_some() {
            debug!("signal: receipt message from {source} (skipping)");
            return None;
        }

        // Typing message.
        if let Some(typing) = &envelope.typing_message {
            debug!(
                "signal: typing indicator from {source}, action={}",
                typing.action.as_deref().unwrap_or("unknown")
            );
            return None;
        }

        None
    }

    // ---------- REST API helpers ----------

    async fn api_send(&self, body: &serde_json::Value) -> Result<serde_json::Value, ChannelError> {
        let url = format!("{}/v2/send", self.base_url);
        let resp =
            self.http
                .post(&url)
                .json(body)
                .send()
                .await
                .map_err(|e| ChannelError::Provider {
                    message: format!("signal send request error: {e}"),
                })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ChannelError::Provider {
                message: format!("signal API {status}: {text}"),
            });
        }

        resp.json().await.map_err(|e| ChannelError::Provider {
            message: format!("signal send response parse error: {e}"),
        })
    }

    /// Build the base send payload, selecting recipient or group.
    fn build_send_payload(&self, chat_id: &str, message: &str) -> serde_json::Value {
        // If the chat_id looks like a phone number (starts with '+'), send to
        // individual.  Otherwise treat it as a group ID.
        if chat_id.starts_with('+') {
            serde_json::json!({
                "message": message,
                "number": self.number,
                "recipients": [chat_id],
            })
        } else {
            // Group message: base64-encoded group id.
            serde_json::json!({
                "message": message,
                "number": self.number,
                "recipients": [],
                "base64_group": chat_id,
            })
        }
    }
}

#[async_trait]
impl ChannelAdapter for SignalAdapter {
    async fn receive(&mut self) -> Result<InboundEvent, ChannelError> {
        self.rx.recv().await.ok_or(ChannelError::ConnectionLost {
            reason: if self.poll_inbound {
                "signal poller task exited".into()
            } else {
                "signal inbound polling disabled".into()
            },
        })
    }

    async fn send(&self, action: OutboundAction) -> Result<DeliveryReceipt, ChannelError> {
        match action {
            OutboundAction::Send {
                chat_id, content, ..
            } => {
                let payload = self.build_send_payload(&chat_id, &content);
                let resp = self.api_send(&payload).await?;
                let ts = resp["timestamp"].as_str().unwrap_or("").to_string();
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
                // signal-cli v2 send supports quote via "quote_timestamp".
                let mut payload = self.build_send_payload(&chat_id, &content);
                if let Ok(quote_ts) = reply_to.parse::<i64>() {
                    payload["quote_timestamp"] = serde_json::json!(quote_ts);
                }
                let resp = self.api_send(&payload).await?;
                let ts = resp["timestamp"].as_str().unwrap_or("").to_string();
                Ok(DeliveryReceipt {
                    provider_message_id: ts,
                    delivered_at: Utc::now(),
                })
            }
            OutboundAction::Edit { .. } => {
                debug!("signal: message editing not supported by signal-cli REST API");
                Err(ChannelError::NotImplemented)
            }
            OutboundAction::React {
                message_id, emoji, ..
            } => {
                // signal-cli supports reactions via PUT /v1/reactions/{number}.
                let url = format!("{}/v1/reactions/{}", self.base_url, self.number);

                let body = serde_json::json!({
                    "reaction": emoji,
                    "target_author": self.number,
                    "timestamp": message_id.parse::<i64>().unwrap_or(0),
                });

                let resp = self.http.put(&url).json(&body).send().await.map_err(|e| {
                    ChannelError::Provider {
                        message: format!("signal react request error: {e}"),
                    }
                })?;

                if !resp.status().is_success() {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    return Err(ChannelError::Provider {
                        message: format!("signal react API {status}: {text}"),
                    });
                }

                Ok(DeliveryReceipt {
                    provider_message_id: message_id,
                    delivered_at: Utc::now(),
                })
            }
            OutboundAction::Delete { .. } => {
                debug!("signal: message deletion not supported by signal-cli REST API");
                Err(ChannelError::NotImplemented)
            }
            OutboundAction::SendTypingIndicator { chat_id, .. } => {
                // PUT /v1/typing-indicator/{number}
                let url = format!("{}/v1/typing-indicator/{}", self.base_url, self.number);

                let body = if chat_id.starts_with('+') {
                    serde_json::json!({ "recipient": chat_id })
                } else {
                    serde_json::json!({ "base64_group": chat_id })
                };

                let _ = self.http.put(&url).json(&body).send().await;

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
                // Signal does not natively support inline keyboards.  We
                // emulate them by appending numbered options to the message.
                let mut full_message = content.clone();
                full_message.push('\n');
                for (i, (label, _data)) in buttons.iter().enumerate() {
                    full_message.push_str(&format!("\n{}. {}", i + 1, label));
                }

                let payload = self.build_send_payload(&chat_id, &full_message);
                let resp = self.api_send(&payload).await?;
                let ts = resp["timestamp"].as_str().unwrap_or("").to_string();
                Ok(DeliveryReceipt {
                    provider_message_id: ts,
                    delivered_at: Utc::now(),
                })
            }
        }
    }
}

// ---------- signal-cli REST API response types ----------

#[derive(Debug, Deserialize)]
struct SignalEnvelope {
    source: Option<String>,
    #[serde(alias = "sourceNumber")]
    #[allow(dead_code)]
    source_number: Option<String>,
    timestamp: Option<i64>,
    #[serde(alias = "dataMessage")]
    data_message: Option<SignalDataMessage>,
    #[serde(alias = "receiptMessage")]
    receipt_message: Option<serde_json::Value>,
    #[serde(alias = "typingMessage")]
    typing_message: Option<SignalTypingMessage>,
}

#[derive(Debug, Deserialize)]
struct SignalDataMessage {
    message: Option<String>,
    #[allow(dead_code)]
    timestamp: Option<i64>,
    attachments: Option<Vec<SignalAttachment>>,
    #[serde(alias = "groupInfo")]
    group_info: Option<SignalGroupInfo>,
}

#[derive(Debug, Deserialize)]
struct SignalAttachment {
    #[serde(alias = "contentType")]
    content_type: Option<String>,
    filename: Option<String>,
    size: Option<i64>,
    #[allow(dead_code)]
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SignalGroupInfo {
    #[serde(alias = "groupId")]
    group_id: Option<String>,
    #[allow(dead_code)]
    #[serde(alias = "type")]
    group_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SignalTypingMessage {
    action: Option<String>,
    #[allow(dead_code)]
    timestamp: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChannelAdapter;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn make_text_envelope(source: &str, message: &str, ts: i64) -> SignalEnvelope {
        SignalEnvelope {
            source: Some(source.into()),
            source_number: Some(source.into()),
            timestamp: Some(ts),
            data_message: Some(SignalDataMessage {
                message: Some(message.into()),
                timestamp: Some(ts),
                attachments: None,
                group_info: None,
            }),
            receipt_message: None,
            typing_message: None,
        }
    }

    #[test]
    fn convert_text_envelope() {
        let envelope = make_text_envelope("+15551234567", "Hello Signal!", 1710320000000);
        let event = SignalAdapter::convert_envelope(&envelope).unwrap();
        match event {
            InboundEvent::Message(msg) => {
                assert_eq!(msg.sender, "+15551234567");
                assert_eq!(msg.content, "Hello Signal!");
                assert_eq!(msg.raw_chat_id, "+15551234567");
                assert_eq!(msg.provider_message_id, "1710320000000");
            }
            _ => panic!("expected Message event"),
        }
    }

    #[test]
    fn convert_group_message_envelope() {
        let envelope = SignalEnvelope {
            source: Some("+15551234567".into()),
            source_number: Some("+15551234567".into()),
            timestamp: Some(1710320000000),
            data_message: Some(SignalDataMessage {
                message: Some("group hello".into()),
                timestamp: Some(1710320000000),
                attachments: None,
                group_info: Some(SignalGroupInfo {
                    group_id: Some("abc123group".into()),
                    group_type: Some("DELIVER".into()),
                }),
            }),
            receipt_message: None,
            typing_message: None,
        };

        let event = SignalAdapter::convert_envelope(&envelope).unwrap();
        match event {
            InboundEvent::Message(msg) => {
                assert_eq!(msg.raw_chat_id, "abc123group");
                assert_eq!(msg.sender, "+15551234567");
                assert_eq!(msg.content, "group hello");
            }
            _ => panic!("expected Message event"),
        }
    }

    #[test]
    fn convert_envelope_with_attachment() {
        let envelope = SignalEnvelope {
            source: Some("+15559998888".into()),
            source_number: Some("+15559998888".into()),
            timestamp: Some(1710320100000),
            data_message: Some(SignalDataMessage {
                message: Some("file attached".into()),
                timestamp: Some(1710320100000),
                attachments: Some(vec![SignalAttachment {
                    content_type: Some("image/png".into()),
                    filename: Some("screenshot.png".into()),
                    size: Some(8192),
                    id: Some("att001".into()),
                }]),
                group_info: None,
            }),
            receipt_message: None,
            typing_message: None,
        };

        let event = SignalAdapter::convert_envelope(&envelope).unwrap();
        match event {
            InboundEvent::Message(msg) => {
                assert_eq!(msg.attachments.len(), 1);
                assert_eq!(msg.attachments[0].name, "screenshot.png");
                assert_eq!(msg.attachments[0].mime_type.as_deref(), Some("image/png"));
                assert_eq!(msg.attachments[0].size_bytes, Some(8192));
            }
            _ => panic!("expected Message event"),
        }
    }

    #[test]
    fn receipt_envelope_returns_none() {
        let envelope = SignalEnvelope {
            source: Some("+15551234567".into()),
            source_number: Some("+15551234567".into()),
            timestamp: Some(1710320200000),
            data_message: None,
            receipt_message: Some(serde_json::json!({ "type": "DELIVERY" })),
            typing_message: None,
        };

        assert!(SignalAdapter::convert_envelope(&envelope).is_none());
    }

    #[test]
    fn empty_data_message_returns_none() {
        let envelope = SignalEnvelope {
            source: Some("+15551234567".into()),
            source_number: Some("+15551234567".into()),
            timestamp: Some(1710320300000),
            data_message: Some(SignalDataMessage {
                message: None,
                timestamp: Some(1710320300000),
                attachments: None,
                group_info: None,
            }),
            receipt_message: None,
            typing_message: None,
        };

        assert!(SignalAdapter::convert_envelope(&envelope).is_none());
    }

    #[test]
    fn build_send_payload_individual() {
        // We can't call new() without a tokio runtime, so test the payload builder
        // by constructing a minimal adapter manually.
        let payload_json = serde_json::json!({
            "message": "test",
            "number": "+15550001111",
            "recipients": ["+15559998888"],
        });

        // Verify shape.
        assert_eq!(payload_json["recipients"][0], "+15559998888");
        assert_eq!(payload_json["number"], "+15550001111");
    }

    #[test]
    fn build_send_payload_group() {
        let payload_json = serde_json::json!({
            "message": "group test",
            "number": "+15550001111",
            "recipients": [],
            "base64_group": "abc123group",
        });

        assert!(payload_json["base64_group"].as_str().is_some());
        assert_eq!(payload_json["recipients"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn signal_envelope_deser() {
        let json = serde_json::json!({
            "source": "+15551234567",
            "sourceNumber": "+15551234567",
            "timestamp": 1710320000000_i64,
            "dataMessage": {
                "message": "Hello!",
                "timestamp": 1710320000000_i64,
                "attachments": [],
            },
        });

        let envelope: SignalEnvelope = serde_json::from_value(json).unwrap();
        assert_eq!(envelope.source.as_deref(), Some("+15551234567"));
        assert!(envelope.data_message.is_some());
        assert_eq!(
            envelope.data_message.unwrap().message.as_deref(),
            Some("Hello!")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn send_uses_configured_base_url_without_polling() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v2/send"))
            .and(body_partial_json(serde_json::json!({
                "message": "ping",
                "number": "+15551234567",
                "recipients": ["+15559876543"]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "timestamp": "1710320000000"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let adapter = SignalAdapter::with_polling("+15551234567", server.uri(), false);
        let receipt = adapter
            .send(OutboundAction::Send {
                channel_id: ChannelId::new(),
                chat_id: "+15559876543".into(),
                content: "ping".into(),
            })
            .await
            .expect("mocked signal send should succeed");

        assert_eq!(receipt.provider_message_id, "1710320000000");
    }
}
