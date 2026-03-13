#![doc = "Channel adapters for Rune: inbound events, outbound actions, and provider stubs."]

mod telegram;
pub mod types;

pub use telegram::TelegramAdapter;
pub use types::*;

use async_trait::async_trait;

/// Adapter trait for bidirectional channel communication.
#[async_trait]
pub trait ChannelAdapter: Send + Sync {
    /// Receive the next inbound event from the channel.
    async fn receive(&mut self) -> Result<InboundEvent, ChannelError>;

    /// Send an outbound action to the channel.
    async fn send(&self, action: OutboundAction) -> Result<DeliveryReceipt, ChannelError>;
}
