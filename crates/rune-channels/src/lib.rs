#![doc = "Channel adapters for Rune: normalized inbound/outbound types plus provider implementations (Telegram, Discord, Slack, WhatsApp, Signal)."]

mod discord;
mod signal;
mod slack;
mod telegram;
pub mod types;
mod whatsapp;

pub use discord::DiscordAdapter;
pub use signal::SignalAdapter;
pub use slack::SlackAdapter;
pub use telegram::TelegramAdapter;
pub use types::*;
pub use whatsapp::WhatsAppAdapter;

use async_trait::async_trait;

/// Adapter trait for bidirectional channel communication.
#[async_trait]
pub trait ChannelAdapter: Send + Sync {
    /// Receive the next inbound event from the channel.
    async fn receive(&mut self) -> Result<InboundEvent, ChannelError>;

    /// Send an outbound action to the channel.
    async fn send(&self, action: OutboundAction) -> Result<DeliveryReceipt, ChannelError>;
}

/// Create a channel adapter by provider name.
///
/// Supported `kind` values: `"telegram"`, `"discord"`, `"slack"`, `"whatsapp"`,
/// `"signal"`.
///
/// The caller must ensure the relevant configuration fields are populated;
/// missing required fields will return a [`ChannelError::Provider`].
pub fn create_adapter(
    kind: &str,
    config: &rune_config::ChannelsConfig,
) -> Result<Box<dyn ChannelAdapter>, ChannelError> {
    match kind {
        "telegram" => {
            let token = config
                .telegram_token
                .as_deref()
                .ok_or_else(|| ChannelError::Provider {
                    message: "telegram_token is required for the Telegram adapter".into(),
                })?;
            Ok(Box::new(TelegramAdapter::new(token)))
        }
        "discord" => {
            let token = config
                .discord_token
                .as_deref()
                .ok_or_else(|| ChannelError::Provider {
                    message: "discord_token is required for the Discord adapter".into(),
                })?;
            let guild_id = config.discord_guild_id.as_deref().unwrap_or("");
            Ok(Box::new(DiscordAdapter::new(
                token,
                guild_id,
                config.discord_channel_ids.clone(),
            )))
        }
        "slack" => {
            let bot_token =
                config
                    .slack_bot_token
                    .as_deref()
                    .ok_or_else(|| ChannelError::Provider {
                        message: "slack_bot_token is required for the Slack adapter".into(),
                    })?;
            let app_token = config.slack_app_token.as_deref().unwrap_or("");
            Ok(Box::new(SlackAdapter::new(
                bot_token,
                app_token,
                config.slack_listen_addr.clone(),
            )))
        }
        "whatsapp" => {
            let access_token =
                config
                    .whatsapp_access_token
                    .as_deref()
                    .ok_or_else(|| ChannelError::Provider {
                        message: "whatsapp_access_token is required for the WhatsApp adapter"
                            .into(),
                    })?;
            let phone_number_id = config.whatsapp_phone_number_id.as_deref().ok_or_else(|| {
                ChannelError::Provider {
                    message: "whatsapp_phone_number_id is required for the WhatsApp adapter".into(),
                }
            })?;
            let verify_token = config
                .whatsapp_verify_token
                .as_deref()
                .unwrap_or("rune-verify");
            Ok(Box::new(WhatsAppAdapter::new(
                access_token,
                phone_number_id,
                verify_token,
                config.whatsapp_listen_addr.clone(),
            )))
        }
        "signal" => {
            let number = config
                .signal_number
                .as_deref()
                .ok_or_else(|| ChannelError::Provider {
                    message: "signal_number is required for the Signal adapter".into(),
                })?;
            let api_url = config
                .signal_api_url
                .as_deref()
                .unwrap_or("http://localhost:8080");
            Ok(Box::new(SignalAdapter::new(number, api_url)))
        }
        other => Err(ChannelError::Provider {
            message: format!("unknown channel adapter kind: {other}"),
        }),
    }
}
