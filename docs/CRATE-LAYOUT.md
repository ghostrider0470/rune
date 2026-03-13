# Crate Layout Draft — OpenClaw Rust Rewrite

## Goal

Propose a practical Rust monorepo layout for an OpenClaw-compatible rewrite.

This is intentionally biased toward:

- clean subsystem boundaries
- local-first operation
- strong testability
- future plugin/channel growth
- keeping phase 1 shippable

This is **not** a recommendation to implement every crate immediately. Some crates should begin as modules and split only when their boundaries stabilize.

---

## Design principles

1. **Core domain types should not depend on transport or UI concerns.**
2. **The gateway daemon is the center of gravity.**
3. **CLI, channels, models, memory, and media should depend inward, not sideways.**
4. **Plugin and extension boundaries should stay explicit.**
5. **Start with fewer crates than the end-state diagram suggests.**

My bias: it is better to begin slightly under-factored and split with evidence than to create 25 crates on day one and spend months managing boundaries.

---

## Recommended workspace shape

```text
openclaw-rust-rewrite/
  Cargo.toml                    # workspace root
  Cargo.lock
  crates/
    rune-core/
    rune-config/
    rune-store/
    rune-runtime/
    rune-tools/
    rune-models/
    rune-channels/
    rune-memory/
    rune-jobs/
    rune-media/
    rune-skills/
    rune-approvals/
    rune-gateway/
    rune-cli/
    rune-api/
    rune-telemetry/
    rune-testkit/
  apps/
    gateway/
    cli/
  web/
    operator-ui/
  docs/
  notes/
```

Two practical interpretations are possible:

- **Conservative start:** keep most functionality in ~8–10 crates and split later
- **Full target shape:** use the full crate map above once subsystem seams are real

I recommend the conservative start.

---

## Minimum viable crate set

If implementation started tomorrow, I would begin with these crates:

### 1. `rune-core`

Holds pure domain concepts:

- IDs
- enums and state machines
- session/message/job/tool/approval domain types
- capability definitions
- shared errors where transport-independent
- normalized channel event/message types

Should avoid direct dependencies on:

- Axum
- SQLx
- Tokio process
- specific provider SDKs

If a type must be reused everywhere, it probably belongs here.

### 2. `rune-config`

Holds:

- config schema
- layered config loading
- env/file/CLI override rules
- path resolution
- secrets reference model

This deserves its own crate because config behavior becomes cross-cutting quickly.

### 3. `rune-store`

Holds persistence adapters and repository interfaces for:

- sessions
- transcripts
- jobs
- approvals
- channel state
- plugin metadata
- memory metadata

Start with SQLite-backed implementations.

Important rule: `rune-store` should not own business workflows; it should own persistence concerns.

### 4. `rune-runtime`

The actual assistant runtime engine:

- session orchestration
- context assembly
- turn execution
- tool loop control
- model invocation orchestration
- transcript writes
- sub-run management

This is where a lot of the OpenClaw behavior actually lives.

### 5. `rune-tools`

Holds tool definitions and tool execution adapters:

- filesystem tools
- process tools
- session/status tools
- memory tool surfaces
- approval-aware wrappers

Why separate from runtime?

Because tools become both a capability surface and a plugin surface. Keeping them isolated helps testing and extension.

### 6. `rune-models`

Provider abstraction for:

- OpenAI-style providers
- Azure OpenAI / Azure AI Foundry style providers
- Anthropic and others later
- embeddings
- transcription / TTS capability routing where appropriate

Azure-specific handling belongs here explicitly.

### 7. `rune-channels`

Normalized channel abstraction and provider adapters.

Start with:

- common inbound/outbound traits
- normalized event/message/reaction types
- Telegram adapter first

This crate should depend on `rune-core`, but not on web UI concerns.

### 8. `rune-gateway`

Daemon and operator-facing control surface:

- HTTP/WS server
- auth/token handling
- health/status endpoints
- wiring runtime + jobs + channels + store
- background service lifecycle

This is the runtime host, not the whole domain.

### 9. `rune-cli`

CLI command surface:

- gateway lifecycle commands
- status
- logs
- sessions/jobs/skills commands
- doctor/health commands

Should call into gateway APIs or shared service interfaces rather than reimplement core logic.

### 10. `rune-testkit`

Test helpers, fixtures, fake providers, fake channels, golden transcript checks, parity harness helpers.

This is worth creating early. Parity work benefits heavily from reusable test scaffolding.

---

## Crates that can start later

These are useful, but do not need to exist on day one.

### `rune-memory`

Split this out once memory indexing/retrieval logic becomes substantial.

Initially, basic memory file logic could live in `rune-runtime` + `rune-store`, but it will likely deserve its own crate once:

- indexing
- retrieval ranking
- snippet selection
- embeddings integration
- import/export workflows

become real.

### `rune-jobs`

A dedicated scheduler/jobs crate becomes valuable once cron, reminders, heartbeats, retries, and long-running executions become complex enough.

Early on, a simpler job engine could live inside gateway/runtime.

### `rune-media`

Make this separate once you have enough media surface:

- audio transcription
- image attachment handling
- TTS
- file conversion/normalization

### `rune-skills`

Should eventually own:

- prompt skill manifests
- skill loading/install/update
- references/assets discovery
- plugin metadata
- isolated extension launcher integration

But it can wait until prompt skills are being implemented for real.

### `rune-approvals`

Good eventual split if approvals become rich enough to justify their own policies, stores, lifecycles, and UI flows.

### `rune-api`

If the operator UI needs a clean backend-for-frontend API surface distinct from the raw gateway control plane, split this out later.

### `rune-telemetry`

Useful once tracing/metrics/event export becomes substantial across the workspace.

---

## Dependency direction

Target dependency flow should look roughly like this:

```text
rune-core
  ↑
rune-config   rune-store   rune-telemetry
  ↑             ↑
rune-models  rune-tools  rune-channels  rune-memory  rune-jobs  rune-media  rune-skills  rune-approvals
   \            |            |              |             |            |             |              /
    \-----------+------------+--------------+-------------+------------+-------------+-------------/
                                 ↑
                            rune-runtime
                                 ↑
                        rune-gateway   rune-cli
                                 ↑
                              apps/*
```

The key idea:

- `rune-core` sits at the center
- infrastructure crates support runtime
- runtime owns behavior orchestration
- gateway and CLI are delivery surfaces

## Dependency rules that should not be broken casually

1. `rune-core` must not depend on transport, database driver, or provider SDK crates.
2. `rune-runtime` may depend on contracts/interfaces from tools/models/channels/store, but delivery formatting belongs outside it.
3. `rune-cli` should be thin and never become a second runtime.
4. `rune-gateway` should wire subsystems together, not become the place where business rules secretly live.
5. Azure-specific provider behavior should stay in `rune-models` or adjacent provider crates, not leak across the whole workspace.
6. Storage backend specifics should stay behind repository interfaces or storage adapters.
7. Plugin/process isolation concerns should not force core session types to know about specific sandbox implementations.

Breaking any of those may be justified later, but only with explicit reason because they are the main protection against architecture rot.

## Crate-to-phase mapping

A practical mapping from architecture to implementation phases:

- Phase 1-2 foundation
  - `rune-core`
  - `rune-config`
  - `rune-store`
  - `rune-runtime`
  - `rune-gateway`
  - `rune-cli`
  - `rune-testkit`
- Phase 3-4 parity core
  - `rune-tools`
  - jobs functionality initially inside runtime/gateway or later `rune-jobs`
  - approvals functionality initially inside runtime/tools or later `rune-approvals`
- Phase 5-7 contextual/runtime breadth
  - `rune-memory`
  - `rune-channels`
  - `rune-media`
- Phase 8+
  - `rune-skills`
  - `rune-api`
  - `rune-telemetry`

The point is to match crate extraction to proven seams, not to force all crates into existence before they have enough weight.

---

## Suggested responsibilities per crate

## `rune-core`

Owns:

- domain entities
- capability/policy primitives
- event types
- state enums
- normalized channel/tool/job/session contracts

Must not own:

- database schemas
- HTTP handlers
- provider SDK clients

## `rune-config`

Owns:

- app config model
- env parsing
- path conventions
- secret references
- default config generation

Must not own:

- runtime policy decisions that belong to domain logic

## `rune-store`

Owns:

- DB migrations
- repositories
- persistence transactions
- transcript storage
- search metadata persistence

Must not own:

- scheduling policy
- tool approval logic
- channel transport logic

## `rune-runtime`

Owns:

- turn loop
- session lifecycle
- compaction/pruning
- tool call orchestration
- context assembly
- multi-agent/task orchestration if added later

Must not own:

- low-level HTTP transport
- direct CLI formatting

## `rune-tools`

Owns:

- built-in tool registry
- tool schemas
- tool execution adapters
- streaming/background process tool support
- capability checks before execution

Must not own:

- top-level session flow

## `rune-models`

Owns:

- provider clients
- request/response normalization
- model capability metadata
- Azure-specific adapter logic
- embeddings/transcription/TTS client wrappers where appropriate

Must not own:

- session history policy

## `rune-channels`

Owns:

- inbound normalization
- outbound send/edit/reply/react abstractions
- channel auth/setup lifecycle helpers
- provider-specific adapters

Must not own:

- core agent orchestration

## `rune-gateway`

Owns:

- daemon bootstrap
- service wiring
- HTTP/WS routing
- auth middleware
- health/status APIs
- background supervisors

Must not own:

- reusable domain types that belong in core

## `rune-cli`

Owns:

- Clap commands
- shell-facing output rendering
- operator command ergonomics

Must not own:

- duplicated business logic from gateway/runtime

---

## App layer vs crate layer

I recommend keeping runnable binaries separated from library crates:

### `apps/gateway`

Tiny binary crate that wires config + gateway and starts the daemon.

### `apps/cli`

Tiny binary crate that wires CLI entrypoints and delegates into `rune-cli`.

Why bother?

Because it keeps the binaries thin and testable, while the real logic remains in libraries.

---

## Recommended phase-by-phase crate rollout

## Phase 1

Start with:

- `rune-core`
- `rune-config`
- `rune-store`
- `rune-runtime`
- `rune-tools`
- `rune-models`
- `rune-channels`
- `rune-gateway`
- `rune-cli`
- `rune-testkit`

Keep these as modules inside existing crates for now if needed:

- memory
- jobs
- media
- skills
- approvals
- telemetry

## Phase 2

Split out if complexity justifies it:

- `rune-memory`
- `rune-jobs`
- `rune-approvals`
- `rune-telemetry`

## Phase 3

Split out when plugin/media surface is real:

- `rune-media`
- `rune-skills`
- `rune-api`

---

## Testing layout recommendation

Monorepo structure should make parity testing easy.

Suggested test strategy:

- unit tests inside each crate
- integration tests under crate-level `tests/`
- shared fakes/fixtures in `rune-testkit`
- golden behavior tests for:
  - tool outputs
  - session turn sequencing
  - transcript persistence
  - CLI responses
  - scheduler behavior
- future parity harness that compares selected OpenClaw workflows against the Rust rewrite

This project will live or die on behavioral fidelity more than on elegant crate diagrams.

---

## Naming recommendation

Use one consistent prefix.

I used `rune-*` here because it matches the existing planning language, but the actual naming should be decided once and kept stable.

Good options:

- `rune-*` for internal codename continuity
- `openclaw-*` if direct product alignment matters more
- a neutral new product prefix if this is intended to stand alone later

My bias: **keep `rune-*` internally during rewrite planning**, and only rename if branding/product positioning changes later.

---

## Opinionated final recommendation

If you want the shortest path to a credible first implementation, do **not** start with the full crate graph.

Start with this practical backbone:

- `rune-core`
- `rune-config`
- `rune-store`
- `rune-runtime`
- `rune-tools`
- `rune-models`
- `rune-channels`
- `rune-gateway`
- `rune-cli`
- `rune-testkit`

Then split out memory, jobs, skills, approvals, and media only when they become painful to keep inside the larger crates.

That gives you:

- enough separation to stay sane
- enough focus to ship parity work
- enough flexibility to grow into a larger Rust monorepo without rewriting everything later
