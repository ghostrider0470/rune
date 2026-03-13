use async_trait::async_trait;

use crate::{ChannelAdapter, ChannelError, DeliveryReceipt, InboundEvent, OutboundAction};

/// Stub Telegram adapter. Compiles but all methods return [`ChannelError::NotImplemented`].
pub struct TelegramAdapter {
    _token: String,
}

impl TelegramAdapter {
    #[must_use]
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            _token: token.into(),
        }
    }
}

#[async_trait]
impl ChannelAdapter for TelegramAdapter {
    async fn receive(&mut self) -> Result<InboundEvent, ChannelError> {
        Err(ChannelError::NotImplemented)
    }

    async fn send(&self, _action: OutboundAction) -> Result<DeliveryReceipt, ChannelError> {
        Err(ChannelError::NotImplemented)
    }
}
