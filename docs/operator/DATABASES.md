# Database and Storage Options

## What the system needs to store

Different data classes should not be forced into one database just because it is convenient.

Core storage domains:

1. **sessions + transcripts**
2. **jobs / cron / reminders / run history**
3. **channel/provider state**
4. **config + secrets metadata**
5. **memory metadata + retrieval index**
6. **search over transcripts/logs/memory**
7. **optional analytics / reporting**

---

## Decision Summary

The following choices are confirmed:

| Component | Choice | Notes |
|-----------|--------|-------|
| Primary DB | **PostgreSQL** via async repo implementations | Default production backend; external or embedded Postgres |
| Embedded fallback | **postgresql_embedded** | Auto-managed Postgres when no `database_url` configured and backend resolves to Postgres |
| Optional local backend | **SQLite** | Lowest-friction local/dev path; default when backend = `auto` and no Postgres/Cosmos config exists |
| Optional Azure-native backend | **Azure Cosmos DB for NoSQL** | First-class document backend for Azure deployments needing managed NoSQL |
| Full-text search | **PostgreSQL FTS** (tsvector/tsquery) | Built-in, powerful ranking and language support |
| Vector search | **pgvector** | PostgreSQL extension for vector similarity search |
| Embeddings | **Remote only** (Azure OpenAI / OpenAI API) | No local embedding models in phase 1 |
| File storage | **Filesystem** | Mounted paths for all durable content |

Key points:
- Rune resolves storage through a `StorageBackend` factory with first-class `postgres`, `sqlite`, and `cosmos` backends
- `backend = "auto"` resolves to PostgreSQL when `database_url` is set, otherwise Cosmos when `cosmos_endpoint` is set, otherwise SQLite
- When backend resolves to PostgreSQL without a `database_url`, the system starts an embedded PostgreSQL instance via `postgresql_embedded` (data stored under `/data/db`)
- When a PostgreSQL connection string is provided, Rune connects to the external Postgres server directly
- Cosmos uses explicit endpoint + key configuration and bootstraps its own `rune` database/container
- SQLite remains the simplest zero-config local option and defaults to `{db_dir}/rune.db` when `sqlite_path` is unset
- PostgreSQL FTS (tsvector/tsquery) and pgvector remain the richest integrated search path; Cosmos provides backend-native vector support; SQLite uses non-vector stubs unless paired with LanceDB
- Remote embeddings only — no local embedding models in phase 1

---

## Runtime backend matrix

| Backend | Select with | Connection/auth config | Startup / doctor / status surfaces | Bootstrap / migrations | Capability notes |
|---|---|---|---|---|---|
| SQLite | `backend = "sqlite"` or `auto` fallback | `sqlite_path` optional; defaults to `{db_dir}/rune.db` | Reports `sqlite` | Creates DB file on first use; SQLite migrations run automatically when enabled | Good local/dev option; no integrated vector search without LanceDB |
| PostgreSQL | `backend = "postgres"` or `auto` with `database_url` | `database_url = "postgres://..."`; if omitted under explicit Postgres, embedded Postgres starts automatically | Reports `postgres (external)` or `postgres (embedded)` depending on resolution | Runs SQL migrations/bootstraps schema; embedded mode provisions local server first | Best feature coverage: strong concurrency, FTS, optional `pgvector` |
| Azure Cosmos DB for NoSQL | `backend = "cosmos"` or `auto` with `cosmos_endpoint` | `cosmos_endpoint` + `cosmos_key` required | Reports `cosmos (nosql)` / `azure-cosmos` | Creates `rune` database + `rune` container if missing; no relational SQL migrations | Document model; vector support is backend-native, relational/FTS expectations differ from Postgres |
| Azure SQL Database | _Not implemented yet_ | Would likely require a SQL Server/TDS connection mode plus Azure AD or SQL auth config | Not reported today | Not defined today | Track in issue #782; current SQL-family support is PostgreSQL only |

### Current state of Azure SQL support

Azure SQL Database is a **tracked roadmap request, not a shipped backend**. The current `StorageBackend` enum only supports `auto`, `sqlite`, `postgres`, and `cosmos`. In practice that means:

- Rune **cannot** be pointed at Azure SQL today as a supported storage backend
- `database_url` is treated as a PostgreSQL connection string, not a generic SQL URL
- `rune doctor`, gateway topology/status, and capability reporting only recognize SQLite, PostgreSQL, and Cosmos
- migration/bootstrap behavior exists for SQLite, PostgreSQL, and Cosmos only

For Azure-hosted relational deployments today, use **Azure Database for PostgreSQL** rather than Azure SQL Database.

### Configuration examples

#### SQLite

```toml
[database]
backend = "sqlite"
run_migrations = true
sqlite_path = "/data/db/rune.db"
```

#### PostgreSQL (Azure Database for PostgreSQL or any external Postgres)

```toml
[database]
backend = "postgres"
run_migrations = true
database_url = "postgresql://user:pass@host:5432/rune?sslmode=require"
```

#### Cosmos DB for NoSQL

```toml
[database]
backend = "cosmos"
run_migrations = true
cosmos_endpoint = "https://example.documents.azure.com:443/"
cosmos_key = "<account-key>"
```


## Evaluation criteria

For this rewrite, the important criteria are:

- low operational burden
- strong Rust ecosystem support
- local-first ergonomics
- good concurrency story
- crash safety / reliability
- observability and migrations
- search support
- future scale path

---

## Option 1 — SQLite

### Good at
- single-user local-first apps
- daemon-local persistence
- simple deployment
- transactional state

### Weak at
- high write concurrency across many independent workers
- large-scale remote multi-tenant workloads
- vector search is possible but less clean than dedicated engines

### Verdict
Not selected. PostgreSQL with embedded fallback provides similar zero-config local ergonomics while offering a stronger feature set (FTS, pgvector, concurrency).

---

## Option 2 — PostgreSQL ✅ Confirmed

### Good at
- multi-user / multi-node deployments
- stronger concurrency
- durable server-grade operations
- richer query patterns
- extensions for vector search (pgvector) and full text (tsvector/tsquery)
- mature migration tooling (Diesel migrations)

### Weak at
- heavier ops burden than SQLite — mitigated by embedded Postgres fallback for local/dev use

### Best use here
Use PostgreSQL for:
- sessions
- transcripts metadata
- job history
- channel state
- approvals
- config metadata
- plugin registry metadata
- full-text search (tsvector/tsquery)
- vector search (pgvector)

### Verdict
**Primary database from day one.** Driver: Diesel + diesel-async. Embedded Postgres via `postgresql_embedded` when no connection string is configured.

---

## Option 3 — LibSQL / Turso

### Good at
- SQLite semantics with remote/sync story
- local-first with optional sync

### Weak at
- ecosystem and operational maturity still less universal than SQLite/Postgres
- extra complexity if sync is not actually needed

### Verdict
Not needed — PostgreSQL covers this use case.

---

## Option 4 — SurrealDB

### Good at
- flexible document/graph-style modeling
- ambitious all-in-one story

### Weak at
- less boring / less proven for this kind of operational core
- increases risk in a system that already has lots of moving parts

### Verdict
Do **not** use as the primary operational store.

---

## Option 5 — DuckDB

### Good at
- analytics
- local OLAP queries
- ad hoc reporting
- offline analysis over exported events/transcripts

### Weak at
- not ideal as the primary transactional runtime DB

### Verdict
Useful as an **optional analytics sidecar**, not the runtime database.

---

## Option 6 — RocksDB / embedded KV

### Good at
- high-performance key/value workloads
- append-heavy internal subsystems
- caches/state snapshots

### Weak at
- poor fit for operator-facing relational queries
- more implementation complexity
- weaker ergonomics for sessions/jobs/admin UI

### Verdict
Only use for narrow hot-path internals if profiling proves a need.

---

## Search / Retrieval

### PostgreSQL FTS ✅ Confirmed

Full-text search via PostgreSQL's built-in tsvector/tsquery. Supports:
- weighted ranking
- language-aware stemming
- phrase search
- index-backed performance (GIN/GiST)

Use for:
- session transcript search
- message search
- logs/events search

### pgvector ✅ Confirmed

Vector search via the pgvector PostgreSQL extension. Supports:
- cosine, L2, and inner product distance
- HNSW and IVFFlat indexing
- native SQL integration

Paired with remote embeddings (Azure OpenAI / OpenAI API).

### Tantivy

Embedded full-text indexing in Rust.

**Not needed** — PostgreSQL FTS is sufficient and avoids an extra dependency.

### Meilisearch

Fast developer-friendly search service. Only if you explicitly want a separate search service.

### Qdrant

Dedicated vector search service. **Not needed** — pgvector covers vector search within PostgreSQL.

---

## Confirmed Architecture

**PostgreSQL (Diesel + diesel-async) + FTS (tsvector/tsquery) + pgvector + remote embeddings + filesystem**

Embedded PostgreSQL fallback when no connection string is configured.

### Pros
- single database engine for everything (relational, FTS, vector)
- strong concurrency and durability
- mature ORM with compile-time checking (Diesel)
- async support via diesel-async
- zero-config local mode via embedded Postgres
- production-ready external Postgres for deployment
- pgvector well-supported for vector search
- no need for separate search/vector services

### Cons
- embedded Postgres is heavier than SQLite (downloads/manages a full Postgres binary)
- slightly more complex initial setup than SQLite

---

## Phased Roadmap

### Phase 1 ✅ Current
- **Primary DB:** PostgreSQL via Diesel + diesel-async
- **Local fallback:** Embedded PostgreSQL via postgresql_embedded
- **Full-text search:** PostgreSQL FTS (tsvector/tsquery)
- **Vector search:** pgvector
- **Embeddings:** Remote only (Azure OpenAI / OpenAI API)
- **Raw content:** filesystem
- **Secrets:** separate secret store / OS keychain, not plain DB secrets

### Phase 2
- Evaluate pgvector performance at scale
- Consider read replicas or connection pooling if needed

### Phase 3
Optional scale path:
- Connection pooling (pgbouncer or similar)
- Read replicas for heavy query workloads
- Dedicated vector service only if pgvector proves insufficient

---

## Avoid Early

- SurrealDB as primary store
- DuckDB as runtime DB
- RocksDB as core DB
- Tantivy when PostgreSQL FTS is available
- Separate search/vector services before you actually need them

The rewrite already has enough moving parts. Keep storage boring first.
