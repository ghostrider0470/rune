//! Filesystem-based inter-agent communication client.
//!
//! Reads messages from an inbox directory, writes messages to a peer's inbox,
//! and archives processed messages. Implements the .comms/ protocol.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};
use uuid::Uuid;

/// A message in the .comms/ protocol.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommsMessage {
    pub id: String,
    pub from: String,
    pub to: String,
    #[serde(rename = "type")]
    pub msg_type: String,
    pub subject: String,
    pub body: String,
    #[serde(default = "default_priority")]
    pub priority: String,
    #[serde(default)]
    pub refs: Option<serde_json::Value>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
}

fn default_priority() -> String {
    "p1".to_string()
}

/// Tool-facing summary for a received comms message.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommsMessageSummary {
    pub id: String,
    pub from: String,
    pub subject: String,
    pub body: String,
    pub priority: String,
    pub created_at: Option<String>,
}

/// Transport abstraction for native inter-agent comms.
#[async_trait]
pub trait CommsTransport: Send + Sync {
    async fn send(&self, message: CommsMessage) -> Result<(), String>;
    async fn receive(&self, agent_id: &str) -> Result<Vec<(PathBuf, CommsMessage)>, String>;
    async fn ack(&self, path: &Path) -> Result<(), String>;
}

/// Built-in transport kinds for native inter-agent comms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommsTransportKind {
    Filesystem,
}

impl CommsTransportKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Filesystem => "filesystem",
        }
    }
}

impl FromStr for CommsTransportKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "filesystem" | "fs" => Ok(Self::Filesystem),
            other => Err(format!("unsupported comms transport: {other}")),
        }
    }
}

/// Filesystem-backed transport for the `.comms/` mailbox protocol.
#[derive(Clone)]
pub struct FsCommsTransport {
    comms_dir: PathBuf,
}

impl FsCommsTransport {
    pub fn new(comms_dir: impl Into<PathBuf>) -> Self {
        Self {
            comms_dir: comms_dir.into(),
        }
    }

    pub fn comms_dir(&self) -> &Path {
        &self.comms_dir
    }
}

#[async_trait]
impl CommsTransport for FsCommsTransport {
    async fn send(&self, message: CommsMessage) -> Result<(), String> {
        let peer_inbox = self.comms_dir.join(&message.to).join("inbox");
        if let Err(e) = tokio::fs::create_dir_all(&peer_inbox).await {
            return Err(format!("failed to create peer inbox: {e}"));
        }

        let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let slug = message
            .subject
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-' || *c == ' ')
            .collect::<String>()
            .replace(' ', "-")
            .to_lowercase();
        let slug = if slug.len() > 40 { &slug[..40] } else { &slug };
        let filename = format!("{timestamp}_{}_{}.json", message.msg_type, slug);
        let path = peer_inbox.join(&filename);

        let json = serde_json::to_string_pretty(&message)
            .map_err(|e| format!("failed to serialize message: {e}"))?;

        tokio::fs::write(&path, json)
            .await
            .map_err(|e| format!("failed to write message: {e}"))
    }

    async fn receive(&self, agent_id: &str) -> Result<Vec<(PathBuf, CommsMessage)>, String> {
        let inbox = self.comms_dir.join(agent_id).join("inbox");
        if !inbox.is_dir() {
            return Ok(Vec::new());
        }

        let mut messages = Vec::new();
        let mut entries = tokio::fs::read_dir(&inbox)
            .await
            .map_err(|e| format!("failed to read comms inbox: {e}"))?;

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            match tokio::fs::read_to_string(&path).await {
                Ok(content) => match serde_json::from_str::<CommsMessage>(&content) {
                    Ok(msg) => messages.push((path, msg)),
                    Err(e) => {
                        warn!(path = %path.display(), error = %e, "failed to parse comms message")
                    }
                },
                Err(e) => warn!(path = %path.display(), error = %e, "failed to read comms message"),
            }
        }

        messages.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(messages)
    }

    async fn ack(&self, path: &Path) -> Result<(), String> {
        let archive_dir = self.comms_dir.join(".archive");
        if let Err(e) = tokio::fs::create_dir_all(&archive_dir).await {
            return Err(format!("failed to create archive dir: {e}"));
        }

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown.json");
        let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let archive_name = format!("{timestamp}_{filename}");
        let archive_path = archive_dir.join(archive_name);

        tokio::fs::rename(path, &archive_path)
            .await
            .map_err(|e| format!("failed to archive message: {e}"))?;

        debug!(from = %path.display(), to = %archive_path.display(), "comms message archived");
        Ok(())
    }
}

pub fn build_comms_transport(
    transport: CommsTransportKind,
    comms_dir: impl Into<PathBuf>,
) -> Arc<dyn CommsTransport> {
    match transport {
        CommsTransportKind::Filesystem => Arc::new(FsCommsTransport::new(comms_dir)),
    }
}

/// The comms client — reads/writes messages using a configurable transport.
#[derive(Clone)]
pub struct CommsClient {
    transport: Arc<dyn CommsTransport>,
    agent_id: String,
    peer_id: String,
}

impl CommsClient {
    pub fn new(
        comms_dir: impl Into<PathBuf>,
        agent_id: impl Into<String>,
        peer_id: impl Into<String>,
    ) -> Self {
        Self::with_transport_kind(CommsTransportKind::Filesystem, comms_dir, agent_id, peer_id)
    }

    pub fn with_transport_kind(
        transport: CommsTransportKind,
        comms_dir: impl Into<PathBuf>,
        agent_id: impl Into<String>,
        peer_id: impl Into<String>,
    ) -> Self {
        Self::with_transport(
            build_comms_transport(transport, comms_dir),
            agent_id,
            peer_id,
        )
    }

    pub fn with_transport(
        transport: Arc<dyn CommsTransport>,
        agent_id: impl Into<String>,
        peer_id: impl Into<String>,
    ) -> Self {
        Self {
            transport,
            agent_id: agent_id.into(),
            peer_id: peer_id.into(),
        }
    }

    /// Read all messages from our inbox.
    pub async fn read_inbox(&self) -> Vec<(PathBuf, CommsMessage)> {
        match self.transport.receive(&self.agent_id).await {
            Ok(messages) => messages,
            Err(e) => {
                warn!(error = %e, "failed to read comms inbox");
                Vec::new()
            }
        }
    }

    /// Read inbox messages as summaries and optionally archive them.
    pub async fn read_inbox_summary(
        &self,
        mark_read: bool,
    ) -> Result<Vec<CommsMessageSummary>, String> {
        let messages = self.transport.receive(&self.agent_id).await?;
        let mut summaries = Vec::with_capacity(messages.len());
        for (path, msg) in messages {
            summaries.push(CommsMessageSummary {
                id: msg.id.clone(),
                from: msg.from.clone(),
                subject: msg.subject.clone(),
                body: msg.body.clone(),
                priority: msg.priority.clone(),
                created_at: msg.created_at.clone(),
            });

            if mark_read {
                self.transport.ack(&path).await?;
            }
        }
        Ok(summaries)
    }

    /// Write a message to the peer's inbox.
    pub async fn send(
        &self,
        msg_type: &str,
        subject: &str,
        body: &str,
        priority: &str,
    ) -> Result<String, String> {
        let id = format!("msg-{}", Uuid::now_v7());
        let now = Utc::now().to_rfc3339();
        let msg = CommsMessage {
            id: id.clone(),
            from: self.agent_id.clone(),
            to: self.peer_id.clone(),
            msg_type: msg_type.to_string(),
            subject: subject.to_string(),
            body: body.to_string(),
            priority: priority.to_string(),
            refs: None,
            created_at: Some(now.clone()),
            expires_at: None,
        };

        self.transport.send(msg).await?;

        info!(id = %id, to = %self.peer_id, msg_type = msg_type, subject = subject, "comms message sent");
        Ok(id)
    }

    /// Send an ack for a received message.
    pub async fn send_ack(&self, original: &CommsMessage, summary: &str) -> Result<String, String> {
        let body = format!("Acknowledged: {}\n\n{}", original.subject, summary);
        self.send("ack", &format!("ack: {}", original.subject), &body, "p2")
            .await
    }

    /// Archive a processed message.
    pub async fn archive(&self, path: &Path) -> Result<(), String> {
        self.transport.ack(path).await
    }

    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }
    pub fn peer_id(&self) -> &str {
        &self.peer_id
    }
    pub fn transport(&self) -> &Arc<dyn CommsTransport> {
        &self.transport
    }
}

#[async_trait]
impl rune_tools::comms_tool::CommsOps for CommsClient {
    async fn send_message(
        &self,
        to: &str,
        msg_type: &str,
        subject: &str,
        body: &str,
        priority: &str,
    ) -> Result<String, String> {
        let client = Self::with_transport(
            self.transport.clone(),
            self.agent_id.clone(),
            to.to_string(),
        );
        client.send(msg_type, subject, body, priority).await
    }

    async fn read_inbox(
        &self,
        mark_read: bool,
    ) -> Result<Vec<rune_tools::comms_tool::CommsMessageSummary>, String> {
        Ok(self
            .read_inbox_summary(mark_read)
            .await?
            .into_iter()
            .map(|msg| rune_tools::comms_tool::CommsMessageSummary {
                id: msg.id,
                from: msg.from,
                subject: msg.subject,
                body: msg.body,
                priority: msg.priority,
                created_at: msg.created_at,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn send_and_read_message() {
        let tmp = TempDir::new().unwrap();
        let comms_dir = tmp.path();

        let sender = CommsClient::new(comms_dir, "rune", "horizon-ai");
        let receiver = CommsClient::new(comms_dir, "horizon-ai", "rune");

        sender
            .send("task", "test task", "do something", "p1")
            .await
            .unwrap();

        let messages = receiver.read_inbox().await;
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].1.msg_type, "task");
        assert_eq!(messages[0].1.from, "rune");
        assert_eq!(messages[0].1.to, "horizon-ai");
        assert_eq!(messages[0].1.subject, "test task");
    }

    #[tokio::test]
    async fn archive_moves_file() {
        let tmp = TempDir::new().unwrap();
        let client = CommsClient::new(tmp.path(), "rune", "horizon-ai");

        let inbox = tmp.path().join("rune").join("inbox");
        tokio::fs::create_dir_all(&inbox).await.unwrap();
        let msg_path = inbox.join("test.json");
        tokio::fs::write(
            &msg_path,
            r#"{"id":"t","from":"x","to":"y","type":"ack","subject":"s","body":"b"}"#,
        )
        .await
        .unwrap();

        assert!(msg_path.exists());
        client.archive(&msg_path).await.unwrap();
        assert!(!msg_path.exists());
        assert!(tmp.path().join(".archive").is_dir());
    }

    #[tokio::test]
    async fn empty_inbox_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let client = CommsClient::new(tmp.path(), "rune", "horizon-ai");
        let messages = client.read_inbox().await;
        assert!(messages.is_empty());
    }

    #[tokio::test]
    async fn parse_transport_kind_aliases() {
        assert_eq!(
            CommsTransportKind::from_str("filesystem").unwrap(),
            CommsTransportKind::Filesystem
        );
        assert_eq!(
            CommsTransportKind::from_str("FS").unwrap(),
            CommsTransportKind::Filesystem
        );
        assert!(CommsTransportKind::from_str("http").is_err());
    }

    #[tokio::test]
    async fn read_inbox_summary_marks_read_when_requested() {
        let tmp = TempDir::new().unwrap();
        let sender = CommsClient::new(tmp.path(), "rune", "horizon-ai");
        let receiver = CommsClient::new(tmp.path(), "horizon-ai", "rune");

        sender
            .send("directive", "ship it", "please ship", "p1")
            .await
            .unwrap();

        let summaries = receiver.read_inbox_summary(true).await.unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].subject, "ship it");
        assert!(receiver.read_inbox().await.is_empty());
    }
}
