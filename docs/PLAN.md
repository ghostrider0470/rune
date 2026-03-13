# Plan — OpenClaw Rust Rewrite

## 1. Objective

Build a Rust-based system that replicates OpenClaw’s practical functionality.

Non-negotiable constraints:

- functionally identical behavior to OpenClaw
- full Azure compatibility across model/runtime/infrastructure integrations
- Docker-first deployment with mountable persistent storage

Build a Rust-based system that replicates OpenClaw’s practical functionality:

- multi-channel personal assistant runtime
- agent/session orchestration
- gateway daemon + control plane
- tool execution and approvals
- cron/heartbeat automation
- memory and retrieval
- media understanding and TTS flows
- skill/plugin system
- frontend/admin UI

Not the goal:

- line-by-line source compatibility
- preserving TypeScript internals
- implementing everything in one phase

Goal is behavioral parity first, then performance and maintainability gains.

---

## 2. Rewrite principles

1. **Protocol-first**
   - define stable internal/external contracts before implementation
2. **Daemon-first**
   - central Rust gateway/service is the core runtime
3. **Async everywhere**
   - use structured concurrency and bounded work queues
4. **Isolate untrusted extensions**
   - skills/plugins should run with explicit capability boundaries
5. **State machines over ad hoc orchestration**
   - channels, sessions, approvals, cron runs, and background jobs should be modeled explicitly
6. **Parity before optimization tricks**
   - preserve expected behavior first, then optimize hot paths
7. **Frontend decoupled from runtime**
   - UI should talk to stable APIs, not runtime internals

---

## 2.1 Azure compatibility requirement

Azure compatibility must be treated as a hard requirement, not a nice-to-have.

That includes:

- Azure OpenAI / Azure AI Foundry compatible model providers
- Azure-style endpoint/auth/header handling
- Azure deployment naming and provider config semantics
- Azure Document Intelligence / OCR integration paths
- Azure storage-friendly deployment and secret handling
- Azure-hosted or Azure-adjacent container deployment patterns
- future compatibility with Azure-native observability/auth/secrets where needed

Do not design the runtime around only generic OpenAI-compatible assumptions if that breaks Azure-specific behavior.

## 3. What must be replicated

## 3.1 Core runtime

- agent runtime and turn loop
- session lifecycle
- context assembly
- transcript persistence
- model invocation abstraction
- tool-call handling
- compaction / pruning / usage accounting

## 3.2 Gateway / daemon

- long-running local service
- WS + HTTP control surface
- health/status endpoints
- auth/token support
- discovery/pairing concepts
- supervised background processes
- remote/local gateway modes

## 3.3 CLI

Need command parity at practical level for:

- top-level command census, global flags/options, and operator mental model
- status / dashboard
- gateway and daemon lifecycle
- sessions
- cron
- channels
- skills / plugins / hooks / webhooks
- config / configure / secrets / security / system / sandbox
- doctor / health / logs / update / backup / reset
- message routing / hooks / browser / ACP adjacencies where operators rely on them
- approvals / devices / pairing / node and nodes workflows
- completion / docs / setup / onboard ergonomics where they materially affect adoption and operability

The command-family source of truth is `PARITY-INVENTORY.md`, which should be treated as the explicit census rather than a rough category list.

## 3.4 Channels / messaging providers

At architecture level, support for provider adapters similar to OpenClaw’s channel model:

- Telegram
- Discord
- WhatsApp
- Signal
- Slack
- Teams
- additional adapters later

Need shared abstractions for:

- inbound normalization
- outbound send/edit/reply/react
- media ingress
- typing/presence
- auth/pairing/setup
- per-channel routing semantics

## 3.5 Agent / multi-agent orchestration

- main agent session
- isolated runs
- sub-agent spawning
- ACP-style external coding-agent harness support or compatible replacement
- session-to-session messaging
- task fan-out and result collection

## 3.6 Tools system

Need first-class runtime tools equivalent in concept to:

- read / write / edit
- exec / process
- cron
- sessions_*
- memory_*
- session_status
- possibly browser/media tools later

Tool layer needs:

- schemas
- auth/capability gating
- approval hooks
- streaming / background task support
- auditability

## 3.7 Memory

- workspace memory files
- long-term memory vs daily memory
- semantic search layer
- optional local embeddings / remote embeddings
- safe snippet retrieval
- memory update workflows

## 3.8 Automation

- cron jobs
- one-shot reminders
- isolated scheduled agent runs
- heartbeat-driven periodic checks
- wake events and notifications

## 3.9 Media

- inbound audio transcription
- image understanding pass-through to models
- optional video/media pipeline later
- TTS replies
- attachment handling

## 3.10 Skills / extensions

Need a new Rust-native extension model that still captures OpenClaw’s usability:

- metadata-triggered skills
- instruction bundles
- resource bundles
- executable helpers
- install/update/distribution path

This should likely split into two layers:

1. **Prompt/knowledge skills**
   - markdown metadata + references/assets
2. **Native runtime plugins**
   - Rust or WASI modules implementing tools/providers/processors

## 3.11 UI / frontend

Need Horizon Tech-style frontend direction, but exact visual system can come later.

UI must cover:

- dashboard/status
- session browser
- logs/events
- channel health
- cron/jobs
- skills/plugins
- config/secrets
- approvals
- memory inspection

---

## 4. Proposed target architecture

## 4.1 High-level components

1. **rune-core**
   - domain types, events, IDs, state machines, policies
2. **rune-runtime**
   - agent loop, sessions, tools, background jobs
3. **rune-gateway**
   - daemon, WS/HTTP APIs, auth, health, control plane
4. **rune-cli**
   - operator CLI
5. **rune-channels**
   - provider adapters
6. **rune-models**
   - provider abstraction for OpenAI/Anthropic/Azure/etc.
7. **rune-memory**
   - memory files, indexing, retrieval
8. **rune-media**
   - transcription / TTS / attachment processing
9. **rune-skills**
   - skill loader + native plugin runtime
10. **rune-ui-api**
   - backend-facing API for frontend
11. **frontend app**
   - Horizon-style web UI

## 4.2 Internal design style

Use event-driven architecture with explicit persistent stores for:

- sessions
- messages/transcripts
- jobs/cron runs
- tool executions
- approvals
- channel state
- skills/plugins
- memory index metadata

Use async message passing between subsystems where it reduces lock contention.

The important constraint is not "everything must be event-sourced." The constraint is that every parity-critical transition must be durable, attributable, and inspectable.

Design bias:

- use explicit domain state machines for sessions, turns, jobs, approvals, deliveries, and processes
- persist canonical current state plus enough event/audit history to reconstruct behavior
- prefer boring durable stores over clever in-memory orchestration
- make restart behavior a design input, not a later bugfix

---

## 4.3 Canonical durable state domains

Regardless of physical backend, the runtime should preserve these logical state domains:

- operational relational state
  - sessions
  - turns
  - jobs and run history
  - approvals
  - tool executions
  - process handles
  - channel delivery metadata
  - provider/channel setup state
- human-visible file state
  - memory markdown files
  - config overlays
  - installed skill/resource bundles
  - exports/diagnostic bundles
  - optional transcript/session file artifacts
- binary/object state
  - inbound attachments
  - generated TTS/audio artifacts
  - media caches worth persisting
  - backup archives
- search/index state
  - FTS indexes
  - retrieval metadata
  - optional embedding/vector metadata

This logical split matters because Docker deployment, Azure mapping, and migration strategy should preserve these domains even when the physical backend changes.

---

## 5. Functional rollout phases

The sequencing source of truth is `docs/IMPLEMENTATION-PHASES.md`.
This section is the compact summary only.

## Phase 0 — planning / spec

- build and maintain the full OpenClaw parity inventory
- freeze protocol and entity/data model assumptions
- decide storage, plugin, and UI architecture

## Phase 1 — daemon/control plane skeleton

- gateway + daemon lifecycle
- config + secrets foundations
- auth/token model
- health/status/logging envelopes
- durable IDs and entity stores

## Phase 2 — session runtime minimum parity

- sessions
- turn lifecycle
- transcript persistence/order
- model abstraction
- usage/accounting/status surfaces

## Phase 3 — tools + approvals

- read/write/edit/exec/process parity
- approval lifecycle
- background inspectability

## Phase 4 — automation + memory

- cron
- heartbeat engine
- memory store + semantic retrieval
- reminders and wake semantics

## Phase 5 — channels

- Telegram first
- then Discord / WhatsApp / Signal based on need
- media attachments and reply semantics

## Phase 6 — skills/plugins

- prompt skills
- Rust-native tool plugins
- install/update/registry workflow

## Phase 7 — advanced parity sweep

- sub-agents
- coding harness integration
- richer media/TTS/OCR breadth
- doctor/health/reporting/diagnostics parity
- broader command-family parity

---

## 5.1 Deployment constraint

Assume containerized deployment is a first-class target.

That means the runtime must work cleanly in Docker with explicit mounted persistent storage for:

- database files
- transcripts/session files
- memory files
- media/attachments
- plugin/skill bundles
- logs/exports/backups
- config overrides and secret references

Design implication:
- never hide critical state in opaque container internals
- keep durable runtime state on mountable filesystem paths
- make local bare-metal and Docker deployments share the same logical directory layout

## 6. Migration strategy

## 6.1 Compatibility mode

Early on, support importing or mirroring:

- workspace structure
- AGENTS/USER/SOUL/MEMORY conventions
- session transcript shapes where possible
- similar CLI verbs

## 6.2 Parallel-run strategy

Best migration path:

1. keep OpenClaw live
2. build Rust daemon in parallel
3. test one channel/provider at a time
4. compare outputs and operator workflows
5. cut over only after subsystem parity is acceptable

---

## 7. Biggest risks

- channel adapter complexity and provider churn
- exact session/tool semantics parity
- reproducing OpenClaw’s scheduling/orchestration behavior cleanly
- plugin/skill safety without killing ergonomics
- frontend scope creep
- overcommitting to perfect parity instead of practical parity

---

## 8. Recommended immediate next planning steps

1. keep `PARITY-INVENTORY.md` as the command/resource/event source of truth and continue filling any remaining breadth gaps explicitly rather than implicitly
2. freeze the protocol/domain model in `PROTOCOLS.md` down to entity states, HTTP resource families, WS event families, approval semantics, config/setup mutation flows, and doctor contracts
3. define plugin/skill architecture in more detail
4. lock the storage/database posture consistently across all docs: PostgreSQL-first operational state, embedded Postgres fallback, file-oriented mounted durable state
5. choose frontend contract and auth model
6. define first implementation milestone: daemon + sessions + tools + status + doctor parity harness
