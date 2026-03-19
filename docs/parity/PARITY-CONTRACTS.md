# Parity Contracts Matrix

This document turns the parity goal into implementation-grade contracts.

It is intentionally stricter than the broad planning docs.
If a subsystem cannot satisfy the contract here, it is not parity-complete.

Hard constraints remain:

- functional parity with OpenClaw
- full Azure compatibility where provider/deployment/storage integrations matter
- Docker-first deployment with mountable persistent storage

---

## 1. How to use this document

Each subsystem below defines:

- contract scope
- invariants that may not regress
- required persisted state
- external surfaces
- failure behavior expectations
- minimum evidence needed to claim parity

This is the release-gating view, not the aspirational architecture view.

---

## 2. Contract status values

Use these status values during implementation planning/execution:

- `unspecified` — contract not yet frozen
- `specified` — contract written but not yet tested
- `implementing` — implementation in progress
- `evidence-partial` — some black-box evidence exists
- `parity-complete` — behavior, failure paths, and evidence all present
- `known-divergence` — intentionally different, explicitly documented

Do not mark a subsystem parity-complete without black-box evidence.

---

## 3. Gateway and control plane

### Scope

- daemon lifecycle
- HTTP API
- WebSocket event streaming
- auth/token handling
- health/status/diagnostics
- operator control path for sessions, jobs, approvals, logs

### Invariants

- operator can determine whether the runtime is healthy, reachable, and authenticated
- daemon state is inspectable without reading arbitrary internal files
- long-running work remains inspectable after the triggering request ends
- restart does not orphan durable sessions/jobs/processes silently

### Required persisted state

- daemon instance metadata where applicable
- auth token metadata or references
- process/session/job linkage metadata
- durable event/log references or retained summaries

### External surfaces

- CLI-facing HTTP control API
- WebSocket subscriptions for live events
- health/status endpoints suitable for probes and operators

### Failure behavior expectations

- auth failure must be explicit and machine-readable
- unavailable daemon must be distinguishable from invalid credentials
- reconnect/restart must preserve durable IDs and inspectable state

### Minimum parity evidence

- start/stop/status golden workflows
- auth success/failure tests
- WS subscribe/reconnect/replay tests
- process/job inspection after daemon restart

---

## 4. Session runtime

### Scope

- session creation and lifecycle
- turn execution
- transcript ordering
- context assembly
- compaction/pruning behavior
- usage/cost accounting

### Invariants

- one authoritative transcript ordering per session
- child sessions retain requester linkage
- context assembly is explainable and reproducible enough for debugging
- compaction cannot silently remove facts that materially change future behavior
- tool and approval actions remain attributable to the originating turn

### Required persisted state

- sessions
- turns
- transcript items
- compaction artifacts or summaries
- usage/accounting summaries
- parent/child linkage

### External surfaces

- session/turn APIs
- transcript inspection
- session status APIs and CLI output
- live turn progress events

### Failure behavior expectations

- failed turns are visible as failed, not silently dropped
- cancellation/retry state is attributable and inspectable
- partial tool progress should not corrupt transcript ordering

### Minimum parity evidence

- create session -> run turn -> persist transcript golden tests
- transcript ordering under tool loop tests
- failure/cancel/retry lifecycle tests
- compaction behavior tests using future-turn replay comparison

---

## 5. Tool system and approvals

### Scope

- tool registration and schema validation
- approval gating
- file/process/session/memory tool semantics
- background execution handles
- process management lifecycle

### Invariants

- tool names are compatibility surface
- argument validation is deterministic and operator-visible
- approval binds to exact payload/command shown to the user
- allow-once does not expand scope
- background work remains inspectable and controllable

### Required persisted state

- tool call records
- approval requests and decisions
- process/session handles for background work
- tool-to-turn/session linkage

### External surfaces

- tool invocation APIs
- approval APIs/UI/CLI flows
- process polling/log/write/kill APIs

### Failure behavior expectations

- denied approvals produce durable audit trail
- exact-match edit failures remain exact-match failures
- background process loss/restart behavior is explicit, not silent

### Minimum parity evidence

- read/write/edit exactness tests
- approval allow-once/allow-always/deny tests
- exec foreground-to-background transition tests
- process poll/log/send-keys/kill tests
- restart visibility for background handles

---

## 6. Scheduler, reminders, and heartbeats

### Scope

- cron jobs
- one-shot reminders
- wake events
- heartbeat-driven checks
- isolated scheduled agent runs
- run history and next-run calculation
- scheduled payload and delivery semantics

### Invariants

- enabled/disabled state is durable
- schedule definition is compatibility surface: `at`, `every`, and cron-expression semantics may not drift silently
- `sessionTarget=main` maps to `systemEvent` payloads and `sessionTarget=isolated` maps to `agentTurn` payloads; invalid combinations fail explicitly
- reminder timing and wording remain operator-predictable
- delivery modes preserve `none`, `announce`, and `webhook` semantics
- heartbeat quiet/no-op semantics are preserved
- repeated heartbeats do not spam without new cause
- isolated scheduled runs remain auditable as isolated runs

### Required persisted state

- jobs
- schedules/due times
- session target and payload kind
- delivery mode and wake mode
- last/next run metadata
- run history with due/manual trigger visibility
- delivered/missed/cancelled status for reminders
- heartbeat anti-spam state sufficient to suppress duplicate notifications across restarts

### External surfaces

- cron APIs/CLI
- reminder APIs/CLI
- heartbeat status/enable/disable APIs and CLI workflows
- run-now, wake, and inspection workflows
- history views

### Failure behavior expectations

- missed runs are visible
- invalid schedules fail explicitly
- invalid `sessionTarget`/payload combinations fail explicitly
- disabled jobs do not run silently
- reminder delivery failures resolve to explicit `missed` outcomes rather than disappearing
- heartbeat no-op and duplicate suppressions remain operator-inspectable even when no outbound message is emitted

### Minimum parity evidence

- cron create/list/update/disable/enable/run/wake tests
- due-only vs forced-run history tests
- `systemEvent` vs `agentTurn` session-target tests
- delivery-mode tests for `none`/`announce`/`webhook`
- reminder due/delivered/missed/cancelled tests
- heartbeat no-op, notify, and duplicate-suppression tests

---

## 7. Memory and retrieval

### Scope

- workspace memory discovery
- daily notes and curated long-term memory
- retrieval/search
- safe snippet extraction
- memory update flows
- privacy boundaries by session type

### Invariants

- files intended only for main session must not leak into shared/external contexts
- retrieval results are attributable to source path and bounds
- memory conventions remain file-oriented and human-inspectable
- memory updates preserve the curated-vs-daily distinction

### Required persisted state

- memory files themselves
- indexing/search metadata
- retrieval audit or trace metadata if needed for debugging

### External surfaces

- memory search/get APIs
- memory file update flows
- context assembly integration

### Failure behavior expectations

- unavailable retrieval backend must fail clearly
- privacy-filtered omissions must not be misrepresented as absence of data

### Minimum parity evidence

- main-session vs shared-session visibility tests
- retrieval attribution tests
- snippet-boundary tests
- memory update convention tests

---

## 8. Channels and messaging adapters

### Scope

- normalized inbound events
- normalized outbound actions
- reply/edit/react behavior
- attachment/media references
- direct vs group routing
- provider setup/auth lifecycle

### Invariants

- provider-specific quirks stay inside adapters unless explicitly surfaced
- reply targeting remains correct
- group/direct participation boundaries remain correct
- duplicate inbound events are deduplicated idempotently
- outbound delivery retains provider message references for edits/replies/reactions

### Required persisted state

- normalized inbound event records or dedupe keys
- outbound delivery records
- provider message IDs and routing metadata
- channel auth/setup state

### External surfaces

- channel provider adapter interface
- channel status/health APIs
- outbound send/reply/edit/react APIs

### Failure behavior expectations

- transient provider failures are distinguishable from permanent failures
- unsupported features fail cleanly by capability
- retries do not duplicate user-visible messages unless provider itself makes that unavoidable and it is documented

### Minimum parity evidence

- Telegram direct/group/reply tests first
- dedupe tests
- outbound edit/react tests where supported
- attachment ingress/egress tests

---

## 9. Media and document understanding

### Scope

- inbound audio transcription
- image understanding handoff
- attachment normalization
- TTS outputs
- Azure Document Intelligence integration path for OCR/document workflows

### Invariants

- media objects have durable references and traceable lifecycle
- OCR/document understanding does not require Azure, but Azure path must be first-class and real
- attachment handling remains inspectable and attributable to session/turn

### Required persisted state

- media metadata
- durable media/document file references
- extraction artifacts and summaries where persisted

### External surfaces

- media processing APIs
- document-understanding provider abstraction
- TTS/transcription provider abstraction

### Failure behavior expectations

- large/unsupported attachments fail explicitly
- transcription/OCR provider errors are normalized and inspectable

### Minimum parity evidence

- voice note -> transcription flow tests
- image attachment handoff tests
- Azure Document Intelligence request/response normalization tests
- TTS generation and delivery tests where configured

---

## 10. Skills and extension model

### Scope

- prompt/resource skills
- selection rules
- installation/update metadata
- native/out-of-process extension manifests and capability declarations

### Invariants

- only the clearly most-specific applicable skill auto-loads up front under the documented rule
- skill loading remains explainable to operator
- resource resolution is deterministic
- executable extensions fail in isolation

### Required persisted state

- skill manifests/metadata
- installed asset/resource bundles
- extension capability declarations
- installation/update records

### External surfaces

- skill discovery/load APIs
- install/update/inspect workflows
- extension manifest validation surfaces

### Failure behavior expectations

- invalid skill manifests fail before runtime ambiguity
- missing resources fail with actionable path information
- extension crashes/timeouts remain isolated

### Minimum parity evidence

- zero/one/multiple-skill selection tests
- resource path resolution tests
- install/update metadata tests
- extension timeout/isolation tests

---

## 11. Storage and deployment contract

### Scope

- local-first persistent state layout
- Docker-first deployment
- mountable persistent storage
- Azure-hosted mappings for DB/files/object storage

### Invariants

- no critical durable state exists only in ephemeral container layers
- same logical paths exist across host and container modes
- local-first mode remains reference behavior
- Azure support is real but optional, not architecture-capturing

### Required persisted state domains

- `/data/db`
- `/data/sessions`
- `/data/memory`
- `/data/media`
- `/data/skills`
- `/data/logs`
- `/data/backups`
- `/config`
- `/secrets`

### External surfaces

- deployment docs and config model
- health/readiness endpoints
- backup/restore workflows

### Failure behavior expectations

- missing mounts/config fail fast and clearly
- read-only or degraded storage modes surface explicit errors
- backup/restore workflows are documented and testable

### Minimum parity evidence

- local Docker deployment with mounted state
- restart durability tests
- PostgreSQL-backed Azure-hosted mode tests
- Azure Files/Blob mapping validation for intended domains

---

## 12. Azure compatibility contract

### Scope

- Azure OpenAI / Azure AI Foundry compatibility
- Azure deployment naming semantics
- Azure auth/header/version handling
- Azure Document Intelligence integration path
- Azure-friendly container/storage/secret patterns

### Invariants

- Azure support is not simulated by generic OpenAI-compatible requests
- deployment identity remains first-class in config and request construction
- API-version and endpoint handling are explicit
- Azure storage/service mappings do not break local-first parity

### Required persisted/config state

- provider endpoint config
- deployment names
- API version settings
- auth/secret references
- optional per-provider headers and retry policy

### External surfaces

- provider config schema
- request construction and error normalization
- deployment docs for Azure-hosted modes

### Failure behavior expectations

- Azure auth/config errors are distinguishable from generic provider failure
- unsupported API-version combinations fail explicitly

### Minimum parity evidence

- request snapshot tests
- Azure error normalization tests
- deployment-name vs model-name config parsing tests
- Document Intelligence integration tests

---

## 13. Evidence checklist template

Use this template per subsystem during execution:

- Contract status:
- Known divergences:
- Golden traces captured:
- Success-path tests implemented:
- Failure-path tests implemented:
- Restart/reconnect tests implemented:
- Azure-specific tests implemented:
- Operator workflow signoff:

---

## 14. Release rule

The rewrite cannot honestly be described as functionally identical until every parity-critical subsystem is either:

- `parity-complete`, or
- `known-divergence` with an explicit approved compatibility exception

If evidence is partial, the claim must remain: parity-seeking, not parity-complete.
