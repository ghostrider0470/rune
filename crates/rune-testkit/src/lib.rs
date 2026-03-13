#![doc = "Shared fixtures and test doubles for Rune crates."]

use async_trait::async_trait;
use rune_core::{ChannelId, NormalizedMessage, SessionId, SessionKind, SessionStatus, TranscriptItem};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Minimal session fixture used by early-wave tests before store/runtime exist.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionFixture {
    pub session_id: SessionId,
    pub kind: SessionKind,
    pub status: SessionStatus,
    pub transcript: Vec<TranscriptItem>,
}

impl Default for SessionFixture {
    fn default() -> Self {
        Self {
            session_id: SessionId::new(),
            kind: SessionKind::Direct,
            status: SessionStatus::Ready,
            transcript: Vec::new(),
        }
    }
}

/// Build a minimal direct-session fixture.
#[must_use]
pub fn fixture_session() -> SessionFixture {
    SessionFixture::default()
}

/// Build a normalized message fixture with safe defaults.
#[must_use]
pub fn fixture_message(content: impl Into<String>) -> NormalizedMessage {
    let mut message = NormalizedMessage::new("test-user", content.into());
    message.channel_id = Some(ChannelId::new());
    message
}

/// Placeholder completion request contract for early-wave fake model tests.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FakeCompletionRequest {
    pub system_prompt: Option<String>,
    pub messages: Vec<String>,
}

/// Placeholder completion response contract for early-wave fake model tests.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FakeCompletionResponse {
    pub content: String,
}

/// Temporary model-provider trait used until `rune-models` lands in Wave 2.
#[async_trait]
pub trait FakeModelProvider: Send + Sync {
    async fn complete(
        &self,
        request: FakeCompletionRequest,
    ) -> Result<FakeCompletionResponse, TestkitError>;
}

/// Canned-response model fake for runtime and provider-facing tests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaticModelProvider {
    response: FakeCompletionResponse,
}

impl StaticModelProvider {
    #[must_use]
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            response: FakeCompletionResponse {
                content: content.into(),
            },
        }
    }
}

#[async_trait]
impl FakeModelProvider for StaticModelProvider {
    async fn complete(
        &self,
        _request: FakeCompletionRequest,
    ) -> Result<FakeCompletionResponse, TestkitError> {
        Ok(self.response.clone())
    }
}

/// Placeholder outbound message capture for early channel-adapter tests.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapturedDelivery {
    pub channel_id: Option<ChannelId>,
    pub content: String,
}

/// Temporary fake channel adapter that stores sent payloads for assertions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FakeChannel {
    deliveries: Vec<CapturedDelivery>,
}

impl FakeChannel {
    #[must_use]
    pub fn deliveries(&self) -> &[CapturedDelivery] {
        &self.deliveries
    }

    pub fn send(&mut self, message: &NormalizedMessage) {
        self.deliveries.push(CapturedDelivery {
            channel_id: message.channel_id,
            content: message.content.clone(),
        });
    }
}

/// Placeholder embedded-DB test helper until the real store crate lands.
#[derive(Debug, Default)]
pub struct TestDb;

impl TestDb {
    /// Construct a placeholder test database handle.
    pub fn new() -> Result<Self, TestkitError> {
        Ok(Self)
    }
}

/// Compare actual output against an expected string.
pub fn assert_golden(actual: &str, expected: &str) -> Result<(), TestkitError> {
    if actual == expected {
        Ok(())
    } else {
        Err(TestkitError::GoldenMismatch {
            expected: expected.to_string(),
            actual: actual.to_string(),
        })
    }
}

/// Compare actual output against a `.expected` file. When `RUNE_UPDATE_GOLDEN=1`
/// is set, the expected file is overwritten with the actual value instead of
/// failing, making it easy to accept new baselines.
pub fn assert_golden_file(
    actual: &str,
    expected_path: impl AsRef<std::path::Path>,
) -> Result<(), TestkitError> {
    use std::fs;
    let path = expected_path.as_ref();

    if std::env::var("RUNE_UPDATE_GOLDEN").as_deref() == Ok("1") {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        fs::write(path, actual).map_err(|e| TestkitError::Io(e.to_string()))?;
        return Ok(());
    }

    let expected = fs::read_to_string(path).map_err(|e| TestkitError::Io(e.to_string()))?;
    assert_golden(actual, &expected)
}

/// Build a `TranscriptItem::UserMessage` fixture from text.
#[must_use]
pub fn fixture_user_message(content: impl Into<String>) -> TranscriptItem {
    TranscriptItem::UserMessage {
        message: fixture_message(content),
    }
}

/// Build a `TranscriptItem::AssistantMessage` fixture from text.
#[must_use]
pub fn fixture_assistant_message(content: impl Into<String>) -> TranscriptItem {
    TranscriptItem::AssistantMessage {
        content: content.into(),
    }
}

/// Build a `TranscriptItem::ToolRequest` fixture.
#[must_use]
pub fn fixture_tool_request(name: impl Into<String>, args: serde_json::Value) -> TranscriptItem {
    TranscriptItem::ToolRequest {
        tool_call_id: rune_core::ToolCallId::new(),
        tool_name: name.into(),
        arguments: args,
    }
}

/// Shared errors for temporary test helpers.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TestkitError {
    #[error("golden mismatch\nexpected: {expected}\nactual: {actual}")]
    GoldenMismatch { expected: String, actual: String },
    #[error("I/O error: {0}")]
    Io(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_fixture_has_valid_defaults() {
        let fixture = fixture_session();
        assert_eq!(fixture.kind, SessionKind::Direct);
        assert_eq!(fixture.status, SessionStatus::Ready);
        assert!(fixture.transcript.is_empty());
    }

    #[test]
    fn message_fixture_has_channel_and_content() {
        let message = fixture_message("hello");
        assert_eq!(message.content, "hello");
        assert!(message.channel_id.is_some());
    }

    #[test]
    fn fake_channel_captures_deliveries() {
        let mut channel = FakeChannel::default();
        let message = fixture_message("captured");
        channel.send(&message);

        assert_eq!(channel.deliveries().len(), 1);
        assert_eq!(channel.deliveries()[0].content, "captured");
    }

    #[test]
    fn golden_helper_detects_match_and_mismatch() {
        assert!(assert_golden("same", "same").is_ok());
        let err = assert_golden("left", "right").unwrap_err();
        assert!(err.to_string().contains("golden mismatch"));
    }

    #[test]
    fn golden_file_creates_and_compares() {
        let dir = std::env::temp_dir().join("rune-testkit-golden");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test.expected");

        // Write baseline
        unsafe { std::env::set_var("RUNE_UPDATE_GOLDEN", "1"); }
        assert_golden_file("baseline content", &path).unwrap();
        unsafe { std::env::remove_var("RUNE_UPDATE_GOLDEN"); }

        // Match succeeds
        assert_golden_file("baseline content", &path).unwrap();

        // Mismatch fails
        let err = assert_golden_file("different", &path).unwrap_err();
        assert!(err.to_string().contains("golden mismatch"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn transcript_fixture_helpers_produce_valid_items() {
        let user = fixture_user_message("hi");
        assert!(matches!(user, rune_core::TranscriptItem::UserMessage { .. }));

        let asst = fixture_assistant_message("hello");
        assert!(matches!(asst, rune_core::TranscriptItem::AssistantMessage { content } if content == "hello"));

        let tool = fixture_tool_request("read", serde_json::json!({"path": "/tmp"}));
        assert!(matches!(tool, rune_core::TranscriptItem::ToolRequest { tool_name, .. } if tool_name == "read"));
    }
}
