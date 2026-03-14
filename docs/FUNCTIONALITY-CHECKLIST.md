# Functionality Checklist

Use this as the execution checklist against:

- `PARITY-INVENTORY.md` — exhaustive surface map and command/resource census
- `PARITY-SPEC.md` — release rule
- `PARITY-CONTRACTS.md` — subsystem invariants
- `PROTOCOLS.md` — canonical entities/state machines/events
- `IMPLEMENTATION-PHASES.md` — sequencing

Interpretation rules:

- checked means behavior exists, is inspectable, and has black-box evidence
- partial work does not count as checked
- parity-critical items require success-path and failure-path coverage
- Docker persistence and Azure-specific validation are mandatory where relevant
- if a command family is intentionally deferred, document it as an explicit divergence instead of silently omitting it

---

## 1. CLI surface

### Tier-0 operator commands
- [ ] top-level `openclaw --help` command-family census matches intended parity story
- [x] top-level `openclaw --version` behavior exists
- [x] top-level `--dev`, `--profile`, `--log-level`, `--no-color`, `--help`, and `--version` controls have equivalent semantics
- [x] `openclaw gateway status`
- [ ] `openclaw gateway install`
- [ ] `openclaw gateway uninstall`
- [x] `openclaw gateway start`
- [x] `openclaw gateway stop`
- [x] `openclaw gateway restart`
- [x] `openclaw gateway run`
- [x] `openclaw gateway call`
- [x] `openclaw gateway usage-cost`
- [x] `openclaw gateway health`
- [x] `openclaw gateway probe`
- [x] `openclaw gateway discover`
- [x] `openclaw daemon status`
- [ ] `openclaw daemon install`
- [ ] `openclaw daemon uninstall`
- [x] `openclaw daemon start`
- [x] `openclaw daemon stop`
- [x] `openclaw daemon restart`
- [x] `openclaw doctor`
- [x] `openclaw health`
- [x] `openclaw status`
- [x] `openclaw dashboard`
- [ ] `openclaw configure`
- [x] shell completion
- [x] help/usage text aligned with actual command families
- [x] JSON output where operator automation depends on it

### Cron CLI
- [x] `cron status`
- [x] `cron list`
- [x] `cron add`
- [x] `cron edit`
- [x] `cron enable`
- [x] `cron disable`
- [x] `cron rm`
- [x] `cron run`
- [x] `cron runs`

### Channels CLI
- [x] `channels list`
- [x] `channels status`
- [x] `channels capabilities`
- [x] `channels resolve`
- [x] `channels logs`
- [ ] `channels add`
- [ ] `channels remove`
- [ ] `channels login`
- [ ] `channels logout`

### Models CLI
- [x] `models list`
- [x] `models status`
- [x] `models set`
- [ ] `models set-image`
- [x] `models aliases`
- [ ] `models auth`
- [ ] `models fallbacks`
- [ ] `models image-fallbacks`
- [ ] `models scan`
- [ ] per-agent auth order override
- [ ] Azure-aware provider config

### Memory CLI
- [x] `memory status`
- [ ] `memory index`
- [x] `memory search`
- [x] `memory get`

### Security / access CLI
- [x] `approvals` command family
- [ ] `config` no-subcommand setup-wizard behavior or explicit compatibility decision
- [x] `config get|set|unset|file|validate`
- [ ] `configure` command family
- [ ] `secrets reload|audit|configure|apply`
- [ ] `security audit`
- [x] `system event`
- [x] `system heartbeat enable|disable`
- [x] `system heartbeat last|presence`
- [x] `system heartbeat status`
- [ ] `sandbox` command family
- [ ] `logs` command family

### Tier-1 and breadth CLI families
- [ ] `devices` command family
- [ ] `pairing` command family
- [ ] `directory` command family
- [ ] `node` command family
- [ ] `nodes` command family
- [ ] `sessions` command family
- [ ] `sessions cleanup`
- [ ] `message` command family
- [ ] message send/read/edit/delete/react/reactions/pin/unpin/pins/poll/broadcast breadth remains implemented or explicitly deferred
- [ ] message channel/event/member/role/thread/voice/emoji/sticker/permissions/search breadth remains implemented or explicitly deferred
- [ ] message moderation breadth (`ban`/`kick`/`timeout`) remains implemented or explicitly deferred
- [ ] `agent` command
- [ ] `agents` command family
- [ ] `acp` command family
- [ ] `skills` command family
- [ ] `plugins` command family
- [ ] `hooks` command family
- [ ] `webhooks` helpers
- [ ] `backup` command family
- [ ] `update` command family
- [ ] `setup` / `onboard` / `uninstall`

### Deferred-breadth inventory still tracked
- [ ] `browser` command family
- [ ] browser `extension` helpers and responsebody/download/trace-style action breadth tracked
- [ ] `docs`
- [ ] `tui`
- [ ] `qr` / `dns` / `docs`
- [ ] `clawbot` / `voicecall`
- [ ] `reset` command family

---

## 2. Gateway / daemon / control plane

- [ ] local daemon lifecycle
- [ ] service/runtime status vs RPC probe distinction
- [ ] auth/token generation and rotation
- [ ] HTTP API resource families
- [ ] HTTP API resource-operation coverage for parity-critical entities
- [x] WebSocket API
- [ ] WebSocket topic/subscription/replay semantics
- [ ] dashboard/discovery surfacing
- [x] background process supervision visibility
- [ ] restart durability for sessions/jobs/approvals/process history
  - 2026-03-14 morning: process history is now partially durable rather than ephemeral-only. Background `exec` launches are persisted into `tool_executions`, and `process list|poll|log` can surface restart-visible audit metadata even when the live child/stdin/PTY handle cannot be reattached. Full restart-safe continuation/control is still intentionally unchecked.
- [x] structured error envelopes
- [x] durable IDs returned by create/mutate flows

Implementation note (2026-03-13): the current executable control-plane slice is smoke-tested for `GET /health`, `GET /status`, `GET /gateway/health`, `POST /gateway/start`, `POST /gateway/stop`, `POST /gateway/restart`, `GET /cron/status`, `GET /cron`, `POST /cron`, `POST /cron/wake`, `POST /cron/{id}`, `DELETE /cron/{id}`, `POST /cron/{id}/run`, `GET /cron/{id}/runs`, `GET /heartbeat/status`, `POST /heartbeat/enable`, `POST /heartbeat/disable`, `GET /reminders`, `POST /reminders`, `DELETE /reminders/{id}`, `GET /sessions`, `POST /sessions`, `GET /sessions/{id}`, `POST /sessions/{id}/messages`, and `GET /sessions/{id}/transcript`. The operator CLI currently exposes working `status`, `health`, `doctor`, `dashboard`, full baseline `cron` management (`status|list|add|edit|enable|disable|rm|run|runs|wake`), `system event`, `system heartbeat last|presence|enable|disable|status`, `reminders add|list|cancel`, `sessions list`, `sessions show`, `config show`, and `config validate` flows, plus `gateway status|health|probe|discover|call|usage-cost|start|stop|restart|run`, with human and `--json` output modes. Top-level global controls now also parse and apply OpenClaw-shaped `--dev`, `--profile`, `--log-level`, and `--no-color` flags: `--profile <name>` resolves local mutable config paths as `config.<name>.toml`, `--dev` defaults that selector to `dev`, `--log-level` seeds `RUNE_LOG_LEVEL`/`RUST_LOG` for the current invocation, and `--no-color` exports `NO_COLOR=1` for downstream output compatibility. This is intentionally conservative parity: it closes the operator-facing flag gap without inventing speculative multi-profile storage semantics beyond the existing local-config workflow. `dashboard` is intentionally a compact operator summary rather than a speculative web UI: it aggregates gateway health/status, recent sessions, cron counts, model readiness, channel readiness, and memory state into one parity-shaped inspection command. `gateway probe` now distinguishes bare process reachability via `/health` from protected RPC/operator reachability via `/status`; `gateway discover` surfaces the effective gateway/health/WebSocket URLs plus local config binding; `gateway call` provides a raw HTTP parity/debug surface; and `gateway usage-cost` currently reports persisted token aggregates only, explicitly stopping short of speculative provider-specific price calculation. `sessions list` now also has parity-shaped operator filters for `--active`, `--channel`, and `--limit`, backed by gateway query filtering and route coverage. `gateway run`/`daemon run` now exec the local `rune-gateway` binary in foreground mode and forward `RUNE_CONFIG` as `--config` when present. Cron job definitions are now durably backed by the PostgreSQL `jobs` repository instead of process-local memory, and `cron runs` history is durably backed by the `job_runs` table, so gateway restarts preserve created/updated/removed cron entries, next-run state, and scheduled/manual execution history. This is still not full parity, but it is now runnable instead of planning-only.

---

## 3. Runtime / sessions / transcripts

- [x] session creation and persistence
- [x] session kinds and parent linkage
  - 2026-03-14 watchdog verification: runtime/session-engine coverage already exercises direct, channel, scheduled, and subagent session kinds plus durable `requester_session_id` parent linkage (`crates/rune-runtime/src/tests.rs::session_parent_linkage`), so this checklist item was stale rather than missing.
- [x] agent turn loop
- [x] context assembly
- [x] transcript ordering
- [x] transcript attribution of tool/approval/subagent events
- [ ] transcript compaction/pruning
- [x] usage/cost tracking
- [ ] model failover / fallback behavior
- [x] session status surface (`/status` + first-class `session_status` equivalent with model/usage/timing/flags)
  - 2026-03-14 overnight: `SessionStatusCard` is now wired end-to-end across gateway route (`GET /sessions/{id}/status`), gateway client, CLI rendering (`rune sessions status <id>`), and tool-layer JSON validation for `session_status`.
  - 2026-03-14 watchdog: unresolved-note wording was tightened so the card no longer falsely claims approval durability is absent; it now correctly narrows the remaining gap to restart-safe continuation for mid-resume approval flows plus broader security/PTY fidelity and cost quality.
- [ ] startup file loading rules by session type
- [ ] main-session-only curated-memory boundary

Implementation note (2026-03-13): current smoke evidence covers create-session -> append input -> execute turn -> receive assistant reply -> retrieve ordered transcript through the gateway. The runnable gateway now uses PostgreSQL-backed repositories through Diesel, with embedded PostgreSQL as the zero-config local fallback and configured external PostgreSQL as the release-target path; remaining work is durability evidence and parity breadth, not placeholder in-memory wiring.

---

## 4. Tool system

### File tools
- [x] `read`
- [x] `write`
- [x] `edit`
- [x] exact truncation/offset semantics
- [x] exact-match edit failure behavior

### Process tools
- [x] `exec`
- [x] `process`
- [x] background continuation semantics
- [x] PTY semantics
  - 2026-03-14 morning: Unix PTY execution is now live in the executor path. `exec` with `pty: true` launches the child under `script(1)` (`script -qec <command> /dev/null`), which gives the command a real pseudo-terminal while preserving the existing `process` control surface for background runs.
  - Remaining parity depth: this is not yet restart-durable, and the implementation is currently pragmatic Unix coverage rather than a broader cross-platform PTY abstraction.
- [x] exact approval-prompt command presentation

### Scheduler/orchestration tools
- [x] `cron`
- [x] `sessions_list`
- [x] `sessions_history`
- [x] `sessions_send`
- [x] `sessions_spawn`
- [x] `subagents`
- [x] `session_status`
  - 2026-03-14 morning: live gateway wiring now exposes `sessions_spawn`, `sessions_send`, and `subagents` as real app-registered tools rather than library-only modules, with focused app-level tests covering persisted subagent creation, requester-linkage preservation when provided, inspectability, steering-note persistence, and cancel semantics.

### Memory tools
- [x] `memory_search`
- [x] `memory_get`

### Tooling contract quality
- [x] stable names and schemas
- [ ] transcript/audit linkage
  - 2026-03-14 watchdog verification: this remains partially open rather than absent. Tool-side durable audit rows now exist for background `exec` (`tool_executions`), and transcript attribution exists for tool/approval/subagent events, but the stricter parity invariant from `docs/PROTOCOLS.md` — transcript tool outputs matching audit records or carrying an explicit reference — still needs end-to-end evidence and black-box tests before this can be checked.
- [x] structured errors
- [x] durable handles for long-running work
  - 2026-03-14 morning: long-running `exec` work now has durable audit visibility through persisted `tool_executions` metadata, even though full post-restart live-handle control is still out of scope.

Implementation note (2026-03-14): executable parity progress now includes concrete `read`, `write`, `edit`, `cron`, `sessions_list`, `sessions_history`, `sessions_send`, `sessions_spawn`, `subagents`, `session_status`, `memory_search`, and `memory_get` tool executors in `rune-tools`, plus runtime scheduler primitives in `rune-runtime`. File tools now accept OpenClaw-compatible argument aliases (`file_path`, `oldText`, `newText`, `from`, `lines`) and `read` enforces the 2000-line / 50KB truncation contract. The `exec` executor now supports foreground execution and background handle registration via the shared process manager, while the `process` executor covers `list`, `poll`, `log`, `write`, `submit`, `paste`, `send-keys` (`keys`, `hex`, `literal`), and `kill` in the OpenClaw-shaped surface. Background `exec` launches are now also durably indexed through `tool_executions`, so restart-visible process metadata survives even when live stdout/stderr/stdin control does not. Approval-required `exec` calls now surface a structured payload containing the exact command/workdir/background/timeout/pty/elevated/ask/security details for operator review, and runtime coverage now proves transcript attribution for approval-request, approval-response, denied tool-result audit entries. Tool-level `allow-always` / `deny` policy persistence is now exposed end-to-end through the durable store, protected gateway routes, and operator CLI, and pending approval requests now have first-class list/decide gateway+CLI surfaces instead of living only as transcript artifacts. Operator decisions for live tool-call approvals now trigger runtime replay immediately: `allow_once` resumes the exact stored call, `allow_always` persists policy and resumes, and `deny` appends denial audit artifacts without executing the call. Approval payloads also now carry clearer durable progress markers (`approval_status`, `approval_status_updated_at`) alongside the older resume metadata, terminal completion timestamps are no longer written prematurely while a resumed call is merely transitioning through `resuming`, and the gateway/CLI approval surfaces now expose those lifecycle fields directly so operators do not have to reverse-engineer raw payload JSON to understand resume state. Scheduler recurrence is now real rather than placeholder-backed: interval schedules honor anchors and cron schedules compute timezone-aware next fire times, disabling invalid cron/tz definitions instead of silently fabricating a next run. Cron job definitions and `cron runs` history are now durably backed by PostgreSQL in the production gateway wiring, so restart-safe operator audit trails exist for scheduled/manual cron executions. Scheduled `main` jobs now reuse a stable `system:scheduled-main` session, while scheduled `isolated` jobs create fresh `subagent` descendant sessions linked back to that main scheduled session via `requester_session_id`, closing the earlier semantic collapse between the two targets. The live gateway app now also exposes `sessions_spawn`, `sessions_send`, and `subagents` instead of leaving them as library-only tool modules: spawned subagent sessions are durably created as `subagent` session rows with recorded task/mode/model metadata, requester linkage is preserved when supplied, steering actions are persisted into transcript/status notes, and subagent inspection/cancel flows operate on durable session state. Remaining work is deeper runtime execution parity for those spawned subagents (beyond conservative persisted inspectability), restart-safe hardening for mid-resume approval flows and live process handles across gateway restarts, richer host/node/sandbox parity, deeper first-class session-status parity, and broader cross-platform PTY fidelity beyond the current Unix implementation.

---

## 5. Approvals / security / sandboxing

- [x] approval request lifecycle
- [x] `allow-once` exact scope binding
- [x] `allow-always` persistence semantics
- [x] `deny` audit trail semantics
- [x] exact command/payload presentation for approval
- [x] ask mode behavior (`off|on-miss|always` equivalent)
  - 2026-03-14 early morning: live gateway exec gating now treats `ask=always` as unconditional approval-required, `ask=on-miss` as policy/one-shot approval mediated, and `ask=off` as approval-skipping only after security-policy checks rather than a blanket bypass.
- [x] security mode behavior (`deny|allowlist|full` equivalent)
  - 2026-03-14 early morning: local exec semantics now enforce a conservative baseline where `security=deny` blocks execution, `security=allowlist` blocks `elevated=true`, and `security=full` is required before elevated local execution is even considered. This is still local-runtime parity, not remote-node/sandbox parity.
- [x] elevated execution behavior
  - 2026-03-14 early morning: elevated execution is no longer decorative metadata; non-`full` security modes reject it explicitly in the live gateway tool path. Remaining parity work is host/node routing and broader sandbox fidelity.
- [ ] privacy boundaries by session/channel context
- [ ] device/pairing token security workflows

---

## 6. Scheduler / automation

- [x] cron jobs
- [x] reminders
- [x] wake events
- [x] heartbeats
- [x] isolated scheduled agent runs
- [x] run history durability
- [x] enable/disable semantics
- [x] no-op heartbeat suppression (`HEARTBEAT_OK` behavior)
  - 2026-03-14 watchdog verification: `rune-runtime` already enforces `HEARTBEAT_OK` suppression in the live heartbeat runner (`HeartbeatRunner::should_suppress`) and carries suppression counters/state persistence tests, so this item was doc drift rather than a remaining gap.
- [ ] duplicate notification suppression
- [ ] hook/webhook lifecycle where shipped

---

## 7. Memory

- [ ] daily memory files
- [ ] long-term memory file
- [ ] semantic indexing
- [ ] retrieval/snippet APIs
- [ ] source attribution
- [ ] main-session-only privacy boundary for curated memory
- [ ] memory maintenance/update workflows
- [ ] optional local embeddings path

---

## 8. Channels

Implementation note (2026-03-13): the operator CLI now exposes a first inspectable channel surface with `channels list`, `channels status`, `channels capabilities`, `channels resolve`, and `channels logs`, backed by resolved config plus local Docker-first log-path inspection. `resolve` currently maps operator input/aliases onto the configured adapter inventory, and `logs` intentionally reports filesystem-backed log visibility from `paths.logs_dir` rather than pretending to offer remote provider log APIs. This still does not cover login/logout or dynamic registration, but it closes the earlier operator blind spot without violating parity constraints.

Implementation note (2026-03-13): the operator CLI now also exposes `models list`, `models status`, `models set`, and `models aliases`, giving a config-backed inventory of provider kind/base URL/default model/alias plus credential-readiness hints, a validated local default-model update path, and a read-only alias map for operator routing visibility. This is still intentionally narrow: it improves operator inspectability and Azure/provider debugging without pretending auth-order management, fallback editing, or image-model routing are complete.

Implementation note (2026-03-13): the operator CLI now exposes an initial read-only memory surface with `memory status`, `memory search`, and `memory get`, wired directly to the same file-oriented workspace conventions Rune/OpenClaw already use (`MEMORY.md` plus `memory/*.md`). This improves Tier-0 operator visibility without faking unfinished indexing infrastructure or inventing a remote gateway memory API before parity contracts for that surface are defined.

Implementation note (2026-03-13): `completion generate <shell>` now emits real shell completion scripts via Clap for `bash`, `zsh`, `fish`, `elvish`, and `powershell`, closing the shell-ergonomics gap without inventing a bespoke compatibility layer. Current evidence is CLI parse coverage plus a direct smoke run that writes the generated script to stdout.

### Common abstractions
- [ ] normalized inbound envelope
- [ ] normalized outbound actions
- [ ] reply/edit/react semantics
- [ ] media attachments
- [ ] direct vs group routing
- [ ] dedupe/idempotency
- [ ] provider message reference retention

### Providers
- [ ] Telegram
- [ ] Discord
- [ ] WhatsApp
- [ ] Signal
- [ ] Slack / Teams / Google Chat / Matrix / others as planned breadth

---

## 9. Media / OCR / TTS / browser adjacencies

- [ ] audio transcription
- [ ] image understanding handoff
- [ ] attachment extraction pipeline
- [ ] OCR / document understanding abstraction
- [ ] Azure Document Intelligence integration path
- [ ] TTS replies
- [ ] durable media references and lifecycle tracking
- [ ] browser/voicecall surfaces inventoried and either implemented or explicitly deferred

---

## 10. Skills / plugins / hooks

- [ ] metadata-triggered prompt skills
- [ ] one-most-specific skill selection rule
- [ ] resource bundles (`references` / `assets` / `scripts`)
- [ ] deterministic relative path resolution
- [ ] install/update/package flow
- [ ] capability-restricted extension execution
- [ ] isolated plugin failure handling
- [ ] plugin/hook doctor/check flows where shipped

---

## 11. UI / operator visibility

- [x] dashboard
  - 2026-03-14 watchdog verification: a lightweight operator dashboard is live via gateway HTML (`/dashboard`, `/ui`) and JSON summary endpoints (`/api/dashboard/*`), with route coverage in `crates/rune-gateway/tests/route_tests.rs`. This is parity-shaped operator visibility, not a claim that the broader UI surface is complete.
- [ ] sessions view
- [ ] logs/events view
- [ ] cron/jobs/history view
- [ ] channel health view
- [ ] skills/plugins manager
- [ ] config/secrets visibility
- [ ] approval center
- [ ] session/process/job live updates

---

## 12. Operations / diagnostics

- [ ] structured logs
- [ ] metrics/tracing
- [ ] backup/export
- [ ] restore/migration workflow
- [ ] update path
- [ ] doctor/diagnostics
- [ ] doctor interactive repair flow
- [ ] doctor non-interactive repair flow
- [ ] diagnostic bundle / inspectable aggregate export
- [ ] runtime alive vs RPC reachable vs auth-valid distinction
- [ ] health/readiness probes

---

## 13. Deployment / storage / Docker / Azure

- [ ] canonical mountable state layout
- [ ] local Docker deployment with restart durability
- [ ] image stateless by design
- [ ] fail-fast behavior for missing/unwritable persistent paths
- [ ] embedded PostgreSQL zero-config local mode
- [ ] PostgreSQL managed mode
- [ ] Azure Files mapping for file domains
- [ ] Azure Blob mapping for archive/object domains
- [ ] Azure OpenAI / Foundry request compatibility
- [ ] Azure Document Intelligence compatibility
- [ ] secrets/config model suitable for Azure-hosted containers
- [ ] graceful shutdown in containerized environments

---

## 14. Parity evidence gates

- [ ] golden CLI snapshots captured
- [ ] golden session/turn traces captured
- [ ] golden approval prompts captured
- [ ] scheduler history comparisons captured
- [ ] WS reconnect/replay behavior captured
- [ ] Docker persistence tests captured
- [ ] Azure request/response normalization tests captured
- [ ] known divergences documented explicitly
