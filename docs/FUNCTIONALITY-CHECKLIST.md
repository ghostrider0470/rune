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
- [ ] top-level `openclaw --version` behavior exists
- [ ] top-level `--dev`, `--profile`, `--log-level`, `--no-color`, `--help`, and `--version` controls have equivalent semantics
- [ ] `openclaw gateway status`
- [ ] `openclaw gateway install`
- [ ] `openclaw gateway uninstall`
- [ ] `openclaw gateway start`
- [ ] `openclaw gateway stop`
- [ ] `openclaw gateway restart`
- [ ] `openclaw gateway run`
- [ ] `openclaw gateway call`
- [ ] `openclaw gateway usage-cost`
- [ ] `openclaw gateway health`
- [ ] `openclaw gateway probe`
- [ ] `openclaw gateway discover`
- [ ] `openclaw daemon status`
- [ ] `openclaw daemon install`
- [ ] `openclaw daemon uninstall`
- [ ] `openclaw daemon start`
- [ ] `openclaw daemon stop`
- [ ] `openclaw daemon restart`
- [ ] `openclaw doctor`
- [ ] `openclaw health`
- [ ] `openclaw status`
- [ ] `openclaw dashboard`
- [ ] `openclaw configure`
- [ ] shell completion
- [ ] help/usage text aligned with actual command families
- [ ] JSON output where operator automation depends on it

### Cron CLI
- [ ] `cron status`
- [ ] `cron list`
- [ ] `cron add`
- [ ] `cron edit`
- [ ] `cron enable`
- [ ] `cron disable`
- [ ] `cron rm`
- [ ] `cron run`
- [ ] `cron runs`

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
- [ ] WebSocket API
- [ ] WebSocket topic/subscription/replay semantics
- [ ] dashboard/discovery surfacing
- [ ] background process supervision visibility
- [ ] restart durability for sessions/jobs/approvals/process history
- [ ] structured error envelopes
- [ ] durable IDs returned by create/mutate flows

---

## 3. Runtime / sessions / transcripts

- [ ] session creation and persistence
- [ ] session kinds and parent linkage
- [ ] agent turn loop
- [ ] context assembly
- [ ] transcript ordering
- [ ] transcript attribution of tool/approval/subagent events
- [ ] transcript compaction/pruning
- [ ] usage/cost tracking
- [ ] model failover / fallback behavior
- [ ] session status surface (`/status` + `session_status` equivalent)
- [ ] startup file loading rules by session type
- [ ] main-session-only curated-memory boundary

---

## 4. Tool system

### File tools
- [ ] `read`
- [ ] `write`
- [ ] `edit`
- [ ] exact truncation/offset semantics
- [ ] exact-match edit failure behavior

### Process tools
- [ ] `exec`
- [ ] `process`
- [ ] background continuation semantics
- [ ] PTY semantics
- [ ] exact approval-prompt command presentation

### Scheduler/orchestration tools
- [ ] `cron`
- [ ] `sessions_list`
- [ ] `sessions_history`
- [ ] `sessions_send`
- [ ] `sessions_spawn`
- [ ] `subagents`
- [ ] `session_status`

### Memory tools
- [ ] `memory_search`
- [ ] `memory_get`

### Tooling contract quality
- [ ] stable names and schemas
- [ ] transcript/audit linkage
- [ ] structured errors
- [ ] durable handles for long-running work

---

## 5. Approvals / security / sandboxing

- [ ] approval request lifecycle
- [ ] `allow-once` exact scope binding
- [ ] `allow-always` persistence semantics
- [ ] `deny` audit trail semantics
- [ ] exact command/payload presentation for approval
- [ ] ask mode behavior (`off|on-miss|always` equivalent)
- [ ] security mode behavior (`deny|allowlist|full` equivalent)
- [ ] elevated execution behavior
- [ ] privacy boundaries by session/channel context
- [ ] device/pairing token security workflows

---

## 6. Scheduler / automation

- [ ] cron jobs
- [ ] reminders
- [ ] wake events
- [ ] heartbeats
- [ ] isolated scheduled agent runs
- [ ] run history durability
- [ ] enable/disable semantics
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
