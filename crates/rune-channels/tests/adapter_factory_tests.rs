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

fn assert_provider_error(err: ChannelError, expected: &str) {
    match err {
        ChannelError::Provider { message } => assert!(message.contains(expected), "{message}"),
        other => panic!("expected provider error, got {other:?}"),
    }
}
