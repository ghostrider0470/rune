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
| Primary DB | **PostgreSQL** via **Diesel** + **diesel-async** | ORM with async support, schema-first migrations |
| Embedded fallback | **postgresql_embedded** | Auto-managed Postgres when no `DATABASE_URL` configured |
| Full-text search | **PostgreSQL FTS** (tsvector/tsquery) | Built-in, powerful ranking and language support |
| Vector search | **pgvector** | PostgreSQL extension for vector similarity search |
| Embeddings | **Remote only** (Azure OpenAI / OpenAI API) | No local embedding models in phase 1 |
| File storage | **Filesystem** | Mounted paths for all durable content |

Key points:
- **Diesel** + **diesel-async** is the confirmed ORM — schema-first, compile-time checked queries, managed migrations
- When no `DATABASE_URL` is configured, the system starts an embedded PostgreSQL instance via `postgresql_embedded` (data stored under `/data/db`)
- When a connection string is provided, connects to the external Postgres server directly
- PostgreSQL FTS (tsvector/tsquery) for full-text search — more powerful than SQLite FTS5, with ranking and language support
- pgvector for vector search — mature PostgreSQL extension, well-supported ecosystem
- Remote embeddings only — no local embedding models in phase 1
- Single operational database remains the default, while the runtime preserves optional backends behind the StorageBackend factory when enterprise requirements justify them

---

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
