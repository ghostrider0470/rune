# Azure Data Options

This document tracks Azure-native data/storage services that may be relevant to the Rust rewrite.
It should follow the shipped parity-first guidance for Azure-hosted storage instead of inventing a cloud-only baseline.

## Candidate areas to evaluate

### Databases
- Azure Database for PostgreSQL
- Azure SQL Database
- Azure Cosmos DB for NoSQL
- Azure Cosmos DB for PostgreSQL
- Azure Cache for Redis (supporting role only)

### Storage
- Azure Blob Storage
- Azure Files
- Azure Managed Disks / VM-attached storage
- Azure Container Apps mounted storage patterns
- Azure Kubernetes Service persistent volumes

## Evaluation lens

For each option, answer:
- fit for OpenClaw functional parity
- fit for local-first vs hosted mode
- Docker/mount compatibility
- transaction/concurrency characteristics
- search/memory implications
- operational complexity
- Azure lock-in tradeoff

## Initial bias

The rewrite should remain architecturally sound even without Azure-specific infrastructure.
Azure-native services should be evaluated as:
- first-class hosted deployment targets
- optional infrastructure backends
- not excuses to weaken local-first parity

## Current parity-first guidance

### Local-first baseline

- operational state stays PostgreSQL-first, with embedded PostgreSQL as the zero-config/local fallback
- durable human-visible state stays on mounted local filesystems
- Azure services are not required for correctness

### Hosted Azure baseline

- **Operational DB:** embedded PostgreSQL on reliable persistent volume for conservative single-instance mode, or Azure Database for PostgreSQL when managed relational storage is preferred
- **Mounted durable file paths:** Azure Files or persistent volumes
- **Archive/large object retention:** Azure Blob Storage

This keeps hosted Azure mode close to the local Docker mental model instead of redesigning the runtime around cloud-only services.

## Storage-specific guidance

### Azure Files

Best fit for mounted directory semantics such as:

- `/data/memory`
- `/data/skills`
- `/data/logs`
- `/data/backups`
- `/config`
- `/data/sessions` if session state remains file-based
- `/data/media` if throughput is acceptable

Use caution for:

- `/data/db` when embedded PostgreSQL is write-heavy
- high-churn search index paths

### Azure Blob Storage

Best fit for object/archive semantics such as:

- backup exports
- media archives
- old transcript archives
- diagnostic bundles

Do not treat Blob Storage as the canonical replacement for mounted directories like `/data/memory` or `/config`.

### Azure Database for PostgreSQL

Preferred managed Azure database when operational state should move off embedded storage.
Best fit for:

- metadata tables
- job history
- approvals
- channel/provider state
- operational indexes and pointers

## Non-defaults

These are not the current parity-first defaults:

- Azure Cosmos DB for NoSQL
- Azure Cosmos DB for PostgreSQL
- Azure SQL Database

They may be revisited for later scale or enterprise constraints, but they should not define the baseline architecture.
