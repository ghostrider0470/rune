//! Filesystem-based inter-agent communication client.
//!
//! Reads messages from an inbox directory, writes messages to a peer's inbox,
//! and archives processed messages. Implements the .comms/ protocol.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

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

pub type InboxItem = (PathBuf, CommsMessage);
pub type CommsOpFuture<T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send>>;

pub trait CommsTransport: Send + Sync {
    fn read_inbox(&self) -> CommsOpFuture<Vec<InboxItem>>;
    fn send(&self, msg: CommsMessage) -> CommsOpFuture<String>;
    fn ack(&self, original: &CommsMessage, summary: &str) -> CommsOpFuture<String>;
    fn archive(&self, path: PathBuf) -> CommsOpFuture<()>;
    fn agent_id(&self) -> &str;
    fn peer_id(&self) -> &str;
    fn backend_name(&self) -> &str;
}

/// Filesystem mailbox transport preserving the existing `.comms/` behavior.
#[derive(Clone)]
pub struct FsCommsTransport {
    comms_dir: PathBuf,
    agent_id: String,
    peer_id: String,
}

impl FsCommsTransport {
    pub fn new(
        comms_dir: impl Into<PathBuf>,
        agent_id: impl Into<String>,
        peer_id: impl Into<String>,
    ) -> Self {
        Self {
            comms_dir: comms_dir.into(),
            agent_id: agent_id.into(),
            peer_id: peer_id.into(),
        }
    }

    pub fn comms_dir(&self) -> &Path {
        &self.comms_dir
    }
}

impl CommsTransport for FsCommsTransport {
    fn read_inbox(&self) -> CommsOpFuture<Vec<InboxItem>> {
        let inbox = self.comms_dir.join(&self.agent_id).join("inbox");
        Box::pin(async move {
            if !inbox.is_dir() {
                return Ok(Vec::new());
            }

            let mut messages = Vec::new();
            let mut entries = match tokio::fs::read_dir(&inbox).await {
                Ok(e) => e,
                Err(e) => {
                    warn!(error = %e, "failed to read comms inbox");
                    return Ok(Vec::new());
                }
            };

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
                    Err(e) => {
                        warn!(path = %path.display(), error = %e, "failed to read comms message")
                    }
                }
            }

            messages.sort_by(|a, b| a.0.cmp(&b.0));
            Ok(messages)
        })
    }

    fn send(&self, msg: CommsMessage) -> CommsOpFuture<String> {
        let peer_inbox = self.comms_dir.join(&self.peer_id).join("inbox");
        let peer_id = self.peer_id.clone();
        Box::pin(async move {
            if let Err(e) = tokio::fs::create_dir_all(&peer_inbox).await {
                return Err(format!("failed to create peer inbox: {e}"));
            }

            let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
            let slug = msg
                .subject
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '-' || *c == ' ')
                .collect::<String>()
                .replace(' ', "-")
                .to_lowercase();
            let slug = if slug.len() > 40 { &slug[..40] } else { &slug };
            let filename = format!("{timestamp}_{}_{}.json", msg.msg_type, slug);
            let path = peer_inbox.join(&filename);

            let json = serde_json::to_string_pretty(&msg)
                .map_err(|e| format!("failed to serialize message: {e}"))?;

            tokio::fs::write(&path, json)
                .await
                .map_err(|e| format!("failed to write message: {e}"))?;

            info!(id = %msg.id, to = %peer_id, msg_type = msg.msg_type, subject = msg.subject, "comms message sent");
            Ok(msg.id)
        })
    }

    fn ack(&self, original: &CommsMessage, summary: &str) -> CommsOpFuture<String> {
        let msg = CommsMessage {
            id: format!("msg-{}", Uuid::now_v7()),
            from: self.agent_id.clone(),
            to: self.peer_id.clone(),
            msg_type: "ack".to_string(),
            subject: format!("ack: {}", original.subject),
            body: format!("Acknowledged: {}\n\n{}", original.subject, summary),
            priority: "p2".to_string(),
            refs: None,
            created_at: Some(Utc::now().to_rfc3339()),
            expires_at: None,
        };
        self.send(msg)
    }

    fn archive(&self, path: PathBuf) -> CommsOpFuture<()> {
        let archive_dir = self.comms_dir.join(".archive");
        Box::pin(async move {
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

            tokio::fs::rename(&path, &archive_path)
                .await
                .map_err(|e| format!("failed to archive message: {e}"))?;

            debug!(from = %path.display(), to = %archive_path.display(), "comms message archived");
            Ok(())
        })
    }

    fn agent_id(&self) -> &str {
        &self.agent_id
    }

    fn peer_id(&self) -> &str {
        &self.peer_id
    }

    fn backend_name(&self) -> &str {
        "filesystem"
    }
}

/// The comms client — reads/writes messages using the configured transport.
#[derive(Clone)]
pub struct CommsClient {
    transport: Arc<dyn CommsTransport>,
}

impl CommsClient {
    pub fn new(
        comms_dir: impl Into<PathBuf>,
        agent_id: impl Into<String>,
        peer_id: impl Into<String>,
    ) -> Self {
        Self::new_with_transport(Arc::new(FsCommsTransport::new(
            comms_dir, agent_id, peer_id,
        )))
    }

    pub fn new_with_transport(transport: Arc<dyn CommsTransport>) -> Self {
        Self { transport }
    }

    /// Read all messages from our inbox.
    pub async fn read_inbox(&self) -> Vec<InboxItem> {
        self.transport.read_inbox().await.unwrap_or_else(|e| {
            warn!(error = %e, "failed to read comms inbox via transport");
            Vec::new()
        })
    }

    /// Write a message to the peer's inbox.
    pub async fn send(
        &self,
        msg_type: &str,
        subject: &str,
        body: &str,
        priority: &str,
    ) -> Result<String, String> {
        self.transport
            .send(CommsMessage {
                id: format!("msg-{}", Uuid::now_v7()),
                from: self.transport.agent_id().to_string(),
                to: self.transport.peer_id().to_string(),
                msg_type: msg_type.to_string(),
                subject: subject.to_string(),
                body: body.to_string(),
                priority: priority.to_string(),
                refs: None,
                created_at: Some(Utc::now().to_rfc3339()),
                expires_at: None,
            })
            .await
    }

    /// Send an ack for a received message.
    pub async fn send_ack(&self, original: &CommsMessage, summary: &str) -> Result<String, String> {
        self.transport.ack(original, summary).await
    }

    /// Archive a processed message.
    pub async fn archive(&self, path: &Path) -> Result<(), String> {
        self.transport.archive(path.to_path_buf()).await
    }

    pub fn agent_id(&self) -> &str {
        self.transport.agent_id()
    }

    pub fn peer_id(&self) -> &str {
        self.transport.peer_id()
    }

    pub fn backend_name(&self) -> &str {
        self.transport.backend_name()
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
}
