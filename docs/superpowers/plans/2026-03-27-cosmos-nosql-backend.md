# Cosmos DB NoSQL Backend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a third storage backend (`cosmos`) to rune-store that implements all 11 repo traits using Azure Cosmos DB for NoSQL with a single-container document model.

**Architecture:** Single container `rune` in database `rune`, synthetic `/pk` partition key, `type` discriminator field. All 11 repo traits implemented by one `CosmosStore` struct. Key-based auth via `azure_data_cosmos` with `key_auth` feature. Vector search via `VectorDistance()` SQL function.

**Tech Stack:** `azure_data_cosmos` 0.31 (with `key_auth` feature), `azure_core`, `serde_json`, `futures` (for `TryStreamExt` on query pagers)

**Spec:** `docs/superpowers/specs/2026-03-27-cosmos-nosql-backend-design.md`

---

## File Structure

```
crates/rune-store/src/
├── cosmos/
│   ├── mod.rs          # CosmosStore struct, client init, ensure_db, helpers, doc types
│   ├── session.rs      # SessionRepo impl
│   ├── turn.rs         # TurnRepo impl
│   ├── transcript.rs   # TranscriptRepo impl
│   ├── job.rs          # JobRepo impl
│   ├── job_run.rs      # JobRunRepo impl
│   ├── approval.rs     # ApprovalRepo impl
│   ├── tool_policy.rs  # ToolApprovalPolicyRepo impl
│   ├── memory.rs       # MemoryEmbeddingRepo impl (vector search)
│   ├── tool_exec.rs    # ToolExecutionRepo impl
│   ├── device.rs       # DeviceRepo impl
│   └── process.rs      # ProcessHandleRepo impl
├── error.rs            # Modify: add azure_data_cosmos error conversions
├── factory.rs          # Modify: add Cosmos arm to build_repos + resolve_backend
├── lib.rs              # Modify: add #[cfg(feature = "cosmos")] pub mod cosmos
```

```
crates/rune-config/src/
└── lib.rs              # Modify: add StorageBackend::Cosmos, cosmos_endpoint, cosmos_key fields
```

```
crates/rune-store/
└── Cargo.toml          # Modify: add cosmos feature + deps
```

```
Cargo.toml              # Modify: add azure_data_cosmos + azure_core to workspace deps
```

---

### Task 1: Add `cosmos` feature flag and dependencies

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Modify: `crates/rune-store/Cargo.toml`

- [ ] **Step 1: Add workspace deps**

In `Cargo.toml` (workspace root), add to `[workspace.dependencies]`:

```toml
azure_data_cosmos = { version = "0.31", features = ["key_auth"] }
azure_core = "0.32"
futures = "0.3"
```

- [ ] **Step 2: Add cosmos feature to rune-store**

In `crates/rune-store/Cargo.toml`, add to `[features]`:

```toml
cosmos = ["dep:azure_data_cosmos", "dep:azure_core", "dep:futures"]
```

And in `[dependencies]`:

```toml
azure_data_cosmos = { workspace = true, optional = true }
azure_core = { workspace = true, optional = true }
futures = { workspace = true, optional = true }
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p rune-store --features cosmos`
Expected: compiles (no cosmos module yet, just deps)

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml crates/rune-store/Cargo.toml
git commit -m "build(store): add cosmos feature flag and azure_data_cosmos deps"
```

---

### Task 2: Config changes — `StorageBackend::Cosmos` and new fields

**Files:**
- Modify: `crates/rune-config/src/lib.rs`

- [ ] **Step 1: Add Cosmos variant to StorageBackend**

Find the `StorageBackend` enum (~line 502) and add:

```rust
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageBackend {
    #[default]
    Auto,
    Sqlite,
    Postgres,
    Cosmos,
}
```

- [ ] **Step 2: Add cosmos fields to DatabaseConfig**

Find `DatabaseConfig` (~line 511) and add the new fields:

```rust
pub struct DatabaseConfig {
    #[serde(default)]
    pub backend: StorageBackend,
    pub database_url: Option<String>,
    /// Cosmos DB NoSQL endpoint URL.
    #[serde(default)]
    pub cosmos_endpoint: Option<String>,
    /// Cosmos DB master key for auth.
    #[serde(default)]
    pub cosmos_key: Option<String>,
    pub max_connections: u32,
    pub run_migrations: bool,
    #[serde(default)]
    pub sqlite_path: Option<PathBuf>,
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p rune-config`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/rune-config/src/lib.rs
git commit -m "feat(config): add StorageBackend::Cosmos and cosmos_endpoint/key fields"
```

---

### Task 3: Error conversions and lib.rs module registration

**Files:**
- Modify: `crates/rune-store/src/error.rs`
- Modify: `crates/rune-store/src/lib.rs`
- Create: `crates/rune-store/src/cosmos/mod.rs` (skeleton)

- [ ] **Step 1: Add azure error conversion to error.rs**

Append to `crates/rune-store/src/error.rs`:

```rust
#[cfg(feature = "cosmos")]
impl From<azure_core::Error> for StoreError {
    fn from(err: azure_core::Error) -> Self {
        let msg = err.to_string();
        if msg.contains("NotFound") || msg.contains("404") {
            StoreError::NotFound {
                entity: "document",
                id: "unknown".to_string(),
            }
        } else if msg.contains("Conflict") || msg.contains("409") {
            StoreError::Conflict(msg)
        } else {
            StoreError::Database(msg)
        }
    }
}
```

- [ ] **Step 2: Register cosmos module in lib.rs**

Add to `crates/rune-store/src/lib.rs`:

```rust
#[cfg(feature = "cosmos")]
pub mod cosmos;
```

- [ ] **Step 3: Create cosmos/mod.rs skeleton**

Create `crates/rune-store/src/cosmos/mod.rs`:

```rust
//! Azure Cosmos DB NoSQL repository implementations.
//!
//! Single-container design: all document types share one container (`rune`)
//! with a synthetic `/pk` partition key and `type` discriminator.

pub mod session;
pub mod turn;
pub mod transcript;
pub mod job;
pub mod job_run;
pub mod approval;
pub mod tool_policy;
pub mod memory;
pub mod tool_exec;
pub mod device;
pub mod process;

use azure_core::credentials::Secret;
use azure_data_cosmos::{
    CosmosAccountEndpoint, CosmosAccountReference, CosmosClient,
    models::{ContainerProperties, PartitionKeyDefinition},
};
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::error::StoreError;

/// Shared Cosmos container client used by all repo implementations.
#[derive(Clone)]
pub struct CosmosStore {
    container: azure_data_cosmos::clients::ContainerClient,
}

impl CosmosStore {
    /// Build a CosmosStore from endpoint + master key.
    pub async fn new(endpoint: &str, key: &str, run_migrations: bool) -> Result<Self, StoreError> {
        let endpoint: CosmosAccountEndpoint = endpoint
            .parse()
            .map_err(|e| StoreError::Database(format!("invalid cosmos endpoint: {e}")))?;

        let account = CosmosAccountReference::with_master_key(
            endpoint,
            Secret::from(key.to_string()),
        );
        let client = CosmosClient::builder()
            .build(account)
            .await
            .map_err(|e| StoreError::Database(format!("cosmos client init failed: {e}")))?;

        if run_migrations {
            ensure_database_and_container(&client).await?;
        }

        let container = client
            .database_client("rune")
            .container_client("rune")
            .await;

        Ok(Self { container })
    }

    /// Access the underlying container client.
    pub fn container(&self) -> &azure_data_cosmos::clients::ContainerClient {
        &self.container
    }
}

/// Ensure the `rune` database and `rune` container exist.
async fn ensure_database_and_container(client: &CosmosClient) -> Result<(), StoreError> {
    // Create database (ignore 409 Conflict = already exists)
    match client.create_database("rune", None).await {
        Ok(_) => info!("created Cosmos database 'rune'"),
        Err(e) if e.http_status() == Some(azure_core::http::StatusCode::Conflict) => {
            info!("Cosmos database 'rune' already exists");
        }
        Err(e) => return Err(StoreError::Database(format!("create database failed: {e}"))),
    }

    // Create container with /pk partition key.
    // Vector embedding policy and indexing policy are set via the JSON properties.
    let properties = ContainerProperties::new(
        "rune".to_string(),
        PartitionKeyDefinition::new(vec!["/pk".to_string()]),
    );

    let db = client.database_client("rune");
    match db.create_container(properties, None).await {
        Ok(_) => info!("created Cosmos container 'rune'"),
        Err(e) if e.http_status() == Some(azure_core::http::StatusCode::Conflict) => {
            info!("Cosmos container 'rune' already exists");
        }
        Err(e) => return Err(StoreError::Database(format!("create container failed: {e}"))),
    }

    Ok(())
}

// ── Document helpers ─────────────────────────────────────────────────

/// Parse a Cosmos document from a serde_json::Value, stripping Cosmos metadata fields.
pub(crate) fn parse_doc<T: for<'de> Deserialize<'de>>(
    val: serde_json::Value,
) -> Result<T, StoreError> {
    serde_json::from_value(val).map_err(|e| StoreError::Serialization(e.to_string()))
}

/// Helper to collect all items from a query pager into a Vec.
pub(crate) async fn collect_query<T: for<'de> Deserialize<'de>>(
    pager: azure_data_cosmos::QueryItemsPager<serde_json::Value>,
) -> Result<Vec<T>, StoreError> {
    use futures::TryStreamExt;
    let mut results = Vec::new();
    let mut stream = pager;
    while let Some(val) = stream.try_next().await.map_err(azure_core::Error::from).map_err(StoreError::from)? {
        results.push(parse_doc(val)?);
    }
    Ok(results)
}

/// Common document fields added to every Cosmos document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DocMeta {
    pub id: String,
    pub pk: String,
    #[serde(rename = "type")]
    pub doc_type: String,
}
```

- [ ] **Step 4: Create empty sub-module files**

Create these files with a placeholder comment so the module compiles:

`crates/rune-store/src/cosmos/session.rs`:
```rust
//! Cosmos SessionRepo implementation.
```

Repeat for: `turn.rs`, `transcript.rs`, `job.rs`, `job_run.rs`, `approval.rs`, `tool_policy.rs`, `memory.rs`, `tool_exec.rs`, `device.rs`, `process.rs` — each with just a module doc comment.

- [ ] **Step 5: Verify it compiles**

Run: `cargo check -p rune-store --features cosmos`
Expected: PASS (or minor fixes needed for exact API types)

- [ ] **Step 6: Commit**

```bash
git add crates/rune-store/src/error.rs crates/rune-store/src/lib.rs crates/rune-store/src/cosmos/
git commit -m "feat(store): add cosmos module skeleton with CosmosStore and startup logic"
```

---

### Task 4: SessionRepo implementation

**Files:**
- Modify: `crates/rune-store/src/cosmos/session.rs`

- [ ] **Step 1: Implement CosmosStore SessionRepo**

Write `crates/rune-store/src/cosmos/session.rs`:

```rust
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use azure_data_cosmos::PartitionKey;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::cosmos::{CosmosStore, collect_query, parse_doc};
use crate::error::StoreError;
use crate::models::*;
use crate::repos::SessionRepo;

/// Cosmos document shape for a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionDoc {
    id: String,
    pk: String,
    #[serde(rename = "type")]
    doc_type: String,
    kind: String,
    status: String,
    workspace_root: Option<String>,
    channel_ref: Option<String>,
    requester_session_id: Option<Uuid>,
    latest_turn_id: Option<Uuid>,
    runtime_profile: Option<String>,
    policy_profile: Option<String>,
    metadata: serde_json::Value,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    last_activity_at: DateTime<Utc>,
}

impl From<SessionDoc> for SessionRow {
    fn from(d: SessionDoc) -> Self {
        SessionRow {
            id: Uuid::parse_str(&d.id).unwrap_or_default(),
            kind: d.kind,
            status: d.status,
            workspace_root: d.workspace_root,
            channel_ref: d.channel_ref,
            requester_session_id: d.requester_session_id,
            latest_turn_id: d.latest_turn_id,
            runtime_profile: d.runtime_profile,
            policy_profile: d.policy_profile,
            metadata: d.metadata,
            created_at: d.created_at,
            updated_at: d.updated_at,
            last_activity_at: d.last_activity_at,
        }
    }
}

impl SessionDoc {
    fn from_new(s: NewSession) -> Self {
        let id_str = s.id.to_string();
        Self {
            id: id_str.clone(),
            pk: id_str,
            doc_type: "session".to_string(),
            kind: s.kind,
            status: s.status,
            workspace_root: s.workspace_root,
            channel_ref: s.channel_ref,
            requester_session_id: s.requester_session_id,
            latest_turn_id: s.latest_turn_id,
            runtime_profile: s.runtime_profile,
            policy_profile: s.policy_profile,
            metadata: s.metadata,
            created_at: s.created_at,
            updated_at: s.updated_at,
            last_activity_at: s.last_activity_at,
        }
    }
}

#[async_trait]
impl SessionRepo for CosmosStore {
    async fn create(&self, session: NewSession) -> Result<SessionRow, StoreError> {
        let doc = SessionDoc::from_new(session);
        let pk = PartitionKey::from(&doc.pk);
        self.container()
            .upsert_item(pk, &doc, None)
            .await
            .map_err(StoreError::from)?;
        Ok(doc.into())
    }

    async fn find_by_id(&self, id: Uuid) -> Result<SessionRow, StoreError> {
        let id_str = id.to_string();
        let pk = PartitionKey::from(&id_str);
        let resp = self.container()
            .read_item::<serde_json::Value>(&pk, &id_str, None)
            .await
            .map_err(|e| {
                if e.http_status() == Some(azure_core::http::StatusCode::NotFound) {
                    StoreError::NotFound { entity: "session", id: id_str.clone() }
                } else {
                    StoreError::from(e)
                }
            })?;
        let val = resp.into_model()?;
        let doc: SessionDoc = parse_doc(val)?;
        Ok(doc.into())
    }

    async fn list(&self, limit: i64, offset: i64) -> Result<Vec<SessionRow>, StoreError> {
        let query = format!(
            "SELECT * FROM c WHERE c.type = 'session' ORDER BY c.created_at DESC OFFSET {} LIMIT {}",
            offset, limit
        );
        let pager = self.container()
            .query_items::<serde_json::Value>(&query, PartitionKey::EMPTY, None)
            .map_err(StoreError::from)?;
        let docs: Vec<SessionDoc> = collect_query(pager).await?;
        Ok(docs.into_iter().map(|d| d.into()).collect())
    }

    async fn find_by_channel_ref(&self, channel_ref: &str) -> Result<Option<SessionRow>, StoreError> {
        let query = format!(
            "SELECT * FROM c WHERE c.type = 'session' \
             AND c.channel_ref = '{}' \
             AND c.status NOT IN ('completed','failed','cancelled') \
             ORDER BY c.created_at DESC OFFSET 0 LIMIT 1",
            channel_ref.replace('\'', "''")
        );
        let pager = self.container()
            .query_items::<serde_json::Value>(&query, PartitionKey::EMPTY, None)
            .map_err(StoreError::from)?;
        let docs: Vec<SessionDoc> = collect_query(pager).await?;
        Ok(docs.into_iter().next().map(|d| d.into()))
    }

    async fn update_status(
        &self,
        id: Uuid,
        status: &str,
        updated_at: DateTime<Utc>,
    ) -> Result<SessionRow, StoreError> {
        let mut row = self.find_by_id(id).await?;
        // Validate FSM transition (reuse same logic as PG)
        let target: rune_core::SessionStatus = status
            .parse()
            .map_err(|e: rune_core::CoreError| StoreError::InvalidTransition(e.to_string()))?;
        let current: rune_core::SessionStatus = row.status
            .parse()
            .map_err(|e: rune_core::CoreError| StoreError::InvalidTransition(e.to_string()))?;
        if !current.can_transition_to(&target) {
            return Err(StoreError::InvalidTransition(
                format!("{} → {} is not allowed", row.status, status),
            ));
        }
        row.status = status.to_string();
        row.updated_at = updated_at;
        row.last_activity_at = updated_at;
        let doc = session_row_to_doc(&row);
        let pk = PartitionKey::from(&doc.pk);
        self.container().upsert_item(pk, &doc, None).await.map_err(StoreError::from)?;
        Ok(row)
    }

    async fn update_metadata(
        &self,
        id: Uuid,
        metadata: serde_json::Value,
        updated_at: DateTime<Utc>,
    ) -> Result<SessionRow, StoreError> {
        let mut row = self.find_by_id(id).await?;
        row.metadata = metadata;
        row.updated_at = updated_at;
        row.last_activity_at = updated_at;
        let doc = session_row_to_doc(&row);
        let pk = PartitionKey::from(&doc.pk);
        self.container().upsert_item(pk, &doc, None).await.map_err(StoreError::from)?;
        Ok(row)
    }

    async fn update_latest_turn(
        &self,
        id: Uuid,
        turn_id: Uuid,
        updated_at: DateTime<Utc>,
    ) -> Result<SessionRow, StoreError> {
        let mut row = self.find_by_id(id).await?;
        row.latest_turn_id = Some(turn_id);
        row.updated_at = updated_at;
        row.last_activity_at = updated_at;
        let doc = session_row_to_doc(&row);
        let pk = PartitionKey::from(&doc.pk);
        self.container().upsert_item(pk, &doc, None).await.map_err(StoreError::from)?;
        Ok(row)
    }

    async fn delete(&self, id: Uuid) -> Result<bool, StoreError> {
        let id_str = id.to_string();
        let pk = PartitionKey::from(&id_str);
        match self.container().delete_item(pk, &id_str, None).await {
            Ok(_) => Ok(true),
            Err(e) if e.http_status() == Some(azure_core::http::StatusCode::NotFound) => Ok(false),
            Err(e) => Err(StoreError::from(e)),
        }
    }

    async fn list_active_channel_sessions(&self) -> Result<Vec<SessionRow>, StoreError> {
        let query = "SELECT * FROM c WHERE c.type = 'session' \
                     AND c.channel_ref != null \
                     AND c.status NOT IN ('completed','failed','cancelled')";
        let pager = self.container()
            .query_items::<serde_json::Value>(query, PartitionKey::EMPTY, None)
            .map_err(StoreError::from)?;
        let docs: Vec<SessionDoc> = collect_query(pager).await?;
        Ok(docs.into_iter().map(|d| d.into()).collect())
    }

    async fn mark_stale_completed(&self, stale_secs: i64) -> Result<u64, StoreError> {
        let cutoff = Utc::now() - chrono::Duration::seconds(stale_secs);
        let cutoff_str = cutoff.to_rfc3339();
        let query = format!(
            "SELECT * FROM c WHERE c.type = 'session' \
             AND c.status = 'running' \
             AND c.last_activity_at < '{}'",
            cutoff_str
        );
        let pager = self.container()
            .query_items::<serde_json::Value>(&query, PartitionKey::EMPTY, None)
            .map_err(StoreError::from)?;
        let docs: Vec<SessionDoc> = collect_query(pager).await?;
        let count = docs.len() as u64;
        let now = Utc::now();
        for doc in docs {
            let id = Uuid::parse_str(&doc.id).unwrap_or_default();
            let _ = self.update_status(id, "completed", now).await;
        }
        Ok(count)
    }
}

/// Convert a SessionRow back to a SessionDoc for upsert.
fn session_row_to_doc(row: &SessionRow) -> SessionDoc {
    let id_str = row.id.to_string();
    SessionDoc {
        id: id_str.clone(),
        pk: id_str,
        doc_type: "session".to_string(),
        kind: row.kind.clone(),
        status: row.status.clone(),
        workspace_root: row.workspace_root.clone(),
        channel_ref: row.channel_ref.clone(),
        requester_session_id: row.requester_session_id,
        latest_turn_id: row.latest_turn_id,
        runtime_profile: row.runtime_profile.clone(),
        policy_profile: row.policy_profile.clone(),
        metadata: row.metadata.clone(),
        created_at: row.created_at,
        updated_at: row.updated_at,
        last_activity_at: row.last_activity_at,
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p rune-store --features cosmos`

- [ ] **Step 3: Commit**

```bash
git add crates/rune-store/src/cosmos/session.rs
git commit -m "feat(store/cosmos): implement SessionRepo"
```

---

### Task 5: TurnRepo implementation

**Files:**
- Modify: `crates/rune-store/src/cosmos/turn.rs`

Follow the same pattern as Task 4. Key differences:
- `TurnDoc` has `session_id` field, `pk = session_id.to_string()` (co-located with session)
- `id = turn_id.to_string()`
- `doc_type = "turn"`
- `list_by_session` queries within a single partition: `PartitionKey::from(session_id.to_string())`
- `mark_stale_failed` is cross-partition with time filter
- `update_status` and `update_usage` do read-modify-upsert

The implementing engineer should mirror the SessionRepo pattern: define `TurnDoc`, `From<TurnDoc> for TurnRow`, `TurnDoc::from_new()`, `turn_row_to_doc()`, and implement all 6 `TurnRepo` trait methods.

- [ ] **Step 1: Write TurnRepo** — Full implementation following session.rs pattern
- [ ] **Step 2: Verify compile** — `cargo check -p rune-store --features cosmos`
- [ ] **Step 3: Commit** — `git commit -m "feat(store/cosmos): implement TurnRepo"`

---

### Task 6: TranscriptRepo implementation

**Files:**
- Modify: `crates/rune-store/src/cosmos/transcript.rs`

Key details:
- `TranscriptItemDoc` has `session_id`, `pk = session_id.to_string()`
- `list_by_session` is single-partition query ordered by `seq`
- `delete_by_session` must query all transcript items in partition, then delete each (Cosmos has no bulk delete by query — iterate and delete)
- `append` uses `create_item` (not upsert) since items are append-only

- [ ] **Step 1: Write TranscriptRepo** — Full implementation
- [ ] **Step 2: Verify compile** — `cargo check -p rune-store --features cosmos`
- [ ] **Step 3: Commit** — `git commit -m "feat(store/cosmos): implement TranscriptRepo"`

---

### Task 7: JobRepo implementation

**Files:**
- Modify: `crates/rune-store/src/cosmos/job.rs`

Key details:
- `pk = format!("job:{}", job_id)` — prefixed to avoid UUID collision with sessions
- `claim_due_jobs` does cross-partition query for enabled jobs with `next_run_at <= now` and `claimed_at IS NULL` (or expired), then upserts each with `claimed_at = now`. Not atomic like SQL but acceptable at Rune's scale.
- `release_claim` sets `claimed_at = null` via read-modify-upsert
- All update methods follow read-modify-upsert pattern

- [ ] **Step 1: Write JobRepo** — Full implementation
- [ ] **Step 2: Verify compile** — `cargo check -p rune-store --features cosmos`
- [ ] **Step 3: Commit** — `git commit -m "feat(store/cosmos): implement JobRepo"`

---

### Task 8: JobRunRepo implementation

**Files:**
- Modify: `crates/rune-store/src/cosmos/job_run.rs`

Key details:
- `pk = format!("job:{}", job_id)` — co-located with parent job
- `list_by_job` is single-partition query ordered by `started_at DESC`
- `complete` does read-modify-upsert

- [ ] **Step 1: Write JobRunRepo** — Full implementation
- [ ] **Step 2: Verify compile** — `cargo check -p rune-store --features cosmos`
- [ ] **Step 3: Commit** — `git commit -m "feat(store/cosmos): implement JobRunRepo"`

---

### Task 9: ApprovalRepo + ToolApprovalPolicyRepo implementation

**Files:**
- Modify: `crates/rune-store/src/cosmos/approval.rs`
- Modify: `crates/rune-store/src/cosmos/tool_policy.rs`

Key details:
- Both use `pk = "global"` — small datasets
- ApprovalRepo: `list(pending_only)` filters by `decision IS NULL`
- ToolApprovalPolicyRepo: `doc_type = "tool_policy"`, natural key is `tool_name`
  - `set_policy` upserts with `id = tool_name`
  - `clear_policy` deletes by `id = tool_name`

- [ ] **Step 1: Write ApprovalRepo** — Full implementation
- [ ] **Step 2: Write ToolApprovalPolicyRepo** — Full implementation
- [ ] **Step 3: Verify compile** — `cargo check -p rune-store --features cosmos`
- [ ] **Step 4: Commit** — `git commit -m "feat(store/cosmos): implement ApprovalRepo and ToolApprovalPolicyRepo"`

---

### Task 10: ToolExecutionRepo + ProcessHandleRepo implementation

**Files:**
- Modify: `crates/rune-store/src/cosmos/tool_exec.rs`
- Modify: `crates/rune-store/src/cosmos/process.rs`

Key details:
- Both use `pk = session_id.to_string()` — co-located with session
- `ToolExecutionRepo::list_recent` is cross-partition with `ORDER BY started_at DESC OFFSET 0 LIMIT N`
- `ProcessHandleRepo::list_active` is cross-partition filtered by status
- `ProcessHandleRepo::find_by_tool_call_id` and `find_by_tool_execution_id` are cross-partition queries

- [ ] **Step 1: Write ToolExecutionRepo** — Full implementation
- [ ] **Step 2: Write ProcessHandleRepo** — Full implementation
- [ ] **Step 3: Verify compile** — `cargo check -p rune-store --features cosmos`
- [ ] **Step 4: Commit** — `git commit -m "feat(store/cosmos): implement ToolExecutionRepo and ProcessHandleRepo"`

---

### Task 11: DeviceRepo implementation

**Files:**
- Modify: `crates/rune-store/src/cosmos/device.rs`

Key details:
- `pk = "global"` for both devices and pairing requests
- Device: `doc_type = "device"`, `id = device_id`
- PairingRequest: `doc_type = "pairing_request"`, `id = request_id`
- `find_device_by_token_hash` and `find_device_by_public_key` are queries within `pk = "global"`
- `take_pairing_request` reads then deletes (consume on use)
- `prune_expired_requests` queries expired, deletes each

- [ ] **Step 1: Write DeviceRepo** — Full implementation with all 13 methods
- [ ] **Step 2: Verify compile** — `cargo check -p rune-store --features cosmos`
- [ ] **Step 3: Commit** — `git commit -m "feat(store/cosmos): implement DeviceRepo"`

---

### Task 12: MemoryEmbeddingRepo implementation (vector search)

**Files:**
- Modify: `crates/rune-store/src/cosmos/memory.rs`

Key details:
- `pk = format!("mem:{}", file_path)`
- `id = format!("{}:{}", file_path, chunk_index)` — natural key
- `embedding` field is `Vec<f32>` serialized as JSON array
- `vector_search` uses `VectorDistance()`:
  ```sql
  SELECT TOP @limit c.file_path, c.chunk_text,
         VectorDistance(c.embedding, @query_vec) AS score
  FROM c WHERE c.type = 'memory_embedding'
  ORDER BY VectorDistance(c.embedding, @query_vec)
  ```
- `keyword_search` uses `CONTAINS(LOWER(c.chunk_text), LOWER(@query))` with score = 1.0
- `delete_by_file` queries all chunks in `pk = "mem:{file_path}"` partition then deletes each
- `count` and `list_indexed_files` are cross-partition aggregation queries

**Note on vector queries:** The `VectorDistance` function and parameterized vector queries may need the embedding passed as a JSON array literal in the query string rather than as a parameter, depending on SDK support. The implementing engineer should test this and fall back to string interpolation of the float array if parameterized queries don't work for vector values.

- [ ] **Step 1: Write MemoryEmbeddingRepo** — Full implementation with vector search
- [ ] **Step 2: Verify compile** — `cargo check -p rune-store --features cosmos`
- [ ] **Step 3: Commit** — `git commit -m "feat(store/cosmos): implement MemoryEmbeddingRepo with vector search"`

---

### Task 13: Factory wiring — connect Cosmos to build_repos

**Files:**
- Modify: `crates/rune-store/src/factory.rs`

- [ ] **Step 1: Add Cosmos to ResolvedBackend**

```rust
enum ResolvedBackend {
    #[cfg(feature = "sqlite")]
    Sqlite,
    #[cfg(feature = "postgres")]
    Postgres,
    #[cfg(feature = "cosmos")]
    Cosmos,
}
```

- [ ] **Step 2: Update resolve_backend**

Add to the `match db.backend` in `resolve_backend()`:

```rust
StorageBackend::Cosmos => {
    #[cfg(feature = "cosmos")]
    return ResolvedBackend::Cosmos;
    #[cfg(not(feature = "cosmos"))]
    panic!("storage backend set to 'cosmos' but the 'cosmos' feature is not compiled in");
}
```

And update `StorageBackend::Auto` to check `cosmos_endpoint`:

```rust
StorageBackend::Auto => {
    if db.database_url.is_some() {
        // ... existing postgres logic
    }
    if db.cosmos_endpoint.is_some() {
        #[cfg(feature = "cosmos")]
        return ResolvedBackend::Cosmos;
        #[cfg(not(feature = "cosmos"))]
        panic!("cosmos_endpoint is set but the 'cosmos' feature is not compiled in");
    }
    // ... existing sqlite fallback
}
```

- [ ] **Step 3: Add build_cosmos_repos function**

```rust
#[cfg(feature = "cosmos")]
async fn build_cosmos_repos(config: &AppConfig) -> Result<(RepoSet, StorageInfo), StoreError> {
    use crate::cosmos::CosmosStore;
    use std::sync::Arc;

    let endpoint = config.database.cosmos_endpoint.as_ref()
        .ok_or_else(|| StoreError::Database("cosmos_endpoint is required".into()))?;
    let key = config.database.cosmos_key.as_ref()
        .ok_or_else(|| StoreError::Database("cosmos_key is required".into()))?;

    let store = CosmosStore::new(endpoint, key, config.database.run_migrations).await?;

    let repos = RepoSet {
        session_repo: Arc::new(store.clone()),
        turn_repo: Arc::new(store.clone()),
        transcript_repo: Arc::new(store.clone()),
        job_repo: Arc::new(store.clone()),
        job_run_repo: Arc::new(store.clone()),
        approval_repo: Arc::new(store.clone()),
        tool_approval_repo: Arc::new(store.clone()),
        memory_embedding_repo: Arc::new(store.clone()),
        tool_execution_repo: Arc::new(store.clone()),
        device_repo: Arc::new(store.clone()),
        process_handle_repo: Arc::new(store),
    };

    let info = StorageInfo {
        backend_name: "cosmos (nosql)",
        #[cfg(feature = "postgres")]
        pgvector_status: None,
        database_url: None,
    };

    Ok((repos, info))
}
```

- [ ] **Step 4: Wire into build_repos**

Update both `#[cfg(feature = "postgres")]` and `#[cfg(not(feature = "postgres"))]` versions of `build_repos` to handle the Cosmos arm:

```rust
#[cfg(feature = "cosmos")]
ResolvedBackend::Cosmos => {
    let (repos, info) = build_cosmos_repos(config).await?;
    // For the postgres-enabled signature, return Ok((repos, info, None))
    // For the non-postgres signature, return Ok((repos, info))
}
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo check -p rune-store --features cosmos,postgres,sqlite`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/rune-store/src/factory.rs
git commit -m "feat(store): wire Cosmos backend into factory and build_repos"
```

---

### Task 14: Full build and integration smoke test

**Files:**
- Modify: `crates/rune-store/Cargo.toml` (add cosmos to default features)

- [ ] **Step 1: Add cosmos to default features**

```toml
[features]
default = ["postgres", "sqlite", "cosmos"]
```

- [ ] **Step 2: Full workspace build**

Run: `cargo build --release 2>&1 | tail -5`
Expected: PASS

- [ ] **Step 3: Test with test instance config**

Create or update `/home/hamza/.rune-test/config.toml` with:

```toml
[database]
backend = "cosmos"
cosmos_endpoint = "<your-cosmos-nosql-endpoint>"
cosmos_key = "<your-cosmos-nosql-key>"
run_migrations = true
```

Run: `scripts/run-test-instance.sh --build`
Expected: Gateway starts, creates database and container, logs "Cosmos container 'rune' already exists"

- [ ] **Step 4: Verify via API**

```bash
curl -s http://127.0.0.1:18792/health
```

Expected: `{"status":"ok","storage_backend":"cosmos (nosql)",...}`

- [ ] **Step 5: Commit and push**

```bash
git add -A
git commit -m "feat(store): Cosmos DB NoSQL backend complete — all 11 repos implemented"
git push origin main
```
