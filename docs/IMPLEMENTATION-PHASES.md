# Implementation Phases

This plan sequences the rewrite so parity can be proven incrementally.

For the OpenClaw-surface navigation view, see [`OPENCLAW-COVERAGE-MAP.md`](OPENCLAW-COVERAGE-MAP.md).

The rule is simple:

- do not build broad feature surface before the contracts are frozen
- do not claim parity for a subsystem without black-box acceptance coverage
- do not optimize or expand scope ahead of compatibility-critical behavior

---

## 1. Delivery strategy

Implementation should proceed in thin vertical slices that can be validated against OpenClaw.

Each phase must end with:

1. a clear subsystem boundary
2. an acceptance test set
3. explicit known gaps
4. a decision on whether to freeze behavior or keep iterating

A phase is not done when the code exists.
A phase is done when the behavior is inspectable and testable.

---

## 2. Phase definitions

## Phase 0 — specification freeze

### Objective
Convert planning assumptions into explicit parity contracts.

### Required outputs

- parity inventory of observable OpenClaw surfaces, with evidence-tiered CLI command-family census (live-help confirmed vs source-confirmed breadth), sampled subcommand breadth, global CLI controls, and minimum control-plane resource/event matrix
- parity contract by subsystem
- protocol definitions for gateway, runtime, tools, scheduler, channels, memory, approvals
- storage and runtime directory model
- Azure compatibility constraints
- explicit open questions with owners/decision deadlines

### Acceptance criteria

- `parity/PARITY-INVENTORY.md` captures the observable CLI/gateway/tool/runtime/channel/config/diagnostic surface to be replicated, including command-family tiering and minimum HTTP/WS surface expectations
- command-family inventory is cross-checked against the local OpenClaw CLI docs/registration, not inferred from memory alone
- `parity/PARITY-SPEC.md` defines what is and is not in scope for parity
- `parity/PROTOCOLS.md` defines canonical entities, states, and event boundaries
- each subsystem has black-box test categories identified
- no implementation starts without a written decision on unresolved protocol assumptions that would cause churn

### Key risk retired
Design ambiguity.

---

## Phase 1 — core daemon and control plane skeleton

This phase should already include enough status/health/doctor groundwork that operators can trust what the runtime is telling them. Do not postpone diagnostics until the end and then discover the core is opaque.

### Objective
Stand up the daemon/gateway/CLI contract without full agent intelligence.

### In scope

- gateway process lifecycle
- health/status endpoints
- auth/token foundation
- CLI-to-gateway communication path
- structured logs/events
- persistent config loading and workspace path model
- durable IDs and entity stores for sessions/jobs/tools/processes

### Out of scope

- full model turn loop
- channels
- skills
- semantic retrieval

### Acceptance criteria

- operator can start, stop, inspect, and query the daemon through CLI workflows equivalent in intent to OpenClaw
- status and health output is durable and machine-readable
- HTTP and WS surfaces expose stable envelopes for future features
- logs/events can be correlated by entity ID and request/correlation ID

### Recommended parity tests

- gateway start/stop/status workflows
- CLI connectivity and failure modes
- health/status endpoint behavior
- auth failure and token validation behavior
- WS connect/subscribe/reconnect basics

### Current implementation evidence (2026-03-13)

- workspace compile/test/clippy gates are green
- gateway binary is runnable in zero-config development mode rather than exiting as a stub
- smoke-tested HTTP flow currently covers: `GET /health`, `GET /status`, `GET /gateway/health`, `POST /gateway/start`, `POST /gateway/stop`, `POST /gateway/restart`, `GET /sessions`, `POST /sessions`, `GET /sessions/{id}`, `POST /sessions/{id}/messages`, and `GET /sessions/{id}/transcript`
- smoke-tested runtime flow currently covers session creation, message execution, assistant reply return, and transcript retrieval end-to-end
- the executable path now wires PostgreSQL-backed Diesel repositories end-to-end, using configured external PostgreSQL when `database.database_url` is set and embedded PostgreSQL as the zero-config local fallback
- remaining gaps are parity breadth and durability validation depth (restart/recovery evidence, broader control-plane resources, and black-box parity capture), not in-memory placeholder wiring

### Key risk retired
Control plane instability.

---

## Phase 2 — session and turn runtime minimum viable parity

### Objective
Implement the session model and the core turn lifecycle.

### In scope

- session creation and persistence
- transcript item model
- turn execution state machine
- prompt/context assembly skeleton
- model provider abstraction
- usage/accounting records
- compaction/pruning contract implementation hooks

### Out of scope

- sophisticated channels
- skill auto-selection beyond minimum contract
- full memory retrieval

### Acceptance criteria

- sessions have stable IDs, statuses, and transcript ordering
- a turn can execute from trigger to final assistant output with persisted audit trail
- failures, cancellations, and retries are visible in session state
- context assembly is inspectable and reproducible enough for debugging

### Recommended parity tests

- create session / append input / run turn / persist transcript
- failed model invocation recovery paths
- session status derivation from stored state
- transcript ordering and replay stability
- compaction no-op behavior and placeholder compaction contracts

### Key risk retired
Runtime state ambiguity.

---

## Phase 3 — first-class tool system and approvals

### Objective
Reach parity on the operational tool loop because this is core to OpenClaw’s value.

### In scope

- tool registration and schemas
- read/write/edit semantics
- exec/process/background semantics
- approval request and approval decision lifecycle
- capability/policy checks
- transcript + audit linkage for tool actions

### Out of scope

- all future tools
- browser/media-heavy tools unless needed for base parity

### Acceptance criteria

- tool names and semantics are stable and operator-recognizable
- approval requests present the exact command/payload to be approved
- background execution survives beyond immediate turn completion
- process handles support poll/log/write/send-keys/kill equivalents as required
- denied or failed actions produce usable audit trails

### Recommended parity tests

- file tool success/failure cases
- exact-match edit behavior
- exec foreground to background transition behavior
- approval allow-once vs allow-always vs deny behavior
- process polling and termination semantics

### Key risk retired
Silent semantic drift in tools.

---

## Phase 4 — scheduling, heartbeats, and isolated jobs

### Objective
Reproduce proactive automation behavior.

### In scope

- cron scheduling
- one-shot reminders
- wake/system-event queueing
- heartbeat runs
- isolated-target scheduled session runs
- job history and next-run computation
- missed-run and disable/enable semantics

### Out of scope

- advanced maintenance automation not present in parity target

### Acceptance criteria

- scheduled jobs are durable and inspectable
- schedule edits and disable/re-enable transitions recompute or clear `next_run_at` rather than preserving stale due state
- heartbeat behavior preserves shipped no-op and duplicate-suppression semantics
- scheduled executions are auditable; `sessionTarget=isolated` cron jobs create descendant subagent sessions while main-target cron jobs, reminders, and heartbeats reuse scheduled session contexts
- operator can list, inspect, enable, disable, wake, and review job history
- `sessionTarget=main` vs `sessionTarget=isolated` semantics are preserved without payload coercion
- `none` / `announce` / `webhook` delivery modes are executable: `announce` broadcasts a `cron_run_completed` event via the session event channel, `webhook` POSTs the job result to the configured URL, and `none` suppresses outbound delivery
- reminder outcomes persist as delivered / missed / cancelled rather than disappearing into logs only
- reminder targets route execution: `"main"` executes in the stable scheduled main session, `"isolated"` creates a one-shot subagent session; unknown targets fall back to `"main"` with a warning
- due jobs and reminders are claimed atomically before execution; stale claims expire after the configured lease duration for crash recovery; concurrent supervisor ticks cannot duplicate execution

### Recommended parity tests

- cron create/list/show/edit/disable/enable/run-now/wake flows
- schedule-edit recompute plus disable/re-enable `next_run_at` transition tests
- due-only vs forced-run history tests
- `systemEvent` vs `agentTurn` session-target validation tests
- delivery-mode execution tests for `none` / `announce` / `webhook` including webhook POST and announce event broadcast
- reminder due/delivered/missed/cancelled plus target-routing flows
- reminder target routing tests: `"main"` vs `"isolated"` session creation
- durable claim/lease tests: atomic claim, stale-claim reclaim, release-and-reclaim, concurrent duplicate suppression
- wake mode normalization and queued-event payload tests
- heartbeat instruction loading, no-op suppression, and duplicate-notification suppression persistence behavior

### Key risk retired
Automation behavior mismatch.

---

## Phase 5 — memory and retrieval parity

### Objective
Preserve the workspace-memory model and privacy boundaries before adding sophistication.

### In scope

- workspace memory file conventions
- daily memory and long-term memory handling
- retrieval API and snippet model
- indexing metadata
- context injection rules by session type
- privacy boundaries for main vs shared/group contexts

### Out of scope

- ambitious multi-source knowledge graph behavior
- advanced vector infrastructure unless needed

### Acceptance criteria

- runtime can read and update memory artifacts according to current behavioral rules
- retrieval results are attributable to source documents
- privacy boundaries are preserved across session kinds
- memory access is testable without model calls

### Recommended parity tests

- main-session vs shared-session memory visibility
- retrieval snippet shape and attribution
- daily note and MEMORY.md conventions
- no-leak tests for restricted files

### Key risk retired
Context leakage and privacy regressions.

---

## Phase 6 — channel abstraction and first production channel

### Objective
Prove the normalized channel contract with one real provider before multiplying adapters.

### Recommended first channel
Telegram.

### In scope

- normalized inbound event model
- normalized outbound action model
- direct vs group routing semantics
- reply handling
- reaction handling where supported
- attachment/media metadata path
- delivery state and retries

### Out of scope

- all channels at once
- provider-specific extras not needed for parity

### Acceptance criteria

- inbound provider events normalize deterministically
- outbound send/reply/edit/react operations preserve expected routing behavior
- group participation rules can be enforced at runtime level
- delivery IDs and provider message IDs are retained for follow-up actions

### Recommended parity tests

- direct-message flow
- group-chat mention flow
- reply-to specific message flow
- reaction emission policy
- duplicate inbound event dedupe
- transient provider failure retry behavior

### Key risk retired
Provider-model leakage into runtime core.

---

## Phase 7 — additional channels and media workflow

### Objective
Broaden parity from one adapter to the channel family and add media behavior.

### In scope

- Discord / WhatsApp / Signal in priority order
- attachment download/upload pipeline
- audio transcription path
- image/media pass-through handling
- TTS response path where parity requires it

### Acceptance criteria

- normalized model remains stable across providers
- media objects have durable references and lifecycle tracking
- provider differences are isolated to adapters

### Recommended parity tests

- attachment ingestion and reference persistence
- voice note transcription flow
- TTS delivery flow
- reply/edit/react support matrix by provider

### Key risk retired
Media and channel fragmentation.

---

## Phase 8 — skills and extension model

### Objective
Preserve prompt-skill ergonomics while defining the safer Rust-native extension future.

### In scope

- prompt-skill discovery and selection rules
- packaged references/assets/scripts model
- install/update metadata path
- native plugin manifest and capability model
- out-of-process execution boundary for plugins

### Acceptance criteria

- current skill-selection behavior can be expressed in the Rust runtime
- one most-specific applicable skill can be auto-loaded consistently
- skill resources resolve predictably
- native plugins declare capabilities and fail in isolation

### Recommended parity tests

- zero-skill / one-skill / multiple-skill selection behavior
- resource path resolution
- manifest validation
- plugin timeout/failure isolation behavior

### Key risk retired
Extension ergonomics vs safety tradeoff.

---

## Phase 9 — subagents and multi-agent orchestration

### Objective
Reach parity on delegated work and descendant session behavior.

### In scope

- subagent spawn/track/steer/kill flows
- requester-session linkage
- push-based result reporting
- isolated runtime policies per subagent
- long-running delegated work tracking

### Acceptance criteria

- descendant sessions retain parent linkage and auditability
- operator can inspect subagent lifecycle cleanly
- results are routed back without polling abuse
- delegated failures are visible and recoverable

### Recommended parity tests

- spawn and await result flow
- steer and cancel flow
- duplicate completion or retry tolerance
- child-session transcript isolation

### Key risk retired
Delegation lifecycle confusion.

---

## Phase 10 — Azure compatibility hardening

### Objective
Prove that “Azure compatible” means real provider and deployment parity, not marketing wording.

### In scope

- Azure OpenAI / Azure AI Foundry request construction
- deployment-name vs model-name semantics
- Azure auth/header/version handling
- Azure Document Intelligence integration contract
- Azure-friendly secrets and container deployment assumptions

### Acceptance criteria

- provider requests can be snapshot-tested against Azure expectations
- configuration cleanly separates endpoint, deployment, API version, and auth settings
- Azure-specific behavior does not contaminate generic provider abstractions
- document/OCR flows have a native Azure integration path

### Recommended parity tests

- request-construction golden tests
- auth/header tests
- config parsing tests for Azure-specific settings
- error normalization tests from Azure responses

### Key risk retired
False Azure compatibility.

---

## Phase 11 — operator UI and final parity sweep

### Objective
Expose the runtime cleanly and close the remaining parity gaps.

### In scope

- dashboard/status
- session browser
- approval center
- logs/events viewer
- job/cron management
- channel health
- skills/plugins management
- final doctor/diagnostics flows

### Acceptance criteria

- operator can perform core daily workflows without dropping to internal state inspection
- every parity-critical subsystem has black-box acceptance coverage
- known divergences are documented explicitly

### Recommended parity tests

- operator workflow tests spanning CLI, gateway, and UI
- approval resolution through UI
- live logs/session updates over WS
- diagnostics bundle generation

### Key risk retired
Operational blind spots.

---

## 3. Cross-phase rules

## 3.1 Freeze parity-critical names early

These should be treated as compatibility surface and not churn casually:

- tool names
- approval decision names
- session kinds/statuses exposed to CLI/UI
- normalized channel event/action names
- core entity IDs and reference formats

## 3.2 Prefer golden traces over “looks right” validation

For critical flows, record OpenClaw behavior and compare:

- command output shape
- approval prompt content
- event ordering
- transcript deltas
- scheduling behavior
- provider request payloads

## 3.3 No subsystem is done without failure-path tests

Every phase must test:

- success path
- user denial path where relevant
- timeout/unavailable dependency path
- duplicate/retry path where relevant
- restart/reconnect path where relevant

## 3.4 Do not overbuild infrastructure early

Do not introduce distributed queues, complex plugin ABI work, or additional storage backends before the parity-critical path on PostgreSQL is proven.

---

## 4. Implementation sequencing rules

These rules should constrain any future execution plan.

### 4.1 Sequence by dependency, not by excitement

Build in this dependency order:

1. contracts and durable entity model
2. daemon/control plane
3. session runtime
4. tool/approval loop
5. scheduler
6. memory/privacy boundaries
7. first real channel
8. media/document path
9. skills/extensions
10. subagents
11. Azure hardening
12. UI sweep

This order is deliberate. Earlier layers define the truth that later layers expose.

### 4.2 Never multiply adapters before the normalized model is proven

Do not build several channels, several providers, or several plugin backends in parallel before one path proves the contract.

Recommended proving paths:

- Telegram first for channels
- Azure OpenAI + one non-Azure provider first for models
- prompt skills first for extensions
- PostgreSQL + mounted filesystem first for state, with embedded Postgres fallback for zero-config local mode

### 4.3 Every milestone should retire one core risk

If a milestone does not clearly retire a risk, it is probably too broad or poorly ordered.

### 4.4 Preserve optionality where it matters

Preserve explicit escape hatches for:

- embedded PG -> external PostgreSQL
- PostgreSQL FTS -> dedicated search engine (Tantivy / Meilisearch)
- process plugins -> WASI plugins
- local files -> Azure Files / object archive mapping

Optionality matters most at boundaries, not in the middle of business logic.

## 5. Suggested milestone packaging

If this becomes an execution roadmap, package milestones as:

1. **M1:** daemon + status + auth + WS skeleton
2. **M2:** sessions + turns + transcript persistence
3. **M3:** tools + approvals + background processes
4. **M4:** cron + heartbeat + reminders
5. **M5:** memory + privacy boundaries
6. **M6:** Telegram + normalized channel model
7. **M7:** media + more channels
8. **M8:** skills + plugins
9. **M9:** subagents
10. **M10:** Azure hardening
11. **M11:** UI + final parity sweep

This sequence keeps the hardest behavioral contracts visible early.

---

## 5. Exit criteria for “functionally identical”

The rewrite can only claim practical parity when all of the following are true:

1. parity-critical subsystems have black-box acceptance coverage
2. known divergences are documented and approved
3. operator workflows are equivalent in practice
4. Azure compatibility has been validated by concrete request/response tests
5. no privacy-boundary regressions remain open
6. tool and scheduling semantics match expected behavior under both success and failure conditions

Until then, call it a parity-seeking rewrite, not a parity-complete rewrite.
