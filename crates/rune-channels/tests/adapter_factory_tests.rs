use rune_channels::{ChannelError, create_adapter};
use rune_config::ChannelsConfig;

#[test]
fn telegram_adapter_requires_token() {
    let config = ChannelsConfig::default();
    let err = match create_adapter("telegram", &config) {
        Ok(_) => panic!("telegram should require token"),
        Err(err) => err,
    };
    assert_provider_error(err, "telegram_token is required");
}

#[test]
fn discord_adapter_requires_token() {
    let config = ChannelsConfig::default();
    let err = match create_adapter("discord", &config) {
        Ok(_) => panic!("discord should require token"),
        Err(err) => err,
    };
    assert_provider_error(err, "discord_token is required");
}

#[test]
fn slack_adapter_requires_bot_token() {
    let config = ChannelsConfig::default();
    let err = match create_adapter("slack", &config) {
        Ok(_) => panic!("slack should require bot token"),
        Err(err) => err,
    };
    assert_provider_error(err, "slack_bot_token is required");
}

#[test]
fn whatsapp_adapter_requires_both_credentials() {
    let config = ChannelsConfig {
        whatsapp_access_token: Some("token".into()),
        ..ChannelsConfig::default()
    };
    let err = match create_adapter("whatsapp", &config) {
        Ok(_) => panic!("whatsapp should require phone number id"),
        Err(err) => err,
    };
    assert_provider_error(err, "whatsapp_phone_number_id is required");
}

#[test]
fn signal_adapter_requires_number() {
    let config = ChannelsConfig::default();
    let err = match create_adapter("signal", &config) {
        Ok(_) => panic!("signal should require number"),
        Err(err) => err,
    };
    assert_provider_error(err, "signal_number is required");
}

#[test]
fn unknown_adapter_kind_returns_provider_error() {
    let config = ChannelsConfig::default();
    let err = match create_adapter("matrix", &config) {
        Ok(_) => panic!("unknown adapter kinds should fail"),
        Err(err) => err,
    };
    assert_provider_error(err, "unknown channel adapter kind: matrix");
}

#[tokio::test(flavor = "current_thread")]
async fn configured_adapter_kinds_construct_successfully() {
    let telegram = create_adapter(
        "telegram",
        &ChannelsConfig {
            telegram_token: Some("telegram-token".into()),
            ..ChannelsConfig::default()
        },
    );
    assert!(telegram.is_ok());

    let discord = create_adapter(
        "discord",
        &ChannelsConfig {
            discord_token: Some("discord-token".into()),
            discord_guild_id: Some("guild-1".into()),
            ..ChannelsConfig::default()
        },
    );
    assert!(discord.is_ok());

    let slack = create_adapter(
        "slack",
        &ChannelsConfig {
            slack_bot_token: Some("xoxb-test".into()),
            slack_app_token: Some("xapp-test".into()),
            ..ChannelsConfig::default()
        },
    );
    assert!(slack.is_ok());

    let whatsapp = create_adapter(
        "whatsapp",
        &ChannelsConfig {
            whatsapp_access_token: Some("wa-token".into()),
            whatsapp_phone_number_id: Some("phone-1".into()),
            whatsapp_verify_token: Some("verify-me".into()),
            ..ChannelsConfig::default()
        },
    );
    assert!(whatsapp.is_ok());

    let signal = create_adapter(
        "signal",
        &ChannelsConfig {
            signal_number: Some("+15551234567".into()),
            signal_api_url: Some("http://localhost:8080".into()),
            ..ChannelsConfig::default()
        },
    );
    assert!(signal.is_ok());
}

#[tokio::test(flavor = "current_thread")]
async fn whatsapp_adapter_uses_default_verify_token_when_missing() {
    let adapter = create_adapter(
        "whatsapp",
        &ChannelsConfig {
            whatsapp_access_token: Some("wa-token".into()),
            whatsapp_phone_number_id: Some("phone-1".into()),
            whatsapp_verify_token: None,
            ..ChannelsConfig::default()
        },
    );
    assert!(adapter.is_ok());
}

#[tokio::test(flavor = "current_thread")]
async fn slack_and_signal_allow_optional_secondary_connection_fields() {
    let slack = create_adapter(
        "slack",
        &ChannelsConfig {
            slack_bot_token: Some("xoxb-test".into()),
            slack_app_token: None,
            ..ChannelsConfig::default()
        },
    );
    assert!(slack.is_ok());

    let signal = create_adapter(
        "signal",
        &ChannelsConfig {
            signal_number: Some("+15551234567".into()),
            signal_api_url: None,
            ..ChannelsConfig::default()
        },
    );
    assert!(signal.is_ok());
}

#[tokio::test(flavor = "current_thread")]
async fn discord_adapter_allows_missing_guild_id_for_send_only_mode() {
    let adapter = create_adapter(
        "discord",
        &ChannelsConfig {
            discord_token: Some("discord-token".into()),
            discord_guild_id: None,
            ..ChannelsConfig::default()
        },
    );
    assert!(adapter.is_ok());
}

#[tokio::test(flavor = "current_thread")]
async fn signal_adapter_defaults_api_url_when_missing() {
    let adapter = create_adapter(
        "signal",
        &ChannelsConfig {
            signal_number: Some("+15551234567".into()),
            signal_api_url: None,
            ..ChannelsConfig::default()
        },
    )
    .expect("signal adapter should construct with default api url");

    let err = adapter
        .send(rune_channels::OutboundAction::Send {
            channel_id: rune_core::ChannelId::new(),
            chat_id: "+15559876543".into(),
            content: "ping".into(),
        })
        .await
        .expect_err("default localhost signal api should not succeed during unit tests");

    assert_provider_error(err, "signal send request error");
}

#[tokio::test(flavor = "current_thread")]
async fn whatsapp_adapter_defaults_verify_token_without_blocking_send_path() {
    let adapter = create_adapter(
        "whatsapp",
        &ChannelsConfig {
            whatsapp_access_token: Some("wa-token".into()),
            whatsapp_phone_number_id: Some("phone-1".into()),
            whatsapp_verify_token: None,
            ..ChannelsConfig::default()
        },
    )
    .expect("whatsapp adapter should construct with default verify token");

    let err = adapter
        .send(rune_channels::OutboundAction::Send {
            channel_id: rune_core::ChannelId::new(),
            chat_id: "15551234567".into(),
            content: "ping".into(),
        })
        .await
        .expect_err("cloud api call should fail with dummy credentials");

    assert_provider_error(err, "whatsapp API");
}

#[tokio::test(flavor = "current_thread")]
async fn channels_config_wiring_supports_optional_listener_and_polling_fields() {
    let config = ChannelsConfig {
        discord_token: Some("discord-token".into()),
        discord_guild_id: Some("guild-1".into()),
        discord_channel_ids: vec!["chan-1".into(), "chan-2".into()],
        slack_bot_token: Some("xoxb-test".into()),
        slack_listen_addr: Some("127.0.0.1:3100".into()),
        whatsapp_access_token: Some("wa-token".into()),
        whatsapp_phone_number_id: Some("phone-1".into()),
        whatsapp_listen_addr: Some("127.0.0.1:3200".into()),
        signal_number: Some("+15551234567".into()),
        ..ChannelsConfig::default()
    };

    assert!(create_adapter("discord", &config).is_ok());
    assert!(create_adapter("slack", &config).is_ok());
    assert!(create_adapter("whatsapp", &config).is_ok());
}

fn assert_provider_error(err: ChannelError, expected: &str) {
    match err {
        ChannelError::Provider { message } => assert!(message.contains(expected), "{message}"),
        other => panic!("expected provider error, got {other:?}"),
    }
}
