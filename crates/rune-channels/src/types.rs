use chrono::{DateTime, Utc};
use rune_core::{AttachmentRef, ChannelId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A message received from or sent to a channel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelMessage {
    pub channel_id: ChannelId,
    /// Provider-native channel/chat identifier (e.g. Telegram chat_id as string).
    /// Used for sending replies back to the correct channel.
    pub raw_chat_id: String,
    pub sender: String,
    pub content: String,
    pub attachments: Vec<AttachmentRef>,
    pub timestamp: DateTime<Utc>,
    pub provider_message_id: String,
}

/// Confirmation that an outbound action was delivered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryReceipt {
    pub provider_message_id: String,
    pub delivered_at: DateTime<Utc>,
}

/// Events arriving from a channel provider.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InboundEvent {
    Message(ChannelMessage),
    Reaction {
        channel_id: ChannelId,
        message_id: String,
        emoji: String,
        user: String,
    },
    Edit {
        channel_id: ChannelId,
        message_id: String,
        new_content: String,
    },
    Delete {
        channel_id: ChannelId,
        message_id: String,
    },
    MemberJoin {
        channel_id: ChannelId,
        user: String,
    },
    MemberLeave {
        channel_id: ChannelId,
        user: String,
    },
}

/// Actions the runtime can dispatch to a channel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutboundAction {
    Send {
        channel_id: ChannelId,
        content: String,
    },
    Reply {
        channel_id: ChannelId,
        /// Provider-native chat identifier for routing the reply.
        chat_id: String,
        reply_to: String,
        content: String,
    },
    Edit {
        channel_id: ChannelId,
        message_id: String,
        new_content: String,
    },
    React {
        channel_id: ChannelId,
        message_id: String,
        emoji: String,
    },
    Delete {
        channel_id: ChannelId,
        message_id: String,
    },
}

/// Channel-layer errors.
#[derive(Debug, Error)]
pub enum ChannelError {
    #[error("provider error: {message}")]
    Provider { message: String },

    #[error("not implemented")]
    NotImplemented,

    #[error("connection lost: {reason}")]
    ConnectionLost { reason: String },
}
