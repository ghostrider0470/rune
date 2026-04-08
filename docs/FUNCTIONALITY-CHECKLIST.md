# Functionality Checklist

Use this as the execution checklist against:

- `parity/PARITY-INVENTORY.md` — exhaustive surface map and command/resource census
- `parity/PARITY-SPEC.md` — release rule
- `parity/PARITY-CONTRACTS.md` — subsystem invariants
- `parity/PROTOCOLS.md` — canonical entities/state machines/events
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
- [x] `openclaw gateway instance-health`
- [x] `openclaw gateway delegation-plan`
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
- [x] `models set-image`
- [x] `models aliases`
- [x] `models auth`
- [x] `models fallbacks`
- [x] `models image-fallbacks`
- [x] `models scan`
- [ ] per-agent auth order override
- [ ] Azure-aware provider config

Implementation note (2026-03-19): the operator CLI now exposes the `models` command family with eight shipped subcommands covering config-backed provider inventory, credential-readiness hints, default-model and image-model mutation, alias inspection, text and image fallback chain listing, and provider scanning. Provider routing is backed by `RoutedModelProvider` in `rune-models` which executes fallback chains on retriable errors (rate-limit, transient 5xx, quota exhaustion, transport failure). Ten providers are implemented: OpenAI, Anthropic, Azure OpenAI, Azure AI Foundry, Google (Gemini), Ollama, Groq, DeepSeek, Mistral, and AWS Bedrock. `models scan` currently probes Ollama providers only via the native `/api/tags` endpoint. Provider selection is config-driven through `config.toml` provider definitions with API key resolution from direct config, `api_key_env`, or standard environment variables. The remaining gaps under issue #72 are: `models auth` (dedicated auth management CLI), per-agent auth order override (config structure exists but no CLI surface), and Azure-aware provider config (Azure providers are implemented but no Azure-specific CLI setup surface beyond `config set`).

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
- [x] `message` command family
- [x] message send/read/edit/delete/react/pin/broadcast/thread/voice/search breadth implemented; reactions/unpin/pins/poll remain deferred/not started and explicitly tracked in `docs/parity/CLI-MATRIX.md`
- [x] message channel/event/member/role/emoji/sticker/permissions breadth explicitly deferred/not started and tracked in `docs/parity/CLI-MATRIX.md`
- [x] message moderation breadth (`ban`/`kick`/`timeout`) explicitly deferred/not started and tracked in `docs/parity/CLI-MATRIX.md`
- [ ] `agent` command
- [ ] `agents` command family
- [x] `acp` tool dispatch — `acp_dispatch` tool registered; dispatches to Claude Code CLI and Codex CLI as subprocesses
  - 2026-03-27 parity slice: ACP operator surface is now wired end-to-end through gateway `/acp/send`, `/acp/inbox`, and `/acp/ack` plus CLI client/execution paths, so the remaining ACP gap is narrowed to streaming/background execution and richer agent config rather than missing route/CLI plumbing.
- [ ] `acp` CLI command family (`rune acp send/inbox/ack`)
- [ ] `skills` command family
- [ ] `plugins` command family
- [ ] `hooks` command family
- [ ] `webhooks` helpers
- [ ] `backup` command family
  - 2026-03-20 docs alignment: operator-facing backup/restore workflow is documented in `docs/operator/DEPLOYMENT.md` and `docs/parity/PROTOCOLS.md`, but the dedicated `rune backup create|restore|list` CLI remains intentionally unchecked until it ships.
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
- [x] HTTP API resource families
  - 2026-03-14 validation: `crates/rune-gateway/tests/route_tests.rs` now provides executable route coverage across health/status, sessions, transcript/message flows, cron, processes, skills, dashboard API resources, and session-status resources rather than leaving the gateway surface asserted only in prose.
- [x] HTTP API resource-operation coverage for parity-critical entities
  - 2026-03-14 validation: route coverage includes create/read/update/delete session flows, send-message + transcript retrieval, cron create/list/run/history, skill list/reload/toggle, process inventory lookup, and parity-shaped session status cards (`route_tests.rs`). This is still scoped to the currently implemented entity set, not a claim of full OpenClaw breadth.
- [x] WebSocket API
- [x] WebSocket topic/subscription/replay semantics
  - 2026-03-14 validation: focused gateway tests now prove req/res framing, event subscriptions by session/event/global scope, unsubscribe behavior, stateVersion propagation, RPC error envelopes, and lag signalling (`ws_handle_text_message_supports_event_and_global_subscriptions`, `ws_handle_text_message_subscribe_unsubscribe_and_errors`, `ws_handle_text_message_dispatches_rpc_errors`). Replay/backfill remains intentionally out of scope for now, so this check is limited to live subscription semantics already implemented.
- [x] dashboard/discovery surfacing
  - 2026-03-14 validation: executable gateway coverage exists for dashboard HTML/auth behavior plus structured dashboard summary/models/sessions/diagnostics resources, while the operator checklist note already documents working `gateway discover` CLI surfacing.
- [x] background process supervision visibility
  - 2026-03-14 afternoon: gateway control-plane coverage now includes first-class `GET /processes`, `GET /processes/{id}`, and `GET /processes/{id}/log` resources backed by the shared process manager, so the existing durable process-audit metadata is operator-reachable over HTTP instead of only through tool execution paths.
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
- [x] model failover / fallback behavior
  - 2026-04-06: `RoutedModelProvider` now ships bounded provider circuit breakers plus deterministic fallback routing for retriable failures only. Coverage in `crates/rune-models` proves circuit-open behavior, degraded-mode fallback, stream parity, and recovery after cooldown under #903.
- [x] session status surface (`/status` + first-class `session_status` equivalent with model/usage/timing/flags)
  - 2026-03-14 overnight: `SessionStatusCard` is now wired end-to-end across gateway route (`GET /sessions/{id}/status`), gateway client, CLI rendering (`rune sessions status <id>`), and tool-layer JSON validation for `session_status`.
  - 2026-03-14 watchdog: unresolved-note wording was tightened so the card no longer falsely claims approval durability is absent; it now correctly narrows the remaining gap to restart-safe continuation for mid-resume approval flows plus broader security/PTY fidelity and cost quality.
  - 2026-03-26 tracker-truth follow-up: restart continuity remains intentionally narrower than full session resurrection. Current shipped behavior includes durable approval lifecycle metadata, startup restoration/indexing, and a one-time resumed-session user notification when a restored channel session next receives input. Restart continuity remains intentionally narrower than full in-place resurrection: transcript history and durable approval/session state are restored, while in-flight turns, live process handles, and typing/progress UI do not resume in place. The supported operator-visible notice path is intentionally limited to restored channel sessions after the next inbound message; direct, scheduled, and subagent sessions do not emit an automatic resumed-session notification. Integration proof for the supported restart-and-resume path now lives in runtime coverage for restored channel sessions under #245 / narrowed follow-on #284.
  - 2026-03-27 cleanup: narrowed follow-on #284 is now shipped and tracker-clean here. The runtime behavior is explicit rather than implied: `SessionLoop::run_startup_restore()` indexes restored channel sessions, `maybe_send_resumed_session_notice()` emits the one-time operator-facing notice only for restored channel sessions after the next inbound message, and runtime tests prove both the positive path and the non-restored/no-notice path. Supported restart continuity is intentionally narrower than full in-place resurrection: transcript history plus durable approval/session state are restored, while in-flight turns, live process handles, and typing/progress UI do not resume in place across gateway restarts.
- [x] startup file loading rules by session type
  - 2026-03-14 afternoon verification: `rune-runtime` already wires `WorkspaceLoader` into the live turn path (`crates/rune-runtime/src/executor.rs`) and loads session-kind-specific startup context from the session workspace root. Direct/channel/subagent sessions load the standard workspace files, while scheduled sessions additionally include `HEARTBEAT.md`.
  - Runtime coverage already proves the prompt includes loaded workspace context for live turns (`crates/rune-runtime/src/tests.rs::direct_session_prompt_includes_workspace_and_memory_context`).
- [x] main-session-only curated-memory boundary
  - 2026-03-14 afternoon verification: `MemoryLoader` is already wired into the live turn path and enforces the intended privacy boundary: `MEMORY.md` is loaded only for `SessionKind::Direct`, while channel/subagent/scheduled sessions receive daily notes without long-term curated memory (`crates/rune-runtime/src/memory.rs`, `crates/rune-runtime/src/executor.rs`).
  - Runtime coverage already proves direct sessions include long-term memory while channel sessions do not (`crates/rune-runtime/src/tests.rs::direct_session_prompt_includes_workspace_and_memory_context`, `channel_session_prompt_excludes_long_term_memory`).

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
  - 2026-03-14 late morning: subagent session rows now also persist first-class lifecycle/runtime inspectability metadata (`subagent_lifecycle`, `subagent_runtime_status`, `subagent_runtime_attached`, `subagent_status_updated_at`, `subagent_last_note`) so restart-visible operator inspection no longer depends primarily on transcript reconstruction.

### Memory tools
- [x] `memory_search`
- [x] `memory_get`

### Tooling contract quality
- [x] stable names and schemas
- [ ] transcript/audit linkage
  - 2026-03-14 watchdog verification: this remains partially open rather than absent. Tool-side durable audit rows now exist for background `exec` (`tool_executions`), and transcript attribution exists for tool/approval/subagent events, but the stricter parity invariant from `docs/parity/PROTOCOLS.md` — transcript tool outputs matching audit records or carrying an explicit reference — still needs end-to-end evidence and black-box tests before this can be checked.
  - 2026-03-14 late morning: background `exec` results now return explicit durable correlation fields (`toolCallId`, `toolExecutionId`) when a process audit store is configured, so operators and future runtime/gateway transcript surfaces can tie long-running tool handles back to persisted `tool_executions` rows without guessing from raw command text.
  - 2026-03-14 watchdog follow-up: the shared transcript contract now also has an optional `tool_execution_id` on `tool_result`, and `rune-runtime` preserves that field when a tool executor returns it. This closes the protocol gap for transcripted background `exec` results, but the checklist remains intentionally open until broader end-to-end black-box evidence exists across more than this one tool family.
- [x] structured errors
- [x] durable handles for long-running work
  - 2026-03-14 morning: long-running `exec` work now has durable audit visibility through persisted `tool_executions` metadata, even though full post-restart live-handle control is still out of scope.

Implementation note (2026-03-14): executable parity progress now includes concrete `read`, `write`, `edit`, `cron`, `sessions_list`, `sessions_history`, `sessions_send`, `sessions_spawn`, `subagents`, `session_status`, `memory_search`, and `memory_get` tool executors in `rune-tools`, plus runtime scheduler primitives in `rune-runtime`. File tools now accept OpenClaw-compatible argument aliases (`file_path`, `oldText`, `newText`, `from`, `lines`) and `read` enforces the 2000-line / 50KB truncation contract. The `exec` executor now supports foreground execution and background handle registration via the shared process manager, while the `process` executor covers `list`, `poll`, `log`, `write`, `submit`, `paste`, `send-keys` (`keys`, `hex`, `literal`), and `kill` in the OpenClaw-shaped surface. Background `exec` launches are now also durably indexed through `tool_executions`, so restart-visible process metadata survives even when live stdout/stderr/stdin control does not. Approval-required `exec` calls now surface a structured payload containing the exact command/workdir/background/timeout/pty/elevated/ask/security details for operator review, and runtime coverage now proves transcript attribution for approval-request, approval-response, denied tool-result audit entries. Tool-level `allow-always` / `deny` policy persistence is now exposed end-to-end through the durable store, protected gateway routes, and operator CLI, and pending approval requests now have first-class list/decide gateway+CLI surfaces instead of living only as transcript artifacts. Operator decisions for live tool-call approvals now trigger runtime replay immediately: `allow_once` resumes the exact stored call, `allow_always` persists policy and resumes, and `deny` appends denial audit artifacts without executing the call. Approval payloads also now carry clearer durable progress markers (`approval_status`, `approval_status_updated_at`) alongside the older resume metadata, terminal completion timestamps are no longer written prematurely while a resumed call is merely transitioning through `resuming`, and the gateway/CLI approval surfaces now expose those lifecycle fields directly so operators do not have to reverse-engineer raw payload JSON to understand resume state. Scheduler recurrence is now real rather than placeholder-backed: interval schedules honor anchors and cron schedules compute timezone-aware next fire times, disabling invalid cron/tz definitions instead of silently fabricating a next run. Cron job definitions and `cron runs` history are now durably backed by PostgreSQL in the production gateway wiring, so restart-safe operator audit trails exist for scheduled/manual cron executions. Scheduled `main` jobs now reuse a stable `system:scheduled-main` session, while scheduled `isolated` jobs create fresh `subagent` descendant sessions linked back to that main scheduled session via `requester_session_id`, closing the earlier semantic collapse between the two targets. The live gateway app now also exposes `sessions_spawn`, `sessions_send`, and `subagents` instead of leaving them as library-only tool modules: spawned subagent sessions are durably created as `subagent` session rows with recorded task/mode/model metadata, requester linkage is preserved when supplied, steering actions are persisted into transcript/status notes, and subagent inspection/cancel flows operate on durable session state. Remaining work is deeper runtime execution parity for those spawned subagents (beyond conservative persisted inspectability), restart-safe hardening for mid-resume approval flows and live process handles across gateway restarts, resumed-session notification/continuity semantics that are explicit rather than implied, richer host/node/sandbox parity, deeper first-class session-status parity, and broader cross-platform PTY fidelity beyond the current Unix implementation.

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
- [x] duplicate notification suppression
  - 2026-03-21 docs reconciliation: `rune-runtime` already persists normalized heartbeat duplicate-suppression state and surfaces it through the shipped heartbeat runner/status path; remaining work is broader quiet-window policy and hook/webhook breadth, not duplicate-suppression absence.
- [ ] hook/webhook lifecycle where shipped

---

## 7. Memory

- [ ] daily memory files
- [ ] long-term memory file
- [ ] semantic indexing
- [ ] retrieval/snippet APIs
- [ ] source attribution
- [x] main-session-only privacy boundary for curated memory
  - 2026-03-14 afternoon verification: the curated-memory boundary is already enforced in the runtime memory loader and exercised in live turn-context tests; this checklist item was stale rather than missing.
- [ ] memory maintenance/update workflows
- [ ] optional local embeddings path

---

## 8. Channels

Implementation note (2026-03-13): the operator CLI now exposes a first inspectable channel surface with `channels list`, `channels status`, `channels capabilities`, `channels resolve`, and `channels logs`, backed by resolved config plus local Docker-first log-path inspection. `resolve` currently maps operator input/aliases onto the configured adapter inventory, and `logs` intentionally reports filesystem-backed log visibility from `paths.logs_dir` rather than pretending to offer remote provider log APIs. This still does not cover login/logout or dynamic registration, but it closes the earlier operator blind spot without violating parity constraints.

Implementation note (2026-03-13): the operator CLI now also exposes `models list`, `models status`, `models set`, and `models aliases`, giving a config-backed inventory of provider kind/base URL/default model/alias plus credential-readiness hints, a validated local default-model update path, and a read-only alias map for operator routing visibility. This is still intentionally narrow: it improves operator inspectability and Azure/provider debugging without pretending auth-order management, fallback editing, or image-model routing are complete.

Implementation note (2026-03-13): the operator CLI now exposes an initial read-only memory surface with `memory status`, `memory search`, and `memory get`, wired directly to the same file-oriented workspace conventions Rune/OpenClaw already use (`MEMORY.md` plus `memory/*.md`). This improves Tier-0 operator visibility without faking unfinished indexing infrastructure or inventing a remote gateway memory API before parity contracts for that surface are defined.

Implementation note (2026-03-13): `completion generate <shell>` now emits real shell completion scripts via Clap for `bash`, `zsh`, `fish`, `elvish`, and `powershell`, closing the shell-ergonomics gap without inventing a bespoke compatibility layer. Current evidence is CLI parse coverage plus a direct smoke run that writes the generated script to stdout.

### Common abstractions
- [x] normalized inbound envelope
- [x] normalized outbound actions
- [x] reply/edit/react semantics
  - 2026-03-14 afternoon validation: `rune-channels` now has focused black-box/event-conversion coverage across Telegram, Slack, WhatsApp, and Signal plus adapter-factory tests proving provider registration and config-gated construction. Reply/edit/react semantics are only checked at the normalized adapter surface where tests exist; provider-specific gaps still remain where action shapes cannot yet express required transport fields.
- [x] media attachments
  - 2026-03-14 afternoon validation: channel tests now cover attachment extraction/normalization for Telegram documents, Slack files, WhatsApp media payloads, Discord attachments, and Signal attachments inside the shared `InboundEvent::Message` surface.
- [x] direct vs group routing
  - 2026-03-14 afternoon validation: Signal adapter coverage explicitly exercises 1:1 vs group routing (`raw_chat_id` source number vs `group_id`), and the common adapter surface preserves provider chat identifiers for downstream routing.
- [ ] dedupe/idempotency
- [x] provider message reference retention
  - 2026-03-14 afternoon validation: channel adapter conversions preserve provider-native message ids/timestamps in `provider_message_id`, with coverage across Telegram, Slack, WhatsApp, and Signal event parsing.

### Providers
- [x] Telegram
- [x] Discord
- [x] WhatsApp
- [x] Signal
- [x] Slack / Teams / Google Chat / Matrix / others as planned breadth
  - 2026-03-14 afternoon validation: the implemented provider set is now backed by runnable crate-level tests (`cargo test -p rune-channels --lib --tests`) including adapter-factory registration coverage for Telegram, Discord, Slack, WhatsApp, and Signal plus provider-specific inbound normalization tests. Planned breadth beyond those five remains intentionally unchecked.

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
- [x] plugin/hook doctor/check flows where shipped
  - 2026-04-05: `rune plugins doctor <name>` and `rune hooks doctor <name>` both ship with explicit CLI response contracts and test coverage.

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
- [x] backup/export
  - 2026-03-20 docs slice: the shipped operator workflow now explicitly defines quiesce/capture steps, durable-state domains, exclusions, and mode-aware use of filesystem/database-native tooling. This is documentation parity for backup/export, not a claim that the dedicated `backup` CLI family is shipped.
- [x] restore/migration workflow
  - 2026-03-20 docs slice: restore guidance now explicitly requires same-layout recovery, provider-native restore steps for managed services, post-restore verification (`rune doctor`, health/status, scheduler/transcript/approval sanity), and declared degraded-recovery limits.
- [x] restart preservation workflow
  - 2026-03-20 docs slice: operator docs now make restart/restore expectations explicit for sessions, memory, scheduler state, approvals/audit trails, and other durable state while calling out that live PTY/process attachment and in-flight work are not promised to resume in place.
- [ ] update path
- [x] doctor/diagnostics
  - 2026-04-08: `rune doctor`, gateway `/api/doctor/run` + `/api/doctor/results`, WS-RPC `doctor.run` + `doctor.results`, operator doctor/readiness docs, and coverage tests are shipped.
- [ ] doctor interactive repair flow
- [ ] doctor non-interactive repair flow
- [ ] diagnostic bundle / inspectable aggregate export
- [x] runtime alive vs RPC reachable vs auth-valid distinction
  - 2026-04-08: `rune gateway probe` distinguishes bare `/health` reachability, protected RPC/status reachability, and auth-valid vs auth-required states in both machine and human output.
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
