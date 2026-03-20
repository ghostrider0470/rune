# Docker Deployment

## Purpose

Define a first-class Docker/container deployment model for the Rust rewrite that:

- preserves functional parity with OpenClaw
- keeps durable state mount-friendly and inspectable
- works equally well for local-first Docker and Azure-hosted containers
- avoids hiding critical runtime state inside disposable container layers

This is not a container-afterthought document.
Container deployment is a primary runtime target and a release constraint.

Hard constraints:

- functional parity with OpenClaw
- full Azure compatibility
- Docker-first deployment with mountable persistent storage

---

## 1. Deployment goals

The containerized runtime must support:

- the same operator workflows as bare-metal deployment
- the same logical state layout across host and container modes
- durable persistence through mounted paths or managed backends
- Azure-compatible hosting without requiring Azure-only architecture
- backup, restore, migration, and inspection without container archaeology

---

## 2. Non-negotiable constraints

1. **Behavioral parity first**
   - Docker deployment must not change session/tool/cron/memory behavior

2. **No critical durable state in ephemeral image layers**
   - container filesystem internals are not a database strategy

3. **Mount-friendly layout**
   - all important file-backed state must live in explicit mountable paths

4. **Same logical directories in all modes**
   - local host, Docker, and Azure-hosted deployments should share one mental model

5. **Fast failure on broken storage**
   - missing or unwritable required mounts must fail explicitly and early

6. **Operator inspectability survives restart**
   - restart must not erase the history of sessions, jobs, approvals, deliveries, or background work records

7. **Azure compatibility**
   - the layout must map cleanly to Azure Files, Blob-backed archival flows, and managed DB services

---

## 3. Release-blocker deployment outcomes

These are not optional nice-to-haves.
They are parity gates for a credible rewrite.

A parity-credible Docker deployment must prove:

- the image is stateless by design
- all parity-critical state is externalized
- the runtime can boot with only mounted/configured durable paths plus env/secrets
- restart preserves sessions, transcripts, jobs, approvals, and diagnostic history
- health/readiness endpoints reflect real dependency state, not just process liveness
- doctor can detect broken mount/storage states
- Azure-hosted deployment preserves the same logical storage contract

If any of those fail, Docker support is not done.

---

## 4. Recommended container topology

## 4.1 Phase-1 topology: single primary runtime container

Start with one main runtime container.

Why:

- simplest parity path
- easiest to debug
- closest to current OpenClaw operational expectations
- easiest local/hosted symmetry
- avoids premature microservice decomposition

The primary container should own:

- gateway/daemon
- agent runtime
- scheduler/cron engine
- tool orchestration
- memory/search integration
- channel adapters
- media pipeline coordination
- doctor/status/diagnostic surfaces

Optional external dependencies may exist later, but the main runtime should still be operable as one primary service.

## 4.2 Avoid early service sprawl

Do not split into many containers early for:

- scheduler
- channels
- memory indexer
- approvals
- media workers
- diagnostics

unless profiling, tenancy, or isolation requirements justify it.

The rewrite is already complex enough.
Early decomposition would make parity validation harder.

---

## 5. Canonical state layout

The runtime should expose a stable logical directory layout:

- `/data/db`
- `/data/sessions`
- `/data/memory`
- `/data/media`
- `/data/skills`
- `/data/logs`
- `/data/backups`
- `/config`
- `/secrets`

This layout should work:

- on local host installs
- in Docker bind-mount deployments
- in Docker named-volume deployments
- in Azure-hosted mounts

If a managed DB is used, `/data/db` may be unused or reserved for local indexes/caches, but the logical slot should still exist in the deployment mental model.

## 5.1 Local ↔ Docker path equivalence

Local-first mode uses `~/.rune/` as the data root. Docker mode uses `/data/` (plus `/config` and `/secrets` at the container root). The two layouts are logically equivalent:

| Purpose | Local path | Docker path | Notes |
|---|---|---|---|
| Database | `~/.rune/db/` | `/data/db` | SQLite file or managed-DB cache/indexes |
| Sessions | `~/.rune/sessions/` | `/data/sessions` | Transcripts, exports, session artifacts |
| Memory | `~/.rune/memory/` | `/data/memory` | Daily notes, long-term memory, workspace knowledge |
| Media | `~/.rune/media/` | `/data/media` | Attachments, audio, images, TTS outputs |
| Skills | `~/.rune/skills/` | `/data/skills` | Installed skills, plugin bundles |
| Logs | `~/.rune/logs/` | `/data/logs` | Structured logs, debug bundles, diagnostic dumps |
| Backups | `~/.rune/backups/` | `/data/backups` | Export archives, snapshot bundles |
| Config | `~/.rune/config/` | `/config` | Config overlays, provider/channel config |
| Secrets | `~/.rune/secrets/` | `/secrets` | Credential files, certificate/key material |

**Parity rule:** any feature that works against a local `~/.rune/` path must work identically against the corresponding Docker `/data/` (or `/config`/`/secrets`) mount. The runtime resolves a single `DATA_ROOT` at startup — `~/.rune` locally, `/data` in Docker — and all subsystem paths are relative to it. `/config` and `/secrets` are resolved independently because container orchestrators often inject them from separate sources.

**Fail-fast contract:** if a required path is missing or unwritable at startup, the runtime must exit with a clear error naming the path and expected permissions. `rune doctor` surfaces the same checks interactively (see PROTOCOLS.md §15.3).

---

## 6. Persistent vs ephemeral state

## 6.1 Persistent state

These should be durable by default.

### `/data/db`
Use for:

- embedded PostgreSQL data directory when no external `DATABASE_URL` is configured
- migration state and local database runtime artifacts
- embedded search/index metadata that intentionally lives alongside the local operational database

Persistence requirement: **yes**

### `/data/sessions`
Use for:

- transcript files
- session exports
- raw or normalized session artifacts
- session-scoped diagnostic artifacts where file-backed

Persistence requirement: **yes**

### `/data/memory`
Use for:

- daily memory files
- long-term memory files
- workspace knowledge docs
- derived memory artifacts if file-based

Persistence requirement: **yes**

### `/data/media`
Use for:

- inbound attachments
- audio/image artifacts
- TTS outputs
- extracted files and media cache that should survive restarts

Persistence requirement: **usually yes**

### `/data/skills`
Use for:

- installed skills
- skill metadata
- plugin bundles
- runtime-managed extension assets

Persistence requirement: **yes** if runtime installation/update is supported

### `/data/logs`
Use for:

- structured file logs if enabled
- debug bundles
- diagnostic dumps
- recent failure artifacts worth preserving for supportability

Persistence requirement: **recommended**

### `/data/backups`
Use for:

- export archives
- snapshot bundles
- restore staging

Persistence requirement: **yes**

### `/config`
Use for:

- config overlays
- operator-provided environment-specific files
- channel/provider configuration files

Persistence requirement: **yes**

### `/secrets`
Use for:

- mounted secret files
- certificate/key material
- provider secret references

Persistence requirement: externalized, but not necessarily stored on a normal persistent volume if the platform injects them separately

## 6.2 Ephemeral state

These may remain non-durable:

- `/tmp`
- download scratch space
- request staging
- transient tool execution scratch files
- rebuildable caches
- short-lived compaction/intermediate files

If lost on restart, the runtime should recover safely.

## 6.3 Explicitly non-ephemeral parity domains

The following must **not** depend only on ephemeral writable layers:

- session and transcript history
- job definitions and run history
- approval records
- process/tool audit records
- memory files
- skill/plugin bundles if runtime-managed
- config overlays
- diagnostic history needed for operator support

---

## 7. Mount strategy

## 7.1 Preferred local development/operator strategy

For local Docker deployments, prefer bind mounts or named volumes that map directly to the canonical layout.

Best for parity and inspectability:

- bind mounts for human-managed files (`memory`, `config`, `skills`, `backups`)
- named volumes or bind mounts for DB and media depending on operator preference

Why:

- easy backup
- easy inspection/editing
- easy migration from non-container installs
- easy comparison with OpenClaw workspace conventions

## 7.2 One mount vs many mounts

### One large mount

Example conceptually:

- host path -> `/data`

Pros:

- simple setup
- easy snapshots
- easy portability

Cons:

- less granular backup policy
- mixes hot and cold data
- harder to assign different storage classes

### Multiple targeted mounts

Example conceptually:

- host path A -> `/data/db`
- host path B -> `/data/memory`
- host path C -> `/data/media`
- host path D -> `/config`

Pros:

- better separation
- different backup/retention policies
- easier cloud mapping

Cons:

- slightly more operational complexity

### Recommendation

Support both, but design for multiple logical paths internally.
That gives flexibility for local and Azure-hosted patterns.

---

## 8. Operational storage guidance by deployment mode

## 8.1 Local-first Docker mode

Recommended:

- **Operational DB:** embedded PostgreSQL in `/data/db` when no external connection string is configured
- **Sessions/memory/media/skills/logs/backups:** mounted local filesystem paths
- **Search:** embedded index in `/data/db` or adjacent path

Why this should be the baseline:

- closest to OpenClaw behavior
- simplest setup
- easiest portability
- easiest backup/restore
- strongest offline/local capability

This should be treated as the reference implementation mode.

---

## 8.2 Hosted Azure Docker/container mode — parity-first

Recommended:

- **Operational DB:** embedded PostgreSQL on reliable persistent volume for conservative single-instance mode, or external PostgreSQL if managed DB is preferred
- **Mounted durable file paths:** Azure Files or persistent volumes
- **Archive/large object retention:** Azure Blob Storage
- **Secrets:** Key Vault references or mounted secret files

Best when:

- one main runtime instance
- mount semantics matter
- operator wants behavior close to local Docker mode

Caution:

Embedded PostgreSQL on network-backed storage still needs validation for latency, fsync behavior, backup posture, and restart semantics. If concurrency, HA, or managed-service operability become the priority, move operational metadata to Azure Database for PostgreSQL.

---

## 8.3 Hosted Azure mode — managed relational pattern

Recommended:

- **Operational DB:** Azure Database for PostgreSQL
- **File-backed durable paths:** Azure Files or persistent volume mounts
- **Backups/archives/media offload:** Azure Blob Storage

This is the strongest managed-Azure pattern that still preserves the mount-friendly file model where needed.

---

## 9. Startup, shutdown, and restart contract

## 9.1 Startup contract

On startup, the runtime must:

- validate required mounted paths and config roots
- fail fast on missing/unwritable parity-critical paths
- validate DB/schema compatibility
- restore durable job state and next-run derivation
- restore pending approvals
- restore session and transcript inspectability
- surface degraded state explicitly if some optional path is unavailable

## 9.2 Shutdown contract

On graceful shutdown, the runtime must:

- flush durable state
- record terminal or interrupted state for active sessions/jobs/processes as needed
- stop accepting new work before teardown when possible
- make interrupted-but-durable work inspectable on restart

## 9.3 Restart contract

After restart, the runtime must preserve and recover at minimum:

- sessions and transcript history
- jobs and next-run state
- approvals still awaiting decision
- process/tool audit history
- durable channel delivery references
- recent logs/diagnostic state where configured durable

Long-running OS child processes may not survive restart in all modes, but their prior existence and terminal or lost state must remain inspectable. The runtime must not pretend the work never existed.

Operators should treat container restart verification as a first-class workflow, not an ad hoc sanity check. At minimum the documented post-restart path should verify:

1. `rune doctor` for writable mounts and path sanity
2. health/status surfaces for dependency readiness
3. scheduler/job state for expected next runs and preserved history
4. session/transcript inspectability for recently active work
5. approval queues / durable audit trails for work that was pending across restart

---

## 10. Probe and doctor expectations in containers

## 10.1 Health/readiness/liveness behavior

Container deployment should provide:

- liveness endpoint proving the process loop is alive
- readiness endpoint proving critical dependencies are usable
- status endpoint for deeper operator inspection

Readiness should fail or degrade when parity-critical dependencies are unavailable, including where relevant:

- auth/bootstrap state is broken
- required durable paths are unwritable
- DB/schema is incompatible
- scheduler core cannot initialize
- gateway control plane cannot serve expected requests

## 10.2 Doctor integration

Doctor must be able to detect container/storage-specific issues such as:

- missing mounts
- wrong ownership/permissions
- read-only mounts where write access is required
- path mismatch between config and mounted layout
- stale schema vs current runtime
- Azure-mounted storage drift or misconfiguration where detectable

---

## 11. Azure-native storage research applied to Docker

## 11.1 Azure Files

Best Azure-native fit for mounted directories required by the runtime.

Good targets:

- `/data/memory`
- `/data/skills`
- `/data/logs`
- `/data/backups`
- `/config`
- `/data/sessions` if session files remain file-based
- `/data/media` if throughput is acceptable

Use caution for:

- `/data/db` if embedded PostgreSQL is expected to absorb heavy write bursts over high-latency network storage
- high-churn search index paths

## 11.2 Azure Blob Storage

Best fit for object/archive semantics, not ordinary mounted workspace semantics.

Good targets:

- backup exports copied out of `/data/backups`
- media archive tiers
- old transcript archives
- diagnostic bundles

Do not treat Blob Storage as the canonical replacement for directories like `/data/memory` or `/config`.

## 11.3 Azure Database for PostgreSQL

Best managed Azure backend if moving operational state out of embedded storage.

Best target for:

- metadata tables
- job history
- approvals
- channel/provider state
- operational indexes/pointers

Use this when hosted reliability and managed DB posture matter more than full embedded-database symmetry with zero-config local mode.

## 11.4 Cosmos DB and Azure SQL

Not recommended as default deployment targets for the parity-first container architecture.

- **Cosmos DB NoSQL:** wrong default model for the runtime core
- **Cosmos DB for PostgreSQL:** overbuilt for early parity needs
- **Azure SQL:** possible enterprise-specific support, but not the default portable path

---

## 12. Backup and restore expectations

Container deployment must make backup/restore straightforward. See [PROTOCOLS.md §15.4](../parity/PROTOCOLS.md#154-backup-and-restore-workflow-contract) for the full behavioral contract and CLI workflow spec.

## 12.1 What must be backup-friendly

- database files or managed DB dumps
- memory documents
- session/transcript artifacts
- skills/plugins bundles
- configuration overlays
- secrets references/config, excluding secret values where policy requires external secret storage
- exports and logs as needed

## 12.2 Backup strategy by mode

### Local-first Docker

- filesystem snapshot of mounted paths
- PostgreSQL-consistent backup/export for `/data/db` when running embedded PostgreSQL
- optional archive bundles into `/data/backups`

### Hosted Azure with mounted filesystem

- volume/share snapshot where available
- PostgreSQL backups if managed DB is used
- Blob Storage replication for backup archives

## 12.3 Restore principle

A restore should reconstruct the runtime without needing image-layer recovery or undocumented hidden paths.

## 12.4 Minimum workflow contract

Any documented Rune backup/restore workflow should make these steps explicit:

1. **Quiesce or coordinate writes** where required (for example, ensure embedded PostgreSQL export/snapshot consistency and avoid mid-write filesystem capture).
2. **Capture all required durable domains**: DB state, sessions, memory, media when retained, skills, logs/exports as policy requires, config overlays, and backup staging outputs.
3. **Exclude secret values from backup metadata** while preserving secret references/config needed to reconnect the runtime after restore.
4. **Restore into the same logical path layout** (`~/.rune/*` locally or `/data/*`, `/config`, `/secrets` in Docker).
5. **Run post-restore verification**: `rune doctor`, health/status checks, scheduler/job sanity, and session/transcript inspectability checks.

If a deployment mode uses managed PostgreSQL or external object storage, the operator docs must state which parts are restored from provider-native snapshots versus local filesystem artifacts.

---

## 13. Upgrade and migration expectations

Container upgrades should be image-replace operations, not state-migration adventures hidden inside ephemeral layers.

Rules:

- runtime version changes may migrate DB/schema, but not relocate hidden state into image internals
- file layout changes must be explicit and documented
- migration tooling should preserve mounted data paths
- rollback should be possible from persistent backup/state snapshots

---

## 14. Scaling posture

## 14.1 Early scaling assumption

Assume:

- single primary runtime instance
- single active scheduler authority
- single writer for embedded DB mode

This is fine for parity-first OpenClaw-style operation.

## 14.2 When to move beyond embedded storage

Move from embedded PostgreSQL to managed PostgreSQL when:

- multi-instance runtime becomes necessary
- scheduler/job state coordination becomes multi-node
- write concurrency grows materially
- backup/restore/HA requirements exceed comfort with embedded local DB management

Do **not** switch storage just because the runtime is on Azure.

---

## 15. Security and secret handling in containers

Container images must not bake in secrets.

Support these patterns:

- environment variables
- mounted secret files in `/secrets`
- Azure Key Vault references or injection paths
- certificate/key mounts for provider integrations when needed

Secrets should be external to the image and easy to rotate.

---

## 16. Container runtime contract

The container image and runtime configuration should satisfy these baseline operational rules:

- image is stateless by design
- all durable paths are externalized
- runtime fails fast if required persistent paths are missing or unwritable
- runtime can run as a non-root user where practical
- health/readiness endpoints reflect actual dependency readiness, not just process liveness
- shutdown drains in-flight work to the extent compatible with bounded graceful termination
- logs go to stdout/stderr first, with optional mirrored durable logs

## 16.1 Required environment/config categories

The deployment model should expect configuration for at least:

- gateway bind/address and auth settings
- workspace/data root mappings
- provider/model configuration including Azure-specific fields
- channel credentials/config
- storage backend selection where optional
- log/telemetry/export settings
- secrets references

## 16.2 Compatibility note on host paths

Physical host paths may differ across environments.
What must remain stable is the logical in-container contract and operator mental model.

---

## 17. Recommended default deployment patterns

## Pattern A — default local-first Docker

Use:

- single runtime container
- embedded PostgreSQL in `/data/db`
- mounted host paths or named volumes for `/data/*`, `/config`, `/secrets`

This should be the default documentation path for parity-first deployment.

## Pattern B — default hosted Azure deployment

Use:

- single runtime container
- Azure Database for PostgreSQL if managed DB is desired, otherwise embedded PostgreSQL on persistent volume for conservative single-instance mode
- Azure Files for mount-style durable paths
- Azure Blob Storage for archives/backups/media offload
- Key Vault for managed secrets

This should be the default Azure-hosted recommendation.

## Pattern C — future larger-scale hosted mode

Use:

- runtime container(s)
- PostgreSQL for operational metadata
- Azure Files/persistent volumes for file semantics
- Blob Storage for object/archive data
- keep the same logical state contract

---

## 18. Evidence required before claiming Docker parity

Before claiming Docker-first parity, capture black-box evidence for:

- boot with mounted persistent storage only
- fail-fast behavior on missing/unwritable required paths
- restart durability for sessions, jobs, approvals, and history
- readiness/liveness/status distinction
- doctor detection of broken storage/mount states
- backup/restore path that does not rely on image-layer state
- Azure-hosted deployment mapping using the same logical durable-state model

---

## 19. Final recommendation

The safest container strategy is:

1. preserve one canonical logical state layout
2. keep all durable state external to the image
3. use PostgreSQL + mounted filesystem as the baseline local-first mode, with embedded Postgres when no external database is configured
4. use Azure Database for PostgreSQL as the preferred managed relational path in Azure when external managed DB is desired
5. use Azure Files for mount-friendly persistent directories
6. use Azure Blob Storage for archives, backups, and object-scale media
7. avoid redesigning the runtime around Cosmos DB or Azure SQL unless a specific hosted requirement justifies it

That gives first-class Docker support, Azure compatibility, and the best chance of remaining behaviorally identical to OpenClaw while staying portable.

---

## 13. Zero-config startup coherence (issue #61)

#### 1. Scope

This slice covers only:

- startup config/env precedence
- local standalone path remapping
- Docker path preservation
- zero-config Ollama bootstrap
- operator-visible startup diagnostics

It does not introduce a setup wizard, new provider abstractions, or multi-provider redesign.

---

#### 2. Startup precedence

Rune resolves startup inputs in this order:

1. `--config <path>`
2. `RUNE_CONFIG`
3. built-in defaults plus `RUNE_*` environment overrides

Operator-visible startup logs must show:

- which config source won
- the resolved config path when one exists
- the requested runtime mode and the resolved runtime mode

---

#### 3. Local versus Docker path contract

#### 3.1 Bare-host zero-config

When all of these are true:

- `mode = "auto"`
- no `database.database_url` is configured
- Docker/Kubernetes runtime signals are absent
- paths are still at the built-in Docker-first defaults

Rune must resolve startup as standalone local mode and remap paths to:

- `~/.rune/db`
- `~/.rune/sessions`
- `~/.rune/memory`
- `~/.rune/media`
- `~/.rune/skills`
- `~/.rune/logs`
- `~/.rune/backups`
- `~/.rune/config`
- `~/.rune/secrets`

This is the zero-config local quick-start contract.

#### 3.2 Docker or server-oriented startup

Rune must preserve the Docker/server layout when either of these is true:

- `database.database_url` is configured
- Docker/Kubernetes runtime signals are present
- the operator explicitly points paths at `/data/...`

The canonical server/container paths remain:

- `/data/db`
- `/data/sessions`
- `/data/memory`
- `/data/media`
- `/data/skills`
- `/data/logs`
- `/data/backups`
- `/config`
- `/secrets`

Startup logs must show whether the active path profile is:

- `docker-default`
- `standalone-home`
- `custom`

---

#### 4. Zero-config Ollama contract

When `models.providers` is empty, Rune uses zero-config model bootstrap:

1. normalize `OLLAMA_HOST` into an effective Ollama base URL when set
2. otherwise probe `http://localhost:11434`
3. when Ollama is reachable, use it as the default provider
4. when pulled models exist, auto-select a default model
5. when no models are pulled, start without a default model and print actionable guidance
6. when Ollama is unreachable, fall back to the echo provider

Accepted `OLLAMA_HOST` forms mirror Ollama itself:

- `http://host:port` or `https://host:port`
- `host:port` which maps to `http://host:port`
- `host` which maps to `http://host:11434`

When `models.providers` is non-empty, explicit provider config wins and zero-config Ollama auto-detect is disabled even if `OLLAMA_HOST` is set. Startup diagnostics should reflect that the env var is ignored rather than treating it as an active probe target.

If `models.default_model` is set while Rune is using zero-config Ollama bootstrap, that configured default must win over the Ollama auto-pick.

When explicit providers are configured, Rune should detect a configured default provider in this order:

1. resolve the default agent model when one exists
2. otherwise resolve `models.default_model` when it exists
3. otherwise, when exactly one provider is configured, treat that provider as the configured default provider even before a default model is chosen
4. otherwise leave the configured default provider unset

If an explicit default model cannot be resolved against the configured provider inventory, startup diagnostics must surface that as unresolved instead of silently inventing a provider.

---

#### 5. Operator-visible startup diagnostics

Startup logging must make these decisions inspectable without reading source code:

- config source: CLI, `RUNE_CONFIG`, or defaults/env
- requested mode versus resolved mode
- active path profile and state root
- storage backend selection
- model bootstrap mode: explicit providers or zero-config Ollama
- raw `OLLAMA_HOST` value when present
- effective zero-config Ollama probe target when relevant
- configured providers summary
- configured default model and its source before runtime probing
- configured default provider and its source before runtime probing when detectable
- resolved provider mode: explicit providers, zero-config Ollama, or echo fallback
- resolved provider detail: configured provider names, Ollama probe target, or echo
- resolved default provider and its source after runtime provider selection
- default model source: agent config, `models.default_model`, or Ollama auto-pick

If `OLLAMA_HOST` is set but unreachable, startup must warn explicitly that Rune is falling back, and must name the normalized probe target instead of silently behaving like localhost probing.

If explicit provider construction fails at startup, Rune must report `echo-fallback` as the resolved provider mode rather than logging the configured provider inventory as if it were active.

---

#### 9. Failure behavior

#### 9.1 Path validation

Required persistent paths must be checked at startup with a real write probe, not only metadata inspection.

Missing or unwritable parity-critical paths must produce explicit warnings or failures that name the path and reason.

This section is the contract referenced by runtime path validation code.
