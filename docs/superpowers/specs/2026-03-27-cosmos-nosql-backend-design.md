# Cosmos DB NoSQL Backend Design

Third storage backend for Rune, alongside SQLite and PostgreSQL. Uses Azure Cosmos DB for NoSQL (serverless) with the `azure_data_cosmos` Rust SDK.

## Decisions

| Decision | Choice | Rationale |
|---|---|---|
| SDK | `azure_data_cosmos` v0.31+ | Official Azure SDK, handles auth/pagination/retries; REST API requires hand-rolled HMAC auth |
| Auth | Master key | Simplest for self-hosted; `cosmos_key` in config alongside `cosmos_endpoint` |
| Pricing tier | Serverless | Pay-per-request, no minimum RU/s, fits Rune's bursty workload |
| Container strategy | Single container | One container `rune` in database `rune`, synthetic `/pk` partition key |
| Feature flag | `cosmos` | `#[cfg(feature = "cosmos")]` gated, not in default features |
| Vector search | DiskANN flat index | `VectorDistance()` SQL function, 1536 dimensions, cosine similarity |

## Document Model

Single container, all document types share `/pk` partition key and `type` discriminator.

### Partition Key Strategy (`/pk`)

| Document type | `pk` value | Co-location rationale |
|---|---|---|
| `session` | `{session_id}` | Point reads |
| `turn` | `{session_id}` | Always queried within session |
| `transcript_item` | `{session_id}` | High volume, always per-session |
| `tool_execution` | `{session_id}` | Per-session audit |
| `process_handle` | `{session_id}` | Per-session lookup |
| `job` | `job:{job_id}` | Prefixed to avoid UUID collision with sessions |
| `job_run` | `job:{job_id}` | Co-located with parent job |
| `approval` | `global` | Small dataset (<100 rows) |
| `tool_policy` | `global` | Small dataset |
| `device` | `global` | Small dataset (<10 rows) |
| `pairing_request` | `global` | Ephemeral, tiny |
| `memory_embedding` | `mem:{file_path}` | Chunks grouped by file for bulk delete/upsert |

### Document Schema

Every document has these common fields:

```json
{
  "id": "<unique-id>",
  "pk": "<partition-key>",
  "type": "<document-type>",
  ...entity-specific fields
}
```

The `id` must be unique within a partition. For most types, `id` = the entity's UUID. For `memory_embedding`, `id` = `{file_path}:{chunk_index}` (natural key).

### Cross-Partition Queries

These queries fan out across partitions but are acceptable at Rune's scale:

- `SessionRepo::list()` — scans all `type = "session"` docs, ~1000 rows
- `SessionRepo::list_active_channel_sessions()` — same, filtered
- `SessionRepo::mark_stale_completed()` — same, with time filter
- `JobRepo::list_enabled()` — scans `type = "job"` in `job:*` partitions, ~20 rows
- `TurnRepo::mark_stale_failed()` — scans `type = "turn"`, filtered by time
- `ToolExecutionRepo::list_recent()` — cross-partition, with ORDER BY + TOP N
- `MemoryEmbeddingRepo::list_indexed_files()` — scans `type = "memory_embedding"` in `mem:*` partitions

All of these are either small datasets or infrequent administrative operations.

## Vector Search

### Container Setup

The container must be created with a vector embedding policy and vector index. This is done at container creation time (immutable after).

**Vector embedding policy:**
- Path: `/embedding`
- Data type: `Float32`
- Dimensions: `1536`
- Distance function: `Cosine`

**Vector index:**
- Type: `flat` (for <50K vectors; upgrade to `diskANN` when scale warrants it)
- Path: `/embedding`

**Indexing policy:**
- Exclude `/embedding/*` from standard indexing (required for performance)
- Include `/type`, `/pk`, `/session_id`, `/status`, `/enabled`, `/file_path`, `/chunk_text` for filtered queries

### Vector Queries

```sql
SELECT TOP @limit c.file_path, c.chunk_text,
       VectorDistance(c.embedding, @query_vec) AS score
FROM c
WHERE c.type = 'memory_embedding'
ORDER BY VectorDistance(c.embedding, @query_vec)
```

### Keyword Search

Cosmos DB NoSQL does not have built-in full-text search with ranking (no `ts_rank`). Options:
- Use `CONTAINS()` or `LIKE` for basic substring matching
- Accept that keyword search is weaker than PG's `tsvector` — vector search is the primary retrieval path on Cosmos
- If full-text ranking is needed later, add Azure AI Search as a sidecar

For now: `CONTAINS(LOWER(c.chunk_text), LOWER(@query))` with manual scoring (match = 1.0).

## Config Changes

### `StorageBackend` Enum

Add `Cosmos` variant:

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

### `DatabaseConfig` Fields

```rust
pub struct DatabaseConfig {
    pub backend: StorageBackend,
    pub database_url: Option<String>,       // PG
    pub cosmos_endpoint: Option<String>,    // Cosmos
    pub cosmos_key: Option<String>,         // Cosmos master key
    pub max_connections: u32,
    pub run_migrations: bool,
    pub sqlite_path: Option<PathBuf>,
}
```

### `config.toml` Example

```toml
[database]
backend = "cosmos"
cosmos_endpoint = "https://rune-nosql.documents.azure.com:443/"
cosmos_key = "..."
run_migrations = true  # creates database + container on startup
```

### Auto-Resolution

`StorageBackend::Auto` gains a new rule: if `cosmos_endpoint` is set (and `database_url` is not), resolve to Cosmos.

## Crate Dependencies

Added to `rune-store/Cargo.toml` under the `cosmos` feature:

```toml
[features]
cosmos = ["dep:azure_data_cosmos", "dep:serde_json"]

[dependencies]
azure_data_cosmos = { version = "0.31", optional = true }
```

No `azure_identity` needed — master key auth doesn't require it.

## File Structure

```
crates/rune-store/src/
├── cosmos/
│   ├── mod.rs          # CosmosStore struct, ContainerClient, helpers
│   ├── session.rs      # CosmosSessionRepo
│   ├── turn.rs         # CosmosTurnRepo
│   ├── transcript.rs   # CosmosTranscriptRepo
│   ├── job.rs          # CosmosJobRepo
│   ├── job_run.rs       # CosmosJobRunRepo
│   ├── approval.rs     # CosmosApprovalRepo
│   ├── tool_policy.rs  # CosmosToolApprovalPolicyRepo
│   ├── memory.rs       # CosmosMemoryEmbeddingRepo
│   ├── tool_exec.rs    # CosmosToolExecutionRepo
│   ├── device.rs       # CosmosDeviceRepo
│   └── process.rs      # CosmosProcessHandleRepo
├── factory.rs          # Updated: ResolvedBackend::Cosmos arm
├── lib.rs              # Updated: #[cfg(feature = "cosmos")] pub mod cosmos
└── ...existing files
```

## Factory Wiring

```rust
enum ResolvedBackend {
    #[cfg(feature = "sqlite")]  Sqlite,
    #[cfg(feature = "postgres")] Postgres,
    #[cfg(feature = "cosmos")]  Cosmos,
}

// In build_cosmos_repos():
let client = CosmosClient::new(&endpoint, key_credential, None)?;
let container = client.database_client("rune").container_client("rune");

// If run_migrations, create database + container with vector policy
if config.database.run_migrations {
    ensure_database_and_container(&client).await?;
}

let store = CosmosStore::new(container);
let repos = RepoSet {
    session_repo: Arc::new(store.clone()),
    // ... all 11 repos backed by the same CosmosStore
};
```

### CosmosStore

A single `CosmosStore` struct holds the `ContainerClient` and implements all 11 repo traits. Since all documents live in one container, there's no reason for separate structs.

```rust
#[derive(Clone)]
pub struct CosmosStore {
    container: ContainerClient,
}
```

## Startup / Migration

When `run_migrations = true`:

1. Create database `rune` (idempotent — skip if exists)
2. Create container `rune` with:
   - Partition key: `/pk`
   - Vector embedding policy: `/embedding`, Float32, 1536, Cosine
   - Vector index: flat on `/embedding`
   - Indexing policy: exclude `/embedding/*`, include key query paths
3. Log success

This replaces SQL migrations — Cosmos is schemaless, so "migration" is just ensuring the container exists with the right policies.

## Implementation Order

1. Config changes (`StorageBackend::Cosmos`, new fields)
2. `cosmos/mod.rs` — `CosmosStore`, startup/migration, helper functions
3. Core repos: `session.rs`, `turn.rs`, `transcript.rs` (hot path)
4. Scheduler repos: `job.rs`, `job_run.rs`
5. Supporting repos: `approval.rs`, `tool_policy.rs`, `tool_exec.rs`, `device.rs`, `process.rs`
6. Memory repo: `memory.rs` (vector search)
7. Factory wiring + integration test
