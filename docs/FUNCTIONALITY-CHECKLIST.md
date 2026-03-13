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
- [ ] top-level `--dev`, `--profile`, `--log-level`, `--no-color`, `--help`, and `--version` controls have equivalent semantics
- [x] `openclaw gateway status`
- [ ] `openclaw gateway install`
- [ ] `openclaw gateway uninstall`
- [x] `openclaw gateway start`
- [x] `openclaw gateway stop`
- [x] `openclaw gateway restart`
- [ ] `openclaw gateway run`
- [ ] `openclaw gateway call`
- [ ] `openclaw gateway usage-cost`
- [x] `openclaw gateway health`
- [ ] `openclaw gateway probe`
- [ ] `openclaw gateway discover`
- [ ] `openclaw daemon status`
- [ ] `openclaw daemon install`
- [ ] `openclaw daemon uninstall`
- [ ] `openclaw daemon start`
- [ ] `openclaw daemon stop`
- [ ] `openclaw daemon restart`
- [x] `openclaw doctor`
- [x] `openclaw health`
- [x] `openclaw status`
- [ ] `openclaw dashboard`
- [ ] `openclaw configure`
- [ ] shell completion
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
- [ ] `channels list`
- [ ] `channels status`
- [ ] `channels capabilities`
- [ ] `channels resolve`
- [ ] `channels logs`
- [ ] `channels add`
- [ ] `channels remove`
- [ ] `channels login`
- [ ] `channels logout`

### Models CLI
- [ ] `models list`
- [ ] `models status`
- [ ] `models set`
- [ ] `models set-image`
- [ ] `models aliases`
- [ ] `models auth`
- [ ] `models fallbacks`
- [ ] `models image-fallbacks`
- [ ] `models scan`
- [ ] per-agent auth order override
- [ ] Azure-aware provider config

### Memory CLI
- [ ] `memory status`
- [ ] `memory index`
- [ ] `memory search`

### Security / access CLI
- [ ] `approvals` command family
- [ ] `config` no-subcommand setup-wizard behavior or explicit compatibility decision
- [ ] `config get|set|unset|file|validate`
- [ ] `configure` command family
- [ ] `secrets reload|audit|configure|apply`
- [ ] `security audit`
- [ ] `system event`
- [ ] `system heartbeat last|enable|disable|presence`
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
- [ ] `qr` / `dns` / `docs` / `completion`
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
- [x] structured error envelopes
- [x] durable IDs returned by create/mutate flows

Implementation note (2026-03-13): the current executable control-plane slice is smoke-tested for `GET /health`, `GET /status`, `GET /gateway/health`, `POST /gateway/start`, `POST /gateway/stop`, `POST /gateway/restart`, `GET /cron/status`, `GET /cron`, `POST /cron`, `POST /cron/{id}`, `DELETE /cron/{id}`, `POST /cron/{id}/run`, `GET /cron/{id}/runs`, `GET /sessions`, `POST /sessions`, `GET /sessions/{id}`, `POST /sessions/{id}/messages`, and `GET /sessions/{id}/transcript`. The operator CLI currently exposes working `status`, `health`, `doctor`, full baseline `cron` management (`status|list|add|edit|enable|disable|rm|run|runs`), `sessions list`, `sessions show`, `config show`, and `config validate` flows, plus `gateway status|health|start|stop|restart`, with human and `--json` output modes. This is still not full parity, but it is now runnable instead of planning-only.

---

## 3. Runtime / sessions / transcripts

- [x] session creation and persistence
- [ ] session kinds and parent linkage
- [x] agent turn loop
- [x] context assembly
- [x] transcript ordering
- [x] transcript attribution of tool/approval/subagent events
- [ ] transcript compaction/pruning
- [x] usage/cost tracking
- [ ] model failover / fallback behavior
- [ ] session status surface (`/status` + `session_status` equivalent)
- [ ] startup file loading rules by session type
- [ ] main-session-only curated-memory boundary

Implementation note (2026-03-13): current smoke evidence covers create-session -> append input -> execute turn -> receive assistant reply -> retrieve ordered transcript through the gateway. The current gateway app uses a transitional in-memory runtime path for executability; release-target durable behavior remains PostgreSQL-backed.

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
- [ ] PTY semantics
- [ ] exact approval-prompt command presentation

### Scheduler/orchestration tools
- [x] `cron`
- [x] `sessions_list`
- [x] `sessions_history`
- [x] `sessions_send`
- [x] `sessions_spawn`
- [x] `subagents`
- [x] `session_status`

### Memory tools
- [x] `memory_search`
- [x] `memory_get`

### Tooling contract quality
- [x] stable names and schemas
- [ ] transcript/audit linkage
- [x] structured errors
- [x] durable handles for long-running work

Implementation note (2026-03-13): executable parity progress now includes concrete `read`, `write`, `edit`, `cron`, `sessions_list`, `sessions_history`, `sessions_send`, `sessions_spawn`, `subagents`, `session_status`, `memory_search`, and `memory_get` tool executors in `rune-tools`, plus runtime scheduler primitives in `rune-runtime`. File tools now accept OpenClaw-compatible argument aliases (`file_path`, `oldText`, `newText`, `from`, `lines`) and `read` enforces the 2000-line / 50KB truncation contract. The `exec` executor now supports foreground execution and background handle registration via the shared process manager, while the `process` executor covers `list`, `poll`, `log`, `write`, `submit`, `paste`, `send-keys` (`keys`, `hex`, `literal`), and `kill` in the OpenClaw-shaped surface. Approval-required `exec` calls now surface a structured payload containing the exact command/workdir/background/timeout/pty/elevated/ask/security details for operator review, and runtime coverage now proves transcript attribution for approval-request, approval-response, and denied tool-result audit entries. Remaining work is allow-once/allow-always persistence, restart durability, PTY fidelity, and runtime-backed subagent lifecycle persistence.

---

## 5. Approvals / security / sandboxing

- [x] approval request lifecycle
- [ ] `allow-once` exact scope binding
- [ ] `allow-always` persistence semantics
- [x] `deny` audit trail semantics
- [x] exact command/payload presentation for approval
- [ ] ask mode behavior (`off|on-miss|always` equivalent)
- [ ] security mode behavior (`deny|allowlist|full` equivalent)
- [ ] elevated execution behavior
- [ ] privacy boundaries by session/channel context
- [ ] device/pairing token security workflows

---

## 6. Scheduler / automation

- [x] cron jobs
- [ ] reminders
- [ ] wake events
- [ ] heartbeats
- [ ] isolated scheduled agent runs
- [ ] run history durability
- [x] enable/disable semantics
- [ ] no-op heartbeat suppression (`HEARTBEAT_OK` behavior)
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

- [ ] dashboard
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
- [ ] SQLite reference mode
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
