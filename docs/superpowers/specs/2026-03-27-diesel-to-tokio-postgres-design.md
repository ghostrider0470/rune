# Replace Diesel PG Backend with tokio-postgres — Design Spec

**Date:** 2026-03-27
**Goal:** Remove Diesel ORM from the PostgreSQL store backend and replace with direct tokio-postgres queries, enabling TLS connections to Azure Cosmos DB PostgreSQL.

## Problem

The `diesel-async` connection pool uses `AsyncDieselConnectionManager` which defaults to NoTls. Azure Cosmos DB PostgreSQL requires `sslmode=require`. This blocks the PG backend entirely. Mem0 already proves tokio-postgres + native-tls works on the same Azure server.

## Approach

Replace all Diesel ORM usage in `rune-store` with raw tokio-postgres queries. The migration is mechanical — every Diesel DSL call becomes a parameterized SQL string with `$1`/`$2` positional params.

## Phases

### Phase 1: Foundation
- `Cargo.toml` — swap diesel deps for tokio-postgres + deadpool-postgres
- `pool.rs` — new connection pool with TLS support
- `schema.rs` — delete entirely
- `models.rs` — remove Diesel derives, add `From<tokio_postgres::Row>` impls
- `error.rs` — replace Diesel error conversion with tokio-postgres
- `factory.rs` — update pool creation
- Migration runner — replace `diesel_migrations` with embedded SQL runner

### Phase 2: Core Repos
- `PgSessionRepo` — 9 methods
- `PgTurnRepo` — 6 methods
- `PgTranscriptRepo` — 3 methods

### Phase 3: Scheduler Repos
- `PgJobRepo` — 9 methods
- `PgJobRunRepo` — 3 methods

### Phase 4: Remaining Repos
- `PgApprovalRepo` — 5 methods
- `PgToolApprovalPolicyRepo` — 4 methods
- `PgToolExecutionRepo` — 4 methods
- `PgProcessHandleRepo` — 7 methods
- `PgDeviceRepo` — 13 methods
- `PgMemoryEmbeddingRepo` — 7 methods (already raw SQL, minimal changes)

## Key Decisions

- **Pool:** `deadpool-postgres` with `tokio-postgres-native-tls` for TLS
- **Migrations:** Custom embedded SQL runner (no new framework dependency). Read existing `migrations/*/up.sql` files, track in a `_rune_pg_migrations` table.
- **Row mapping:** `From<tokio_postgres::Row>` impls on model structs for type-safe extraction
- **SQL style:** Same SQL as the existing Diesel `sql_query` calls and SQLite implementations. Parameterized with `$1`/`$2`.

## Files Changed

| File | Action |
|------|--------|
| `crates/rune-store/Cargo.toml` | Remove 3 diesel deps, add tokio-postgres + deadpool-postgres + native-tls |
| `crates/rune-store/src/schema.rs` | Delete entirely |
| `crates/rune-store/src/pool.rs` | Rewrite — new pool type with TLS |
| `crates/rune-store/src/models.rs` | Remove Diesel derives, add From<Row> impls |
| `crates/rune-store/src/error.rs` | Replace Diesel error conversion |
| `crates/rune-store/src/factory.rs` | Update pool creation |
| `crates/rune-store/src/pg.rs` | Rewrite all 69 methods across 11 repos |
| `crates/rune-store/src/lib.rs` | Remove schema module |
| `Cargo.toml` (workspace) | Remove diesel workspace deps |

## Success Criteria

1. `backend = "postgres"` connects to Azure Cosmos DB with TLS
2. All 69 repo methods work with tokio-postgres
3. Existing SQLite backend unaffected
4. Migrations run on startup
5. All existing tests pass
