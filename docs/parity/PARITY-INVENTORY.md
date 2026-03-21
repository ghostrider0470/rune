# OpenClaw Parity Inventory

Status: active master inventory.
Role: implementation-grade replication map for the Rust rewrite.

This document is the anchor parity document for the rewrite.
It inventories observable OpenClaw surfaces first, then spells out the minimum behavioral, operational, and deployment parity expected from the Rust system.

Hard constraints:

- functional parity with OpenClaw
- full Azure compatibility
- Docker-first deployment with mountable persistent storage
- no speculative feature work beyond the parity inventory and operator instructions

This inventory is organized by observable surface, not by idealized internal architecture.
If a user, operator, channel adapter, tool client, scheduler, extension, or automation flow can observe it, it belongs here.

---

## 1. Reading guide

### Parity levels

- **exact** — names, semantics, or machine-facing behavior are compatibility-critical
- **close** — operator workflow and practical outcomes must match, but wording/format may vary slightly
- **internal-flex** — implementation can differ materially if upstream behavior remains equivalent

### Evidence rule

A surface is not parity-complete because a feature exists.
It is parity-complete when there is black-box evidence for:

- success path
- failure path
- restart/reconnect behavior where relevant
- operator inspectability
- Docker persistence where relevant
- Azure-specific behavior where relevant

### Inventory discipline rule

Every surface in this document must end up in exactly one of these states:

- implemented and evidenced
- intentionally deferred with explicit divergence note
- rejected with explicit compatibility decision

Silent omission is not acceptable.

---

## 2. Non-negotiable release principles

The rewrite must preserve:

1. operator ability to control and diagnose the runtime
2. session, turn, and transcript integrity
3. tool names, schemas, and approval boundaries
4. scheduler, reminder, wake, and heartbeat semantics
5. memory privacy boundaries and file-oriented conventions
6. channel routing and participation boundaries
7. first-class Azure compatibility
8. Docker-first, mount-friendly persistence

Strong stance:

- **CLI surface is a product surface**
- **doctor/status/diagnostics are release-critical**
- **tool names are protocol**
- **memory stays human-readable and file-oriented**
- **Azure support must be explicit, not hand-waved as generic OpenAI compatibility**
- **durable state must not hide inside ephemeral container layers**

---

## 3. Surface map at a glance

Primary parity domains:

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
17. UI/operator visibility surfaces

---

## 4. CLI parity inventory

The CLI is one of the most visible OpenClaw surfaces.
Exact internal implementation does not matter; practical command coverage, naming gravity, and operator mental model do.

## 4.1 Top-level CLI contract

### Surface
- `openclaw <command>`
- shell completion
- human-readable output
- JSON output for many commands
- help/docs surfaces aligned with the live CLI mental model

### Parity
- **close** for formatting
- **exact** for command-family existence when operator workflows depend on it

### Required behaviors
- stable top-level command families
- stable global controls for `--dev`, `--profile <name>`, `--log-level <level>`, `--no-color`, `--version`, and `--help`
- profile isolation semantics that preserve state/config separation expectations
- machine-readable output where current CLI provides `--json`
- actionable help, examples, and docs pointers
- clear exit semantics on failure
- inspectable CLI -> gateway request path
- recognizable taxonomy to an experienced OpenClaw operator

### Evidence
- command census
- command coverage matrix
- representative JSON snapshots
- exit code and error behavior comparisons for major workflows

---

## 4.2 Command-family census from local OpenClaw

The parity inventory should distinguish between:

- **live-help confirmed** surfaces visible from the current local CLI
- **source-confirmed** deeper breadth visible in local command registration/source/tests
- **documented/operator-adjacent** breadth that may be plugin- or extension-provided today but is still part of the product surface story

### 4.2.1 Live-help confirmed top-level families

The current local `openclaw --help` surface confirms at least:

- `acp`
- `agent`
- `agents`
- `approvals`
- `backup`
- `browser`
- `channels`
- `clawbot`
- `completion`
- `config`
- `configure`
- `cron`
- `daemon`
- `dashboard`
- `devices`
- `directory`
- `dns`
- `docs`
- `doctor`
- `gateway`
- `health`
- `hooks`
- `logs`
- `memory`
- `message`
- `models`
- `node`
- `system`
- `nodes`
- `onboard`
- `pairing`
- `plugins`
- `qr`
- `reset`
- `sandbox`
- `secrets`
- `security`
- `sessions`
- `setup`
- `skills`
- `status`
- `system`
- `tui`
- `uninstall`
- `update`
- `webhooks`

### 4.2.2 Live-help confirmed breadth from sampled families

Local help also confirms meaningful subcommand/action breadth inside at least these families:

- `channels`: `list`, `status`, `capabilities`, `resolve`, `logs`, `add`, `remove`, `login`, `logout`
- `models`: `list`, `status`, `set`, `set-image`, `aliases`, `auth`, `fallbacks`, `image-fallbacks`, `scan`
- `gateway`: `status`, `install`, `uninstall`, `start`, `stop`, `restart`, `run`, `call`, `usage-cost`, `health`, `probe`, `discover`
- `browser`: `status`, `start`, `stop`, `tabs`, `tab`, `open`, `focus`, `close`, `screenshot`, `snapshot`, `navigate`, `resize`, `click`, `type`, `press`, `hover`, `drag`, `select`, `upload`, `fill`, `dialog`, `wait`, `evaluate`, `console`, `pdf`, `storage`, `trace`, `waitfordownload`

### 4.2.3 Source-confirmed / locally documented breadth

The local OpenClaw tree also shows evidence for these additional or deeper surfaces:

- `voicecall`
- browser `extension` helpers and a broader browser action surface than top-level help alone implies
- deeper `models` alias/fallback/auth/order flows
- deeper `devices`, `pairing`, `node`, and `nodes` operational breadth
- plugin-registered command extension capability
- generated shell completion and compatibility-writing scripts
- gateway/dev scripts for reachability, WS smoke, node E2E, and protocol generation

### 4.2.4 Parity interpretation

Not every family must ship in the first parity milestone, but every family is part of the parity inventory and must be either:

- implemented, or
- explicitly deferred and documented as a known divergence

Silent disappearance is not acceptable.

The rewrite should preserve the experienced operator’s mental model that OpenClaw is a broad operational surface, not only a chat runtime with a few helper commands.

---

## 4.3 Command-family priority tiers

### Tier 0 — release blockers

- `gateway`
- `daemon`
- `doctor`
- `health`
- `status`
- `cron`
- `channels`
- `models`
- `memory`
- `approvals`
- `sessions`
- `config`
- `configure`
- `secrets`
- `security`
- `system`
- `sandbox`
- `logs`
- `dashboard`
- `completion`

Rationale:
- `configure` matters because OpenClaw is explicitly operable through interactive setup, not only file editing
- `completion` matters because shell ergonomics are part of the observable CLI contract, even if lower risk than gateway/status/doctor

### Tier 1 — must-follow closely for practical replacement

- `devices`
- `pairing`
- `directory`
- `node`
- `nodes`
- `agent`
- `agents`
- `acp`
- `message`
- `skills`
- `plugins`
- `hooks`
- `webhooks`
- `backup`
- `update`
- `setup`
- `onboard`
- `uninstall`

### Tier 2 — breadth parity after core replacement is trustworthy

- `browser`
- `tui`
- `qr`
- `dns`
- `docs`
- `clawbot`
- `voicecall`
- `reset` where it is mostly operator convenience rather than core compatibility

---

## 4.4 Core command-family expectations

## 4.4.1 `gateway`

Observed subflows include:

- `openclaw gateway status`
- `openclaw gateway install`
- `openclaw gateway uninstall`
- `openclaw gateway start`
- `openclaw gateway stop`
- `openclaw gateway restart`
- `openclaw gateway run`
- `openclaw gateway call`
- `openclaw gateway usage-cost`
- `openclaw gateway health`
- `openclaw gateway probe`
- `openclaw gateway discover`

### Parity
- **exact** for lifecycle semantics and operator expectations
- **close** for wording and visual formatting

### Required behaviors
- distinguish service/process up from RPC/API reachability
- surface auth/token failures clearly
- surface dashboard/discovery URLs where relevant
- expose config mismatch between CLI context and service context

### Edge cases
- process alive, RPC down
- process alive, auth invalid
- stale token
- wrong config/profile bound to running service
- stale service entrypoint after upgrade

---

## 4.4.2 `daemon`

This currently behaves as a real operator-visible family and also as a legacy alias surface for gateway/service management. The rewrite should preserve the operator compatibility story even if implementation ownership collapses underneath.

Observed subflows include:

- `openclaw daemon status`
- `openclaw daemon install`
- `openclaw daemon uninstall`
- `openclaw daemon start`
- `openclaw daemon stop`
- `openclaw daemon restart`

### Parity
- **exact** in operational role

### Required behaviors
- installable service mode where supported
- clear runtime/service distinction from `gateway`
- service-state visibility and repair hooks

---

## 4.4.3 `doctor`

### Parity
- **exact** in concept and operator value

### Required behaviors
- validate config/state compatibility
- repair common runtime drift
- run or offer migrations
- generate/repair gateway auth material when needed
- support interactive and non-interactive operation
- support safe repair vs aggressive repair distinction
- distinguish read-only diagnosis from `--repair/--fix` and aggressive `--force` repair semantics
- support token-generation and workspace-suggestion control surfaces where they materially affect operator workflow
- detect broken mount/storage states in Docker deployments

### Minimum doctor check families
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

---

## 4.4.4 `health` and `status`

### Parity
- **close** for formatting
- **exact** for the content operators rely on

### Required behaviors
- quick confidence check vs deeper status card
- current model/runtime information where applicable
- session/time/usage/cost visibility where supported
- machine-readable output where used by automation

---

## 4.4.5 `cron`

Observed subflows include:

- `cron status`
- `cron list`
- `cron add`
- `cron show`
- `cron edit`
- `cron enable`
- `cron disable`
- `cron rm`
- `cron run`
- `cron runs`
- `cron wake`

### Parity
- **exact** for workflow and job-payload concepts

### Required behaviors
- `at`, `every`, and cron expression schedules
- timezone support
- enabled/disabled state
- schedule edits and disable/re-enable transitions recompute or clear `next_run_at` instead of preserving stale due state
- main-session `systemEvent` jobs
- isolated `agentTurn` jobs
- retained `announce`/`webhook`/`none` delivery-mode metadata
- history inspection
- due-only vs forced run semantics

---

## 4.4.6 `channels`

Observed subflows include:

- `channels list`
- `channels status`
- `channels capabilities`
- `channels resolve`
- `channels logs`
- `channels add`
- `channels remove`
- `channels login`
- `channels logout`

### Parity
- **exact** for workflow coverage

### Required behaviors
- account inventory
- capability matrix visibility
- identifier resolution
- multi-account support
- credential health and provider-specific auth state

---

## 4.4.7 `models`

Observed subflows include:

- `models list`
- `models status`
- `models set`
- `models set-image`
- `models aliases`
- `models auth`
- `models fallbacks`
- `models image-fallbacks`
- `models scan`
- auth profile management and ordering

### Parity
- **close** overall
- **exact** for Azure-relevant semantics, auth-profile ordering, and fallback intent

### Required behaviors
- configured defaults and overrides visible
- per-agent auth order where supported
- image model distinction
- deployment-aware Azure config

Implementation note (2026-03-19): Rune now ships eight of nine `models` subcommands: `list`, `status`, `set`, `set-image`, `aliases`, `fallbacks`, `image-fallbacks`, and `scan`. `models list` renders each configured provider with kind, base URL, default model, credential source, and credential-readiness hints. `models status` shows the resolved default text and image models plus per-provider credential readiness. `models aliases` maps alias names to their resolved provider/model targets with credential status. `models set` and `models set-image` mutate the local `config.toml` default, validating against the configured provider inventory. `models fallbacks` and `models image-fallbacks` list configured text and image fallback chains respectively, with both human and `--json` output modes. `models scan` probes locally reachable providers for available models (currently Ollama only, via the native `/api/tags` endpoint). Provider routing is wired through `RoutedModelProvider` in `rune-models`, which dispatches to the primary provider and walks the configured fallback chain on retriable errors (rate-limit, transient 5xx, quota exhaustion, HTTP transport). Ten providers are implemented: OpenAI, Anthropic, Azure OpenAI, Azure AI Foundry, Google (Gemini), Ollama, Groq, DeepSeek, Mistral, and AWS Bedrock. API key resolution follows a priority chain: direct `api_key` field → `api_key_env` environment variable → standard provider defaults (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `AWS_ACCESS_KEY_ID`/`AWS_SECRET_ACCESS_KEY`). The remaining unshipped gap is per-agent auth order (config structure exists in `rune-config` but no CLI surface), and deeper Azure-specific setup commands (Azure OpenAI and Azure AI Foundry providers are fully implemented but operator config is through `config set` and `config.toml` editing, not a dedicated Azure setup flow). `models auth` is now implemented as an inspectable auth-status surface with provider-by-provider credential source/readiness and config-management hints; direct secret mutation still flows through `config set` and `config.toml`.

---

## 4.4.8 `memory`

Observed subflows include:

- `memory status`
- `memory index`
- `memory search`

Current local help also confirms `sessions` as a list-oriented surface with filters and maintenance support such as:

- `sessions --active <minutes>`
- `sessions --agent <id>`
- `sessions --all-agents`
- `sessions --json`
- `sessions cleanup`

### Parity
- **exact** for indexing/search concepts

### Required behaviors
- index status visibility
- force reindex path
- retrieval backend availability surfaced clearly
- result attribution with path and line context

---

## 4.4.9 `approvals`

Observed subflows include:

- `approvals get`
- `approvals set`
- `approvals allowlist add/remove/...`

### Parity
- **exact** for policy semantics

### Required behaviors
- approval policy inspection/mutation
- per-agent and wildcard allowlists where supported
- import/export or whole-policy replacement workflow

---

## 4.5 Breadth command families beyond the core runtime

These are real OpenClaw surface area and must remain inventoried.

### Devices / pairing
Observed local subcommands include:

- `devices list`
- `devices remove`
- `devices clear`
- `devices approve`
- `devices reject`
- `devices rotate`
- `devices revoke`
- `pairing list`
- `pairing approve`

Required concepts:
- pairing requests
- approve/reject/list/remove/rotate/revoke
- token handling and auditability

### Directory
Observed local subcommands include:

- `directory self`
- `directory peers list`
- `directory groups list`
- `directory groups members`

Required concepts:
- self/peer/group lookup
- normalized user/group references

### `node` and `nodes`
OpenClaw has both singular `node` and plural `nodes` families.
That distinction should be preserved unless there is a very strong compatibility reason to collapse it.

Observed local `node` subcommands include:

- `node run`
- `node status`
- `node install`
- `node uninstall`
- `node stop`
- `node restart`

Observed local `nodes` subcommands include:

- `nodes status`
- `nodes describe`
- `nodes list`
- `nodes pending`
- `nodes approve`
- `nodes reject`
- `nodes rename`
- `nodes invoke`
- `nodes run`
- `nodes push`
- `nodes notify`
- `nodes location get`
- `nodes camera list|snap|clip`
- `nodes screen record`
- `nodes canvas snapshot|present|hide|navigate|eval|a2ui push|a2ui reset`

Required concepts:
- paired node status and invocation
- remote execution and media/camera/screen/canvas/location flows
- approval and pairing flows for nodes

### Agent / agents / ACP
Observed local families include:

- `agent`
- `agents`
- `acp`
- `acp client`

Required concepts:
- direct agent-turn invocation via CLI
- isolated agent/session management
- ACP bridge/runtime distinction
- thread-bound persistent ACP sessions where supported

### Browser / TUI / dashboard / QR
Observed local families include:

- `browser` with status/start/stop/reset-profile/tabs/tab/close/open/focus/profiles/create-profile/delete-profile
- browser state and action helpers such as cookies/storage/screenshot/snapshot/navigate/fill/wait/evaluate/click/type/press/hover/drag/upload/download/dialog/console/pdf/trace/highlight/errors/requests/responsebody/resize/scrollintoview
- browser `extension` helpers
- `tui`
- `dashboard`
- `qr`

These are not necessarily first-milestone parity, but they are real operator surfaces.

### Skills / plugins / hooks / webhooks
Observed local families include:

- `skills list|info|check`
- `plugins list|info|enable|disable|uninstall|install|update|doctor`
- `hooks list|info|check|enable|disable|install|update`
- `webhooks gmail setup|run`

Required concepts:
- extension lifecycle
- enable/disable/install/update/doctor flows
- webhook setup/run helpers

### Config / secrets / security / system / sandbox / configure
Observed local families include:

- `config` as both non-interactive helper family and setup-wizard entry when run without subcommand
- `config get|set|unset|file|validate`
- `configure`
- `secrets reload|audit|configure|apply`
- `security audit`
- `system event`
- `system heartbeat last|enable|disable|presence|status`
- `sandbox list|recreate|explain`

These are release-critical because they directly affect operability, setup posture, security posture, isolation posture, and proactive behavior.

`configure` should not be treated as cosmetic. It is part of the operator onboarding and mutation surface, especially where credentials, channels, gateway mode, and agent defaults are established interactively.

### Other breadth surfaces
Observed local families also include:

- `logs`
- `message`
- message breadth for `send`, `read`, `edit`, `delete`, `react`, `reactions`, `pin`, `unpin`, `pins`, `poll`, `broadcast`, `channel`, `event`, `member`, `role`, `thread`, `voice`, `emoji`, `permissions`, `search`, `sticker`, and moderation actions such as `ban`, `kick`, `timeout`
- `backup`
- `reset`
- `update` and `update wizard|status`
- `setup`
- `onboard`
- `uninstall`
- `docs`
- `completion`
- `clawbot`
- `voicecall`
- `dns`

Important distinction:
- `docs`, `completion`, `setup`, `configure`, `onboard`, and `update` are not runtime-core in the same way as sessions/tools/gateway
- but they are still parity-relevant because they shape how operators discover, bootstrap, repair, and trust the system

### Microsoft 365 (`ms365`)

First parity slice: read-only mail and calendar surfaces.

Observed subcommands:

- `ms365 mail unread` — list unread messages from the authenticated mailbox
- `ms365 calendar upcoming` — list upcoming calendar events

Required concepts:
- mailbox folder selection (`--folder`)
- result limiting (`--limit`)
- calendar look-ahead window (`--hours`)
- JSON and human-readable output

Parity level: **close** — operator workflow and practical outcomes match; naming/format may differ from upstream.

Status: CLI/API shape implemented (issue #206). Gateway endpoint and Graph integration deferred.

---

## 5. Gateway, daemon, and control-plane parity inventory

## 5.1 Daemon lifecycle

### Parity
- **exact** in operational role

### Required behaviors
- local service lifecycle
- persistent service identity/config binding
- restart-safe recovery of durable state
- inspectable distinction between runtime alive and runtime reachable

---

## 5.2 HTTP control plane

### Minimum resource families
At minimum the rewrite must expose stable resource families for:

- health
- status
- auth
- sessions
- turns
- tools
- processes
- approvals
- jobs/cron
- channels
- memory
- logs
- config
- doctor
- optionally devices/pairing/nodes if those surfaces are shipped in the same release tier

### Minimum conceptual matrix
The Rust rewrite does not need byte-identical route names, but it does need a stable API matrix that can express at least:

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
- optional `GET|POST /devices/*`, `/pairing/*`, `/nodes/*`

### Release-blocker resource operations
At minimum, parity-critical resources must support enough operations to preserve CLI and dashboard workflows:

- health: get
- status: get
- auth: create/rotate/inspect where exposed
- sessions: list/get/create/update/history/send where conceptually valid
- turns: list/get/create/cancel/retry where conceptually valid
- tools: invoke/get/history
- processes: list/get/poll/log/write/paste/kill
- approvals: list/get/create decision/update
- jobs: list/get/create/update/remove/run/history
- channels: list/get/status/add/remove/login/logout/resolve where conceptually valid
- memory: status/index/search/get
- config: get/set/unset/file/validate/effective-view where conceptually valid
- doctor: run/check/repair/report
- logs: tail/query/export where surfaced

### Parity
- **close** for route names
- **exact** for resource concepts, durable IDs, and machine semantics

### Required behaviors
- structured errors
- durable IDs for create/mutate flows
- async handling for long work
- enough metadata for CLI/UI correlation
- list/get/create/update/delete where conceptually valid
- run/trigger/cancel where active work entities require it

---

## 5.3 WebSocket / live event streaming

### Parity
- **exact** in capability
- **close** in frame format if clients remain behaviorally equivalent

### Minimum event families
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

### Minimum subscription semantics
The event plane must preserve enough topic granularity for:

- all-events firehose for privileged operator tools
- session-scoped subscription
- process-scoped subscription
- job/cron-scoped subscription
- diagnostics/log subscription
- replay or catch-up from cursor/sequence/timestamp where practical

### Required behaviors
- subscribe by entity/topic
- reconnect behavior
- stable ordering within entity streams
- replay/catch-up support where practical
- duplicate tolerance on reconnect

---

## 5.4 Auth and operator access

### Parity
- **exact** in security effect

### Required behaviors
- gateway token or equivalent auth
- explicit auth failures
- token generation/rotation workflows
- support auth modes equivalent in effect to `none|token|password|trusted-proxy`
- support bind/exposure postures equivalent in effect to `loopback|lan|tailnet|auto|custom`
- support for loopback/open modes only when explicitly configured

---

## 5.5 Dashboard / discoverability references

### Parity
- **close**

### Required behaviors
- operator can discover where to inspect the runtime
- URLs and status data remain consistent with actual configuration

---

## 6. Session, runtime, and transcript parity inventory

## 6.1 Session identity and kinds

### Parity
- **exact**

### Required behaviors
- stable session IDs/keys
- recognizable kinds such as main/shared/direct/cron/heartbeat/subagent/system/channel-backed contexts
- parent/requester linkage for descendant sessions

---

## 6.2 Turn lifecycle

### Parity
- **exact**

### Required behaviors
- trigger acceptance
- context assembly
- model invocation
- iterative tool loop
- approval waits
- subagent waits
- transcript finalization
- usage/accounting persistence

### Edge cases
- cancellation
- retry
- provider failure
- partial tool progress before failure

---

## 6.3 Transcript ordering and attribution

### Parity
- **exact**

### Required behaviors
- one authoritative ordering
- attributable assistant/user/system/tool/approval/event notes
- no silent reordering after background work or retries

---

## 6.4 Context assembly

### Parity
- **exact** in policy and practical effect
- **internal-flex** in implementation

### Required behaviors
- startup file loading by session context
- tool availability injection
- skill-selection injection
- memory retrieval injection with privacy rules
- explainability for debugging

### Key file rules to preserve
- read `SOUL.md`, `USER.md`, recent daily memory files on startup
- read `MEMORY.md` only in main session
- preserve group/shared-context privacy restrictions

---

## 6.5 Compaction, pruning, and usage accounting

### Parity
- **exact** at behavioral level
- **internal-flex** in storage strategy

### Required behaviors
- preserve decisions, unresolved tasks, important tool outcomes, and safety facts
- feed `/status` and dashboard/session counters
- maintain future-turn continuity

---

## 6.6 Session inspection and status

### Surface
- session inspection APIs/CLI/UI
- in-session `/status`
- `session_status` tool semantics

### Parity
- **exact** for fields operators and agents rely on

### Required behaviors
- model override/current model visibility
- usage/time/cost visibility where supported
- reasoning/verbose/elevated flags when applicable
- optional per-session model override through status tooling

---

## 7. Tool parity inventory

Tool semantics are among the strictest parity surfaces.
Tool names matter.
Argument schemas matter.
Approval behavior matters.

## 7.1 File tools

### `read`

#### Parity
- **exact**

#### Required behaviors
- `path`/`file_path` aliases if supported
- deterministic truncation rules
- `offset`/`limit` behavior
- text vs image/binary handling distinction
- explicit file-not-found vs access-error behavior

### `write`

#### Parity
- **exact**

#### Required behaviors
- create or overwrite
- auto-create parent directories
- predictable path handling

### `edit`

#### Parity
- **exact**

#### Required behaviors
- exact-match replacement only
- failure when old text does not match exactly
- no fuzzy patching under the same tool name

---

## 7.2 Process tools

### `exec`

#### Parity
- **exact**

#### Required behaviors
- foreground execution with bounded wait
- background continuation
- PTY option for TTY-required workflows
- timeout handling
- working directory handling
- elevated/security/ask modes where supported
- approval interception for sensitive commands
- exact command presentation when approval is required

### `process`

#### Parity
- **exact**

#### Required behaviors
- list/poll/log/write/send-keys/submit/paste/kill style management
- stable process/session handle
- interactive stdin/TTY control

---

## 7.3 Scheduler tool

### `cron`

#### Parity
- **close** for durable scheduler semantics and operator inspection
- **exact** for `sessionTarget`/payload validation and persisted run-history semantics

#### Required behaviors
- `status|list|add|show|update|remove|run|runs|wake`
- explicit job schema
- `systemEvent` vs `agentTurn`
- `sessionTarget=main` requiring `systemEvent`
- `sessionTarget=isolated` requiring `agentTurn`
- retained `announce`/`webhook`/`none` delivery-mode metadata
- immediate and next-heartbeat wake modes

---

## 7.4 Session and orchestration tools

### Named contracts
- `sessions_list`
- `sessions_history`
- `sessions_send`
- `sessions_spawn`
- `subagents`
- `session_status`

#### Parity
- **exact**

#### Required behaviors
- inspect active/recent sessions
- fetch history from other sessions
- message another session
- spawn isolated `subagent` or `acp` runs/sessions
- thread-bound persistent ACP sessions where supported
- explicit runtime distinction: `subagent` vs `acp`
- push-based completion instead of poll abuse
- subagent steer and kill flows

---

## 7.5 Memory tools

### Named contracts
- `memory_search`
- `memory_get`

#### Parity
- **exact**

#### Required behaviors
- mandatory recall step for memory-sensitive questions
- semantic search over `MEMORY.md` + `memory/*.md`
- source attribution with file and line context
- disabled backend surfaced explicitly
- bounded snippet retrieval by path/from/lines

---

## 7.6 Higher-order orchestration helper

### `multi_tool_use.parallel`

### Parity
- **internal-flex** unless directly exposed in the rewrite

The rewrite still needs an equivalent orchestration strategy if the surrounding runtime depends on parallel tool execution as a first-class behavior.

---

## 8. Approval, sandbox, and security parity inventory

## 8.1 Approval semantics

### Critical decisions
- `allow-once`
- `allow-always`
- `deny`

### Parity
- **exact**

### Required behaviors
- approvals bind to the exact command or payload presented
- allow-once does not cover later commands
- denied actions remain visible and auditable
- the agent/runtime must show the full command when approval is required

---

## 8.2 Ask, security, and elevated execution modes

### Parity
- **exact** in effect

### Required behaviors
- `ask` modes such as `off|on-miss|always`
- security modes such as `deny|allowlist|full`
- elevated execution path where supported
- host/gateway/node execution distinctions where applicable

---

## 8.3 Privacy boundaries

### Parity
- **exact**

### Required behaviors
- main-session-only memory stays main-session-only
- shared/group chats do not leak private user state
- assistant is not treated as the user’s proxy in group/shared contexts
- external actions remain gated and cautious

---

## 8.4 Device/pairing/token security

### Parity
- **close** overall
- **exact** for approval and token-scope concepts

### Required behaviors
- pending pairing workflow
- explicit approval/rejection
- token rotation and revocation

---

## 9. Scheduler, reminders, wake, heartbeat, hooks, and webhooks parity inventory

## 9.1 Cron jobs

### Parity
- **close** for durable scheduler semantics, gateway coverage, and operator inspection
- **exact** for schedule parsing, `sessionTarget`/payload validation, and persisted due-vs-manual run history

### Required behaviors
- durable job definition
- enabled/disabled state
- next-run calculation
- due-only vs forced run semantics
- run history retention
- explicit `at`, `every`, and cron-expression schedule support
- explicit `sessionTarget=main` vs `sessionTarget=isolated` behavior
- explicit `systemEvent` vs `agentTurn` payload validation
- explicit `none` / `announce` / `webhook` delivery-mode retention and inspection

Implementation note (2026-03-19): Rune now has durable cron semantics end to end around creation, inspection, persistence, execution, and delivery: gateway routes cover `GET /cron/status`, `GET /cron`, `GET /cron/{id}`, `POST /cron`, `POST /cron/wake`, `POST /cron/{id}`, `DELETE /cron/{id}`, `POST /cron/{id}/run`, and `GET /cron/{id}/runs`; CLI flows cover `cron status|list|add|show|edit|enable|disable|rm|run|runs|wake`; runtime schedule handling computes interval anchors and timezone-aware cron next-fire times; invalid cron/tz definitions fail instead of silently fabricating a next run; schedule edits and disable/re-enable transitions recompute `next_run_at` from the current schedule rather than preserving stale cadence; and durable `jobs`/`job_runs` persistence preserves created jobs, next-run state, and scheduled/manual run history across gateway restarts. Scheduled `main` jobs reuse the stable `system:scheduled-main` session while scheduled `isolated` jobs create fresh descendant `subagent` sessions linked through `requester_session_id`. Delivery modes are now executable: `announce` broadcasts a `cron_run_completed` event via the session event channel, `webhook` POSTs the result payload to the configured URL (30 s timeout), and `none` suppresses outbound delivery. Due jobs are claimed atomically before execution via `claimed_at`; stale claims expire after 300 s for crash recovery, preventing concurrent duplicate execution. The remaining gaps are narrower: the CLI create/edit surface is narrower than the gateway schema (`cron add` is one-shot `system_event` only and `cron edit` only mutates name/delivery mode), and webhook delivery does not retry on failure.

---

## 9.2 Reminders

### Parity
- **close** for one-shot timing, durable outcomes, and operator inspection

### Required behaviors
- one-shot timing
- reminder text shaped like a reminder when delivered
- mention that it is a reminder depending on timing/context
- retained target chat/session metadata
- delivered / missed / cancelled terminal outcomes remain inspectable

Implementation note (2026-03-19): Rune now ships a complete executable reminder surface with durable outcomes and target routing: gateway routes expose `GET /reminders`, `POST /reminders`, and `DELETE /reminders/{id}`; CLI flows expose `reminders add|list|cancel`; reminders persist through the scheduler job repository as `reminder` jobs; due-checking records delivery attempts; successful sends mark reminders delivered; failed sends mark reminders missed with persisted error context; and user/operator cancellation produces an explicit cancelled terminal outcome instead of silent disappearance. Reminder target routing is now shipped: `"main"` (default) executes in the stable `system:scheduled-main` session, `"isolated"` creates a one-shot subagent session under it, and unknown targets fall back to `"main"` with a warning. Due reminders are claimed atomically before execution via `claimed_at` with the same lease/expiry semantics as cron jobs. Outcome details (`outcome_at`, `last_error`) are surfaced through the gateway reminder response. The remaining gap is narrower: final wording parity for reminder delivery text and broader target values beyond `"main"` / `"isolated"`.

---

## 9.3 Wake events

### Parity
- **partial**

### Required behaviors
- immediate wake
- next-heartbeat wake
- optional context carry-forward

Implementation note (2026-03-19): Rune currently normalizes wake mode to `now` or `next-heartbeat`, defaults omitted mode to `next-heartbeat`, accepts both `next-heartbeat` and `next_heartbeat`, and exposes that control through both `cron wake` and `system event`. Optional `context_messages` is preserved on the emitted `wake_event` payload for subscribers. The remaining gap is substantive rather than cosmetic: this is currently an operator/event-bus queueing surface, not a fully wired downstream wake-execution path.

---

## 9.4 Heartbeats

### Parity
- **close** for shipped no-op and duplicate-suppression semantics
- **partial** for broader quiet-window policy

### Required behaviors
- read `HEARTBEAT.md` if present
- follow heartbeat instruction strictly
- do not infer unrelated old tasks
- return `HEARTBEAT_OK` when nothing needs attention
- suppress outbound delivery when heartbeat result is no-op ack
- respect quiet windows and anti-spam behavior
- maintain minimal state for proactive checks
- persist enough anti-spam state to avoid duplicate notifications after restart

Implementation note (2026-03-19): Rune now has a real heartbeat runner with persisted runner state, `HEARTBEAT.md` prompt loading, due-checking, suppression of no-op `HEARTBEAT_OK` responses, duplicate-notification suppression via normalized-response fingerprinting, persisted suppression counters/fingerprint state, supervisor execution, and operator surfaces through `GET /heartbeat/status`, `POST /heartbeat/enable`, `POST /heartbeat/disable`, and CLI `system heartbeat presence|last|enable|disable|status`. The shipped anti-spam contract is now specifically: no-op heartbeat acknowledgements suppress outbound delivery, normalized duplicate notifications are suppressed and survive restart, and those suppressions remain inspectable through runner state. The remaining gap has narrowed to quiet-window policy and broader anti-spam behavior beyond that shipped no-op/duplicate contract.

---

## 9.5 Hooks and webhooks

### Parity
- **close** overall
- **exact** where existing operator automation depends on setup/run semantics

### Required behaviors
- install/check/enable/disable/update style lifecycle for hooks where shipped
- webhook setup/run helpers where shipped
- inspectable trigger and failure behavior

---

## 10. Memory parity inventory

## 10.1 File-oriented memory model

### Surfaces
- `MEMORY.md`
- `memory/YYYY-MM-DD.md`
- optional `memory/*.md`
- lightweight state files such as heartbeat-state

### Parity
- **exact**

### Required behaviors
- curated long-term vs daily/raw distinction
- human-editable markdown as the primary memory representation
- memory survives session restarts because files survive

---

## 10.2 Memory retrieval

### Parity
- **exact**

### Required behaviors
- semantic search
- safe snippet extraction
- bounded recall payloads
- source path and line attribution
- session-type privacy filtering

---

## 10.3 Memory update conventions

### Parity
- **exact** in workflow

### Required behaviors
- “remember this” becomes a file update, not implicit model memory
- important events/decisions get written down
- heartbeat or maintenance flows can distill daily notes into long-term memory

---

## 11. Channel parity inventory

## 11.1 Normalized inbound envelope

### Parity
- **exact** conceptually

### Required fields
- channel/provider name
- event/message ID or dedupe key
- chat/conversation identifier
- sender identifier
- timestamp
- text/caption
- attachments/media references
- reply/thread/mention metadata
- direct vs group context
- edit/reaction/delete metadata where supported

---

## 11.2 Normalized outbound actions

### Actions
- send
- reply
- edit
- react
- send media/attachment
- optional typing/presence

### Parity
- **exact** in action family

---

## 11.3 Provider inventory

Observed/planned provider families include at least:

- Telegram
- Discord
- WhatsApp
- Signal
- Slack
- Teams
- Google Chat
- Matrix
- Tlon
- iMessage/SMS bridge variants
- other adapters over time

### Parity stance
- first production parity should prove one provider fully, likely Telegram
- the abstraction must still preserve broader provider breadth

---

## 11.4 Channel behavior rules

### Parity
- **exact**

### Required behaviors
- direct vs group routing correctness
- reply targeting correctness
- reaction behavior lightweight and capability-aware
- duplicate inbound dedupe
- persistent provider message references for edits/replies/reactions

---

## 12. Media, OCR, TTS, browser, and voice parity inventory

## 12.1 Inbound media handling

### Parity
- **exact** conceptually

### Required behaviors
- attachment normalization
- durable media references
- inspectable lifecycle tied to session/turn

---

## 12.2 Audio transcription

### Parity
- **exact**

### Required behaviors
- inbound voice/audio notes transcribed when possible
- provider abstraction for transcription backend

---

## 12.3 Image understanding handoff

### Parity
- **exact** conceptually

### Required behaviors
- image attachments available to model/runtime
- provider-specific image model details hidden behind normalized abstraction

---

## 12.4 OCR and document understanding

### Parity
- **exact** for having a first-class document-understanding path
- **exact** for Azure Document Intelligence compatibility

### Required behaviors
- scanned PDF/image extraction path
- structured extraction where provider supports it
- traceable extraction artifacts

---

## 12.5 TTS replies

### Parity
- **close** to exact depending on configured features

### Required behaviors
- TTS reply path where enabled
- voice-first responses when policy/config says so
- configurable provider and fallback behavior
- generated media artifact handling and delivery

---

## 12.6 Browser/voicecall adjacencies

### Parity
- **inventory-exact**, implementation may be phased

### Required behaviors when shipped
- browser state and action surfaces remain automatable and inspectable
- voicecall surface remains a first-class adjunct rather than disappearing silently

---

## 13. Skills, plugins, hooks, and extension parity inventory

## 13.1 Prompt skill selection rules

### Parity
- **exact**

### Required behaviors
- inspect available skills before loading
- if exactly one skill clearly applies, load it
- if multiple apply, choose the most specific one
- do not pre-load multiple skills up front by default
- resolve relative resource paths deterministically from the skill directory

---

## 13.2 Skill package structure

### Components
- `SKILL.md`
- references/assets/scripts directories where used
- metadata/instructions in package form

### Parity
- **exact** in concept

---

## 13.3 Skill and extension lifecycle

### Surfaces
- local installed skills
- add/update/publish workflows where supported
- plugin enable/disable/install/update/doctor flows
- hook install/check/enable/disable/update flows

### Parity
- **close** initially if necessary
- **exact** eventually where operator workflows depend on them

---

## 13.4 Extension isolation

### Parity
- **exact** in safety effect
- **internal-flex** in ABI or process model

### Required behaviors
- capability declaration
- failure isolation
- timeout boundaries
- explainable loading behavior

---

## 14. Config, secrets, and precedence parity inventory

## 14.1 Layered config model

### Parity
- **exact** in effect

### Required behaviors
- environment variables
- config files
- CLI overrides where applicable
- service vs CLI config visibility and mismatch detection
- agent-specific and provider-specific overrides

---

## 14.2 Minimum config census

The rewrite must preserve equivalent configuration coverage for at least:

### Runtime and gateway
- gateway host/port/bind URL
- gateway auth mode, token, loopback/open flags
- daemon install/run/profile selection
- dashboard/discovery settings where surfaced

### Models and providers
- default model
- per-agent model
- image model
- fallback model lists
- provider auth profiles and ordering
- request timeout and retry policy

### Azure-specific
- Azure endpoint
- Azure deployment name
- Azure API version
- Azure auth mode
- Azure custom headers
- Azure Document Intelligence endpoint/auth/version fields where relevant

### Channels
- provider account definitions
- provider credentials/auth material
- capability flags or provider options
- direct/group behavior config where relevant

### Scheduler and proactive behavior
- heartbeat enable/disable/presence/quiet-window behavior
- scheduler defaults
- cron retention and failure policy
- delivery defaults

### Memory and retrieval
- workspace and memory roots
- indexing toggles
- retrieval backend selection
- embedding backend selection where used

### Media
- TTS settings
- transcription settings
- OCR/document provider settings
- fallback behavior

### Security and approvals
- ask/security/elevated defaults
- allowlists
- approval policy defaults
- sandbox config where relevant

### Storage and observability
- workspace/data/config/secrets roots
- database URL or embedded database mode settings
- search/vector extension toggles or prerequisites where relevant
- log level and sinks
- tracing/export controls

### Breadth surfaces if shipped
- browser settings
- nodes/devices/pairing settings
- webhook/hook settings

### Parity
- **exact** in capability coverage
- **close** for exact key names only if a compatibility layer exists

---

## 14.3 Config mutation, validation, and repair flows

### Parity
- **exact** conceptually

### Required behaviors
- config get/set/unset/file/validate workflows where relied on
- doctor-assisted repair and migration
- safe legacy key/layout handling
- explicit CLI-runtime-service mismatch reporting

---

## 15. Storage, Docker, backup, and persistence parity inventory

## 15.1 Canonical durable path model

The Rust rewrite should preserve a stable logical layout:

- `/data/db`
- `/data/sessions`
- `/data/memory`
- `/data/media`
- `/data/skills`
- `/data/logs`
- `/data/backups`
- `/config`
- `/secrets`

### Parity
- **exact** for the logical domain model
- **close** for physical implementation details

---

## 15.2 Docker-first runtime behavior

### Parity
- **exact** as a hard rewrite constraint

### Required behaviors
- image stateless by design
- all critical state externalized
- bind mounts, named volumes, and cloud mounts supported
- restart durability
- fast failure on missing/unwritable required mounts
- no image-layer archaeology required for backup or restore

---

## 15.3 Database posture under the Docker/Azure constraint

The durable filesystem model does **not** imply SQLite-first.
The rewrite’s current planning direction is:

- PostgreSQL from day one for operational metadata
- embedded PostgreSQL when no external `DATABASE_URL` is configured
- external PostgreSQL for managed/server deployments
- file-oriented state remains mounted and human-inspectable regardless of DB choice

Parity implication:
- the database engine may change from OpenClaw internals as long as operator-visible behavior, inspectability, migration safety, and mounted durable-state expectations remain intact

---

## 15.4 Backup, restore, and migration

### Parity
- **close** today, but operationally release-critical

### Required behaviors
- inspectable durable state
- DB and file-backed state restorable in a documented way
- upgrade/migration paths that preserve mounted state
- rollback possible from durable backup snapshots

---

## 16. Azure compatibility inventory

## 16.1 Azure model provider support

### Must support
- Azure OpenAI endpoint shapes
- Azure AI Foundry-hosted access where relevant
- deployment name as first-class config
- API version handling
- Azure auth/header conventions
- Azure-compatible error normalization

### Parity
- **exact** as a hard constraint

---

## 16.2 Azure Document Intelligence

### Parity
- **exact** as a hard constraint

### Required behaviors
- first-class OCR/document-understanding integration path
- clean abstraction so Azure is real, not bolted on

---

## 16.3 Azure hosting compatibility

### Target environments
- Azure Container Apps
- AKS
- App Service for Containers
- Azure VM/VMSS with Docker or system service

### Parity
- **exact** as deployment hard constraint

### Required behaviors
- env/file/secret injection patterns
- health/readiness endpoints
- graceful shutdown
- externalized durable state

---

## 16.4 Azure storage mapping

### Recommended mappings
- operational relational metadata -> PostgreSQL locally embedded or externally managed depending on mode
- mount-friendly file domains -> local mounts or Azure Files
- archive/object payloads -> Azure Blob Storage
- secrets -> Key Vault or injected secrets

### Parity
- **exact** in compatibility requirement
- **internal-flex** in adapter implementation

---

## 17. Doctor, status, logging, diagnostics, and dashboard parity inventory

## 17.1 Status surfaces

### Parity
- **exact** in operator value

### Required behaviors
- tell the operator what is running
- tell the operator whether the runtime is reachable
- tell the operator what config/runtime context is in effect
- expose dashboard/session/usage context where relevant
- distinguish process-alive, control-plane-reachable, and auth-valid states

---

## 17.2 Doctor and repair surface

### Parity
- **exact** in operational role

### Required behaviors
- detect broken or partial installs
- detect missing or inconsistent config
- detect token/auth/service drift
- run or offer state migrations
- repair common broken states non-destructively where possible
- support interactive and non-interactive repair paths

---

## 17.3 Logging

### Parity
- **close**

### Required behaviors
- structured logs to stdout/stderr and optional files
- entity/correlation IDs
- session/job/tool/process/channel traceability

---

## 17.4 Diagnostic bundles and logs viewers

### Parity
- **close**, but high operator value

### Required behaviors
- enough introspection to debug without a debugger
- channel logs, recent failures, job history, session history, approval history
- exportable diagnostic bundle or equivalent inspectable aggregate

---

## 17.5 Health probes

### Parity
- **exact** conceptually

### Required behaviors
- liveness/readiness split where useful
- dependency-aware health reporting
- degraded vs hard-fail distinction

---

## 18. Multi-agent, spawned sessions, ACP, and background work inventory

## 18.1 Subagents and descendant sessions

### Parity
- **exact**

### Required behaviors
- isolated runtime/workspace policy when spawned
- parent linkage
- push-based completion reporting
- steer/kill controls

---

## 18.2 ACP harness compatibility

### Parity
- **exact** for externally visible runtime distinction and thread-bound behavior

### Required behaviors
- `runtime: "acp"`
- explicit `agentId`
- persistent thread/session semantics where requested
- resume existing session IDs
- not conflated with local PTY execution path

---

## 18.3 Background process persistence

### Parity
- **exact**

### Required behaviors
- initiating turn may end while process handle remains inspectable
- restart must not erase the existence/history of background work
- terminal or lost state remains auditable

---

## 19. UI and operator visibility inventory

Even if UI is not the first implementation focus, the rewrite must preserve operator visibility across parity-critical entities.

### Required views
- dashboard/status
- sessions/transcripts
- approvals
- cron/jobs/history
- logs/events
- channel health
- config/secrets references
- skills/extensions status
- live session/process/job updates

### Parity
- **close** in visual design
- **exact** in visibility and control coverage

---

## 20. Priority ranking

## Tier 0 — absolute release blockers

1. gateway lifecycle/status/auth
2. session/turn/transcript integrity
3. file/process tools
4. approval semantics
5. cron/reminder/heartbeat behavior
6. memory privacy boundaries
7. Docker-first mountable persistence
8. Azure provider compatibility
9. doctor/repair workflow
10. config validation/mutation/introspection parity

## Tier 1 — must-follow closely for practical replacement

1. channels abstraction plus at least one full production provider
2. model/auth/fallback config behaviors
3. live WS observability
4. background process inspectability
5. memory indexing/search CLI and tooling
6. TTS/transcription baseline parity
7. logging/diagnostic bundle visibility
8. extension lifecycle basics

## Tier 2 — breadth parity after the core replacement is trustworthy

1. broader channels
2. nodes/devices/pairing breadth
3. directory/webhook helpers
4. richer skills distribution and marketplace flows
5. broader browser/UI breadth
6. voicecall/clawbot/dns/qr convenience breadth

---

## 21. Known design implications for the Rust rewrite

1. **Do not collapse doctor into a trivial health check.** That is a real regression.
2. **Do not treat Azure as just another OpenAI-compatible base URL.** Deployment names and API versions are real parity concerns.
3. **Do not replace file-oriented memory with opaque DB-only memory.** That breaks OpenClaw’s human-inspectable model.
4. **Do not weaken exact-match `edit` semantics.** It is a tool contract.
5. **Do not lose the distinction between runtime alive and RPC/API reachable.** `gateway status` depends on it.
6. **Do not assume Docker implies ephemeral state.** The hard constraint is the opposite.
7. **Do not hide skill-loading decisions.** Explainability is part of parity.
8. **Do not fake background inspectability.** If a process outlives the turn, it must remain controllable and auditable.
9. **Do not silently drop breadth command families from the product story.** Deferred is acceptable; vanished is not.
10. **Do not let the database decision distort the mounted durable-state contract.** Postgres is fine; hidden state is not.

---

## 22. Evidence checklist template

Use this checklist per surface/domain:

- [ ] command/endpoint/resource inventory captured
- [ ] success-path snapshots captured
- [ ] failure-path snapshots captured
- [ ] restart/reconnect behavior tested
- [ ] audit/log visibility verified
- [ ] Docker persistence verified
- [ ] Azure-specific path verified where relevant
- [ ] known divergences documented explicitly

---

## 23. Immediate planning consequences

This inventory implies:

- `FUNCTIONALITY-CHECKLIST.md` should track these domains at command/resource/event granularity, not broad buckets only
- `IMPLEMENTATION-PHASES.md` should prioritize doctor/status/parity harnesses early, not late
- `DOCKER-DEPLOYMENT.md` and `AZURE-COMPATIBILITY.md` remain hard-constraint docs, not optional appendices
- `PROTOCOLS.md` should prefer command/resource/event matrices and state transitions over generic architecture prose
- storage/database docs must not accidentally reintroduce a SQLite-first assumption if the current plan is PostgreSQL-first with embedded fallback
- every future scope cut must reference this inventory explicitly so parity erosion is visible

---

## 24. Bottom line

If the Rust rewrite can:

- run as a Docker-first, persistently mounted service
- preserve session/tool/approval/scheduler/memory semantics
- remain operator-diagnosable through gateway status, doctor, logs, and live inspection
- support real Azure endpoint/auth/deployment/document behavior
- and map the current OpenClaw CLI/control surfaces closely enough that an experienced operator does not need to relearn the product

then it has a credible parity path.

If it misses any of those, it is not yet an OpenClaw replacement — only an OpenClaw-inspired Rust system.
