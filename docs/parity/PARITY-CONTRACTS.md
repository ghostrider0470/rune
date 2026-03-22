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
- `next_run_at` is derived scheduler state, not sticky metadata: create, schedule-edit, disable, and re-enable transitions must clear or recompute the executable next-fire time from the current job contract rather than preserving stale cadence
- `sessionTarget=main` maps to `systemEvent` payloads and `sessionTarget=isolated` maps to `agentTurn` payloads; invalid combinations fail explicitly
- reminder timing and terminal outcomes remain operator-predictable; reminder `target` routes execution: `"main"` delivers through the stable scheduled main session, `"isolated"` creates a one-shot subagent session under it; unknown targets fall back to `"main"` with a warning
- delivery modes are executable runtime behavior: `announce` broadcasts a `cron_run_completed` event via the session event channel, `webhook` POSTs the job result payload to the configured URL (30 s timeout, no retry), and `none` suppresses outbound delivery; all three modes remain durable and inspectable as job metadata
- due jobs and reminders are claimed atomically before execution via `claimed_at`; stale claims older than the configured lease duration (default 300 s) expire and become reclaimable for crash recovery; concurrent supervisor ticks cannot duplicate execution
- heartbeat no-op and fingerprint-based duplicate-suppression semantics are preserved; persisted suppression state survives restart well enough to avoid replaying the same notification solely because the process restarted; broader quiet-window policy is still follow-on work
- repeated heartbeats do not spam without new cause within the shipped no-op/duplicate-suppression contract
- `sessionTarget=isolated` cron jobs remain auditable as isolated descendant runs; main-target cron jobs, reminders, and heartbeats reuse scheduled session contexts
- wake requests preserve explicit `now` vs `next-heartbeat` mode selection on the operator/event surface; durable wake execution is not yet a shipped contract

### Required persisted state

- jobs
- schedules/due times
- session target and payload kind
- delivery mode and webhook URL
- last/next run metadata
- claim/lease state (`claimed_at`) for atomic due-work acquisition
- run history with due/manual trigger visibility
- delivered/missed/cancelled status for reminders
- heartbeat anti-spam state sufficient to suppress duplicate notifications across restarts

### External surfaces

- cron APIs/CLI, including inspection and wake/system-event queueing, with any narrower-than-gateway CLI create/edit surface called out explicitly
- reminder APIs/CLI, including terminal-outcome inspection for delivered/missed/cancelled reminders
- heartbeat status/enable/disable APIs plus CLI presence/last/status workflows
- run-now, wake, and inspection workflows
- history views

### Failure behavior expectations

- missed runs are visible
- invalid schedules fail explicitly; stored cron jobs whose next run can no longer be computed disable rather than fabricating future fire times
- schedule edits and disable/re-enable transitions must not preserve stale `next_run_at`; if a valid next fire cannot be derived, inspection should surface `next_run_at=null` or a disabled job rather than silently keeping the old cadence
- invalid `sessionTarget`/payload combinations fail explicitly
- disabled jobs do not run silently
- reminder delivery failures resolve to explicit `missed` outcomes rather than disappearing
- invalid wake modes fail explicitly
- heartbeat no-op and duplicate suppressions remain operator-inspectable even when no outbound message is emitted

### Minimum parity evidence

- cron create/list/show/update/disable/enable/run/wake tests
- schedule-edit recompute plus disable/re-enable `next_run_at` transition tests
- due-only vs forced-run history tests
- `systemEvent` vs `agentTurn` session-target tests
- delivery-mode execution tests: `announce` event broadcast, `webhook` POST, `none` suppression
- durable claim/lease tests: atomic claim, stale-claim reclaim, release-and-reclaim, concurrent duplicate suppression
- reminder due/delivered/missed/cancelled and target-routing tests
- reminder target routing tests: `"main"` vs `"isolated"` session creation
- wake mode normalization/event-payload tests
- heartbeat instruction-loading, no-op, notify, and duplicate-suppression persistence tests

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
- recovery expectations are explicit: restore recovers durable operator-visible state, not hidden image-layer state or undocumented live runtime handles

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

Local-first mode uses `~/.rune/` as the equivalent root (e.g. `~/.rune/db/` ↔ `/data/db`).
See [DEPLOYMENT.md §5.1](../operator/DEPLOYMENT.md#51-local--docker-path-equivalence) for the full mapping table and fail-fast contract.

### External surfaces

- deployment docs and config model
- health/readiness endpoints
- backup/restore workflows
- operator runbook for backup, restore, and post-restore verification

### Failure behavior expectations

- missing mounts/config fail fast and clearly
- read-only or degraded storage modes surface explicit errors
- backup/restore workflows are documented and testable (see [PROTOCOLS.md §15.4](PROTOCOLS.md#154-backup-and-restore-workflow-contract) for the full contract and target CLI workflow spec)
- the current shipped recovery interface is explicit even before the dedicated `rune backup` CLI lands
- degraded-recovery cases are declared explicitly instead of being discovered during an incident

#### Read-only filesystem detection

Writability is verified by write-probe (create + delete temp file), not by metadata/permission-bit inspection. This catches bind-mount RO, UID mismatch, SELinux/AppArmor denials, and filesystem-level RO mounts.

All three detection surfaces must agree:

- `rune doctor` CLI — Fail status per unwritable path, mode-aware fix hint
- startup validation (`validate_paths`) — exit with clear error naming the path
- gateway `POST /api/doctor/run` — per-path writability findings in response

Unwritable required paths are Fail, never Warn. Silent fallback to ephemeral storage is never acceptable.

#### Secrets-never-logged contract

Secret **values** must never appear in: log output (any level), error messages, status/health/doctor responses, WebSocket payloads, transcript items, or diagnostic bundles.

Scope: provider API keys, channel tokens, database credentials, certificate key material, any value from `/secrets` or secret-reference config fields.

Secret **references** (key names, vault paths, env var names) may appear in diagnostics. Values may not.

Violation is a release-blocking defect.

See [PROTOCOLS.md §3.7](PROTOCOLS.md#secrets-never-logged-invariant) for the full invariant definition.

### Minimum parity evidence

- local Docker deployment with mounted state
- backup workflow documentation naming included durable-state domains, exclusions, and whether the shipped path is runbook/native tooling or a dedicated CLI (see [PROTOCOLS.md §15.4](PROTOCOLS.md#154-backup-and-restore-workflow-contract))
- restore workflow documentation naming prerequisites, same-layout restore rules, and post-restore verification checks (see [PROTOCOLS.md §15.4](PROTOCOLS.md#154-backup-and-restore-workflow-contract))
- restart-preservation documentation naming which state must survive container restart and which post-restart checks operators should run
- degraded-recovery documentation naming what is not expected to survive restore in place (for example, live PTY/process attachment or unmanaged provider-side state)
- restart durability tests
- PostgreSQL-backed Azure-hosted mode tests
- Azure Files/Blob mapping validation for intended domains
- write-probe detection of read-only mounts (Docker bind-mount with `ro` flag)
- no secret values in `rune doctor` output, `/api/doctor/run` response, or structured logs

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

## 13. Agent templates (#63)

### Scope

- built-in agent template definitions (slug, name, description, category, mode, spells)
- `rune agents templates` listing surface with optional `--category` filter
- `rune agents start --template <slug>` launch path: resolves template by slug, creates a subagent session via the gateway, renders session id / template / mode
- JSON and human-readable output modes

### Invariants

- at least 4 built-in templates ship with the binary
- template slugs are unique
- all three categories (developer, operator, personal) are represented
- `--category` filter returns only matching templates
- `--template` with an unknown slug returns a user-facing error naming the slug and suggesting `rune agents templates`

### Required persisted state

None — built-in templates are compiled into the binary. Session state is persisted by the gateway.

### External surfaces

- `rune agents templates [--category <cat>]` CLI command
- `rune agents start --template <slug>` CLI command
- `rune agents templates --json` / `rune agents start --json` for machine-readable output

### Failure behavior expectations

- unknown `--category` value returns empty list, not an error
- unknown `--template` slug returns a descriptive error (not a panic)

### Minimum parity evidence

- CLI parse tests for `rune agents templates` with and without `--category`
- CLI parse tests for `rune agents start --template <slug>` and missing `--template`
- core unit tests: slug uniqueness, minimum count, category coverage, serde roundtrip
- output render tests: empty list, populated list (human + JSON), template start (human + JSON)

### Contract status

`implemented` — template listing and `start --template` launch path wired end-to-end.

### Current parity boundary

The currently shipped `rune agents` operator surface is:

- `list`
- `show`
- `status`
- `tree`
- `templates`
- `start --template`

That surface is intentionally narrower than full subagent-control parity.
Internal lifecycle/session logic for descendant agents exists, but Rune does not yet expose a client-facing transport contract for subagent `steer` or `kill` actions.
Until that transport surface exists on the public CLI/API/event path, `steer` and `kill` remain blocked and must not be described as shipped parity.

---

## 14. Evidence checklist template

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

## 15. Release rule

The rewrite cannot honestly be described as functionally identical until every parity-critical subsystem is either:

- `parity-complete`, or
- `known-divergence` with an explicit approved compatibility exception

If evidence is partial, the claim must remain: parity-seeking, not parity-complete.

---

## 16. Zero-config startup coherence (issue #61)

#### 1. Config and environment coherence

#### Contract

Rune startup must behave predictably across file config, environment overrides, and zero-config local defaults.

#### Invariants

- `--config` overrides `RUNE_CONFIG`
- `RUNE_CONFIG` overrides built-in defaults
- `RUNE_*` environment variables override file values in the loaded config
- explicit `models.providers` disables zero-config Ollama auto-detect
- empty `models.providers` enables zero-config Ollama probing
- `models.default_model` overrides the Ollama auto-picked model when both are present
- explicit default-provider detection follows: default agent model, then `models.default_model`, then single explicit provider when only one exists
- bare-host `mode = "auto"` with untouched Docker-default paths resolves to standalone local mode
- Docker/Kubernetes or explicit server signals preserve server-oriented path layout
- unresolved explicit default-model references stay unresolved in diagnostics; Rune does not guess a provider

#### Operator evidence

Startup logs must surface:

- config source
- requested mode and resolved mode
- path profile
- model bootstrap mode
- raw `OLLAMA_HOST` when present plus whether it is relevant
- effective zero-config Ollama probe target when relevant
- configured providers summary
- configured default model and source
- configured default provider and source when detectable
- resolved provider mode and detail
- resolved default provider and source after runtime provider selection
- default model source

---

#### 2. Zero-config local runtime

#### Contract

A first local boot with no custom config must start in a coherent standalone shape.

#### Required behavior

- default Docker-first paths are remapped to `~/.rune/*` on a bare host
- required local directories are auto-created for standalone mode
- path validation remains explicit and operator-visible
- zero-config Ollama probing uses the normalized `OLLAMA_HOST` target when provided, otherwise localhost

#### Failure behavior

- unreachable `OLLAMA_HOST` must warn explicitly before fallback and include the normalized probe target
- reachable Ollama with no pulled models must emit actionable operator guidance
- explicit provider build failure must surface as `echo-fallback` in runtime provider resolution
- missing or unwritable required paths must surface clear path-specific diagnostics

---

#### 3. Non-goals

This slice does not include:

- guided setup flows
- secret management redesign
- multi-provider fallback redesign
- generalized environment-to-provider mapping beyond the current local Ollama path
