//! Microsoft Teams channel adapter.
//!
//! Current scope is focused on adapter-factory parity: the adapter can be
//! constructed from config and exposes the normalized [`ChannelAdapter`] trait,
//! while inbound webhook hosting and outbound Bot Framework calls remain follow-up
//! work.

use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;

use crate::{ChannelAdapter, ChannelError, DeliveryReceipt, InboundEvent, OutboundAction};

const TEAMS_API_BASE: &str = "https://smba.trafficmanager.net";

/// Microsoft Teams adapter placeholder backed by Bot Framework credentials.
///
/// This adapter currently supports construction and normalized trait wiring so
/// the runtime can recognize/configure Teams as a first-class channel kind.
/// Transport-specific send/receive behavior is intentionally left for follow-up
/// implementation work.
pub struct TeamsAdapter {
    app_id: String,
    app_password: String,
    tenant_id: Option<String>,
    listen_addr: Option<String>,
    api_base: String,
    client: Client,
}

impl TeamsAdapter {
    pub fn new(
        app_id: &str,
        app_password: &str,
        tenant_id: Option<&str>,
        listen_addr: Option<String>,
    ) -> Self {
        Self::with_api_base(app_id, app_password, tenant_id, listen_addr, TEAMS_API_BASE)
    }

    fn with_api_base(
        app_id: &str,
        app_password: &str,
        tenant_id: Option<&str>,
        listen_addr: Option<String>,
        api_base: &str,
    ) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            app_id: app_id.to_string(),
            app_password: app_password.to_string(),
            tenant_id: tenant_id.map(ToOwned::to_owned),
            listen_addr,
            api_base: api_base.trim_end_matches('/').to_string(),
            client,
        }
    }

    #[cfg(test)]
    pub fn app_id(&self) -> &str {
        &self.app_id
    }

    #[cfg(test)]
    pub fn tenant_id(&self) -> Option<&str> {
        self.tenant_id.as_deref()
    }

    #[cfg(test)]
    pub fn listen_addr(&self) -> Option<&str> {
        self.listen_addr.as_deref()
    }
}

#[async_trait]
impl ChannelAdapter for TeamsAdapter {
    async fn receive(&mut self) -> Result<InboundEvent, ChannelError> {
        let _ = (
            &self.app_id,
            &self.app_password,
            &self.tenant_id,
            &self.listen_addr,
            &self.api_base,
            &self.client,
        );

        Err(ChannelError::NotImplemented)
    }

    async fn send(&self, action: OutboundAction) -> Result<DeliveryReceipt, ChannelError> {
        let _ = (
            &self.app_id,
            &self.app_password,
            &self.tenant_id,
            &self.listen_addr,
            &self.api_base,
            &self.client,
            action,
        );

        Err(ChannelError::NotImplemented)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use rune_core::ChannelId;

    use crate::ChannelMessage;

    #[test]
    fn teams_adapter_constructs() {
        let adapter = TeamsAdapter::new(
            "app-id",
            "app-password",
            Some("tenant-id"),
            Some("127.0.0.1:3400".into()),
        );

        assert_eq!(adapter.app_id(), "app-id");
        assert_eq!(adapter.tenant_id(), Some("tenant-id"));
        assert_eq!(adapter.listen_addr(), Some("127.0.0.1:3400"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn teams_adapter_send_and_receive_are_explicitly_unimplemented() {
        let mut adapter = TeamsAdapter::new("app-id", "app-password", None, None);

        let receive_err = adapter
            .receive()
            .await
            .expect_err("receive should be stubbed");
        assert!(matches!(receive_err, ChannelError::NotImplemented));

        let send_err = adapter
            .send(OutboundAction::Send {
                channel_id: ChannelId::new(),
                chat_id: "chat-1".into(),
                content: "hello".into(),
            })
            .await
            .expect_err("send should be stubbed");
        assert!(matches!(send_err, ChannelError::NotImplemented));
    }

    #[test]
    fn teams_message_types_are_available() {
        let _message = ChannelMessage {
            channel_id: ChannelId::new(),
            raw_chat_id: "conversation-1".into(),
            sender: "user-1".into(),
            content: "hello".into(),
            attachments: vec![],
            timestamp: Utc::now(),
            provider_message_id: "provider-1".into(),
        };
    }
}
