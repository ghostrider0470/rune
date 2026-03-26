# Native Comms System — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add native Rust-level inter-agent communication to Rune — supervisor inbox polling, type-based message dispatch, outbound from cron results, and a `comms_send` tool for the agent.

**Architecture:** A new `comms.rs` module in rune-runtime handles reading/writing/archiving JSON messages. The supervisor polls every tick (10s). Simple types (ack, status, directive) are handled at code level. Complex types (task, question) create isolated agent turns. Cron results auto-write to the peer inbox. A `comms_send` tool lets the LLM proactively send messages.

**Tech Stack:** Rust, tokio, serde_json, chrono, uuid

---

### Task 1: Add Comms Config

**Files:**
- Modify: `crates/rune-config/src/lib.rs`

- [ ] **Step 1: Add CommsConfig struct**

In `crates/rune-config/src/lib.rs`, add after `PluginsConfig`:

```rust
/// Inter-agent communication via filesystem mailbox.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommsConfig {
    /// Whether comms is enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Root directory of the .comms/ mailbox.
    #[serde(default)]
    pub comms_dir: Option<String>,
    /// This agent's ID in the protocol.
    #[serde(default = "default_comms_agent_id")]
    pub agent_id: String,
    /// The peer agent's ID.
    #[serde(default = "default_comms_peer_id")]
    pub peer_id: String,
}

fn default_comms_agent_id() -> String { "rune".to_string() }
fn default_comms_peer_id() -> String { "horizon-ai".to_string() }

impl Default for CommsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            comms_dir: None,
            agent_id: default_comms_agent_id(),
            peer_id: default_comms_peer_id(),
        }
    }
}
```

Add to `AppConfig`:
```rust
    #[serde(default)]
    pub comms: CommsConfig,
```

- [ ] **Step 2: Build**

Run: `cargo build -p rune-config`

- [ ] **Step 3: Commit**

```bash
git add crates/rune-config/src/lib.rs
git commit -m "feat(config): add [comms] config section for inter-agent mailbox"
```

---

### Task 2: Comms Client Module

**Files:**
- Create: `crates/rune-runtime/src/comms.rs`

- [ ] **Step 1: Create the comms client**

Create `crates/rune-runtime/src/comms.rs`:

```rust
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
```

- [ ] **Step 2: Add module to lib.rs**

In `crates/rune-runtime/src/lib.rs`:
```rust
pub mod comms;
pub use comms::CommsClient;
```

- [ ] **Step 3: Build and test**

Run: `cargo test -p rune-runtime comms -- --nocapture`

- [ ] **Step 4: Commit**

```bash
git add crates/rune-runtime/src/comms.rs crates/rune-runtime/src/lib.rs
git commit -m "feat(runtime): add CommsClient for inter-agent filesystem mailbox"
```

---

### Task 3: Comms Send Tool

**Files:**
- Create: `crates/rune-tools/src/comms_tool.rs`
- Modify: `crates/rune-tools/src/lib.rs`

- [ ] **Step 1: Create the comms tool executor**

Create `crates/rune-tools/src/comms_tool.rs`:

```rust
//! Inter-agent comms tool — lets the agent send messages to peer agents.

use std::sync::Arc;

use async_trait::async_trait;

use crate::definition::{ToolCall, ToolResult};
use crate::error::ToolError;
use crate::executor::ToolExecutor;

/// Trait for comms operations, implemented by the runtime layer.
#[async_trait]
pub trait CommsOps: Send + Sync {
    /// Send a message to the peer agent.
    async fn send_message(
        &self,
        to: &str,
        msg_type: &str,
        subject: &str,
        body: &str,
        priority: &str,
    ) -> Result<String, String>;
}

/// Tool executor for inter-agent comms.
pub struct CommsToolExecutor<C: CommsOps> {
    comms: Arc<C>,
}

impl<C: CommsOps> CommsToolExecutor<C> {
    pub fn new(comms: Arc<C>) -> Self {
        Self { comms }
    }
}

#[async_trait]
impl<C: CommsOps> ToolExecutor for CommsToolExecutor<C> {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let args = &call.arguments;
        let to = args.get("to").and_then(|v| v.as_str()).unwrap_or("horizon-ai");
        let msg_type = args.get("type").and_then(|v| v.as_str()).unwrap_or("status");
        let subject = args.get("subject").and_then(|v| v.as_str()).unwrap_or("message from rune");
        let body = args.get("body").and_then(|v| v.as_str()).unwrap_or("");
        let priority = args.get("priority").and_then(|v| v.as_str()).unwrap_or("p1");

        if body.is_empty() {
            return Err(ToolError::InvalidArguments("body is required".into()));
        }

        match self.comms.send_message(to, msg_type, subject, body, priority).await {
            Ok(id) => Ok(ToolResult {
                tool_call_id: call.tool_call_id,
                output: format!("Message sent: {id}"),
                is_error: false,
                tool_execution_id: None,
            }),
            Err(e) => Err(ToolError::ExecutionFailed(format!("comms send failed: {e}"))),
        }
    }
}
```

- [ ] **Step 2: Add module to rune-tools lib.rs**

In `crates/rune-tools/src/lib.rs`, add:
```rust
pub mod comms_tool;
```

- [ ] **Step 3: Build**

Run: `cargo build -p rune-tools`

- [ ] **Step 4: Commit**

```bash
git add crates/rune-tools/src/comms_tool.rs crates/rune-tools/src/lib.rs
git commit -m "feat(tools): add comms_send tool for inter-agent messaging"
```

---

### Task 4: Supervisor Inbox Check + Cron Outbound

**Files:**
- Modify: `crates/rune-gateway/src/supervisor.rs`

- [ ] **Step 1: Add CommsClient to SupervisorDeps**

In `crates/rune-gateway/src/supervisor.rs`, add to `SupervisorDeps`:

```rust
    pub comms: Option<Arc<rune_runtime::CommsClient>>,
```

- [ ] **Step 2: Add check_comms_inbox function**

Add a new function in `supervisor.rs`:

```rust
/// Process messages from the comms inbox.
async fn check_comms_inbox(deps: &SupervisorDeps) {
    let Some(ref comms) = deps.comms else { return };

    let messages = comms.read_inbox().await;
    if messages.is_empty() {
        return;
    }

    info!(count = messages.len(), "processing comms inbox messages");

    for (path, msg) in messages {
        debug!(id = %msg.id, msg_type = %msg.msg_type, from = %msg.from, subject = %msg.subject, "processing comms message");

        match msg.msg_type.as_str() {
            "ack" | "result" => {
                // Archive silently
                if let Err(e) = comms.archive(&path).await {
                    warn!(error = %e, "failed to archive comms message");
                }
            }
            "status" => {
                // Respond with gateway status
                let sessions_count = match deps.session_engine.session_repo().list_active_channel_sessions().await {
                    Ok(s) => s.len(),
                    Err(_) => 0,
                };
                let jobs = deps.scheduler.list_jobs(false).await;
                let body = format!(
                    "Gateway status:\n- Active sessions: {}\n- Cron jobs: {}\n- Uptime: running",
                    sessions_count, jobs.len()
                );
                if let Err(e) = comms.send("result", &format!("re: {}", msg.subject), &body, "p2").await {
                    warn!(error = %e, "failed to send comms status response");
                }
                if let Err(e) = comms.archive(&path).await {
                    warn!(error = %e, "failed to archive comms message");
                }
            }
            "directive" => {
                // Ack and archive — directive content available via workspace context
                info!(subject = %msg.subject, "received comms directive");
                if let Err(e) = comms.send_ack(&msg, "Directive received and logged.").await {
                    warn!(error = %e, "failed to send comms directive ack");
                }
                if let Err(e) = comms.archive(&path).await {
                    warn!(error = %e, "failed to archive comms message");
                }
            }
            "task" | "question" => {
                // Create isolated agent turn
                let prompt = format!(
                    "[Inter-Agent Comms] This message is from {} via the .comms/ mailbox.\n\
                    Priority: {}\nSubject: {}\n\n{}\n\n\
                    Respond with a clear, actionable answer. Your response will be sent back as a result message.",
                    msg.from, msg.priority, msg.subject, msg.body
                );
                match run_agent_turn(deps, &prompt, None, rune_core::SessionTarget::Isolated).await {
                    Ok(response) => {
                        let truncated = if response.len() > 3000 { &response[..3000] } else { &response };
                        if let Err(e) = comms.send("result", &format!("re: {}", msg.subject), truncated, &msg.priority).await {
                            warn!(error = %e, "failed to send comms task result");
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "comms task agent turn failed");
                        let _ = comms.send("result", &format!("re: {} [error]", msg.subject), &format!("Agent turn failed: {e}"), "p1").await;
                    }
                }
                if let Err(e) = comms.archive(&path).await {
                    warn!(error = %e, "failed to archive comms message");
                }
            }
            other => {
                warn!(msg_type = other, "unknown comms message type, archiving");
                if let Err(e) = comms.archive(&path).await {
                    warn!(error = %e, "failed to archive comms message");
                }
            }
        }
    }
}
```

- [ ] **Step 3: Call check_comms_inbox in supervisor_loop**

In `supervisor_loop`, after the plugin re-scan block (or after stale session cleanup), add:

```rust
        // --- Inter-agent comms inbox check ---
        check_comms_inbox(&deps).await;
```

- [ ] **Step 4: Add cron result outbound to deliver_result**

In `deliver_result` (the function that passes to `deliver_result_standalone`), after the existing call, add comms outbound:

```rust
    // Write cron result to comms peer inbox
    if matches!(job.delivery_mode, SchedulerDeliveryMode::Announce) {
        if let Some(ref comms) = deps.comms {
            let status_str = match status {
                rune_runtime::scheduler::JobRunStatus::Completed => "completed",
                rune_runtime::scheduler::JobRunStatus::Failed => "FAILED",
                _ => "finished",
            };
            let truncated = if output.len() > 3000 { &output[..3000] } else { output };
            let subject = format!("[cron:{}] {}", job.name.as_deref().unwrap_or("unknown"), status_str);
            if let Err(e) = comms.send("result", &subject, truncated, "p2").await {
                warn!(error = %e, "failed to write cron result to comms");
            }
        }
    }
```

- [ ] **Step 5: Build**

Run: `cargo build -p rune-gateway`

- [ ] **Step 6: Commit**

```bash
git add crates/rune-gateway/src/supervisor.rs
git commit -m "feat(supervisor): native comms inbox check + cron result outbound"
```

---

### Task 5: Wire Comms in Gateway Startup

**Files:**
- Modify: `apps/gateway/src/main.rs`
- Modify: `crates/rune-gateway/src/server.rs`

- [ ] **Step 1: Create CommsClient from config**

In the gateway startup code (either `main.rs` or `server.rs` — wherever `SupervisorDeps` is constructed), create the comms client:

```rust
    let comms = if config.comms.enabled {
        if let Some(ref dir) = config.comms.comms_dir {
            Some(Arc::new(rune_runtime::CommsClient::new(
                dir,
                &config.comms.agent_id,
                &config.comms.peer_id,
            )))
        } else {
            None
        }
    } else {
        None
    };
```

Pass `comms` to `SupervisorDeps`:
```rust
        comms,
```

- [ ] **Step 2: Register comms_send tool**

In `register_real_tool_definitions` or the tool executor construction, add the `comms_send` tool definition:

```rust
        ToolDefinition {
            name: "comms_send".into(),
            description: "Send a message to Horizon AI (or another peer agent) via the .comms/ inter-agent mailbox. Use this to report status, ask questions, or dispatch tasks to the peer.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "to": { "type": "string", "default": "horizon-ai" },
                    "type": { "type": "string", "enum": ["task", "question", "status", "result"] },
                    "subject": { "type": "string" },
                    "body": { "type": "string" },
                    "priority": { "type": "string", "enum": ["p0", "p1", "p2"], "default": "p1" }
                },
                "required": ["subject", "body"]
            }),
            category: ToolCategory::External,
            requires_approval: false,
        },
```

And wire the `CommsToolExecutor` into the composite tool executor (find where `CronToolExecutor`, `MessageToolExecutor`, etc. are wired — add `CommsToolExecutor` there with a `CommsOps` impl that wraps `CommsClient`).

- [ ] **Step 3: Add config.toml section**

Add to `/home/hamza/Development/rune/config.toml`:

```toml
[comms]
enabled = true
comms_dir = "/home/hamza/Development/rune.worktrees/.comms"
agent_id = "rune"
peer_id = "horizon-ai"
```

- [ ] **Step 4: Build**

Run: `cargo build --release --bin rune-gateway`

- [ ] **Step 5: Commit**

```bash
git add apps/gateway/src/main.rs crates/rune-gateway/src/server.rs
git commit -m "feat(gateway): wire CommsClient and comms_send tool"
```

---

### Task 6: Build, Deploy, Verify

- [ ] **Step 1: Full build**

```bash
cargo build --release --bin rune-gateway
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p rune-runtime comms -- --nocapture
```

- [ ] **Step 3: Restart gateway**

```bash
systemctl --user restart rune-gateway && sleep 3
```

- [ ] **Step 4: Send a test message and verify pickup**

```bash
python3 -c "
import json, os
from datetime import datetime, timezone

inbox = '/home/hamza/Development/rune.worktrees/.comms/rune/inbox'
os.makedirs(inbox, exist_ok=True)

msg = {
    'id': 'msg-test-001',
    'from': 'horizon-ai',
    'to': 'rune',
    'type': 'status',
    'subject': 'Status check',
    'body': 'Report your current status.',
    'priority': 'p1',
    'created_at': datetime.now(timezone.utc).isoformat()
}

path = os.path.join(inbox, '20260326T200000Z_status_status-check.json')
with open(path, 'w') as f:
    json.dump(msg, f, indent=2)
print(f'Test message written to {path}')
"
```

Wait 15 seconds, then check:

```bash
sleep 15
ls /home/hamza/Development/rune.worktrees/.comms/rune/inbox/
ls /home/hamza/Development/rune.worktrees/.comms/horizon-ai/inbox/
ls /home/hamza/Development/rune.worktrees/.comms/.archive/ | tail -3
```

Expected: rune/inbox/ empty (processed), horizon-ai/inbox/ has a response, .archive/ has the original.

- [ ] **Step 5: Check logs**

```bash
journalctl --user -u rune-gateway --since "30s ago" --no-pager -o cat | grep -i comms
```

Expected: "processing comms inbox messages", "comms message sent", "comms message archived"

- [ ] **Step 6: Push**

```bash
git push origin main
```
