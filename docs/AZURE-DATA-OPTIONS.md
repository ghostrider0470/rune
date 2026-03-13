# Azure Data Options

This document tracks Azure-native data/storage services that may be relevant to the Rust rewrite.

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

## Working assumption

Likely outcome:
- local-first mode remains SQLite/filesystem-centric
- hosted Azure mode may use Postgres/Azure storage services selectively
- Cosmos is only justified if it wins clearly on a specific subsystem, not because it is available
