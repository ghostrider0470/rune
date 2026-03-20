# Azure Compatibility

## Purpose

Define the non-negotiable Azure compatibility requirements for the Rust rewrite while preserving the primary goal:

- remain functionally identical to OpenClaw from the operator/user perspective
- support Azure-native deployment and provider integrations cleanly
- avoid designing the runtime in a way that forces Azure lock-in

Azure compatibility is mandatory.
Azure dependency is not.

Hard constraints:

- functional parity with OpenClaw
- full Azure compatibility
- Docker-first deployment with mountable persistent storage

---

## 1. Compatibility stance

The runtime should treat Azure as a first-class environment across:

- model/provider access
- OCR/document extraction
- secrets/config patterns
- hosted container deployment
- durable storage choices
- observability and operational posture

But the runtime must still preserve:

- local-first operation
- Docker-first deployment support
- mount-friendly state layout
- provider abstraction that does not leak Azure-specific assumptions into unrelated subsystems

That means:

- Azure-specific support must be explicit and complete where needed
- Azure-specific infrastructure must remain optional where local or non-Azure equivalents work fine
- Azure support must be real request/response and deployment compatibility, not branding over generic OpenAI-compatible code paths

---

## 2. Release-blocker Azure outcomes

A parity-credible rewrite must prove all of the following:

- Azure model requests support endpoint, deployment name, API version, auth mode, and Azure-specific headers correctly
- Azure error responses normalize into coherent provider/runtime error classes without losing Azure-relevant details
- Azure Document Intelligence is a first-class document-understanding path
- Azure-hosted container deployments can use the same logical persistent-state model as local Docker deployments
- Azure-friendly secret injection and probe behavior are documented and testable

If those are not proven, Azure compatibility is not done.

---

## 3. Azure parity requirements

## 3.1 Model/runtime provider compatibility

Must support, at minimum:

- Azure OpenAI endpoint shapes and request/body conventions
- Azure AI Foundry-hosted model access patterns where relevant
- Azure deployment-name semantics instead of only generic model IDs
- Azure API versioning requirements
- Azure authentication/header conventions
- Azure-compatible error handling and retry behavior

Provider config must support:

- endpoint base URL
- deployment name
- logical model mapping where needed
- API version
- auth method
- request timeout/retry policy
- optional custom headers

The provider layer must not assume that `model = gpt-4.x` is sufficient.
In Azure, deployment identity is operationally important and must be represented directly.
That matters most for Azure OpenAI, where `deployment_name` is part of the request path and cannot be collapsed into a generic model field.
Azure AI Foundry must also remain distinct from Azure OpenAI when its OpenAI-path APIs expect a normal `model` field instead of Azure OpenAI deployment-path semantics.

## 3.2 Minimum Azure request-construction contract

The provider abstraction must be able to express, at minimum:

- endpoint base separate from deployment name
- deployment name separate from logical model alias
- API version as explicit config, not hidden default magic
- auth mode selection such as API key vs token-bearing flows where later needed
- Azure-specific headers and request quirks without leaking them into non-Azure providers

Current parity-critical wire-shape expectations are:

- **Azure OpenAI:** construct paths like `{base_url}/openai/deployments/{deployment_name}/chat/completions?api-version={api_version}`
- **Azure OpenAI:** send `api-key` header auth by default and do not assume `Authorization: Bearer ...`
- **Azure OpenAI:** omit the `model` field from the JSON body because deployment identity lives in the URL path
- **Azure OpenAI:** preserve Azure request-body conventions such as `max_tokens`
- **Azure AI Foundry (OpenAI-path):** construct paths like `{base_url}/openai/v1/chat/completions` without Azure OpenAI deployment-path routing
- **Azure AI Foundry (OpenAI-path):** keep the logical `model` in the request body and use the provider's required Azure header conventions
- **Azure AI Foundry (non-OpenAI families):** allow family-specific Azure headers such as version headers where the upstream contract requires them

At a minimum, the rewrite should be able to snapshot-test these dimensions:

- Azure OpenAI chat/completions request path construction with deployment name + API version separation
- Azure OpenAI body construction rules, especially omission of `model`
- Azure OpenAI auth/header construction, especially `api-key` vs `Authorization`
- Azure AI Foundry OpenAI-path request construction with `model` retained in the body
- responses API or equivalent request path construction where supported
- image/multimodal request construction if that surface is supported
- timeout/retry classification for Azure-specific transient failures

## 3.3 Error normalization contract

Azure-specific provider errors should normalize into runtime-friendly classes while preserving Azure-relevant detail such as:

- auth/permission failure
- deployment not found or misnamed
- unsupported API version
- rate limiting / retry-after
- quota exhaustion
- content filtering / policy blocks where returned by provider
- transient upstream/service errors

Do not flatten all of that into a generic provider failure.

## 3.4 Document/OCR compatibility

The runtime must support Azure Document Intelligence as a native integration path for:

- OCR
- scanned PDFs
- mixed-document extraction
- structured field extraction where applicable

This should sit behind a document-understanding abstraction so:

- Azure Document Intelligence is first-class
- other OCR/document providers can still be added later
- workflow semantics remain stable even if the backend provider changes

## 3.5 Deployment compatibility

The runtime must deploy cleanly to Azure-hosted container environments including at least:

- Azure Container Apps
- AKS
- App Service for Containers
- Azure VM / VMSS with Docker or system service

Support expectations:

- environment-variable configuration
- file-mounted configuration/secrets where supported
- externalized durable state
- health endpoints for probes
- structured logs to stdout/stderr and optional file sinks
- graceful shutdown compatible with container orchestrators

## 3.6 Storage compatibility

The storage architecture must work in both:

1. local-first host or Docker deployments
2. Azure-hosted deployments with managed data/storage services

The runtime must not require cloud-only storage primitives for core behavior.

---

## 4. Design principles for Azure support

## 4.1 Preserve OpenClaw behavior first

Azure support must not change user-visible behavior around:

- sessions
- tools
- transcripts
- memory files
- cron/heartbeat
- approvals
- CLI/operator workflows

Infrastructure can vary.
Behavior cannot.

## 4.2 Separate logical storage domains from physical backends

The rewrite should define storage by domain, not by cloud product:

- operational metadata
- transcript/raw content
- memory documents
- media/attachments
- search indexes
- secrets references
- logs/exports/backups

Then map those domains to different backends depending on deployment mode.

This avoids accidental lock-in and makes local-first and Azure-hosted modes easier to keep behaviorally identical.

## 4.3 Prefer boring defaults

For parity-first implementation, the default stack should remain simple:

- PostgreSQL for operational state, with embedded Postgres fallback for zero-config local mode
- filesystem for durable raw content
- PostgreSQL FTS / pgvector for search and retrieval where planned

Azure-managed services should be supported as hosted deployment options, not made mandatory for correctness.

## 4.4 Do not hide critical state in container internals

Even in Azure-hosted mode, durable state must be clearly externalized:

- mounted persistent filesystem
- managed database
- object storage
- backup/export location

No critical state should depend on ephemeral writable container layers.

---

## 5. Azure configuration census

The rewrite must preserve equivalent Azure-relevant configuration coverage for at least:

### Model/provider config
- Azure resource endpoint / base URL
- Azure deployment name where the provider uses deployment-path routing
- Azure logical model name where the provider keeps model identity in the request body
- Azure API version where required by the provider surface
- Azure auth mode / header mode
- Azure API key or token reference
- optional Azure custom headers
- request timeout and retry policy
- logical model aliasing where needed

### Document understanding config
- Azure Document Intelligence endpoint
- Azure Document Intelligence auth material/reference
- Azure Document Intelligence API version where relevant
- model/classifier selection where exposed

### Deployment/hosting config
- health/readiness bind settings
- external base URL or gateway bind configuration
- mounted data/config/secret roots
- log/tracing/export settings suitable for Azure-hosted containers

### Secret/config sourcing
- env var bindings
- mounted secret file bindings
- Key Vault-backed injection/reference patterns

### Storage/backends
- PostgreSQL connection settings where used
- Azure Files-backed mounted path expectations
- Blob archive/bucket/container settings where used

Azure config coverage is parity-critical even when key names differ internally.

---

## 6. Azure data/storage research and recommendations

## 6.1 Storage domains

Think in these domains:

1. **Operational relational state**
   - sessions metadata
   - jobs/cron/reminders
   - approvals
   - channel state
   - tool execution records
   - config metadata
2. **Raw durable files**
   - transcripts if file-backed
   - memory markdown files
   - attachments/media
   - skills/plugins bundles
   - exports/backups
3. **Search/index data**
   - FTS indexes
   - semantic retrieval metadata
4. **Secrets/config references**
   - secret pointers, not necessarily raw secret material

Different Azure services fit different domains.

---

## 6.2 Azure Cosmos DB for NoSQL

### Strengths

- globally distributed managed document store
- flexible schema
- strong Azure integration story
- useful when modeling independent JSON/event documents at scale

### Weaknesses for this rewrite

- poor fit for parity-first local-first architecture
- does not map naturally to the runtime’s relational/transactional core
- increases divergence from the confirmed PostgreSQL-first architecture and embedded local fallback
- can encourage redesigning behavior around document-store constraints
- operational cost/model complexity is unnecessary for a single-operator or small deployment baseline

### Recommendation

Do **not** make Cosmos DB NoSQL the primary operational store for phase 1 or the default Azure hosted mode.

Possible later use cases:

- specialized event/archive workloads
- high-scale cloud-specific telemetry or denormalized views

But it should not define the core runtime model.

### Verdict

- **Local-first mode:** no
- **Hosted Azure mode:** optional niche use only
- **Primary recommendation:** not recommended for core parity runtime

---

## 6.3 Azure Cosmos DB for PostgreSQL

This is effectively a managed distributed PostgreSQL/Citus-style option.

### Strengths

- PostgreSQL semantics in Azure
- scale-out story for larger distributed workloads
- better fit than Cosmos NoSQL if the runtime remains relational

### Weaknesses for this rewrite

- complexity is unnecessary for parity-first personal/small-team deployment
- distributed Postgres is solving a scale problem the rewrite does not initially have
- adds operational assumptions that are not aligned with local-first symmetry

### Recommendation

Do not target Cosmos DB for PostgreSQL as the default hosted backend.

If a future enterprise/multi-tenant/server edition appears and scale-out Postgres becomes necessary, it can be evaluated then. For now it is premature.

### Verdict

- **Local-first mode:** no
- **Hosted Azure mode:** not default; maybe later enterprise scale target
- **Primary recommendation:** defer

---

## 6.4 Azure Database for PostgreSQL

### Strengths

- clean managed PostgreSQL option
- best Azure-managed relational candidate when the runtime needs server-grade DB beyond the embedded PostgreSQL local fallback
- strong fit for operator-facing relational queries, job state, approvals, channel state, and metadata
- relatively portable compared with Azure-specific NoSQL services
- lowest lock-in among major Azure managed database options considered here

### Weaknesses

- higher ops and cost than the embedded PostgreSQL local mode
- unnecessary for single-node local-first baseline
- requires deliberate schema/repository design to preserve parity between embedded/local and managed PostgreSQL deployments

### Recommendation

This is the best managed Azure database option for **hosted Azure mode** if the rewrite needs a managed relational store.

Use it for:

- operational metadata
- job/cron/reminder state
- approvals/audit tables
- channel/provider state
- search metadata pointers

Do **not** use it as a reason to eliminate filesystem-based memory/transcript layout if OpenClaw-style file semantics are part of compatibility goals.

### Verdict

- **Local-first mode:** embedded PostgreSQL fallback instead
- **Hosted Azure mode:** strong recommended option when moving beyond embedded local DB
- **Primary recommendation:** preferred managed relational backend

---

## 6.5 Azure SQL

### Strengths

- mature managed relational database in Azure
- excellent Azure enterprise posture
- familiar for organizations already standardized on SQL Server/Azure SQL

### Weaknesses for this rewrite

- weaker local-first symmetry with the embedded/local PostgreSQL path
- higher portability friction than PostgreSQL
- more likely to pull the runtime toward Azure/SQL Server-specific behavior
- less natural fit for a Rust local-first project when the baseline stack is PostgreSQL-first

### Recommendation

Support only if there is a strong external requirement from enterprise hosting environments.

It should **not** be the default target because:

- it complicates portability
- it increases lock-in risk
- Azure Database for PostgreSQL is a better fit for a parity-first runtime that may later run outside Azure

### Verdict

- **Local-first mode:** no
- **Hosted Azure mode:** optional enterprise-specific backend only
- **Primary recommendation:** not default

---

## 6.6 Azure Blob Storage

### Strengths

- best Azure-native fit for large binary/object data
- good for attachments, media, exports, backups, and archival artifacts
- aligns well with cloud-hosted container patterns where local disk is ephemeral
- relatively non-invasive if abstracted as object storage

### Weaknesses

- not a POSIX filesystem replacement
- poor fit for workflows that expect ordinary file mutation semantics
- can complicate direct workspace-style file handling if overused

### Recommendation

Blob Storage is a good fit in **hosted Azure mode** for:

- backup/export targets
- media/attachments at scale
- cold transcript/archive storage
- snapshot bundles and logs retention

It should not replace the canonical local workspace/memory file semantics unless the runtime explicitly introduces a consistent sync/export layer.
Do not treat Blob Storage as the canonical replacement for directories like `/data/memory` or `/config`.

### Verdict

- **Local-first mode:** no, use local mounted filesystem
- **Hosted Azure mode:** recommended for object/archive payloads
- **Primary recommendation:** yes for object storage, not as the primary workspace filesystem

---

## 6.7 Azure Files

### Strengths

- SMB/NFS-style shared filesystem semantics
- more natural lift for a mount-based state layout than Blob Storage
- usable with containerized workloads that need mounted persistent directories
- easiest Azure-native path when the application expects real files and directories

### Weaknesses

- slower and less elegant than local disk for hot-path database workloads
- not ideal as the primary database disk for embedded PostgreSQL under heavy concurrent write patterns
- may introduce latency for fine-grained filesystem-heavy operations

### Recommendation

Azure Files is the most relevant Azure-native option for preserving a mount-friendly OpenClaw-like state layout in hosted container deployments.

Good uses:

- `/data/memory`
- `/data/skills`
- `/data/logs`
- `/data/backups`
- `/config`
- `/data/sessions` if session files remain file-based
- `/data/media` if throughput is acceptable

Use caution for:

- `/data/db` when embedded PostgreSQL is expected to absorb heavy write bursts over network-backed storage
- high-churn search index paths
- write-heavy hot-path runtime metadata

For those, local node/container persistent disk or managed relational DB is usually better.

### Verdict

- **Local-first mode:** not applicable; use local bind mounts/volumes
- **Hosted Azure mode:** recommended for shared mount-style durable files
- **Primary recommendation:** yes for mount-friendly durable files, not ideal for all hot-path data

---

## 7. Recommended backend mapping by deployment mode

## 7.1 Local-first mode

This should remain the reference behavior and easiest development/testing path.

### Recommended stack

- **Operational DB:** embedded PostgreSQL via `postgresql_embedded`
- **Durable files:** local filesystem / Docker bind mounts or named volumes
- **Search:** PostgreSQL FTS + pgvector when vector retrieval is enabled
- **Media/attachments:** local mounted filesystem
- **Memory docs:** local mounted filesystem
- **Secrets:** env vars, local secret files, or OS-native secret store

### Why

- best parity with OpenClaw-style local operation
- lowest complexity
- easiest to reason about and back up
- strongest portability
- best for preserving identical behavior across bare-metal and Docker

### What not to use by default

- Cosmos DB NoSQL
- Cosmos DB for PostgreSQL
- Azure SQL
- Blob Storage for canonical workspace state

---

## 7.2 Hosted Azure mode — conservative parity-first

This mode should preserve the same logical layout while adopting managed Azure services only where they clearly help.

### Recommended stack

- **Operational DB:** embedded PostgreSQL on reliable persistent volume for conservative single-instance mode, or Azure Database for PostgreSQL if managed DB is preferred
- **Mount-friendly durable files:** Azure Files or persistent volumes
- **Object/archive storage:** Azure Blob Storage
- **Secrets:** Azure Key Vault references or injected env/file secrets
- **Search:** keep embedded first unless scale proves otherwise

### Best fit when

- one primary runtime instance
- parity is more important than cloud-native redesign
- file layout compatibility matters
- Docker/container deployment should remain simple

### Notes

If the embedded PostgreSQL fallback is used in hosted Azure mode, place it on reliable persistent storage with clear backup expectations. Network-backed storage for embedded PostgreSQL still needs validation for latency, fsync behavior, backup posture, and restart semantics. For stronger multi-instance or server-grade durability, Azure Database for PostgreSQL is the better managed step up.

---

## 7.3 Hosted Azure mode — managed relational backend

### Recommended stack

- **Operational DB:** Azure Database for PostgreSQL
- **Memory/session/media files:** Azure Files for mount-style paths where real filesystem semantics matter
- **Archive/media/backups:** Azure Blob Storage
- **Secrets:** Azure Key Vault

### Best fit when

- managed DB operations matter more than local symmetry
- HA/backup/restore posture needs to be cleaner
- future scale beyond a single embedded DB is expected

### Tradeoff

This slightly reduces local/hosted symmetry but preserves behavioral parity if the runtime keeps the same logical storage domains and path semantics where needed.

---

## 8. Azure-hosted persistent storage patterns for Docker/container deployments

## 8.1 Single-container, mounted-state pattern

Use when parity and simplicity matter most.

Pattern:

- one main runtime container
- persistent mounted directory for durable file domains
- optional managed DB or embedded DB
- secrets via env/Key Vault/file mounts

Best Azure fit:

- Azure Container Apps with Azure Files where supported/appropriate
- AKS with Azure Files or Azure Disk-backed persistent volumes
- VM with Docker + host-mounted disk

This is the best early hosted pattern because it keeps the same mental model as local Docker.

## 8.2 Split-state pattern

Pattern:

- managed relational DB for operational metadata
- mounted filesystem for workspace-like durable files
- object storage for archives and large media

This is likely the best long-term Azure-hosted pattern.

Mapping:

- operational metadata -> Azure Database for PostgreSQL
- memory/session files/skills/logs -> Azure Files or persistent volume
- backups/media archives -> Azure Blob Storage

## 8.3 Do not rely on ephemeral writable layers

Never store critical runtime state only in:

- container writable layer
- `/tmp`
- orchestrator-local ephemeral scratch space

Ephemeral-only is acceptable for:

- temp downloads
- transient model payload staging
- caches that can be rebuilt
- compaction scratch files

---

## 9. Canonical Azure-compatible state model

Regardless of backend, the runtime should preserve these logical domains:

- `/data/db`
- `/data/sessions`
- `/data/memory`
- `/data/media`
- `/data/skills`
- `/data/logs`
- `/data/backups`
- `/config`
- `/secrets`

In Azure-hosted mode, these may map to:

- managed database instead of `/data/db`
- Azure Files-backed mounts for file domains
- Blob Storage for backup/archive replication

But the logical contract should stay stable.

---

## 10. What should stay local/file-based for parity

To stay closest to OpenClaw behavior, the following should remain file-oriented even in hosted mode unless there is a very strong reason to change:

- workspace memory documents
- operator-managed config overlays
- installed prompt/skill bundles
- exports/debug bundles
- human-readable logs where enabled

These are easier to inspect, back up, diff, and migrate when they stay ordinary files.

---

## 11. What can move to managed Azure services without breaking parity

These can move to managed Azure services more safely if the abstraction boundary is clean:

- operational relational state -> Azure Database for PostgreSQL
- backup/archive payloads -> Azure Blob Storage
- secrets source -> Azure Key Vault
- observability export -> Azure Monitor / OpenTelemetry collectors

The user-visible runtime behavior should remain unchanged.

---

## 12. Azure hosting guidance

Preferred hosting patterns in order:

1. Azure VM / VMSS with Docker and mounted persistent disk when maximum control and filesystem parity matter most
2. Azure Container Apps for a simpler managed container story when mount/storage constraints are satisfied
3. AKS when broader orchestration or surrounding platform requirements justify it
4. App Service for Containers only when its hosting model aligns cleanly with mount/state needs

The selection should be driven by storage and operability constraints, not by prestige.

### Hosting contract requirements

Whichever Azure host is chosen, the deployment must support:

- injected env/config/secrets without image rebuild
- persistent durable state mapping matching the logical layout
- readiness/liveness probing
- graceful shutdown and restart recovery
- structured stdout/stderr logging
- optional file-backed logs and diagnostic exports

---

## 13. Decision matrix

## 13.1 Primary storage/database decisions

These are the planning-default decisions unless later evidence forces a revision.

### Default local-first mode

- operational metadata: **embedded PostgreSQL**
- durable human-visible files: **mounted local filesystem**
- object/archive payloads: **mounted local filesystem**, optionally copied elsewhere
- search: **PostgreSQL FTS first**, **pgvector when semantic retrieval is enabled**

### Default hosted Azure mode

- operational metadata: **Azure Database for PostgreSQL** when managed relational storage is desired; **embedded PostgreSQL on persistent volume** only for conservative single-instance deployments
- mount-style durable file domains: **Azure Files**
- archive/backup/large-object payloads: **Azure Blob Storage**
- secrets source: **Azure Key Vault** or equivalent secret injection path

### Explicit non-defaults

Not recommended as primary defaults for the parity-first architecture:

- Cosmos DB NoSQL
- Cosmos DB for PostgreSQL
- Azure SQL

Reason: they either distort the local-first parity path, add unnecessary complexity, or increase lock-in without solving the main early problem.

---

## 14. Evidence required before claiming Azure parity

Before claiming Azure compatibility, capture black-box evidence for:

- request construction with endpoint + deployment + API version separation
- auth/header behavior for Azure-specific provider access
- error normalization for common Azure failure classes
- Document Intelligence request/response path
- local Docker and Azure-hosted deployment using the same logical durable-state model
- secret/config injection pattern suitable for Azure-hosted containers
- readiness/health behavior in Azure-style hosting environments

---

## 15. Recommendations summary

## Default local-first recommendation

Use:

- embedded PostgreSQL
- local mounted filesystem
- PostgreSQL-backed search

Do not make Azure services mandatory for correctness.

## Default hosted Azure recommendation

Use:

- Azure Database for PostgreSQL for operational metadata when managed DB is desired
- Azure Files for mount-friendly durable directories
- Azure Blob Storage for archives, backups, and large object payloads
- Azure Key Vault for secrets in managed environments

Avoid as defaults:

- Cosmos DB NoSQL
- Cosmos DB for PostgreSQL
- Azure SQL

## Rule of thumb

- if the subsystem behaves like relational operational metadata -> PostgreSQL
- if it behaves like human-visible files/directories -> Azure Files or local mounted filesystem
- if it behaves like large object/archive storage -> Blob Storage
- if local-first mode is enough -> keep PostgreSQL (embedded when appropriate) + mounted filesystem

This gives strong Azure compatibility without turning the architecture into an Azure-only system.

---

## 16. Decision guidance

For the planning phase, the safest architecture choice is:

1. design around local-first logical storage domains
2. make all durable paths mount-friendly
3. support embedded PostgreSQL as the baseline operational store
4. define Azure Database for PostgreSQL as the primary managed relational upgrade path
5. treat Azure Files and Blob Storage as hosted deployment mappings, not core behavioral dependencies
6. treat Azure request semantics and Document Intelligence as release-blocker parity surfaces

That keeps the rewrite close to OpenClaw, fully Azure-compatible, and operationally sane.
