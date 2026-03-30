# Protocols

This document defines the behavioral contracts the Rust rewrite must implement to achieve OpenClaw parity.

This is not an internal architecture essay.
It is the protocol companion to `PARITY-INVENTORY.md`.
That inventory says **what observable surfaces exist**.
This document says **how those surfaces behave as entities, commands, state transitions, resources, and events**.

The point is not to preserve TypeScript internals.
The point is to preserve observable behavior, subsystem boundaries, and operator expectations.

Hard constraints remain:

- functional parity with OpenClaw
- full Azure compatibility
- Docker-first deployment with mountable persistent storage

---

## 1. Protocol design principles

1. **Behavioral parity over structural similarity**
   - match what operators, channels, tools, and clients observe
   - internal crate layout may differ

2. **Stable contracts before optimization**
   - protocol shape, event semantics, and approval boundaries come before micro-optimizations

3. **Explicit state transitions**
   - sessions, jobs, approvals, tools, processes, and channel deliveries must be modeled as state machines
   - avoid hidden implicit transitions

4. **Durable identifiers and auditability**
   - every long-lived entity needs a stable ID
   - every state-changing action should be attributable to actor, time, and cause

5. **Separation of command and event views**
   - commands request work
   - events describe facts that happened
   - read models summarize current state

6. **At-least-once internal event tolerance**
   - internal delivery should tolerate duplicate events
   - consumers must be idempotent where retries are possible

7. **No silent semantic drift**
   - if a protocol intentionally diverges from OpenClaw, document it as an explicit compatibility decision

8. **Operator inspectability is part of the protocol**
   - `status`, `doctor`, logs, dashboard views, and live event streams are not secondary niceties
   - if a state exists, operators need a coherent way to observe and diagnose it

---

## 2. Observable surface alignment

`PARITY-INVENTORY.md` is the anchor inventory.
This protocol layer must support, at minimum, parity across these observable domains:

1. CLI command families and shell completion
2. gateway/daemon lifecycle and control plane
3. HTTP/WS APIs and live event model
4. sessions, turns, transcripts, runtime state
5. tools and tool schemas
6. approvals, sandboxing, security policy
7. cron, reminders, wake, heartbeat, hooks/webhooks
8. memory and retrieval
9. channels and provider adapters
10. media, transcription, OCR, TTS, browser/voice adjacencies
11. skills, plugins, hooks, extension packaging
12. config, secrets, precedence, validation, mutation
13. storage, Docker, backup/restore, durable filesystem model
14. Azure provider, document, storage, hosting compatibility
15. doctor, health, status, logging, diagnostics, dashboard visibility
16. subagents, ACP, background processes, descendant sessions

This document focuses on the protocol-heavy parts of that list.

---

## 3. Canonical subsystem contracts

The rewrite should preserve these subsystem boundaries even if crate boundaries differ.

### 3.1 Gateway contract

The gateway is the control-plane boundary.
It owns:

- daemon lifecycle exposure
- HTTP API
- WebSocket event streaming
- auth and session tokens
- status/health surfaces
- operator control commands
- approval presentation and submission
- dashboard-facing control/read APIs

The gateway does **not** own model logic, channel adapters, or tool business logic directly.
It coordinates them through runtime services.

### 3.2 Runtime contract

The runtime owns:

- session creation and turn execution
- prompt/context assembly
- tool loop execution
- background jobs
- scheduling integration
- sub-agent orchestration
- compaction/pruning behavior
- usage accounting
- startup context loading rules by session kind

The runtime is the source of truth for turn state.

### 3.3 Channel adapter contract

Every channel adapter must translate provider-specific payloads into a normalized inbound event model and translate normalized outbound actions into provider API calls.

Adapters own:

- provider auth/session setup
- inbound webhook/poll payload parsing
- provider retry/backoff
- provider-specific message IDs and delivery handles
- media upload/download mechanics

Adapters must not leak provider-specific semantics into core runtime types except through explicit extension fields.

### 3.4 Tool runtime contract

The tool runtime owns:

- tool registration and schemas
- argument validation
- approval routing when required
- execution lifecycle
- streaming/background execution handles
- audit records
- capability/policy enforcement

### 3.5 Scheduler contract

The scheduler owns:

- cron schedules
- one-shot reminders
- heartbeat triggers
- wake events
- missed-run policy
- run isolation
- run history and next-run derivation

### 3.6 Memory contract

The memory subsystem owns:

- workspace document discovery
- daily and long-term memory conventions
- retrieval/index metadata
- snippet retrieval
- memory update APIs
- safety boundaries for what can be recalled and exposed

### 3.7 Config and secrets contract

The config subsystem owns:

- layered config resolution
- config mutation and validation workflows
- service-vs-CLI context visibility
- provider-, channel-, and agent-specific overrides
- secrets references and injection model

#### Secrets-never-logged invariant

Secret values must never appear in:

- log output (structured or unstructured, any severity level)
- error messages returned to callers or rendered in CLI output
- status/health/doctor endpoint responses
- WebSocket event payloads
- transcript items or tool result summaries
- diagnostic bundles or debug dumps

This applies to provider API keys, channel tokens, database credentials, certificate key material, and any value loaded from `/secrets` or injected via secret-reference config fields.

Secret references (e.g. key names, vault paths, environment variable names) may appear in diagnostics. Secret values may not.

Violation of this invariant is a release-blocking defect, not a cosmetic issue.

### 3.8 Doctor and diagnostics contract

The diagnostic subsystem owns:

- health/readiness/liveness reporting
- doctor checks and repair workflows
- inspectable mismatch and drift reports
- diagnostic bundle/export generation where shipped

#### Read-only filesystem detection semantics

Writability of configured storage paths is verified by **write-probe**, not by metadata inspection. The probe creates and immediately deletes a temporary file in the target directory. This catches failure modes that permission-bit checks miss:

- bind-mount read-only (`ro` flag in Docker or fstab)
- UID/GID mismatch between the runtime process and the mount owner
- SELinux or AppArmor policy denials
- filesystem-level read-only mounts

Detection surfaces:

| Surface | Behavior |
|---|---|
| `rune doctor` (CLI) | reports Fail (not Warn) per unwritable path with mode-aware fix hint |
| `AppConfig::validate_paths()` (startup) | exits with a clear error naming the path and expected permissions |
| `POST /api/doctor/run` (gateway) | includes per-path writability findings in the response body |

Fix hints are **mode-aware**: standalone mode suggests `mkdir -p` / `chmod`; server/Docker mode suggests volume mount adjustments and UID guidance.

An unwritable required storage path is a **Fail**, not a warning. Silent fallback to ephemeral storage is never acceptable.

---

## 4. Entity model

These are protocol-level entities, not necessarily database tables.

## 4.1 Session

Represents one logical conversational execution context.

### Required protocol fields

A session record should expose at least:

- `session_id`
- `kind`
- `status`
- `created_at`
- `updated_at`
- `workspace_root`
- `channel_ref` when channel-backed
- `requester_session_id` when descendant
- `runtime_profile` or runtime config snapshot reference
- `policy_profile` or policy snapshot reference
- `latest_turn_id` when present
- `last_activity_at`

### Behavioral invariants

- a session has one authoritative transcript ordering
- child/sub-agent sessions must record their parent/requester relationship
- session status must be derivable without reading arbitrary raw logs

### Minimum conceptual states

- `created`
- `ready`
- `running`
- `waiting_for_tool`
- `waiting_for_approval`
- `waiting_for_subagent`
- `suspended`
- `completed`
- `failed`
- `cancelled`

### Transition rules

Minimum transition expectations:

- `created -> ready`
- `ready -> running`
- `running -> waiting_for_tool | waiting_for_approval | waiting_for_subagent | completed | failed | cancelled`
- `waiting_for_tool -> running | failed | cancelled`
- `waiting_for_approval -> running | failed | cancelled`
- `waiting_for_subagent -> running | failed | cancelled`
- `suspended -> ready | cancelled | failed`

Transitions should be emitted as structured events with causation metadata.
Silent state mutation is not acceptable for parity-critical paths.

## 4.2 Turn

Represents one unit of model-driven work inside a session.

Required attributes:

- `turn_id`
- `session_id`
- `trigger` (user message, cron, heartbeat, system wake, subagent request, etc.)
- `started_at`, `ended_at`
- `status`
- `usage_summary`
- `compaction_summary` when applicable
- `model_ref`
- `channel_action_summary` when outbound effects occurred

Turn invariants:

- a turn is append-only in history after completion except for explicit repair/migration operations
- all tool calls during a turn must be attributable to that turn
- if a turn results in background work, the transcript must still make that understandable

## 4.3 Transcript item

Transcript items are the canonical history model.

Supported conceptual kinds:

- user message
- assistant message
- system/instruction message
- tool request
- tool result
- approval request
- approval response
- status/event note
- channel delivery note
- subagent result note
- cron/heartbeat/wake note

Transcript invariants:

- transcript ordering must be stable and reproducible
- tool outputs recorded in transcript must match tool audit records or contain a reference to them
- compaction must preserve enough information for future turns to behave consistently

## 4.4 Tool execution

Represents one invocation of a first-class tool.

Required attributes:

- `tool_call_id`
- `tool_name`
- `session_id`
- `turn_id`
- `arguments`
- `approval_state`
- `execution_mode` (`inline`, `streaming`, `background`)
- `started_at`, `ended_at`
- `result_summary`
- `error_summary` if failed
- `handle_ref` if long-running
- `host_ref` when execution host matters

State model:

- `created`
- `validation_failed`
- `awaiting_approval`
- `approved`
- `denied`
- `running`
- `streaming`
- `backgrounded`
- `succeeded`
- `failed`
- `cancelled`

## 4.5 Approval

Represents an explicit human or policy gate.

Required attributes:

- `approval_id`
- `subject_type` and `subject_id`
- `reason`
- `policy_source`
- `presented_payload`
- `decision` (`pending`, `allow-once`, `allow-always`, `deny`)
- `decided_by`
- `decided_at`

Approval invariants:

- approval must bind to the exact command or payload presented to the user
- `allow-once` cannot silently expand to cover later commands
- denied actions must leave an audit trail and a user-visible explanation path

## 4.6 Process handle

Represents a long-running or interactive execution handle, especially for `exec`/`process` parity.

Required attributes:

- `process_id`
- `tool_call_id`
- `session_id`
- `pty`
- `cwd`
- `host`
- `status`
- `started_at`
- `ended_at`
- `exit_code`
- `io_retention_policy`

State model:

- `starting`
- `running`
- `backgrounded`
- `completed`
- `failed`
- `killed`
- `lost`

## 4.7 Job / scheduled run

Represents scheduled work.

Required attributes:

- `job_id`
- `job_type` (`cron`, `heartbeat`, `reminder`, `wake`, `maintenance`)
- `schedule` or `due_at`
- `policy`
- `last_run_at`
- `next_run_at`
- `last_result`
- `enabled`
- `payload_kind`
- `delivery_mode`

## 4.8 Channel delivery

Represents one outbound provider delivery attempt.

Required attributes:

- `delivery_id`
- `channel`
- `destination`
- `source_session_id`
- `message_kind`
- `provider_message_id`
- `attempt_count`
- `status`
- `sent_at`
- `related_message_ref`

## 4.9 Config snapshot

Represents the resolved config view that meaningfully affects runtime behavior.

Required attributes:

- `config_snapshot_id`
- `source_layers`
- `resolved_profile`
- `provider_overrides`
- `channel_overrides`
- `approval_policy_ref`
- `workspace_roots`

This matters because service-vs-CLI mismatches are operator-visible and parity-critical.

---

## 5. Command and event model

Protocols should distinguish commands from events.

## 5.1 Command envelope

Commands request state changes.
A canonical command envelope should include:

- `command_id`
- `command_type`
- `issued_at`
- `actor`
- `target`
- `payload`
- `correlation_id`
- `causation_id`
- `idempotency_key` where relevant

Examples:

- create session
- append inbound message
- start turn
- invoke tool
- submit approval decision
- send channel message
- spawn subagent
- schedule job
- cancel process
- rotate auth token
- run doctor checks

## 5.2 Event envelope

Events describe facts that occurred.
A canonical event envelope should include:

- `event_id`
- `event_type`
- `occurred_at`
- `origin`
- `subject`
- `payload`
- `correlation_id`
- `causation_id`
- `sequence`

Examples:

- session created
- inbound message recorded
- turn started
- tool approval requested
- tool execution completed
- process backgrounded
- approval granted
- subagent completed
- outbound delivery failed
- cron run triggered
- doctor check failed
- config validation failed

## 5.3 Idempotency rules

The following operations must be idempotent by explicit key or natural identity:

- inbound webhook ingestion
- approval submission
- process poll/result recording
- job trigger dispatch
- retryable outbound channel delivery recording
- config mutation retries where transport duplication is possible

---

## 6. HTTP control-plane protocol

Exact routes can differ from OpenClaw if CLI and UI behavior remain equivalent, but the Rust rewrite should expose a stable HTTP control API with the same resource concepts and enough metadata for CLI/UI parity.

## 6.1 Minimum resource families

At minimum:

- `/health`
- `/status`
- `/auth/*`
- `/sessions/*`
- `/turns/*`
- `/tools/*`
- `/processes/*`
- `/approvals/*`
- `/jobs/*`
- `/channels/*`
- `/memory/*`
- `/logs/*`
- `/config/*`
- `/doctor/*`

Optional in the first release tier, but already part of the inventory if shipped:

- `/devices/*`
- `/pairing/*`
- `/nodes/*`
- `/skills/*`
- `/plugins/*`
- `/hooks/*`
- `/webhooks/*`
- `/browser/*`

## 6.2 Minimum conceptual matrix

The API must be able to express, at minimum:

- `GET /health`
- `GET /status`
- `POST /auth/*`
- `GET|POST /sessions/*`
- `GET|POST /turns/*`
- `GET|POST /tools/*`
- `GET|POST /processes/*`
- `GET|POST /approvals/*`
- `GET|POST /jobs/*`
- `GET|POST /channels/*`
- `GET|POST /memory/*`
- `GET|POST /config/*`
- `GET|POST /doctor/*`
- `GET /logs/*`

## 6.3 Resource semantics

Read endpoints must be safe and side-effect free.
Mutating endpoints must return durable IDs for created work.
Long-running operations must support asynchronous completion semantics.
Every response should include enough metadata for CLI/UI correlation.

Expected operation shapes by family:

- list/get/create/update/delete where conceptually valid
- run/trigger/cancel where active work entities require it
- inspect/history endpoints where operator workflows depend on them

## 6.4 Error model

Every error response should include:

- stable error code
- human-readable message
- retriable flag when meaningful
- approval-required flag when meaningful
- correlation/request ID
- optional structured details

Example error classes:

- auth failure
- validation failure
- policy denied
- approval required
- not found
- conflict
- provider unavailable
- timeout
- dependency failure
- config mismatch
- storage unavailable

---

## 7. WebSocket / live event protocol

The WebSocket surface exists for live observability and interactive control, not as an opaque transport for arbitrary internal state.

## 7.1 Capabilities

Must support:

- subscription to session events
- subscription to logs/events/jobs/processes
- live turn/tool/progress updates
- approval request notifications
- channel delivery updates
- optional interactive commands if authorized

## 7.2 Frame envelope

Every WS frame should carry:

- `type`
- `timestamp`
- `topic`
- `payload`
- `correlation_id` where applicable
- `sequence` or cursor where applicable

Conceptual message kinds:

- snapshot
- event
- patch/update
- progress
- ack
- error
- keepalive

## 7.3 Minimum event families

The live stream should be able to carry at least:

- session created/updated/completed/failed/cancelled
- turn started/progressed/completed/failed/cancelled
- tool requested/approval-required/running/backgrounded/completed/failed
- approval created/resolved/expired
- process started/output/backgrounded/exited/killed/lost
- cron job created/updated/triggered/completed/failed
- heartbeat run started/no-op/notified/failed
- channel delivery queued/sent/edited/reacted/failed
- doctor/diagnostic warning/failure events
- logs/warnings/diagnostics events

## 7.4 Ordering and replay expectations

- within a single subscribed entity stream, ordering should be stable
- reconnect behavior should support replay from cursor, sequence, or timestamp where practical
- clients must tolerate duplicate event delivery on reconnect

---

## 8. Session runtime protocol

## 8.1 Session creation contract

Inputs:

- initiating actor/channel/job
- session kind
- runtime/model config
- workspace root
- parent/requester linkage when present

Outputs:

- stable session ID
- initial status
- initial transcript seed or system context reference

## 8.2 Turn execution contract

A turn must proceed through these logical stages:

1. trigger accepted
2. context assembled
3. model invoked
4. tool requests handled as needed
5. approvals awaited if needed
6. transcript finalized
7. usage/accounting persisted
8. outbound channel actions emitted if needed

The runtime may iterate within a turn, but the persisted record must make the iteration understandable after the fact.

## 8.3 Context assembly contract

Context assembly must be deterministic given the same stored state, policy, retrieval inputs, and trigger.

Inputs may include:

- transcript slice
- system/user files such as `AGENTS.md`, `SOUL.md`, `USER.md`, `MEMORY.md` as allowed by policy
- skill instructions
- memory retrieval snippets
- current tool availability
- session/channel metadata

Parity requirements:

- startup file loading rules must respect session context
- `MEMORY.md` must remain main-session-only unless an explicit future compatibility decision says otherwise
- injected context must remain explainable and inspectable in debugging tools

## 8.4 Session status contract

Status tooling must expose enough fields to preserve `/status` and `session_status` semantics, including where applicable:

- current model or model override
- usage/time/cost summary
- reasoning/verbose/elevated flags
- runtime/session identity
- relevant approval/sandbox posture summaries

## 8.5 Compaction/pruning contract

Compaction is allowed only if it preserves future behavioral compatibility.

Compaction output must preserve at minimum:

- user intent
- decisions made
- unresolved tasks/questions
- tool outcomes that affect future work
- safety/policy relevant facts
- enough transcript continuity for the next turn

Compaction must not silently remove facts that would materially change future agent behavior.

---

## 9. Tool protocol

## 9.1 Tool registration

Every first-class tool needs:

- stable name
- human description
- argument schema
- execution mode support
- approval policy
- capability requirements
- result schema or output conventions

Tool names are part of the compatibility surface.
Semantics must remain stable even if implementation changes.

## 9.2 Tool invocation lifecycle

Logical sequence:

1. tool requested by runtime/model/operator
2. arguments validated
3. policy evaluated
4. approval requested if required
5. tool executed
6. result normalized
7. transcript and audit records written
8. long-running handle exposed if applicable

## 9.3 Background process tools

For `exec` and `process`-style semantics, the protocol must preserve:

- foreground execution with bounded wait
- background continuation when work exceeds immediate wait
- stable process/session handles
- polling/log retrieval
- interactive stdin/PTY support where applicable
- explicit kill/cancel actions

Behavioral parity requirements:

- backgrounded work must remain inspectable after the initiating turn completes
- process/session IDs must be sufficient for later polling and operator intervention
- approval text must show the exact command being approved

## 9.4 File tools

For `read`, `write`, and `edit` semantics:

- working directory and workspace boundaries must be explicit
- truncation behavior must be predictable and documented
- `edit` must remain exact-match/surgical rather than fuzzy unless compatibility is explicitly revised
- errors must make mismatch causes clear

## 9.5 Tool result contract

Every tool result should expose:

- success/failure status
- human-readable summary
- structured payload when available
- truncation indicator when output is partial
- references to background handles or durable artifacts when applicable
- approval context when an approval gate materially affected execution
- exact durable handle identifiers for follow-up inspection when backgrounded

## 9.6 Tool semantic compatibility table

The following tool families are parity-critical and should be treated as named contracts, not just implementations.

### File tools

- `read`
  - deterministic truncation rules
  - explicit offset/limit behavior
  - clear file-not-found vs access error distinction
- `write`
  - create-or-overwrite semantics
  - parent directory creation behavior documented and stable
- `edit`
  - exact-match replacement semantics
  - mismatch must fail clearly; no fuzzy substitute under the same tool name

### Process tools

- `exec`
  - bounded foreground wait
  - optional PTY behavior
  - background continuation when configured
  - explicit approval presentation for elevated/sensitive commands
- `process`
  - poll/log/write/send-keys/paste/kill style lifecycle support
  - stable process/session handle identity

### Automation tools

- `cron`
  - explicit schedule payload
  - isolated run support
  - inspection/history semantics

### Coordination tools

- `sessions_list`
- `sessions_history`
- `sessions_send`
- `sessions_spawn`
- `subagents`
- `session_status`
- `memory_search`
- `memory_get`

These must remain distinct in semantics and auditability.

If implementation convenience pushes toward merging these semantics, preserve compatibility at the surface anyway.

---

## 10. Approval, security, and sandbox protocol

## 10.1 Approval request contract

An approval request must contain:

- exact action to approve
- reason approval is required
- scope of decision
- available choices
- consequences of deny

Choices must preserve OpenClaw-style semantics where applicable:

- allow-once
- allow-always
- deny

## 10.2 Approval submission contract

Approval submission must bind to:

- the original approval ID
- the exact command or payload presented
- one decision only
- one actor identity

If the command changes materially, a new approval is required.

## 10.3 Ask / security / elevated contract

The runtime must support policy states equivalent in effect to:

- ask modes: `off`, `on-miss`, `always`
- security modes: `deny`, `allowlist`, `full`
- elevated execution where the environment supports it
- host distinctions where execution target matters

## 10.4 Privacy-boundary contract

The protocol must preserve:

- main-session-only memory boundaries
- group/shared context privacy restrictions
- caution for external actions and user-voice boundaries

These are not prompt flourishes.
They are runtime behavior requirements.

---

## 11. Scheduler protocol

## 11.1 Cron contract

Cron jobs must preserve:

- explicit schedule definition
- `at`, `every`, and cron-expression schedule concepts
- enable/disable state
- derived `next_run_at` semantics that recompute on schedule edits and re-enable transitions, and clear executable due state when disabled
- run history
- isolated execution context for `sessionTarget=isolated` jobs
- configurable runtime/model if supported
- due-only vs forced-run semantics
- explicit wake semantics for `now` vs `next-heartbeat` where wake flows exist

Validation rules:

- `sessionTarget=main` requires a `systemEvent` payload
- `sessionTarget=isolated` requires an `agentTurn` payload
- invalid target/payload pairings fail as validation errors, not silent coercions
- invalid cron expressions or timezone values fail explicitly and must not fabricate future run times

Run-history expectations:

- history should distinguish at least due-triggered vs operator-forced/manual runs
- history rows should remain attributable to the originating job, trigger kind, and resulting scheduled session/run

Current surface note:

- gateway create/update accepts full schedule and payload schemas
- current CLI `cron add` creates one-shot `system_event` jobs only
- current CLI `cron edit` is limited to name and delivery-mode mutation
- `cron wake` and `system event` share the same wake-queueing surface
- schedule edits and disable/re-enable transitions recompute `next_run_at` from the current schedule definition rather than preserving stale cadence
- due jobs are claimed atomically before execution via `claimed_at`; stale claims expire after the configured lease duration (default 300 s) for crash recovery

## 11.2 Payload and delivery contract

Scheduled work must preserve the conceptual distinction between:

- `systemEvent` payloads for main-session jobs
- `agentTurn` payloads for isolated jobs

Session-target semantics must preserve:

- main-target runs reusing the stable scheduled main-session context
- isolated-target runs creating fresh isolated/subagent descendant sessions with requester linkage

Delivery modes must preserve:

- `none`
- `announce`
- `webhook`

Current shipped delivery semantics:

- the selected delivery mode is stored durably and remains operator-visible over gateway/CLI inspection surfaces
- runtime execution branches on delivery mode after job completion: `announce` broadcasts a `cron_run_completed` event via the session event channel, `webhook` POSTs the result payload to the configured URL (30 s timeout, no automatic retry), and `none` suppresses outbound delivery
- run history remains the durable audit surface regardless of delivery mode

Wake semantics currently preserve:

- accepted modes normalize to `now` or `next-heartbeat`
- omitted mode defaults to `next-heartbeat`
- both `next-heartbeat` and `next_heartbeat` are accepted on the operator surface and normalize to the same queued wake mode
- optional `context_messages` counts are surfaced on the queued wake event payload
- the current shipped contract is queueing/inspection of wake events, not guaranteed downstream wake consumption

## 11.3 Heartbeat contract

Heartbeat behavior must preserve the existing intent:

- read heartbeat instruction file if configured
- do not infer unrelated old tasks beyond what heartbeat contract allows
- perform lightweight proactive checks
- stay quiet when nothing needs attention
- surface no-op as `HEARTBEAT_OK` semantics where applicable

Heartbeat invariants:

- heartbeat runs are isolated and auditable
- repeated heartbeats should not duplicate notifications without new cause within the shipped no-op and normalized-fingerprint duplicate-suppression contract
- quiet/no-op outcomes should still be representable in run history or persisted heartbeat state
- anti-spam state must survive restart well enough to avoid replaying the same notification solely because the process restarted
- broader quiet-window policy is not yet part of the shipped runtime contract

Heartbeat suppression semantics:

- `HEARTBEAT_OK`-style no-op responses suppress outbound delivery
- duplicate non-noop responses may be suppressed when their normalized fingerprint matches the last delivered notification
- suppression decisions should remain inspectable through runner state/status surfaces even when no outbound message is emitted

## 11.4 Reminder contract

Reminders need:

- due time
- target channel/session metadata
- reminder payload/instruction
- one-shot-only reminder semantics distinct from cron jobs
- delivered / missed / cancelled outcome tracking
- reminder wording that reads like a reminder when fired

Reminder outcome semantics must preserve:

- successful delivery marking the reminder terminal as delivered
- failed delivery attempts marking the reminder terminal as missed with inspectable error context
- operator/user cancellation marking the reminder terminal as cancelled rather than silently deleting auditability
- reminder target routing: `"main"` (default) executes in the stable scheduled main session, `"isolated"` creates a one-shot subagent session under it; unknown targets fall back to `"main"` with a warning
- due reminders are claimed atomically before execution; stale claims expire after the configured lease duration for crash recovery

---

## 12. Channel protocol

## 12.1 Normalized inbound event

All channel adapters must produce a normalized inbound event carrying at least:

- channel name
- provider event ID or dedupe key
- conversation/chat identifier
- sender identifier
- message identifier
- timestamp
- text/caption body
- attachments/media references
- reply/thread/mention metadata
- direct vs group context
- reaction/edit/delete semantics if supported

## 12.2 Normalized outbound action

Core outbound actions:

- send message
- reply to message
- edit message
- react to message
- send attachment/media
- send typing/presence if supported

## 12.3 Channel parity rules

- provider-specific features may be missing only when provider itself lacks them
- direct vs group routing must preserve privacy and participation boundaries
- reply attribution and thread targeting must remain correct
- reaction behavior must be lightweight and not flood channels
- delivery references must be durable enough to support edits/replies/reactions later

---

## 13. Memory and retrieval protocol

## 13.1 Memory source contract

Supported source categories:

- workspace memory files
- daily notes
- curated long-term memory
- derived retrieval index records
- optional external document-understanding outputs

## 13.2 Retrieval contract

A retrieval request should specify:

- query or intent
- scope
- session context
- maximum result count/size
- privacy/policy filters

Retrieval result should include:

- snippet text
- source path/reference
- relevance score or rank
- extraction boundaries
- timestamps where available

## 13.3 Privacy rules

Memory retrieval must preserve the same privacy boundaries that prompt assembly uses.
For example, files reserved for main-session use must not leak into shared or external contexts.

---

## 14. Skills and plugin protocol

## 14.1 Prompt skill contract

A prompt skill is a packaged behavior/instruction bundle.
It should expose:

- metadata
- trigger description
- instruction body
- optional references/assets/scripts
- capability expectations

Parity requirements:

- skill selection should remain inspectable and explainable, not magical
- only one clearly most-specific skill should be auto-loaded up front under the current documented behavior
- relative resource paths must resolve deterministically from the skill root

## 14.2 Native plugin contract

Native plugins are runtime extensions for tools, providers, processors, or integrations.

Minimum protocol requirements:

- manifest and version
- declared capabilities
- input/output schema
- failure isolation boundary
- timeout/cancellation handling
- structured logs
- upgrade compatibility strategy

For early phases, out-of-process isolation is the safer default.

### Native `PLUGIN.md` manifest v1

Rune-native plugins use a YAML frontmatter contract at the top of `PLUGIN.md`.
The runtime currently supports schema version `1`.

Required operator-visible behavior:

- `schema_version` defaults to `1` when omitted, but any other value is rejected with a field-specific diagnostic
- `name` defaults to the plugin directory name when omitted and must not be empty
- `version` defaults to `0.0.0` when omitted and must not be empty when provided
- `description` defaults to `Plugin: <name>` when omitted and must not be empty when provided
- `binary` defaults to `./plugin` when omitted and must not be empty when provided
- `capabilities` is a comma-separated declaration list; duplicate entries are rejected
- `hooks` is a comma-separated list of runtime hook event names; unknown events are rejected explicitly

Example:

```md
---
schema_version: 1
name: example-plugin
version: 1.2.3
description: Example native plugin
binary: ./bin/example
capabilities: hooks, commands
hooks: pre_tool_call, post_tool_call
---
```

Validation failures must be readable without source inspection. Runtime diagnostics render as:

- manifest path
- exact field name
- human-readable reason for rejection

This keeps plugin author feedback actionable in logs, doctor output, and future plugin management surfaces.

---

## 15. Config, doctor, and observability protocol

## 15.1 Layered config contract

The runtime must preserve layered config behavior with equivalent effect across:

- environment variables
- config files
- CLI overrides where applicable
- service profile vs CLI profile distinction
- provider/channel/agent-specific overrides

## 15.2 Config mutation and validation contract

The protocol must support workflows equivalent to:

- interactive configure/setup entry where expected by operators
- get
- set
- unset
- file inspection
- validation
- mismatch reporting

The operator must be able to tell when the CLI is talking to a runtime with materially different config than expected.

## 15.3 Doctor contract

Doctor is not just a health endpoint.
It must support check families equivalent to:

- daemon/service install state
- gateway auth/token state
- config parse/validation state
- path existence/writability
- DB/schema/version compatibility
- scheduler state sanity
- memory/index/search availability
- provider/channel credential completeness where configured
- model provider probe readiness where safe
- Docker mount/persistent-path sanity

Where supported, repair flows should distinguish between:

- detect only
- suggest fix
- perform fix interactively
- perform fix non-interactively when explicitly requested

## 15.4 Backup and restore workflow contract

Backup/restore is part of the operator contract, not an implementation afterthought.

### 15.4.1 Backup scope

A compliant backup workflow must preserve or explicitly account for:

- database state (`/data/db` or managed PostgreSQL equivalent)
- session/transcript artifacts (`/data/sessions`)
- memory files (`/data/memory`)
- media artifacts when retained (`/data/media`)
- skills/plugins bundles (`/data/skills`)
- logs/exports when operator policy requires them (`/data/logs`)
- backup archives/staging outputs (`/data/backups`)
- config overlays (`/config`)
- secret references/config metadata without exposing secret values (`/secrets` references, env-var names, vault paths)

### 15.4.2 Backup behavior expectations

Backup workflows must be:

- **documented** - operator can tell what is included and what is intentionally excluded
- **restorable** - output is suitable for reconstructing runtime state
- **image-independent** - no reliance on hidden image-layer state or container archaeology
- **mode-aware** - local embedded-PostgreSQL, mounted Docker, and managed-PostgreSQL deployments can differ in mechanism but not in operator clarity
- **secret-safe** - secret values never appear in archive manifests, logs, doctor output, or status surfaces

### 15.4.3 Restore behavior expectations

A restore workflow must, at minimum, be able to reconstruct enough durable state to recover:

- session and transcript inspectability
- memory files and curated workspace state
- scheduler/job state and next-run derivation
- pending approvals and related operator auditability
- config overlays and path layout expectations
- logs/exports when the operator explicitly treated them as part of the retained recovery set

Restore is a durable-state recovery contract, not a promise to resurrect every live runtime handle in place.
Live child-process attachment, PTY continuity, or in-flight turn continuation may be lost across restore unless explicitly implemented and documented.

Where a restore cannot fully recreate a subsystem (for example, external provider-managed data), the limitation must be stated explicitly in operator docs together with the required provider-native recovery step.

### 15.4.4 Minimum operator evidence

The project should provide enough operator-facing documentation and/or commands that a reviewer can verify:

- which operator workflow is shipped today (documented runbook/native tooling versus dedicated CLI)
- what to snapshot or export before upgrade
- where backup artifacts live (`/data/backups` or equivalent)
- how local paths map to Docker-mounted paths
- what preconditions are required for a clean restore
- what post-restore checks to run (`doctor`, health/status, scheduler state sanity)
- which degraded-recovery cases are expected (for example, provider-managed state, live process handles, or in-flight work)

### 15.4.5 Target CLI workflow contract (`rune backup`, not yet shipped)

The currently shipped operator-facing interface is the documented workflow in deployment/operator docs plus filesystem- and database-native tooling.
The `rune backup` command family remains the intended future primary interface for snapshot and recovery workflows.
When it ships, these commands must implement the behavioral expectations defined in §15.4.1–§15.4.4.

#### Planned subcommands

| Subcommand | Purpose |
|---|---|
| `rune backup create [--output <path>]` | Snapshot all durable state domains into a restorable archive |
| `rune backup restore <archive>` | Restore runtime state from a backup archive |
| `rune backup list` | List available backup archives in the configured backups directory |

#### `rune backup create` contract

1. **Quiesce coordination** — coordinate with the runtime to ensure database consistency (e.g., WAL checkpoint for embedded PostgreSQL) before capture.
2. **Scope** — capture all 9 durable domains per §15.4.1: db, sessions, memory, media (when retained), skills, logs (when policy requires), backups staging, config overlays, and secret references (never secret values).
3. **Output** — write a self-contained archive to `/data/backups` (or `~/.rune/backups/` locally) by default. `--output` overrides the destination path.
4. **Manifest** — the archive must include a manifest listing included domains, capture timestamp, runtime version, and any excluded domains with reason.
5. **Secret safety** — secret values must not appear in the archive, its manifest, or any log output during the operation. Secret references (env var names, vault paths) are preserved.

#### `rune backup restore` contract

1. **Precondition** — the runtime should be stopped or in a quiesced state before restore. The command must refuse to overwrite live state without explicit confirmation.
2. **Target layout** — restore into the same logical path layout that the running mode expects (`~/.rune/*` locally, `/data/*` + `/config` + `/secrets` in Docker).
3. **Post-restore verification** — on completion, emit a checklist of recommended verification steps: run `rune doctor`, check health/status endpoints, verify scheduler/job state, and inspect recent session/transcript history.
4. **Partial restore** — where a restore cannot fully recreate a subsystem (e.g., external managed PostgreSQL data, provider-side state), the command must state the limitation explicitly in its output.

#### `rune backup list` contract

1. List archives in the configured backups directory with timestamp, size, and manifest summary.
2. Indicate whether each archive was created by the current runtime version or an older one.

## 15.5 Logging and diagnostics contract

Every major action should emit structured events/logs with:

- timestamp
- subsystem
- entity IDs
- severity
- event type
- correlation IDs
- human-readable summary
- structured details

Observability parity target:
- enough information to debug session turns, tool actions, approvals, jobs, channel deliveries, config mismatches, and doctor failures without attaching a debugger or reading arbitrary internal state

---

## 16. Compatibility test contract

These protocols are not complete unless they are testable.
Each protocol family needs black-box tests.

## 16.1 Required parity test categories

1. CLI-to-gateway behavior tests
2. session turn lifecycle tests
3. tool lifecycle and approval tests
4. process/background continuation tests
5. scheduler/heartbeat tests
6. channel normalization and outbound routing tests
7. memory privacy-boundary tests
8. reconnect/replay tests for live event streams
9. Azure provider request-construction tests
10. doctor/status/config-mismatch tests
11. Docker mount/restart recovery tests

## 16.2 Golden behavior approach

For parity-critical flows, use golden traces from real OpenClaw behavior where practical:

- command inputs
- gateway responses
- event sequences
- transcript deltas
- approval prompts
- tool result shapes
- doctor/status outputs
- Azure request payloads

The Rust rewrite should be judged against those traces, not just against abstract design intent.

---

## 17. Explicit non-goals for protocol design

This document does not require:

- identical internal class/module names
- identical persistence schema to TypeScript OpenClaw
- identical HTTP route names if the same CLI/UI behaviors and machine contracts are preserved
- in-process plugin loading in phase 1

What it does require is preservation of the behavioral contract seen by operators, channels, tools, and tests.

---

## Issue #73 subagent control transport gap

The current client-visible `rune agents` surface supports:

- `list`
- `show`
- `status`
- `tree`
- `templates`
- `start --template`

`steer` and `kill` are not parity-shipped yet.

The blocker is not absence of all internal lifecycle logic. Rune already has internal subagent lifecycle/session transitions, but it does **not** yet expose a supported client-facing transport contract for interactive subagent control. Until that transport exists, parity status should be treated as:

- inspectable: yes
- startable: yes
- steerable through a supported public transport: no
- kill/cancel through a supported public transport: no

Any future parity claim for `steer` or `kill` must include both:

1. a callable client-facing transport surface, and
2. operator-visible success/failure semantics for those actions.
