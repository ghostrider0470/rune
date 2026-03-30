# Phases 25-27: Implementation Specification

> Generated 2026-03-15. Authoritative reference for implementing phases 25 through 27.
> Every type, endpoint, wire example, error case, and acceptance criterion is defined
> here so that implementation can proceed without guessing.
> Note: this file is the target implementation spec. For the currently shipped Memory Bank surface, see `docs/reference/memory-bank-current-state.md`.

---

## Table of Contents

1. [Phase 25 — Memory Bank & Architectural Knowledge](#phase-25--memory-bank--architectural-knowledge)
2. [Phase 26 — Extended Channel Support](#phase-26--extended-channel-support)
3. [Phase 27 — Calendar & Email Integration](#phase-27--calendar--email-integration)

---

## Phase 25 — Memory Bank & Architectural Knowledge

### 25.1 Overview

Structured project knowledge base persisted in `.rune/knowledge/` that auto-updates
on significant code changes and injects rich context into every agent session. Builds
on the hybrid memory search infrastructure from Phase 10 (`memory_index.rs`). Exposes
a `memory_bank` tool for read/update/search, and an `/onboard` slash command that
generates a project briefing from the knowledge base.

### 25.2 Rust Types

File: `crates/rune-runtime/src/memory_bank.rs`

```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Knowledge document kinds ────────────────────────────────────────

/// The four canonical knowledge document types stored in `.rune/knowledge/`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeKind {
    Architecture,
    Decisions,
    Conventions,
    Dependencies,
}

impl KnowledgeKind {
    /// Canonical filename inside `.rune/knowledge/`.
    pub fn filename(&self) -> &'static str {
        match self {
            Self::Architecture => "ARCHITECTURE.md",
            Self::Decisions => "DECISIONS.md",
            Self::Conventions => "CONVENTIONS.md",
            Self::Dependencies => "DEPENDENCIES.md",
        }
    }

    pub const ALL: [KnowledgeKind; 4] = [
        Self::Architecture,
        Self::Decisions,
        Self::Conventions,
        Self::Dependencies,
    ];
}

// ── Knowledge document ──────────────────────────────────────────────

/// A single knowledge document with metadata.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KnowledgeDoc {
    pub id: Uuid,
    pub kind: KnowledgeKind,
    pub content: String,
    pub version: i32,
    pub updated_at: DateTime<Utc>,
    pub updated_by: String,
    /// SHA-256 of content for change detection.
    pub content_hash: String,
}

/// Payload for creating or replacing a knowledge document.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KnowledgeDocUpsert {
    pub kind: KnowledgeKind,
    pub content: String,
    pub updated_by: String,
}

// ── Architectural Decision Record ───────────────────────────────────

/// A single ADR entry within DECISIONS.md.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub id: String,
    pub title: String,
    pub date: String,
    pub status: DecisionStatus,
    pub context: String,
    pub decision: String,
    pub consequences: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionStatus {
    Proposed,
    Accepted,
    Deprecated,
    Superseded,
}

// ── Staleness detection ─────────────────────────────────────────────

/// A knowledge chunk that may be stale relative to recent file changes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StalenessReport {
    pub kind: KnowledgeKind,
    pub stale_sections: Vec<StaleSection>,
    pub checked_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StaleSection {
    /// Heading or line range in the knowledge doc.
    pub section: String,
    /// Files that changed since this section was last updated.
    pub changed_files: Vec<String>,
    pub confidence: f64,
}

// ── Onboarding briefing ─────────────────────────────────────────────

/// Result of the `/onboard` command.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OnboardingBriefing {
    pub project_name: String,
    pub generated_at: DateTime<Utc>,
    pub architecture_summary: String,
    pub key_decisions: Vec<DecisionRecord>,
    pub conventions: String,
    pub dependency_highlights: Vec<DependencyHighlight>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DependencyHighlight {
    pub name: String,
    pub version: String,
    pub rationale: String,
}

// ── Memory Bank facade ──────────────────────────────────────────────

/// Configuration for the memory bank subsystem.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryBankConfig {
    /// Root directory for knowledge files. Default: `.rune/knowledge/`.
    pub knowledge_dir: PathBuf,
    /// Auto-update on file changes exceeding this threshold (new files).
    pub auto_update_threshold: usize,
    /// Staleness window: sections not updated within this many days are flagged.
    pub staleness_days: u32,
}

impl Default for MemoryBankConfig {
    fn default() -> Self {
        Self {
            knowledge_dir: PathBuf::from(".rune/knowledge"),
            auto_update_threshold: 3,
            staleness_days: 14,
        }
    }
}

/// Central facade for memory bank operations.
pub struct MemoryBank {
    config: MemoryBankConfig,
    repo: Arc<dyn MemoryBankRepo>,
    index: Arc<rune_tools::memory_index::MemoryIndex>,
}
```

### 25.3 Repository Trait

File: `crates/rune-store/src/repos.rs` (append)

```rust
// ── Memory bank repository ──────────────────────────────────────────

/// Persistence contract for memory bank knowledge documents.
#[async_trait]
pub trait MemoryBankRepo: Send + Sync {
    /// Upsert a knowledge document. Increments version on conflict.
    async fn upsert(&self, doc: NewKnowledgeDoc) -> Result<KnowledgeDocRow, StoreError>;

    /// Fetch a knowledge document by kind.
    async fn find_by_kind(&self, kind: &str) -> Result<Option<KnowledgeDocRow>, StoreError>;

    /// List all knowledge documents.
    async fn list_all(&self) -> Result<Vec<KnowledgeDocRow>, StoreError>;

    /// Delete a knowledge document by kind. Returns true if removed.
    async fn delete_by_kind(&self, kind: &str) -> Result<bool, StoreError>;

    /// Record a staleness check result.
    async fn record_staleness(
        &self,
        kind: &str,
        stale_sections: serde_json::Value,
        checked_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), StoreError>;
}
```

Store model rows:

```rust
#[derive(Clone, Debug, Serialize, Deserialize, Queryable, Insertable)]
#[diesel(table_name = knowledge_docs)]
pub struct KnowledgeDocRow {
    pub id: Uuid,
    pub kind: String,
    pub content: String,
    pub version: i32,
    pub content_hash: String,
    pub updated_by: String,
    pub updated_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Insertable)]
#[diesel(table_name = knowledge_docs)]
pub struct NewKnowledgeDoc {
    pub kind: String,
    pub content: String,
    pub content_hash: String,
    pub updated_by: String,
}
```

### 25.4 SQL Migration

```sql
-- 20260315120000_create_knowledge_docs

CREATE TABLE knowledge_docs (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    kind         TEXT NOT NULL UNIQUE,
    content      TEXT NOT NULL,
    version      INT  NOT NULL DEFAULT 1,
    content_hash TEXT NOT NULL,
    updated_by   TEXT NOT NULL DEFAULT 'system',
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_knowledge_docs_kind ON knowledge_docs (kind);

-- Staleness audit log
CREATE TABLE knowledge_staleness_checks (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    kind            TEXT NOT NULL REFERENCES knowledge_docs(kind) ON DELETE CASCADE,
    stale_sections  JSONB NOT NULL DEFAULT '[]',
    checked_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_staleness_checks_kind ON knowledge_staleness_checks (kind);
```

### 25.5 Wire Protocol

#### `GET /api/memory-bank`

List all knowledge documents.

**Response 200:**
```json
{
  "documents": [
    {
      "id": "aaaaaaaa-...",
      "kind": "architecture",
      "content": "# Architecture\n...",
      "version": 3,
      "content_hash": "e3b0c44298fc...",
      "updated_by": "agent:coder",
      "updated_at": "2026-03-15T10:00:00Z"
    }
  ]
}
```

#### `GET /api/memory-bank/:kind`

Fetch a single knowledge document.

**Response 200:**
```json
{
  "id": "aaaaaaaa-...",
  "kind": "decisions",
  "content": "# Decisions\n...",
  "version": 5,
  "content_hash": "abc123...",
  "updated_by": "agent:coder",
  "updated_at": "2026-03-15T10:00:00Z"
}
```

**Response 404:**
```json
{ "error": "not_found", "message": "knowledge document 'decisions' not found" }
```

#### `PUT /api/memory-bank/:kind`

Create or replace a knowledge document.

**Request:**
```json
{
  "content": "# Architecture\n\nRune is a...",
  "updated_by": "agent:coder"
}
```

**Response 200:**
```json
{
  "id": "aaaaaaaa-...",
  "kind": "architecture",
  "content": "# Architecture\n\nRune is a...",
  "version": 4,
  "content_hash": "def456...",
  "updated_by": "agent:coder",
  "updated_at": "2026-03-15T12:00:00Z"
}
```

**Response 400:**
```json
{ "error": "invalid_kind", "message": "kind must be one of: architecture, decisions, conventions, dependencies" }
```

#### `DELETE /api/memory-bank/:kind`

**Response 200:**
```json
{ "deleted": true }
```

**Response 404:**
```json
{ "error": "not_found", "message": "knowledge document 'conventions' not found" }
```

#### `POST /api/memory-bank/search`

Hybrid search across the knowledge base using the existing memory index infrastructure.

**Request:**
```json
{
  "query": "error handling patterns",
  "limit": 10
}
```

**Response 200:**
```json
{
  "results": [
    {
      "file_path": ".rune/knowledge/CONVENTIONS.md",
      "chunk_text": "## Error Handling\n\nAll crates use thiserror...",
      "rrf_score": 0.0328,
      "keyword_rank": 1,
      "vector_rank": 3
    }
  ]
}
```

#### `POST /api/memory-bank/staleness`

Run a staleness check across all knowledge documents.

**Response 200:**
```json
{
  "reports": [
    {
      "kind": "architecture",
      "stale_sections": [
        {
          "section": "## Gateway Layer",
          "changed_files": ["crates/rune-gateway/src/routes.rs"],
          "confidence": 0.82
        }
      ],
      "checked_at": "2026-03-15T12:30:00Z"
    }
  ]
}
```

#### `POST /api/memory-bank/onboard`

Generate a project onboarding briefing.

**Response 200:**
```json
{
  "project_name": "rune",
  "generated_at": "2026-03-15T12:30:00Z",
  "architecture_summary": "Rune is a multi-agent runtime...",
  "key_decisions": [
    {
      "id": "ADR-001",
      "title": "Use Diesel over SQLx",
      "date": "2026-02-01",
      "status": "accepted",
      "context": "...",
      "decision": "...",
      "consequences": "..."
    }
  ],
  "conventions": "## Naming\n- snake_case for modules...",
  "dependency_highlights": [
    { "name": "axum", "version": "0.8", "rationale": "First-class tower integration" }
  ]
}
```

### 25.6 Tool Registration

File: `crates/rune-tools/src/memory_bank_tool.rs`

```rust
use serde::{Deserialize, Serialize};

/// Tool input variants for the `memory_bank` tool.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum MemoryBankToolInput {
    Read { kind: String },
    Update { kind: String, content: String },
    Search { query: String, limit: Option<usize> },
    ListAll,
    CheckStaleness,
    Onboard,
}

/// Tool output for memory_bank operations.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum MemoryBankToolOutput {
    Document {
        kind: String,
        content: String,
        version: i32,
    },
    Documents {
        documents: Vec<MemoryBankDocSummary>,
    },
    SearchResults {
        results: Vec<MemoryBankSearchHit>,
    },
    StalenessReport {
        reports: Vec<StalenessReportSummary>,
    },
    Briefing {
        briefing: String,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct MemoryBankDocSummary {
    pub kind: String,
    pub version: i32,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct MemoryBankSearchHit {
    pub file_path: String,
    pub chunk_text: String,
    pub rrf_score: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct StalenessReportSummary {
    pub kind: String,
    pub stale_count: usize,
}
```

The tool definition JSON for `ToolRegistry::register`:

```json
{
  "name": "memory_bank",
  "description": "Read, update, and search the project knowledge base (ARCHITECTURE.md, DECISIONS.md, CONVENTIONS.md, DEPENDENCIES.md).",
  "parameters": {
    "type": "object",
    "required": ["action"],
    "properties": {
      "action": {
        "type": "string",
        "enum": ["read", "update", "search", "list_all", "check_staleness", "onboard"]
      },
      "kind": {
        "type": "string",
        "enum": ["architecture", "decisions", "conventions", "dependencies"],
        "description": "Required for read/update."
      },
      "content": {
        "type": "string",
        "description": "Required for update."
      },
      "query": {
        "type": "string",
        "description": "Required for search."
      },
      "limit": {
        "type": "integer",
        "description": "Max results for search. Default 10."
      }
    }
  }
}
```

### 25.7 Context Injection

File: `crates/rune-runtime/src/executor.rs` — modify `TurnExecutor`:

```rust
/// Added field:
memory_bank: Option<Arc<MemoryBank>>,

/// In build_system_prompt() or equivalent, prepend knowledge context:
async fn inject_memory_bank_context(&self, prompt: &mut String) {
    if let Some(mb) = &self.memory_bank {
        if let Ok(docs) = mb.list_all().await {
            for doc in &docs {
                prompt.push_str(&format!(
                    "\n\n<!-- knowledge:{} v{} -->\n{}\n<!-- /knowledge:{} -->\n",
                    doc.kind, doc.version, doc.content, doc.kind
                ));
            }
        }
    }
}
```

### 25.8 Error Cases

```rust
/// Memory bank errors.
#[derive(Debug, thiserror::Error)]
pub enum MemoryBankError {
    #[error("unknown knowledge kind: {kind}")]
    UnknownKind { kind: String },           // → HTTP 400

    #[error("knowledge document not found: {kind}")]
    NotFound { kind: String },              // → HTTP 404

    #[error("content too large: {size} bytes (max {max})")]
    ContentTooLarge { size: usize, max: usize }, // → HTTP 413

    #[error("concurrent update conflict for {kind} (expected version {expected}, found {found})")]
    VersionConflict {
        kind: String,
        expected: i32,
        found: i32,
    },                                      // → HTTP 409

    #[error("memory index error: {0}")]
    Index(#[from] rune_tools::memory_index::MemoryIndexError), // → HTTP 502

    #[error("store error: {0}")]
    Store(#[from] rune_store::error::StoreError), // → HTTP 500

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),             // → HTTP 500
}
```

### 25.9 Edge Cases

1. **Empty knowledge base** — `/onboard` returns a briefing with placeholder sections indicating no knowledge has been captured yet.
2. **Concurrent updates** — Two agents update the same document simultaneously. The upsert uses `version` as an optimistic lock; the second writer gets `VersionConflict` and must re-read before retrying.
3. **Very large projects** — `content` field is capped at 256 KiB per document. Exceeding this returns `ContentTooLarge`.
4. **Stale embedding index** — After a knowledge doc update, the old chunks must be deleted from `memory_embeddings` and the new content re-chunked and re-embedded before search reflects changes.
5. **Missing `.rune/knowledge/` directory** — Auto-created on first write. Read operations return empty results, not errors.
6. **File-system sync** — Knowledge docs are persisted both in PostgreSQL (authoritative) and mirrored to the filesystem directory for git-tracking. DB wins on conflict.
7. **Non-UTF-8 content** — Rejected at the API layer with HTTP 400 before reaching the store.

### 25.10 Integration Test Scenarios

```rust
#[cfg(test)]
mod tests {
    /// Verify upsert creates a new doc with version=1, then increments on update.
    #[tokio::test]
    async fn test_upsert_creates_and_increments_version();

    /// Verify find_by_kind returns None for missing documents.
    #[tokio::test]
    async fn test_find_by_kind_returns_none_when_empty();

    /// Verify list_all returns all four document kinds after seeding.
    #[tokio::test]
    async fn test_list_all_after_seeding_four_docs();

    /// Verify delete_by_kind removes the doc and subsequent find returns None.
    #[tokio::test]
    async fn test_delete_by_kind_removes_doc();

    /// Verify search returns relevant chunks after indexing knowledge docs.
    #[tokio::test]
    async fn test_search_returns_indexed_knowledge_chunks();

    /// Verify staleness check flags sections where referenced files changed.
    #[tokio::test]
    async fn test_staleness_check_flags_changed_files();

    /// Verify onboard generates a briefing containing all four sections.
    #[tokio::test]
    async fn test_onboard_generates_complete_briefing();

    /// Verify context injection prepends knowledge to system prompt.
    #[tokio::test]
    async fn test_context_injection_adds_knowledge_to_prompt();

    /// Verify ContentTooLarge is returned for oversized documents.
    #[tokio::test]
    async fn test_content_too_large_rejected();

    /// Verify concurrent version conflict is detected.
    #[tokio::test]
    async fn test_version_conflict_on_concurrent_update();

    /// PUT /api/memory-bank/:kind round-trips through the gateway.
    #[tokio::test]
    async fn test_gateway_put_and_get_round_trip();

    /// POST /api/memory-bank/search returns hybrid results.
    #[tokio::test]
    async fn test_gateway_search_endpoint();
}
```

### 25.11 Acceptance Criteria

- [ ] `.rune/knowledge/` directory auto-created on first knowledge write
- [ ] ARCHITECTURE.md, DECISIONS.md, CONVENTIONS.md, DEPENDENCIES.md can each be created, read, updated, and deleted via API
- [ ] Knowledge documents are persisted in PostgreSQL with version tracking
- [ ] Knowledge documents are mirrored to the filesystem for git-tracking
- [ ] Content hash (SHA-256) is computed and stored on every write
- [ ] Optimistic concurrency via `version` column prevents silent overwrites
- [ ] Content exceeding 256 KiB is rejected with HTTP 413
- [ ] Knowledge docs are chunked and indexed into `memory_embeddings` on write
- [ ] `POST /api/memory-bank/search` returns RRF-scored hybrid results
- [ ] Staleness detection compares knowledge sections against recent git changes
- [ ] `/onboard` command produces a structured briefing from all knowledge docs
- [ ] `memory_bank` tool is registered in `ToolRegistry` and callable by agents
- [ ] `TurnExecutor` injects knowledge context into the system prompt
- [ ] All four knowledge kinds are returned by `list_all` after full seeding
- [ ] Gateway routes are authenticated (require `auth_token` when configured)
- [ ] Integration tests pass against embedded PostgreSQL

### 25.12 Dependencies

| Crate | Version | Purpose |
|---|---|---|
| `sha2` | `0.10` | Content hashing (SHA-256) |
| `tokio` | `1` | Async runtime (already in workspace) |
| `serde` | `1` | Serialization (already in workspace) |
| `chrono` | `0.4` | Timestamps (already in workspace) |
| `uuid` | `1` | Document IDs (already in workspace) |
| `diesel` | `2` | Persistence (already in workspace) |
| `diesel-async` | `0.6` | Async Diesel (already in workspace) |
| `tracing` | `0.1` | Instrumentation (already in workspace) |
| `thiserror` | `2` | Error types (already in workspace) |

---

## Phase 26 — Extended Channel Support

### 26.1 Overview

Add seven new channel adapters beyond the core five (Telegram, Discord, Slack,
WhatsApp, Signal). Each adapter implements the existing `ChannelAdapter` trait
from `crates/rune-channels/src/lib.rs` and is registered via `create_adapter`.

New files in `crates/rune-channels/src/`:
- `line.rs` — LINE Messaging API
- `mattermost.rs` — Mattermost Bot API + WebSocket
- `matrix.rs` — Matrix client-server API
- `feishu.rs` — Feishu/Lark Bot API
- `irc.rs` — IRC client
- `google_chat.rs` — Google Chat API
- `teams.rs` — Microsoft Teams Bot Framework

### 26.2 Rust Types

#### 26.2.1 Adapter Structs

Each adapter follows the same pattern as existing adapters. All implement:

```rust
#[async_trait]
pub trait ChannelAdapter: Send + Sync {
    async fn receive(&mut self) -> Result<InboundEvent, ChannelError>;
    async fn send(&self, action: OutboundAction) -> Result<DeliveryReceipt, ChannelError>;
}
```

File: `crates/rune-channels/src/line.rs`

```rust
use async_trait::async_trait;
use reqwest::Client;
use tokio::sync::mpsc;

use crate::{ChannelAdapter, ChannelError, DeliveryReceipt, InboundEvent, OutboundAction};

pub struct LineAdapter {
    channel_access_token: String,
    channel_secret: String,
    listen_addr: Option<String>,
    client: Client,
    rx: mpsc::Receiver<InboundEvent>,
    tx: mpsc::Sender<InboundEvent>,
}

impl LineAdapter {
    pub fn new(
        channel_access_token: &str,
        channel_secret: &str,
        listen_addr: Option<String>,
    ) -> Self;
}

#[async_trait]
impl ChannelAdapter for LineAdapter { /* ... */ }
```

File: `crates/rune-channels/src/mattermost.rs`

```rust
pub struct MattermostAdapter {
    base_url: String,
    bot_token: String,
    team_id: Option<String>,
    channel_ids: Vec<String>,
    client: Client,
    rx: mpsc::Receiver<InboundEvent>,
    tx: mpsc::Sender<InboundEvent>,
}

impl MattermostAdapter {
    pub fn new(
        base_url: &str,
        bot_token: &str,
        team_id: Option<&str>,
        channel_ids: Vec<String>,
    ) -> Self;
}

#[async_trait]
impl ChannelAdapter for MattermostAdapter { /* ... */ }
```

File: `crates/rune-channels/src/matrix.rs`

```rust
pub struct MatrixAdapter {
    homeserver_url: String,
    access_token: String,
    room_ids: Vec<String>,
    client: Client,
    rx: mpsc::Receiver<InboundEvent>,
    tx: mpsc::Sender<InboundEvent>,
}

impl MatrixAdapter {
    pub fn new(
        homeserver_url: &str,
        access_token: &str,
        room_ids: Vec<String>,
    ) -> Self;
}

#[async_trait]
impl ChannelAdapter for MatrixAdapter { /* ... */ }
```

File: `crates/rune-channels/src/feishu.rs`

```rust
pub struct FeishuAdapter {
    app_id: String,
    app_secret: String,
    verification_token: String,
    listen_addr: Option<String>,
    client: Client,
    rx: mpsc::Receiver<InboundEvent>,
    tx: mpsc::Sender<InboundEvent>,
}

impl FeishuAdapter {
    pub fn new(
        app_id: &str,
        app_secret: &str,
        verification_token: &str,
        listen_addr: Option<String>,
    ) -> Self;
}

#[async_trait]
impl ChannelAdapter for FeishuAdapter { /* ... */ }
```

File: `crates/rune-channels/src/irc.rs`

```rust
pub struct IrcAdapter {
    server: String,
    port: u16,
    nickname: String,
    channels: Vec<String>,
    use_tls: bool,
    password: Option<String>,
    rx: mpsc::Receiver<InboundEvent>,
    tx: mpsc::Sender<InboundEvent>,
}

impl IrcAdapter {
    pub fn new(
        server: &str,
        port: u16,
        nickname: &str,
        channels: Vec<String>,
        use_tls: bool,
        password: Option<&str>,
    ) -> Self;
}

#[async_trait]
impl ChannelAdapter for IrcAdapter { /* ... */ }
```

File: `crates/rune-channels/src/google_chat.rs`

```rust
pub struct GoogleChatAdapter {
    service_account_key: String,
    space_ids: Vec<String>,
    listen_addr: Option<String>,
    client: Client,
    rx: mpsc::Receiver<InboundEvent>,
    tx: mpsc::Sender<InboundEvent>,
}

impl GoogleChatAdapter {
    pub fn new(
        service_account_key: &str,
        space_ids: Vec<String>,
        listen_addr: Option<String>,
    ) -> Self;
}

#[async_trait]
impl ChannelAdapter for GoogleChatAdapter { /* ... */ }
```

File: `crates/rune-channels/src/teams.rs`

```rust
pub struct TeamsAdapter {
    app_id: String,
    app_password: String,
    tenant_id: Option<String>,
    listen_addr: Option<String>,
    client: Client,
    rx: mpsc::Receiver<InboundEvent>,
    tx: mpsc::Sender<InboundEvent>,
}

impl TeamsAdapter {
    pub fn new(
        app_id: &str,
        app_password: &str,
        tenant_id: Option<&str>,
        listen_addr: Option<String>,
    ) -> Self;
}

#[async_trait]
impl ChannelAdapter for TeamsAdapter { /* ... */ }
```

#### 26.2.2 Updated `lib.rs` Module Declarations

```rust
// crates/rune-channels/src/lib.rs — add to existing module list:
mod feishu;
mod google_chat;
mod irc;
mod line;
mod matrix;
mod mattermost;
mod teams;

pub use feishu::FeishuAdapter;
pub use google_chat::GoogleChatAdapter;
pub use irc::IrcAdapter;
pub use line::LineAdapter;
pub use matrix::MatrixAdapter;
pub use mattermost::MattermostAdapter;
pub use teams::TeamsAdapter;
```

#### 26.2.3 Updated `create_adapter` Match Arms

```rust
pub fn create_adapter(
    kind: &str,
    config: &rune_config::ChannelsConfig,
) -> Result<Box<dyn ChannelAdapter>, ChannelError> {
    match kind {
        // ... existing five adapters unchanged ...

        "line" => {
            let token = config.line_channel_access_token.as_deref()
                .ok_or_else(|| ChannelError::Provider {
                    message: "line_channel_access_token is required for the LINE adapter".into(),
                })?;
            let secret = config.line_channel_secret.as_deref()
                .ok_or_else(|| ChannelError::Provider {
                    message: "line_channel_secret is required for the LINE adapter".into(),
                })?;
            Ok(Box::new(LineAdapter::new(token, secret, config.line_listen_addr.clone())))
        }
        "mattermost" => {
            let base_url = config.mattermost_base_url.as_deref()
                .ok_or_else(|| ChannelError::Provider {
                    message: "mattermost_base_url is required for the Mattermost adapter".into(),
                })?;
            let token = config.mattermost_bot_token.as_deref()
                .ok_or_else(|| ChannelError::Provider {
                    message: "mattermost_bot_token is required for the Mattermost adapter".into(),
                })?;
            Ok(Box::new(MattermostAdapter::new(
                base_url, token,
                config.mattermost_team_id.as_deref(),
                config.mattermost_channel_ids.clone(),
            )))
        }
        "matrix" => {
            let homeserver = config.matrix_homeserver_url.as_deref()
                .ok_or_else(|| ChannelError::Provider {
                    message: "matrix_homeserver_url is required for the Matrix adapter".into(),
                })?;
            let token = config.matrix_access_token.as_deref()
                .ok_or_else(|| ChannelError::Provider {
                    message: "matrix_access_token is required for the Matrix adapter".into(),
                })?;
            Ok(Box::new(MatrixAdapter::new(
                homeserver, token, config.matrix_room_ids.clone(),
            )))
        }
        "feishu" => {
            let app_id = config.feishu_app_id.as_deref()
                .ok_or_else(|| ChannelError::Provider {
                    message: "feishu_app_id is required for the Feishu adapter".into(),
                })?;
            let app_secret = config.feishu_app_secret.as_deref()
                .ok_or_else(|| ChannelError::Provider {
                    message: "feishu_app_secret is required for the Feishu adapter".into(),
                })?;
            let verify = config.feishu_verification_token.as_deref()
                .ok_or_else(|| ChannelError::Provider {
                    message: "feishu_verification_token is required for the Feishu adapter".into(),
                })?;
            Ok(Box::new(FeishuAdapter::new(
                app_id, app_secret, verify, config.feishu_listen_addr.clone(),
            )))
        }
        "irc" => {
            let server = config.irc_server.as_deref()
                .ok_or_else(|| ChannelError::Provider {
                    message: "irc_server is required for the IRC adapter".into(),
                })?;
            let nickname = config.irc_nickname.as_deref()
                .ok_or_else(|| ChannelError::Provider {
                    message: "irc_nickname is required for the IRC adapter".into(),
                })?;
            Ok(Box::new(IrcAdapter::new(
                server,
                config.irc_port.unwrap_or(6697),
                nickname,
                config.irc_channels.clone(),
                config.irc_use_tls.unwrap_or(true),
                config.irc_password.as_deref(),
            )))
        }
        "google_chat" => {
            let key = config.google_chat_service_account_key.as_deref()
                .ok_or_else(|| ChannelError::Provider {
                    message: "google_chat_service_account_key is required for the Google Chat adapter".into(),
                })?;
            Ok(Box::new(GoogleChatAdapter::new(
                key, config.google_chat_space_ids.clone(),
                config.google_chat_listen_addr.clone(),
            )))
        }
        "teams" => {
            let app_id = config.teams_app_id.as_deref()
                .ok_or_else(|| ChannelError::Provider {
                    message: "teams_app_id is required for the Teams adapter".into(),
                })?;
            let app_password = config.teams_app_password.as_deref()
                .ok_or_else(|| ChannelError::Provider {
                    message: "teams_app_password is required for the Teams adapter".into(),
                })?;
            Ok(Box::new(TeamsAdapter::new(
                app_id, app_password,
                config.teams_tenant_id.as_deref(),
                config.teams_listen_addr.clone(),
            )))
        }

        other => Err(ChannelError::Provider {
            message: format!("unknown channel adapter kind: {other}"),
        }),
    }
}
```

### 26.3 Config Additions

File: `crates/rune-config/src/lib.rs` — add fields to `ChannelsConfig`:

```rust
pub struct ChannelsConfig {
    // ... existing fields ...

    // ── LINE ────────────────────────────────────
    #[serde(default)]
    pub line_channel_access_token: Option<String>,
    #[serde(default)]
    pub line_channel_secret: Option<String>,
    #[serde(default)]
    pub line_listen_addr: Option<String>,

    // ── Mattermost ──────────────────────────────
    #[serde(default)]
    pub mattermost_base_url: Option<String>,
    #[serde(default)]
    pub mattermost_bot_token: Option<String>,
    #[serde(default)]
    pub mattermost_team_id: Option<String>,
    #[serde(default)]
    pub mattermost_channel_ids: Vec<String>,

    // ── Matrix ──────────────────────────────────
    #[serde(default)]
    pub matrix_homeserver_url: Option<String>,
    #[serde(default)]
    pub matrix_access_token: Option<String>,
    #[serde(default)]
    pub matrix_room_ids: Vec<String>,

    // ── Feishu / Lark ───────────────────────────
    #[serde(default)]
    pub feishu_app_id: Option<String>,
    #[serde(default)]
    pub feishu_app_secret: Option<String>,
    #[serde(default)]
    pub feishu_verification_token: Option<String>,
    #[serde(default)]
    pub feishu_listen_addr: Option<String>,

    // ── IRC ─────────────────────────────────────
    #[serde(default)]
    pub irc_server: Option<String>,
    #[serde(default)]
    pub irc_port: Option<u16>,
    #[serde(default)]
    pub irc_nickname: Option<String>,
    #[serde(default)]
    pub irc_channels: Vec<String>,
    #[serde(default)]
    pub irc_use_tls: Option<bool>,
    #[serde(default)]
    pub irc_password: Option<String>,

    // ── Google Chat ─────────────────────────────
    #[serde(default)]
    pub google_chat_service_account_key: Option<String>,
    #[serde(default)]
    pub google_chat_space_ids: Vec<String>,
    #[serde(default)]
    pub google_chat_listen_addr: Option<String>,

    // ── Microsoft Teams ─────────────────────────
    #[serde(default)]
    pub teams_app_id: Option<String>,
    #[serde(default)]
    pub teams_app_password: Option<String>,
    #[serde(default)]
    pub teams_tenant_id: Option<String>,
    #[serde(default)]
    pub teams_listen_addr: Option<String>,
}
```

TOML example:

```toml
[channels]
enabled = ["telegram", "matrix", "irc"]

matrix_homeserver_url = "https://matrix.example.org"
matrix_access_token = "syt_..."
matrix_room_ids = ["!room1:example.org", "!room2:example.org"]

irc_server = "irc.libera.chat"
irc_port = 6697
irc_nickname = "rune-bot"
irc_channels = ["#rune", "#dev"]
irc_use_tls = true
```

Environment variable override:

```
RUNE_CHANNELS__MATRIX_HOMESERVER_URL=https://matrix.example.org
RUNE_CHANNELS__MATRIX_ACCESS_TOKEN=syt_...
RUNE_CHANNELS__IRC_SERVER=irc.libera.chat
```

### 26.4 Wire Protocol

No new gateway HTTP endpoints. Channels are configured declaratively via `AppConfig` and managed by the runtime. The existing `GET /health` response already reports active channels.

### 26.5 SQL Migration

No new tables. Channel adapters are stateless; message persistence uses the existing `sessions`/`turns`/`transcript_items` tables.

### 26.6 Error Cases

All adapters produce the same `ChannelError` variants already defined in `types.rs`:

```rust
#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    #[error("provider error: {message}")]
    Provider { message: String },

    #[error("not implemented")]
    NotImplemented,

    #[error("connection lost: {reason}")]
    ConnectionLost { reason: String },
}
```

Per-adapter error scenarios:

| Adapter | Error | Variant | Cause |
|---|---|---|---|
| LINE | Invalid signature | `Provider` | Webhook HMAC-SHA256 mismatch |
| LINE | Token expired | `Provider` | Channel access token revoked |
| Mattermost | WebSocket disconnect | `ConnectionLost` | Server restart or network failure |
| Mattermost | Invalid team/channel | `Provider` | Configured IDs do not exist |
| Matrix | Sync timeout | `ConnectionLost` | Long-poll `/sync` interrupted |
| Matrix | Unknown room | `Provider` | Bot not invited to configured room |
| Feishu | Verification failed | `Provider` | Event callback verification mismatch |
| Feishu | Token refresh failure | `Provider` | `app_secret` invalid or rotated |
| IRC | Nick collision | `Provider` | Nickname already in use on server |
| IRC | TLS handshake failure | `ConnectionLost` | Certificate validation failed |
| Google Chat | Auth failure | `Provider` | Service account key invalid |
| Google Chat | Space not found | `Provider` | Configured space ID does not exist |
| Teams | Token exchange failure | `Provider` | App ID/password rejected by Azure AD |
| Teams | Tenant mismatch | `Provider` | Message from unexpected tenant |

### 26.7 Edge Cases

1. **IRC message length** — IRC has a 512-byte line limit. Messages exceeding this must be split across multiple PRIVMSG commands. The adapter splits on word boundaries.
2. **IRC reconnection** — Server disconnects are common. The adapter implements exponential backoff reconnection (1s, 2s, 4s, ... capped at 60s).
3. **Matrix /sync pagination** — Initial sync for rooms with large histories must use `since` tokens to avoid loading the entire room timeline. The adapter only processes events after connection.
4. **Feishu token rotation** — Feishu app tokens expire every 2 hours. The adapter maintains a background refresh task that renews 5 minutes before expiry.
5. **Teams proactive messaging** — Teams requires a conversation reference to send proactive messages. The adapter stores conversation references from inbound activities.
6. **Mattermost WebSocket reconnection** — The Mattermost WebSocket API requires re-authentication after reconnection. The adapter re-sends the `authentication_challenge` event.
7. **LINE webhook replay** — LINE may replay webhook events on delivery failure. The adapter deduplicates by `webhookEventId`.
8. **Google Chat service account scope** — The adapter requests `https://www.googleapis.com/auth/chat.bot` scope. Missing IAM permissions produce a clear `Provider` error.
9. **Concurrent `receive` calls** — Only one task should call `receive()` per adapter instance. The `mpsc::Receiver` is not cloneable; attempting to call from multiple tasks will fail at compile time.

### 26.8 Integration Test Scenarios

```rust
// crates/rune-channels/tests/extended_channel_tests.rs

/// Verify create_adapter returns Ok for each new adapter kind with valid config.
#[test]
fn test_create_adapter_line_with_valid_config();

#[test]
fn test_create_adapter_mattermost_with_valid_config();

#[test]
fn test_create_adapter_matrix_with_valid_config();

#[test]
fn test_create_adapter_feishu_with_valid_config();

#[test]
fn test_create_adapter_irc_with_valid_config();

#[test]
fn test_create_adapter_google_chat_with_valid_config();

#[test]
fn test_create_adapter_teams_with_valid_config();

/// Verify create_adapter returns ChannelError::Provider for missing required fields.
#[test]
fn test_create_adapter_line_missing_token_returns_error();

#[test]
fn test_create_adapter_mattermost_missing_base_url_returns_error();

#[test]
fn test_create_adapter_matrix_missing_homeserver_returns_error();

#[test]
fn test_create_adapter_feishu_missing_app_id_returns_error();

#[test]
fn test_create_adapter_irc_missing_server_returns_error();

#[test]
fn test_create_adapter_google_chat_missing_key_returns_error();

#[test]
fn test_create_adapter_teams_missing_app_id_returns_error();

/// Verify IRC adapter splits long messages at word boundaries.
#[tokio::test]
async fn test_irc_message_splitting_at_512_bytes();

/// Verify LINE adapter deduplicates replayed webhooks.
#[tokio::test]
async fn test_line_webhook_deduplication();

/// Verify Matrix adapter only processes events after initial sync token.
#[tokio::test]
async fn test_matrix_skips_historical_events();

/// Verify Mattermost adapter re-authenticates WebSocket on reconnect.
#[tokio::test]
async fn test_mattermost_ws_reconnect_reauth();

/// Verify config round-trips all new fields through TOML + env.
#[test]
fn test_channels_config_new_fields_toml_roundtrip();

/// Verify config loads new fields from RUNE_ environment variables.
#[test]
fn test_channels_config_new_fields_env_override();

/// Verify unknown adapter kind still returns error.
#[test]
fn test_create_adapter_unknown_kind_returns_error();
```

### 26.9 Acceptance Criteria

- [ ] All seven adapters implement `ChannelAdapter` trait
- [ ] `create_adapter` supports `"line"`, `"mattermost"`, `"matrix"`, `"feishu"`, `"irc"`, `"google_chat"`, `"teams"`
- [ ] Each adapter returns `ChannelError::Provider` when required config fields are missing
- [ ] `ChannelsConfig` includes all new fields with `#[serde(default)]`
- [ ] Config fields are overridable via `RUNE_CHANNELS__*` environment variables
- [ ] IRC adapter splits messages exceeding 512 bytes
- [ ] IRC adapter reconnects with exponential backoff
- [ ] LINE adapter validates webhook HMAC-SHA256 signatures
- [ ] LINE adapter deduplicates replayed webhook events
- [ ] Feishu adapter refreshes app tokens before expiry
- [ ] Matrix adapter uses `/sync` tokens to skip historical events
- [ ] Mattermost adapter re-authenticates WebSocket on reconnection
- [ ] Teams adapter stores conversation references for proactive messaging
- [ ] Google Chat adapter handles service account JWT authentication
- [ ] All existing channel tests continue to pass
- [ ] Unknown adapter kind still returns descriptive error
- [ ] `channels.enabled` list supports all twelve adapter names

### 26.10 Dependencies

| Crate | Version | Purpose |
|---|---|---|
| `reqwest` | `0.12` | HTTP client for LINE, Mattermost, Feishu, Google Chat, Teams (already in workspace) |
| `tokio-tungstenite` | `0.24` | WebSocket client for Mattermost |
| `irc` | `1.0` | IRC protocol client for IRC adapter |
| `hmac` | `0.12` | HMAC-SHA256 for LINE webhook verification |
| `sha2` | `0.10` | SHA-256 for LINE webhook verification |
| `jsonwebtoken` | `9` | JWT generation for Google Chat service account auth |
| `base64` | `0.22` | Encoding for various auth flows |
| `tokio` | `1` | Async runtime (already in workspace) |
| `serde` | `1` | Serialization (already in workspace) |
| `async-trait` | `0.1` | Trait async methods (already in workspace) |
| `tracing` | `0.1` | Instrumentation (already in workspace) |

---

## Phase 27 — Calendar & Email Integration

### 27.1 Overview

New crate `crates/rune-productivity/` providing calendar and email integration via
provider traits with Google Calendar, Outlook Calendar, Gmail API, and IMAP/SMTP
implementations. Contact lookup provides context for scheduling and email composition.
Exposes `calendar` and `email` tools to agents and REST endpoints for the gateway.

### 27.2 Crate Layout

```
crates/rune-productivity/
├── Cargo.toml
└── src/
    ├── lib.rs         — Crate root, re-exports
    ├── calendar.rs    — CalendarProvider trait + types
    ├── email.rs       — EmailProvider trait + types
    ├── contacts.rs    — ContactProvider trait + types
    ├── google.rs      — Google Calendar + Gmail implementations
    ├── outlook.rs     — Outlook Calendar + Outlook Mail implementations
    ├── imap_smtp.rs   — Generic IMAP/SMTP email implementation
    └── error.rs       — Unified error type
```

### 27.3 Rust Types

#### 27.3.1 Error Type

File: `crates/rune-productivity/src/error.rs`

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProductivityError {
    #[error("authentication failed: {message}")]
    Auth { message: String },                // → HTTP 401

    #[error("provider error: {message}")]
    Provider { message: String },            // → HTTP 502

    #[error("not found: {resource} {id}")]
    NotFound { resource: String, id: String }, // → HTTP 404

    #[error("conflict: {message}")]
    Conflict { message: String },            // → HTTP 409

    #[error("rate limited: retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },   // → HTTP 429

    #[error("validation error: {message}")]
    Validation { message: String },          // → HTTP 400

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),            // → HTTP 502

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),              // → HTTP 500

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),         // → HTTP 500
}
```

#### 27.3.2 Calendar Types

File: `crates/rune-productivity/src/calendar.rs`

```rust
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A calendar event.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CalendarEvent {
    /// Provider-native event ID.
    pub id: String,
    pub summary: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub start: EventTime,
    pub end: EventTime,
    pub attendees: Vec<Attendee>,
    pub organizer: Option<String>,
    pub status: EventStatus,
    pub recurrence: Option<Vec<String>>,
    pub reminders: Vec<Reminder>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    /// Provider-specific metadata.
    pub provider_data: Option<serde_json::Value>,
}

/// Time representation supporting both datetime and all-day events.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EventTime {
    DateTime { date_time: DateTime<Utc>, time_zone: Option<String> },
    Date { date: NaiveDate },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Attendee {
    pub email: String,
    pub name: Option<String>,
    pub response_status: AttendeeResponse,
    pub optional: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttendeeResponse {
    NeedsAction,
    Accepted,
    Declined,
    Tentative,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventStatus {
    Confirmed,
    Tentative,
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reminder {
    pub method: ReminderMethod,
    pub minutes_before: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReminderMethod {
    Email,
    Popup,
    Sms,
}

/// Query parameters for listing events.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventQuery {
    pub time_min: DateTime<Utc>,
    pub time_max: DateTime<Utc>,
    pub calendar_id: Option<String>,
    pub query: Option<String>,
    pub max_results: Option<u32>,
    pub single_events: Option<bool>,
}

/// Payload for creating a new event.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NewEvent {
    pub summary: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub start: EventTime,
    pub end: EventTime,
    pub attendees: Vec<String>,
    pub reminders: Vec<Reminder>,
    pub calendar_id: Option<String>,
}

/// Payload for updating an existing event.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventUpdate {
    pub summary: Option<String>,
    pub description: Option<String>,
    pub location: Option<String>,
    pub start: Option<EventTime>,
    pub end: Option<EventTime>,
    pub attendees: Option<Vec<String>>,
    pub status: Option<EventStatus>,
}

/// List of calendars the user has access to.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CalendarList {
    pub calendars: Vec<CalendarInfo>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CalendarInfo {
    pub id: String,
    pub summary: String,
    pub primary: bool,
    pub access_role: String,
}

/// Calendar provider trait.
#[async_trait::async_trait]
pub trait CalendarProvider: Send + Sync {
    /// List available calendars.
    async fn list_calendars(&self) -> Result<CalendarList, ProductivityError>;

    /// List events within a time range.
    async fn list_events(&self, query: EventQuery) -> Result<Vec<CalendarEvent>, ProductivityError>;

    /// Get a single event by ID.
    async fn get_event(
        &self,
        calendar_id: &str,
        event_id: &str,
    ) -> Result<CalendarEvent, ProductivityError>;

    /// Create a new event.
    async fn create_event(&self, event: NewEvent) -> Result<CalendarEvent, ProductivityError>;

    /// Update an existing event.
    async fn update_event(
        &self,
        calendar_id: &str,
        event_id: &str,
        update: EventUpdate,
    ) -> Result<CalendarEvent, ProductivityError>;

    /// Delete an event.
    async fn delete_event(
        &self,
        calendar_id: &str,
        event_id: &str,
    ) -> Result<(), ProductivityError>;

    /// Find free/busy slots for a list of attendees.
    async fn free_busy(
        &self,
        emails: &[String],
        time_min: DateTime<Utc>,
        time_max: DateTime<Utc>,
    ) -> Result<Vec<FreeBusyResult>, ProductivityError>;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FreeBusyResult {
    pub email: String,
    pub busy: Vec<TimePeriod>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimePeriod {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}
```

#### 27.3.3 Email Types

File: `crates/rune-productivity/src/email.rs`

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// An email message.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmailMessage {
    /// Provider-native message ID.
    pub id: String,
    pub thread_id: Option<String>,
    pub from: EmailAddress,
    pub to: Vec<EmailAddress>,
    pub cc: Vec<EmailAddress>,
    pub bcc: Vec<EmailAddress>,
    pub subject: String,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub attachments: Vec<EmailAttachment>,
    pub labels: Vec<String>,
    pub is_read: bool,
    pub is_starred: bool,
    pub date: DateTime<Utc>,
    pub in_reply_to: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmailAddress {
    pub email: String,
    pub name: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmailAttachment {
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: u64,
    /// Attachment ID for downloading; not the raw content.
    pub id: String,
}

/// Query parameters for listing email.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmailQuery {
    pub folder: Option<String>,
    pub query: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub subject: Option<String>,
    pub after: Option<DateTime<Utc>>,
    pub before: Option<DateTime<Utc>>,
    pub is_unread: Option<bool>,
    pub max_results: Option<u32>,
    pub page_token: Option<String>,
}

/// Payload for sending email.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NewEmail {
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub in_reply_to: Option<String>,
    pub thread_id: Option<String>,
}

/// Payload for drafting email (saved but not sent).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DraftEmail {
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
}

/// Paginated email listing result.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmailPage {
    pub messages: Vec<EmailMessage>,
    pub next_page_token: Option<String>,
    pub total_estimate: Option<u64>,
}

/// Email provider trait.
#[async_trait::async_trait]
pub trait EmailProvider: Send + Sync {
    /// List messages matching a query.
    async fn list_messages(&self, query: EmailQuery) -> Result<EmailPage, ProductivityError>;

    /// Get a single message by ID.
    async fn get_message(&self, message_id: &str) -> Result<EmailMessage, ProductivityError>;

    /// Send an email.
    async fn send(&self, email: NewEmail) -> Result<EmailMessage, ProductivityError>;

    /// Create a draft.
    async fn create_draft(&self, draft: DraftEmail) -> Result<EmailMessage, ProductivityError>;

    /// Mark messages as read.
    async fn mark_read(&self, message_ids: &[String]) -> Result<(), ProductivityError>;

    /// Mark messages as unread.
    async fn mark_unread(&self, message_ids: &[String]) -> Result<(), ProductivityError>;

    /// Move messages to a folder/label.
    async fn move_to(
        &self,
        message_ids: &[String],
        folder: &str,
    ) -> Result<(), ProductivityError>;

    /// Download an attachment by ID.
    async fn download_attachment(
        &self,
        message_id: &str,
        attachment_id: &str,
    ) -> Result<Vec<u8>, ProductivityError>;

    /// List available folders/labels.
    async fn list_folders(&self) -> Result<Vec<EmailFolder>, ProductivityError>;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmailFolder {
    pub id: String,
    pub name: String,
    pub unread_count: Option<u64>,
    pub total_count: Option<u64>,
}
```

#### 27.3.4 Contact Types

File: `crates/rune-productivity/src/contacts.rs`

```rust
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Contact {
    pub id: String,
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub organization: Option<String>,
    pub title: Option<String>,
    pub notes: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContactQuery {
    pub query: String,
    pub max_results: Option<u32>,
}

/// Contact provider trait for email/calendar context enrichment.
#[async_trait::async_trait]
pub trait ContactProvider: Send + Sync {
    /// Search contacts by name/email.
    async fn search(&self, query: ContactQuery) -> Result<Vec<Contact>, ProductivityError>;

    /// Resolve an email address to a contact.
    async fn resolve_email(&self, email: &str) -> Result<Option<Contact>, ProductivityError>;
}
```

#### 27.3.5 Provider Implementations

File: `crates/rune-productivity/src/google.rs`

```rust
use reqwest::Client;

/// Google Calendar + Gmail provider using OAuth2 access tokens.
pub struct GoogleProvider {
    access_token: String,
    refresh_token: Option<String>,
    client_id: String,
    client_secret: String,
    client: Client,
}

impl GoogleProvider {
    pub fn new(
        access_token: &str,
        refresh_token: Option<&str>,
        client_id: &str,
        client_secret: &str,
    ) -> Self;

    /// Refresh the OAuth2 access token using the refresh token.
    async fn refresh_access_token(&mut self) -> Result<(), ProductivityError>;
}

#[async_trait::async_trait]
impl CalendarProvider for GoogleProvider { /* ... */ }

#[async_trait::async_trait]
impl EmailProvider for GoogleProvider { /* ... */ }

#[async_trait::async_trait]
impl ContactProvider for GoogleProvider { /* ... */ }
```

File: `crates/rune-productivity/src/outlook.rs`

```rust
/// Outlook Calendar + Mail provider using Microsoft Graph API.
pub struct OutlookProvider {
    access_token: String,
    refresh_token: Option<String>,
    client_id: String,
    client_secret: String,
    tenant_id: String,
    client: Client,
}

impl OutlookProvider {
    pub fn new(
        access_token: &str,
        refresh_token: Option<&str>,
        client_id: &str,
        client_secret: &str,
        tenant_id: &str,
    ) -> Self;
}

#[async_trait::async_trait]
impl CalendarProvider for OutlookProvider { /* ... */ }

#[async_trait::async_trait]
impl EmailProvider for OutlookProvider { /* ... */ }

#[async_trait::async_trait]
impl ContactProvider for OutlookProvider { /* ... */ }
```

File: `crates/rune-productivity/src/imap_smtp.rs`

```rust
/// Generic IMAP + SMTP email provider for any standard mailbox.
pub struct ImapSmtpProvider {
    imap_host: String,
    imap_port: u16,
    smtp_host: String,
    smtp_port: u16,
    username: String,
    password: String,
    use_tls: bool,
}

impl ImapSmtpProvider {
    pub fn new(
        imap_host: &str,
        imap_port: u16,
        smtp_host: &str,
        smtp_port: u16,
        username: &str,
        password: &str,
        use_tls: bool,
    ) -> Self;
}

#[async_trait::async_trait]
impl EmailProvider for ImapSmtpProvider { /* ... */ }
```

#### 27.3.6 Crate Root

File: `crates/rune-productivity/src/lib.rs`

```rust
#![doc = "Personal productivity integrations for Rune: calendar, email, and contacts."]

pub mod calendar;
pub mod contacts;
pub mod email;
mod error;
pub mod google;
pub mod imap_smtp;
pub mod outlook;

pub use calendar::{CalendarEvent, CalendarProvider, EventQuery, NewEvent, EventUpdate};
pub use contacts::{Contact, ContactProvider, ContactQuery};
pub use email::{EmailMessage, EmailProvider, EmailQuery, NewEmail, EmailPage};
pub use error::ProductivityError;
pub use google::GoogleProvider;
pub use imap_smtp::ImapSmtpProvider;
pub use outlook::OutlookProvider;
```

### 27.4 Config Additions

File: `crates/rune-config/src/lib.rs` — add to `AppConfig`:

```rust
pub struct AppConfig {
    // ... existing fields ...
    #[serde(default)]
    pub productivity: ProductivityConfig,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProductivityConfig {
    #[serde(default)]
    pub calendar: CalendarConfig,
    #[serde(default)]
    pub email: EmailConfig,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CalendarConfig {
    /// Provider kind: "google", "outlook", or "none".
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub google_client_id: Option<String>,
    #[serde(default)]
    pub google_client_secret: Option<String>,
    #[serde(default)]
    pub google_refresh_token: Option<String>,
    #[serde(default)]
    pub outlook_client_id: Option<String>,
    #[serde(default)]
    pub outlook_client_secret: Option<String>,
    #[serde(default)]
    pub outlook_tenant_id: Option<String>,
    #[serde(default)]
    pub outlook_refresh_token: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EmailConfig {
    /// Provider kind: "google", "outlook", "imap_smtp", or "none".
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub imap_host: Option<String>,
    #[serde(default)]
    pub imap_port: Option<u16>,
    #[serde(default)]
    pub smtp_host: Option<String>,
    #[serde(default)]
    pub smtp_port: Option<u16>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub use_tls: Option<bool>,
}
```

TOML example:

```toml
[productivity.calendar]
provider = "google"
google_client_id = "..."
google_client_secret = "..."
google_refresh_token = "..."

[productivity.email]
provider = "imap_smtp"
imap_host = "imap.example.com"
imap_port = 993
smtp_host = "smtp.example.com"
smtp_port = 587
username = "bot@example.com"
password = "..."
use_tls = true
```

### 27.5 Wire Protocol

#### `GET /api/calendars`

List available calendars.

**Response 200:**
```json
{
  "calendars": [
    {
      "id": "primary",
      "summary": "hamza@example.com",
      "primary": true,
      "access_role": "owner"
    }
  ]
}
```

**Response 501:**
```json
{ "error": "not_configured", "message": "no calendar provider configured" }
```

#### `GET /api/calendars/events`

List events within a time range.

**Query params:** `time_min`, `time_max` (ISO-8601), `calendar_id` (optional), `q` (optional text search), `max_results` (optional int), `single_events` (optional bool).

**Response 200:**
```json
{
  "events": [
    {
      "id": "abc123",
      "summary": "Team standup",
      "description": null,
      "location": "Room 42",
      "start": { "date_time": "2026-03-16T09:00:00Z", "time_zone": "America/New_York" },
      "end": { "date_time": "2026-03-16T09:30:00Z", "time_zone": "America/New_York" },
      "attendees": [
        { "email": "alice@example.com", "name": "Alice", "response_status": "accepted", "optional": false }
      ],
      "organizer": "hamza@example.com",
      "status": "confirmed",
      "recurrence": null,
      "reminders": [{ "method": "popup", "minutes_before": 10 }],
      "created_at": "2026-03-01T12:00:00Z",
      "updated_at": "2026-03-14T08:00:00Z",
      "provider_data": null
    }
  ]
}
```

**Response 400:**
```json
{ "error": "validation", "message": "time_min is required" }
```

#### `POST /api/calendars/events`

Create a new event.

**Request:**
```json
{
  "summary": "Design review",
  "start": { "date_time": "2026-03-17T14:00:00Z" },
  "end": { "date_time": "2026-03-17T15:00:00Z" },
  "attendees": ["alice@example.com"],
  "reminders": [{ "method": "popup", "minutes_before": 15 }]
}
```

**Response 201:**
```json
{
  "id": "def456",
  "summary": "Design review",
  "start": { "date_time": "2026-03-17T14:00:00Z", "time_zone": null },
  "end": { "date_time": "2026-03-17T15:00:00Z", "time_zone": null },
  "attendees": [
    { "email": "alice@example.com", "name": null, "response_status": "needs_action", "optional": false }
  ],
  "status": "confirmed"
}
```

**Response 409:**
```json
{ "error": "conflict", "message": "time slot conflicts with existing event 'Team standup'" }
```

#### `PATCH /api/calendars/events/:calendar_id/:event_id`

Update an event.

**Request:**
```json
{
  "summary": "Design review (updated)",
  "location": "Room 7"
}
```

**Response 200:** Updated `CalendarEvent` JSON.

**Response 404:**
```json
{ "error": "not_found", "message": "event 'xyz789' not found in calendar 'primary'" }
```

#### `DELETE /api/calendars/events/:calendar_id/:event_id`

**Response 200:**
```json
{ "deleted": true }
```

#### `POST /api/calendars/freebusy`

**Request:**
```json
{
  "emails": ["alice@example.com", "bob@example.com"],
  "time_min": "2026-03-16T08:00:00Z",
  "time_max": "2026-03-16T18:00:00Z"
}
```

**Response 200:**
```json
{
  "results": [
    {
      "email": "alice@example.com",
      "busy": [
        { "start": "2026-03-16T09:00:00Z", "end": "2026-03-16T09:30:00Z" },
        { "start": "2026-03-16T14:00:00Z", "end": "2026-03-16T15:00:00Z" }
      ]
    },
    {
      "email": "bob@example.com",
      "busy": []
    }
  ]
}
```

#### `GET /api/email/messages`

**Query params:** `folder`, `q`, `from`, `to`, `subject`, `after`, `before` (ISO-8601), `is_unread` (bool), `max_results` (int), `page_token`.

**Response 200:**
```json
{
  "messages": [
    {
      "id": "msg-001",
      "thread_id": "thread-001",
      "from": { "email": "alice@example.com", "name": "Alice" },
      "to": [{ "email": "hamza@example.com", "name": "Hamza" }],
      "cc": [],
      "bcc": [],
      "subject": "Project update",
      "body_text": "Hi Hamza, ...",
      "body_html": null,
      "attachments": [],
      "labels": ["INBOX"],
      "is_read": false,
      "is_starred": false,
      "date": "2026-03-15T10:00:00Z",
      "in_reply_to": null
    }
  ],
  "next_page_token": "token-xyz",
  "total_estimate": 42
}
```

#### `GET /api/email/messages/:message_id`

**Response 200:** Full `EmailMessage` JSON.

**Response 404:**
```json
{ "error": "not_found", "message": "message 'msg-999' not found" }
```

#### `POST /api/email/send`

**Request:**
```json
{
  "to": ["alice@example.com"],
  "cc": [],
  "bcc": [],
  "subject": "Re: Project update",
  "body_text": "Thanks Alice, ...",
  "in_reply_to": "msg-001",
  "thread_id": "thread-001"
}
```

**Response 200:**
```json
{
  "id": "msg-002",
  "thread_id": "thread-001",
  "subject": "Re: Project update",
  "date": "2026-03-15T12:00:00Z"
}
```

**Response 400:**
```json
{ "error": "validation", "message": "'to' must contain at least one recipient" }
```

#### `POST /api/email/draft`

Same request as send. **Response 201:** Full `EmailMessage` JSON.

#### `POST /api/email/mark-read`

**Request:**
```json
{ "message_ids": ["msg-001", "msg-003"] }
```

**Response 200:**
```json
{ "updated": 2 }
```

#### `POST /api/email/move`

**Request:**
```json
{
  "message_ids": ["msg-001"],
  "folder": "Archive"
}
```

**Response 200:**
```json
{ "moved": 1 }
```

#### `GET /api/email/folders`

**Response 200:**
```json
{
  "folders": [
    { "id": "INBOX", "name": "Inbox", "unread_count": 5, "total_count": 42 },
    { "id": "SENT", "name": "Sent", "unread_count": 0, "total_count": 128 }
  ]
}
```

#### `GET /api/contacts/search`

**Query params:** `q` (required), `max_results` (optional int).

**Response 200:**
```json
{
  "contacts": [
    {
      "id": "c-001",
      "name": "Alice Smith",
      "email": "alice@example.com",
      "phone": "+1555...",
      "organization": "Acme Corp",
      "title": "Engineer",
      "notes": null
    }
  ]
}
```

### 27.6 SQL Migration

```sql
-- 20260315130000_create_productivity_oauth_tokens

CREATE TABLE productivity_oauth_tokens (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    provider       TEXT NOT NULL,
    account_email  TEXT NOT NULL,
    access_token   TEXT NOT NULL,
    refresh_token  TEXT,
    expires_at     TIMESTAMPTZ,
    scopes         TEXT[] NOT NULL DEFAULT '{}',
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (provider, account_email)
);

CREATE INDEX idx_oauth_tokens_provider ON productivity_oauth_tokens (provider);

-- Email drafts persisted locally
CREATE TABLE email_drafts (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    provider     TEXT NOT NULL,
    account      TEXT NOT NULL,
    to_addrs     TEXT[] NOT NULL DEFAULT '{}',
    cc_addrs     TEXT[] NOT NULL DEFAULT '{}',
    subject      TEXT NOT NULL DEFAULT '',
    body_text    TEXT,
    body_html    TEXT,
    in_reply_to  TEXT,
    thread_id    TEXT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_email_drafts_account ON email_drafts (account);
```

### 27.7 Tool Registration

Two tools are registered in `ToolRegistry`:

**`calendar` tool definition:**

```json
{
  "name": "calendar",
  "description": "Manage calendar events: list, create, update, delete, and check free/busy.",
  "parameters": {
    "type": "object",
    "required": ["action"],
    "properties": {
      "action": {
        "type": "string",
        "enum": ["list_calendars", "list_events", "get_event", "create_event", "update_event", "delete_event", "free_busy"]
      },
      "time_min": { "type": "string", "description": "ISO-8601. Required for list_events, free_busy." },
      "time_max": { "type": "string", "description": "ISO-8601. Required for list_events, free_busy." },
      "calendar_id": { "type": "string" },
      "event_id": { "type": "string", "description": "Required for get/update/delete." },
      "summary": { "type": "string" },
      "description": { "type": "string" },
      "location": { "type": "string" },
      "start": { "type": "string", "description": "ISO-8601 datetime or YYYY-MM-DD for all-day." },
      "end": { "type": "string" },
      "attendees": { "type": "array", "items": { "type": "string" } },
      "emails": { "type": "array", "items": { "type": "string" }, "description": "For free_busy." },
      "query": { "type": "string" }
    }
  }
}
```

**`email` tool definition:**

```json
{
  "name": "email",
  "description": "Read, send, draft, and manage email messages.",
  "parameters": {
    "type": "object",
    "required": ["action"],
    "properties": {
      "action": {
        "type": "string",
        "enum": ["list_messages", "get_message", "send", "create_draft", "mark_read", "mark_unread", "move_to", "list_folders", "search_contacts"]
      },
      "query": { "type": "string", "description": "Search query for list_messages or contacts." },
      "folder": { "type": "string" },
      "message_id": { "type": "string" },
      "message_ids": { "type": "array", "items": { "type": "string" } },
      "to": { "type": "array", "items": { "type": "string" } },
      "cc": { "type": "array", "items": { "type": "string" } },
      "bcc": { "type": "array", "items": { "type": "string" } },
      "subject": { "type": "string" },
      "body_text": { "type": "string" },
      "body_html": { "type": "string" },
      "in_reply_to": { "type": "string" },
      "max_results": { "type": "integer" },
      "page_token": { "type": "string" }
    }
  }
}
```

### 27.8 Error Cases

| Scenario | Error Variant | HTTP Status |
|---|---|---|
| No provider configured | `Validation` | 501 |
| OAuth token expired, refresh fails | `Auth` | 401 |
| Google API quota exceeded | `RateLimited` | 429 |
| Event not found | `NotFound` | 404 |
| Time slot conflict detected by provider | `Conflict` | 409 |
| Missing required field (e.g., `to` empty) | `Validation` | 400 |
| IMAP connection refused | `Provider` | 502 |
| SMTP authentication failure | `Auth` | 401 |
| Outlook Graph API 5xx | `Provider` | 502 |
| Attachment too large (>25 MB) | `Validation` | 400 |
| Invalid email address format | `Validation` | 400 |
| Thread not found for reply | `NotFound` | 404 |
| Folder/label does not exist | `NotFound` | 404 |

### 27.9 Edge Cases

1. **All-day events** — `EventTime::Date` variant handles events with no specific time. Serializes as `{"date": "2026-03-16"}` without time_zone.
2. **Recurring events** — `list_events` with `single_events: true` expands recurrences into individual instances. Without it, only the master event is returned.
3. **OAuth token refresh race** — Multiple concurrent requests may attempt refresh simultaneously. The provider uses a `tokio::sync::Mutex<String>` for the access token to serialize refreshes.
4. **IMAP IDLE** — The IMAP provider uses IDLE for push notifications when supported. Falls back to polling every 30 seconds on servers without IDLE.
5. **HTML-only emails** — If `body_text` is None but `body_html` is present, the tool strips HTML tags for a plain-text summary in the tool result.
6. **Large mailboxes** — Email listing is paginated. The `page_token` mechanism prevents loading entire mailboxes into memory.
7. **Time zone handling** — All times are stored and transmitted as UTC. The `time_zone` field in `EventTime` is informational for display purposes only.
8. **Draft auto-save** — Drafts created via `create_draft` are persisted in both the provider (e.g., Gmail Drafts folder) and locally in `email_drafts` for resilience.
9. **Contact resolution during compose** — When agents use partial names in `to` fields, the system resolves them via `ContactProvider::search` before sending. Ambiguous matches return `Validation` error.
10. **Rate limiting** — Google Calendar and Gmail APIs have per-user quotas. The provider tracks `Retry-After` headers and returns `RateLimited` with the wait duration.

### 27.10 Integration Test Scenarios

```rust
// crates/rune-productivity/tests/calendar_tests.rs

/// Verify Google provider lists calendars from mock API.
#[tokio::test]
async fn test_google_list_calendars();

/// Verify list_events filters by time_min/time_max.
#[tokio::test]
async fn test_google_list_events_time_range();

/// Verify create_event returns the new event with provider ID.
#[tokio::test]
async fn test_google_create_event();

/// Verify update_event applies partial updates.
#[tokio::test]
async fn test_google_update_event_partial();

/// Verify delete_event removes the event.
#[tokio::test]
async fn test_google_delete_event();

/// Verify free_busy returns busy periods for multiple attendees.
#[tokio::test]
async fn test_google_free_busy();

/// Verify Outlook provider implements CalendarProvider.
#[tokio::test]
async fn test_outlook_list_events();

/// Verify all-day events use EventTime::Date variant.
#[tokio::test]
async fn test_all_day_event_serialization();

// crates/rune-productivity/tests/email_tests.rs

/// Verify Gmail list_messages with query filter.
#[tokio::test]
async fn test_google_list_messages_with_query();

/// Verify send dispatches via Gmail API and returns message ID.
#[tokio::test]
async fn test_google_send_email();

/// Verify create_draft persists locally and remotely.
#[tokio::test]
async fn test_google_create_draft();

/// Verify mark_read updates provider state.
#[tokio::test]
async fn test_google_mark_read();

/// Verify IMAP/SMTP provider lists messages from INBOX.
#[tokio::test]
async fn test_imap_list_inbox();

/// Verify IMAP/SMTP provider sends via SMTP.
#[tokio::test]
async fn test_smtp_send_email();

/// Verify pagination with page_token.
#[tokio::test]
async fn test_email_pagination();

/// Verify empty to-list is rejected with Validation error.
#[tokio::test]
async fn test_send_rejects_empty_recipients();

// crates/rune-productivity/tests/contacts_tests.rs

/// Verify contact search returns matches.
#[tokio::test]
async fn test_contact_search();

/// Verify resolve_email returns None for unknown address.
#[tokio::test]
async fn test_resolve_unknown_email();

// crates/rune-productivity/tests/gateway_tests.rs

/// Verify GET /api/calendars/events round-trips through gateway.
#[tokio::test]
async fn test_gateway_list_events();

/// Verify POST /api/email/send round-trips through gateway.
#[tokio::test]
async fn test_gateway_send_email();

/// Verify 501 when no provider configured.
#[tokio::test]
async fn test_gateway_returns_501_without_provider();

/// Verify OAuth token refresh on 401 retry.
#[tokio::test]
async fn test_oauth_token_refresh_on_401();
```

### 27.11 Acceptance Criteria

- [ ] `CalendarProvider` trait is implemented for Google Calendar and Outlook Calendar
- [ ] `EmailProvider` trait is implemented for Gmail API, Outlook Mail, and IMAP/SMTP
- [ ] `ContactProvider` trait is implemented for Google and Outlook
- [ ] OAuth2 tokens are persisted in `productivity_oauth_tokens` and auto-refreshed
- [ ] `calendar` tool is registered in `ToolRegistry` and callable by agents
- [ ] `email` tool is registered in `ToolRegistry` and callable by agents
- [ ] All calendar CRUD operations work: list, get, create, update, delete
- [ ] Free/busy query returns busy periods for multiple attendees
- [ ] All-day events serialize correctly with `EventTime::Date`
- [ ] Recurring events expand when `single_events: true`
- [ ] Email listing supports pagination via `page_token`
- [ ] Email send validates at least one recipient
- [ ] Email drafts are persisted both locally and in the provider
- [ ] IMAP provider supports IDLE push with polling fallback
- [ ] Rate limiting returns `429` with `retry_after_secs`
- [ ] Contact search resolves partial names for email composition
- [ ] Gateway endpoints return `501` when no provider is configured
- [ ] All gateway endpoints require auth when `auth_token` is configured
- [ ] `ProductivityConfig` fields are overridable via `RUNE_PRODUCTIVITY__*` env vars
- [ ] Integration tests pass with mocked provider APIs

### 27.12 Dependencies

| Crate | Version | Purpose |
|---|---|---|
| `reqwest` | `0.12` | HTTP client for Google/Outlook APIs (already in workspace) |
| `async-imap` | `0.10` | IMAP client with async support |
| `lettre` | `0.11` | SMTP client for sending email |
| `mail-parser` | `0.9` | RFC 5322 email parsing |
| `oauth2` | `5` | OAuth2 token management and refresh |
| `jsonwebtoken` | `9` | JWT for service account auth |
| `base64` | `0.22` | Encoding for email MIME (already used in Phase 26) |
| `tokio` | `1` | Async runtime (already in workspace) |
| `serde` | `1` | Serialization (already in workspace) |
| `chrono` | `0.4` | Timestamps (already in workspace) |
| `uuid` | `1` | IDs (already in workspace) |
| `thiserror` | `2` | Error types (already in workspace) |
| `tracing` | `0.1` | Instrumentation (already in workspace) |
| `async-trait` | `0.1` | Trait async methods (already in workspace) |

### 27.13 Cargo.toml

```toml
[package]
name = "rune-productivity"
version = "0.1.0"
edition = "2021"

[dependencies]
async-imap = "0.10"
async-trait = "0.1"
base64 = "0.22"
chrono = { version = "0.4", features = ["serde"] }
jsonwebtoken = "9"
lettre = { version = "0.11", features = ["tokio1-native-tls"] }
mail-parser = "0.9"
oauth2 = "5"
reqwest = { version = "0.12", features = ["json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
tokio = { version = "1", features = ["sync"] }
tracing = "0.1"
uuid = { version = "1", features = ["v4", "serde"] }

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
wiremock = "0.6"
```
