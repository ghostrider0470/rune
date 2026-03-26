//! Filesystem-based inter-agent communication client.
//!
//! Reads messages from an inbox directory, writes messages to a peer's inbox,
//! and archives processed messages. Implements the .comms/ protocol.

use std::path::{Path, PathBuf};

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

fn default_priority() -> String { "p1".to_string() }

/// The comms client — reads/writes messages to the filesystem mailbox.
#[derive(Clone)]
pub struct CommsClient {
    comms_dir: PathBuf,
    agent_id: String,
    peer_id: String,
}

impl CommsClient {
    pub fn new(comms_dir: impl Into<PathBuf>, agent_id: impl Into<String>, peer_id: impl Into<String>) -> Self {
        Self {
            comms_dir: comms_dir.into(),
            agent_id: agent_id.into(),
            peer_id: peer_id.into(),
        }
    }

    /// Read all messages from our inbox.
    pub async fn read_inbox(&self) -> Vec<(PathBuf, CommsMessage)> {
        let inbox = self.comms_dir.join(&self.agent_id).join("inbox");
        if !inbox.is_dir() {
            return Vec::new();
        }

        let mut messages = Vec::new();
        let mut entries = match tokio::fs::read_dir(&inbox).await {
            Ok(e) => e,
            Err(e) => {
                warn!(error = %e, "failed to read comms inbox");
                return Vec::new();
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
                    Err(e) => warn!(path = %path.display(), error = %e, "failed to parse comms message"),
                },
                Err(e) => warn!(path = %path.display(), error = %e, "failed to read comms message"),
            }
        }

        // Sort by filename (timestamp-based) for consistent ordering
        messages.sort_by(|a, b| a.0.cmp(&b.0));
        messages
    }

    /// Write a message to the peer's inbox.
    pub async fn send(&self, msg_type: &str, subject: &str, body: &str, priority: &str) -> Result<String, String> {
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

        let peer_inbox = self.comms_dir.join(&self.peer_id).join("inbox");
        if let Err(e) = tokio::fs::create_dir_all(&peer_inbox).await {
            return Err(format!("failed to create peer inbox: {e}"));
        }

        let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let slug = subject.chars()
            .filter(|c| c.is_alphanumeric() || *c == '-' || *c == ' ')
            .collect::<String>()
            .replace(' ', "-")
            .to_lowercase();
        let slug = if slug.len() > 40 { &slug[..40] } else { &slug };
        let filename = format!("{timestamp}_{msg_type}_{slug}.json");
        let path = peer_inbox.join(&filename);

        let json = serde_json::to_string_pretty(&msg)
            .map_err(|e| format!("failed to serialize message: {e}"))?;

        tokio::fs::write(&path, json).await
            .map_err(|e| format!("failed to write message: {e}"))?;

        info!(id = %id, to = %self.peer_id, msg_type = msg_type, subject = subject, "comms message sent");
        Ok(id)
    }

    /// Send an ack for a received message.
    pub async fn send_ack(&self, original: &CommsMessage, summary: &str) -> Result<String, String> {
        let body = format!(
            "Acknowledged: {}\n\n{}",
            original.subject, summary
        );
        self.send("ack", &format!("ack: {}", original.subject), &body, "p2").await
    }

    /// Archive a processed message.
    pub async fn archive(&self, path: &Path) -> Result<(), String> {
        let archive_dir = self.comms_dir.join(".archive");
        if let Err(e) = tokio::fs::create_dir_all(&archive_dir).await {
            return Err(format!("failed to create archive dir: {e}"));
        }

        let filename = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown.json");
        let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let archive_name = format!("{timestamp}_{filename}");
        let archive_path = archive_dir.join(archive_name);

        tokio::fs::rename(path, &archive_path).await
            .map_err(|e| format!("failed to archive message: {e}"))?;

        debug!(from = %path.display(), to = %archive_path.display(), "comms message archived");
        Ok(())
    }

    pub fn agent_id(&self) -> &str { &self.agent_id }
    pub fn peer_id(&self) -> &str { &self.peer_id }
    pub fn comms_dir(&self) -> &Path { &self.comms_dir }
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

        // Sender writes to receiver's inbox (horizon-ai/inbox/)
        // But read_inbox reads from agent's own inbox
        // So: rune sends → horizon-ai/inbox/, horizon-ai reads from horizon-ai/inbox/
        sender.send("task", "test task", "do something", "p1").await.unwrap();

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

        // Create a fake inbox message
        let inbox = tmp.path().join("rune").join("inbox");
        tokio::fs::create_dir_all(&inbox).await.unwrap();
        let msg_path = inbox.join("test.json");
        tokio::fs::write(&msg_path, r#"{"id":"t","from":"x","to":"y","type":"ack","subject":"s","body":"b"}"#).await.unwrap();

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
