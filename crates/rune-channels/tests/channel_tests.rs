use rune_channels::{
    ChannelAdapter, ChannelError, ChannelMessage, DeliveryReceipt, InboundEvent, OutboundAction,
    TelegramAdapter,
};
use rune_core::ChannelId;

#[test]
fn telegram_adapter_is_object_safe() {
    let adapter = TelegramAdapter::new("test-token");
    let _boxed: Box<dyn ChannelAdapter> = Box::new(adapter);
}

#[tokio::test]
async fn telegram_receive_with_bad_token_returns_provider_error() {
    let mut adapter = TelegramAdapter::with_base_url("bad-token", "http://127.0.0.1:1");
    let err = adapter.receive().await.unwrap_err();
    assert!(matches!(err, ChannelError::Provider { .. }));
}

#[tokio::test]
async fn telegram_send_with_bad_token_returns_provider_error() {
    let adapter = TelegramAdapter::with_base_url("bad-token", "http://127.0.0.1:1");
    let action = OutboundAction::Send {
        channel_id: ChannelId::new(),
        chat_id: "123456".into(),
        content: "hello".into(),
    };
    let err = adapter.send(action).await.unwrap_err();
    assert!(matches!(err, ChannelError::Provider { .. }));
}

#[test]
fn inbound_event_message_roundtrips_via_serde() {
    let msg = ChannelMessage {
        channel_id: ChannelId::new(),
        raw_chat_id: "chat-1".into(),
        sender: "user-1".into(),
        content: "hello".into(),
        attachments: vec![],
        timestamp: chrono::Utc::now(),
        provider_message_id: "msg-123".into(),
    };
    let event = InboundEvent::Message(msg);
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "message");
    let restored: InboundEvent = serde_json::from_value(json).unwrap();
    assert_eq!(event, restored);
}

#[test]
fn inbound_event_variants_serialize_with_correct_tags() {
    let cid = ChannelId::new();

    let cases: Vec<(InboundEvent, &str)> = vec![
        (
            InboundEvent::Reaction {
                channel_id: cid,
                message_id: "m1".into(),
                emoji: "👍".into(),
                user: "u1".into(),
            },
            "reaction",
        ),
        (
            InboundEvent::Edit {
                channel_id: cid,
                message_id: "m1".into(),
                new_content: "edited".into(),
            },
            "edit",
        ),
        (
            InboundEvent::Delete {
                channel_id: cid,
                message_id: "m1".into(),
            },
            "delete",
        ),
        (
            InboundEvent::MemberJoin {
                channel_id: cid,
                user: "u1".into(),
            },
            "member_join",
        ),
        (
            InboundEvent::MemberLeave {
                channel_id: cid,
                user: "u1".into(),
            },
            "member_leave",
        ),
    ];

    for (event, expected_tag) in cases {
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], expected_tag, "tag mismatch for {event:?}");
        let restored: InboundEvent = serde_json::from_value(json).unwrap();
        assert_eq!(event, restored);
    }
}

#[test]
fn outbound_action_variants_roundtrip() {
    let cid = ChannelId::new();

    let cases: Vec<(OutboundAction, &str)> = vec![
        (
            OutboundAction::Send {
                channel_id: cid,
                chat_id: "chat-1".into(),
                content: "hi".into(),
            },
            "send",
        ),
        (
            OutboundAction::Reply {
                channel_id: cid,
                chat_id: "chat-1".into(),
                reply_to: "m1".into(),
                content: "re".into(),
            },
            "reply",
        ),
        (
            OutboundAction::Edit {
                channel_id: cid,
                chat_id: "chat-1".into(),
                message_id: "m1".into(),
                new_content: "v2".into(),
            },
            "edit",
        ),
        (
            OutboundAction::React {
                channel_id: cid,
                message_id: "m1".into(),
                emoji: "🔥".into(),
            },
            "react",
        ),
        (
            OutboundAction::Delete {
                channel_id: cid,
                chat_id: "chat-1".into(),
                message_id: "m1".into(),
            },
            "delete",
        ),
    ];

    for (action, expected_tag) in cases {
        let json = serde_json::to_value(&action).unwrap();
        assert_eq!(json["type"], expected_tag);
        let restored: OutboundAction = serde_json::from_value(json).unwrap();
        assert_eq!(action, restored);
    }
}

#[test]
fn delivery_receipt_roundtrips() {
    let receipt = DeliveryReceipt {
        provider_message_id: "msg-456".into(),
        delivered_at: chrono::Utc::now(),
    };
    let json = serde_json::to_string(&receipt).unwrap();
    let restored: DeliveryReceipt = serde_json::from_str(&json).unwrap();
    assert_eq!(receipt, restored);
}
