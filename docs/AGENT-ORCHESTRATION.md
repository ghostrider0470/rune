# Agent Orchestration Playbook — Rune (OpenClaw Rust Rewrite)

This document is the master instruction set for AI agents building this project in parallel. Every agent working on this repo MUST read this file before writing any code.

---

## Hard constraints (non-negotiable)

1. **Functionally identical** to OpenClaw from user/operator perspective
2. **Fully Azure compatible** — Azure OpenAI, Azure AI Foundry, Document Intelligence, Azure-friendly deployment
3. **Docker-first** — all durable state under mountable `/data/*` and `/config/*` paths
4. **PostgreSQL** via Diesel + diesel-async, with embedded PostgreSQL fallback for zero-config local dev
5. **No speculative features** — only build what's in the parity spec or explicitly requested by the operator (Hamza)

---

## Project state

- **15+ design docs** in `docs/` define architecture, parity contracts, protocols, and phasing
- **A runnable implementation baseline already exists** — the Cargo workspace, crate graph, gateway/CLI binaries, PostgreSQL store wiring, embedded PostgreSQL fallback, runtime skeleton, and a growing parity-oriented test surface are already in the repo
- **Treat this document as implementation context, not workflow authority** — use it for subsystem direction and repo-state reality, but use the accepted batch-branch workflow docs/ADRs for current branch/PR execution rules
- **Crate prefix is `rune-`** — all crate names use `rune-*` (e.g., `rune-core`, `rune-runtime`)

### Current implementation reality (keep this in sync with code)

As of 2026-03-14 early-morning execution:
- workspace `cargo test` is green
- gateway and CLI are runnable, not stubs
- embedded PostgreSQL fallback is wired for zero-config local development
- cron definitions and `cron runs` history are durably persisted
- scheduled `main` vs `isolated` execution semantics are no longer collapsed together
- per-session turn aggregates (usage/model/timing) already exist on the gateway session surfaces
- durable approval requests and decision-time resume are wired end-to-end for approval-gated tool calls
- local conservative `ask` / `security` / `elevated` exec semantics are enforced in the live gateway tool path
- background `exec` launches now persist restart-visible metadata into durable tool-execution audit rows, with honest degraded `process` inspection after restart

Highest-leverage remaining parity gaps:
1. restart-safe continuation guarantees for approval-resumed turns and live process handles across gateway restarts
2. broader persistence/inspectability for subagent lifecycle beyond scheduled descendants
   - Note: baseline durable parent/requester linkage for direct/channel/scheduled/subagent sessions already exists in runtime tests, and live `sessions_spawn` now also preserves requester linkage when the caller provides `sessionKey`/`requesterSessionId`; remaining gap is richer lifecycle/runtime inspectability rather than the linkage primitive itself.
3. deeper session-status parity quality (cost fidelity, unresolved-note reduction, broader runtime linkage)
4. richer host/node/sandbox parity beyond the current local conservative execution baseline
5. cross-platform PTY fidelity beyond the current Unix `script(1)`-backed implementation

---

## Architecture summary

```
rune-core           (domain types, IDs, state machines, errors)
  ^
rune-config         (layered config, env/file/CLI, secrets refs)
rune-store          (Diesel repos, migrations, persistence)
rune-testkit        (shared fakes, fixtures, golden test helpers)
  ^
rune-models         (provider abstraction: Azure OpenAI, OpenAI, Anthropic)
rune-tools          (tool registry, schemas, execution adapters)
rune-channels       (normalized channel model, provider adapters)
  ^
rune-runtime        (session engine, turn loop, context assembly, tool orchestration)
  ^
rune-gateway        (daemon, HTTP/WS, auth, health, service wiring)
rune-cli            (Clap commands, operator output)
  ^
apps/gateway        (thin binary)
apps/cli            (thin binary)
```

**Dependency flows downward.** No crate may depend on a crate below it in this diagram.

---

## Historical wave execution model

The wave model below is historical planning context, not the current default workflow authority.

Current default execution is the accepted batch-branch model:
- one active coding branch
- one active PR
- one coherent milestone batch
- no git worktrees by default

Use this section to understand older crate sequencing intent, not to override the current contributor workflow or ADR-backed execution rules.

### Wave 0 — Workspace bootstrap
**Status:** completed baseline
**Agents: 1**

| Task | Details |
|------|---------|
| Initialize Cargo workspace | Root `Cargo.toml` with `[workspace]`, all 10 phase-1 crate members, shared `[workspace.dependencies]` for common deps (serde, thiserror, tokio, tracing, uuid, diesel, axum, clap) |
| Create crate skeletons | `cargo init --lib` for each library crate. Each gets a `lib.rs` with a `// TODO` comment. `apps/gateway` and `apps/cli` get `--bin`. |
| Add `rust-toolchain.toml` | Pin stable Rust edition 2024 |
| Add `.gitignore` | Standard Rust `.gitignore` + `/data/` + `/secrets/` |
| Verify | `cargo check` passes on the empty workspace |

**Output gate:** `cargo check` succeeds. All 10 crates + 2 app binaries exist.

**Current repo status:** satisfied. Treat Wave 0 as complete and build forward from the existing workspace unless a regression requires repair.

For current branch/PR execution rules, see:
- [`contributor/WORKFLOW.md`](contributor/WORKFLOW.md)
- [`contributor/EXECUTION-SPEED-POLICY.md`](contributor/EXECUTION-SPEED-POLICY.md)
- [`adr/ADR-0001-execution-workflow-and-speed.md`](adr/ADR-0001-execution-workflow-and-speed.md)
- [`adr/ADR-0004-project-2-execution-model.md`](adr/ADR-0004-project-2-execution-model.md)

---

### Wave 1 — Core types + config + test scaffolding
**Agents: 3 (parallel)**

#### Agent 1A: `rune-core`
Build the domain type foundation. Read `docs/parity/PROTOCOLS.md` for the canonical entity model.

Implement:
- `SessionId`, `TurnId`, `ToolCallId`, `JobId`, `ApprovalId`, `ChannelId` — newtype wrappers over `uuid::Uuid`, with `Display`, `FromStr`, `Serialize`, `Deserialize`
- `SessionStatus` enum: `Created`, `Ready`, `Running`, `WaitingForTool`, `WaitingForApproval`, `WaitingForSubagent`, `Suspended`, `Completed`, `Failed`, `Cancelled`
- `SessionKind` enum: `Direct`, `Channel`, `Scheduled`, `Subagent`
- `TurnStatus` enum: `Started`, `ModelCalling`, `ToolExecuting`, `Completed`, `Failed`, `Cancelled`
- `ApprovalDecision` enum: `AllowOnce`, `AllowAlways`, `Deny`
- `ToolCategory` enum: `FileRead`, `FileWrite`, `ProcessExec`, `ProcessBackground`, `SessionControl`, `MemoryAccess`, `SchedulerControl`
- `TranscriptItem` enum: `UserMessage`, `AssistantMessage`, `ToolRequest`, `ToolResult`, `ApprovalRequest`, `ApprovalResponse`, `StatusNote`, `SubagentResult`
- Shared error types (`CoreError`)
- `NormalizedMessage` struct for cross-channel message normalization
- All types derive `Clone`, `Debug`, `Serialize`, `Deserialize` at minimum
- Use `thiserror` for error types
- **No** dependencies on Axum, Diesel, Tokio, or any provider SDK

**Tests:** Unit tests for serialization round-trips, `Display`/`FromStr` on IDs, enum variant coverage.

#### Agent 1B: `rune-config`
Build the config loading system. Read `docs/operator/DEPLOYMENT.md` for the canonical path layout.

Implement:
- `AppConfig` struct with sections: `gateway`, `database`, `models`, `channels`, `memory`, `media`, `logging`, `paths`
- `GatewayConfig`: `host`, `port`, `auth_token` (optional)
- `DatabaseConfig`: `database_url` (optional — triggers embedded PG when absent), `max_connections`, `run_migrations`
- `ModelsConfig`: vec of `ModelProviderConfig` (provider name, endpoint, deployment_name, api_version, api_key_env, model_alias)
- `PathsConfig` with defaults: `db_dir` → `/data/db`, `sessions_dir` → `/data/sessions`, `memory_dir` → `/data/memory`, `media_dir` → `/data/media`, `skills_dir` → `/data/skills`, `logs_dir` → `/data/logs`, `backups_dir` → `/data/backups`, `config_dir` → `/config`, `secrets_dir` → `/secrets`
- Layered loading: defaults → config file (TOML) → env vars → CLI overrides
- Use `figment` for layered defaults → config file (TOML) → env vars → CLI overrides to match the confirmed stack
- `ConfigError` type
- Depends only on `rune-core` (for shared types if needed, otherwise standalone)

**Tests:** Default config loads. Env var override works. Missing optional fields use defaults. Invalid config returns clear errors.

#### Agent 1C: `rune-testkit`
Build the shared test infrastructure.

Implement:
- `TestDb` helper: spins up an embedded PostgreSQL instance for integration tests, runs migrations, provides connection, drops on `Drop`
- `FakeModelProvider`: implements the model provider trait (define a placeholder trait if `rune-models` isn't ready — use the same trait signature planned in `docs/parity/PROTOCOLS.md`). Returns canned responses.
- `FakeChannel`: implements channel adapter trait. Captures sent messages for assertion.
- `fixture!` macro or helper functions for creating test sessions, turns, transcript items with sensible defaults
- Golden test helper: compare actual output against `.expected` files, with update-on-env-var (`RUNE_UPDATE_GOLDEN=1`)
- Depends on `rune-core`

**Tests:** Verify fixture helpers produce valid domain objects. Golden test helper detects diff and passes on match.

**Output gate:** `cargo check` passes. `cargo test` passes for all three crates. No cross-dependencies between 1A/1B/1C except on `rune-core`.

---

### Wave 2 — Storage + model abstraction + tools
**Agents: 3 (parallel)**

#### Agent 2A: `rune-store`
Build the persistence layer. Read `docs/operator/DATABASES.md` for decisions.

Implement:
- Diesel schema + migrations for: `sessions`, `turns`, `transcript_items`, `jobs`, `approvals`, `tool_executions`, `channel_deliveries`
- Repository traits: `SessionRepo`, `TurnRepo`, `TranscriptRepo`, `JobRepo`, `ApprovalRepo`, `ToolExecutionRepo`
- PostgreSQL implementations using `diesel-async`
- Embedded PostgreSQL bootstrap (using `postgresql_embedded` crate) when `DATABASE_URL` is not set
- `StoreError` type
- Depends on `rune-core`, `rune-config`

**Tests:** Integration tests using `TestDb` from `rune-testkit`. CRUD for each entity. Verify migrations run cleanly. Test embedded PG bootstrap.

#### Agent 2B: `rune-models`
Build the provider abstraction. Read `docs/AZURE-COMPATIBILITY.md`.

Implement:
- `ModelProvider` trait: `async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, ModelError>`
- `CompletionRequest`: messages, model, temperature, max_tokens, tools (optional), system prompt
- `CompletionResponse`: content, usage (prompt_tokens, completion_tokens), finish_reason, tool_calls (optional)
- `AzureOpenAiProvider`: constructs Azure-specific URLs (`{endpoint}/openai/deployments/{deployment}/chat/completions?api-version={version}`), handles Azure auth headers
- `OpenAiProvider`: standard OpenAI-compatible endpoint
- `ModelError` type with variants for auth, rate limit, context length, provider-specific
- Config-driven provider selection
- Depends on `rune-core`, `rune-config`
- Uses `reqwest` for HTTP

**Tests:** Unit tests with mock HTTP (use `wiremock`). Verify Azure URL construction. Verify header handling. Verify error mapping from HTTP status codes. Test provider selection from config.

#### Agent 2C: `rune-tools`
Build the tool system skeleton. Read `docs/parity/PROTOCOLS.md` tool execution contract.

Implement:
- `ToolDefinition` struct: name, description, parameters (JSON Schema), category, requires_approval
- `ToolRegistry`: register tools, lookup by name, list all
- `ToolExecutor` trait: `async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError>`
- Built-in tool stubs (not full implementations yet): `read_file`, `write_file`, `edit_file`, `list_files`, `search_files`, `execute_command`, `list_sessions`, `get_session_status`
- `ToolCall` struct: tool_call_id, tool_name, arguments (serde_json::Value)
- `ToolResult` struct: tool_call_id, output (String), is_error
- Approval check hook (trait method, not full implementation)
- Depends on `rune-core`

**Tests:** Registry add/lookup/list. Tool schema validation. Stub execution returns expected shapes.

**Output gate:** `cargo check` passes. `cargo test` passes. Store migrations run against embedded PG. Model provider constructs correct Azure requests.

---

### Wave 3 — Runtime engine
**Agents: 1** (this is the critical path — it wires everything together)

#### Agent 3A: `rune-runtime`
Build the session engine and turn loop. This is the heart of the system. Read `docs/parity/PROTOCOLS.md` and `docs/IMPLEMENTATION-PHASES.md` Phase 2.

Implement:
- `SessionEngine`: creates sessions, manages lifecycle, persists state via `rune-store`
- `TurnExecutor`: executes a single turn:
  1. Load session context
  2. Assemble prompt (system + transcript + context)
  3. Call model provider
  4. If tool calls in response → execute tools → loop back to model
  5. Persist transcript items at each step
  6. Update session/turn status
- `ContextAssembler`: builds the prompt from session history, system instructions, relevant context
- `CompactionStrategy` trait + no-op implementation: for future transcript pruning
- Turn state machine enforcement (Started → ModelCalling → ToolExecuting → ... → Completed/Failed)
- Usage tracking per turn
- Error recovery: failed model calls update turn status, don't crash
- Depends on `rune-core`, `rune-config`, `rune-store`, `rune-models`, `rune-tools`

**Tests:** Full turn cycle with `FakeModelProvider` and in-memory store. Tool loop with multi-step tool calls. Failed model call recovery. Session status transitions. Transcript ordering verification.

**Output gate:** A turn can execute end-to-end: create session → send message → model responds → tools called → final response persisted. All via test fakes.

---

### Wave 4 — Delivery surfaces
**Agents: 3 (parallel)**

#### Agent 4A: `rune-gateway`
Build the daemon and HTTP/WS server. Read `docs/IMPLEMENTATION-PHASES.md` Phase 1.

Implement:
- Axum-based HTTP server
- Routes: `GET /health`, `GET /status`, `POST /sessions`, `POST /sessions/{id}/messages`, `GET /sessions/{id}`, `GET /sessions/{id}/transcript`
- WebSocket endpoint: `GET /ws` with subscribe-to-session events
- Auth middleware: bearer token from config
- Daemon lifecycle: start, graceful shutdown
- Service wiring: construct runtime, store, model provider from config
- Structured JSON logging via `tracing` + `tracing-subscriber`
- Background service supervisor (placeholder for scheduler, channels)
- Depends on `rune-core`, `rune-config`, `rune-store`, `rune-runtime`, `rune-models`, `rune-tools`

**Tests:** HTTP endpoint tests with `axum::test`. Health/status return correct shapes. Auth rejection on bad token. Session create → message → response flow through HTTP.

#### Agent 4B: `rune-cli`
Build the operator CLI. Read `docs/parity/PARITY-INVENTORY.md` for Tier-0 commands.

Implement:
- Clap-based CLI with subcommands:
  - `rune gateway start` — start the daemon (calls into gateway)
  - `rune gateway stop` — graceful shutdown
  - `rune status` — query gateway status
  - `rune health` — health check
  - `rune doctor` — diagnostic checks (config valid, DB reachable, model provider reachable)
  - `rune sessions list` — list sessions
  - `rune sessions show <id>` — show session details
  - `rune config show` — dump resolved config
  - `rune config validate` — validate config file
- Output formatting: JSON (`--json`) and human-readable (default)
- Gateway client: HTTP calls to the gateway API
- Depends on `rune-core`, `rune-config`
- Uses `reqwest` for gateway communication

**Tests:** CLI argument parsing. Output format switching. Config validation catches errors.

#### Agent 4C: `rune-channels` (skeleton)
Build the normalized channel abstraction. Read `docs/parity/PROTOCOLS.md` channel contracts.

Implement:
- `ChannelAdapter` trait: `async fn receive(&mut self) -> Result<InboundEvent>`, `async fn send(&self, action: OutboundAction) -> Result<DeliveryReceipt>`
- `InboundEvent` enum: `Message`, `Reaction`, `Edit`, `Delete`, `MemberJoin`, `MemberLeave`
- `OutboundAction` enum: `Send`, `Reply`, `Edit`, `React`, `Delete`
- `ChannelMessage` struct: channel_id, sender, content, attachments metadata, timestamp, provider_message_id
- `DeliveryReceipt` struct: provider_message_id, delivered_at
- Telegram adapter stub (compiles, returns `unimplemented!()` for now — real implementation in a later wave)
- Depends on `rune-core`

**Tests:** Trait object construction. InboundEvent/OutboundAction serialization. Message normalization from fake provider payloads.

**Output gate:** `cargo check` passes. Gateway starts and responds to health checks. CLI can query gateway status. Channel traits compile and are usable from runtime.

---

### Wave 5 — App binaries + integration
**Agents: 2 (parallel)**

#### Agent 5A: `apps/gateway`
Thin binary:
```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // load config
    // init tracing
    // build services
    // start gateway
}
```

#### Agent 5B: `apps/cli`
Thin binary:
```rust
fn main() -> anyhow::Result<()> {
    // parse CLI args
    // dispatch to rune-cli
}
```

**Output gate:** `cargo build` produces two binaries. Gateway binary starts and serves `/health`. CLI binary can run `rune health` against the gateway.

---

## Escalation protocol

### When to call Hamza (the operator)

**STOP and ask** before proceeding if:

1. **Architecture conflict** — you discover the docs contradict each other or a design decision seems wrong
2. **Dependency ambiguity** — you need a type/trait from another crate that doesn't exist yet and the contract isn't clear from docs
3. **Scope creep** — you're tempted to build something not in your wave's task list
4. **Breaking change** — you need to change a type/trait that another agent's crate already depends on
5. **External service needed** — you need an API key, cloud resource, or access to OpenClaw source code
6. **Test infrastructure gap** — `rune-testkit` doesn't provide what you need and the workaround is ugly
7. **Performance concern** — embedded PG startup is too slow for tests, compilation is broken, etc.
8. **Open question hit** — you encounter one of the 2 unresolved questions from `notes/open-questions.md`

### How to escalate

Add a file: `ESCALATION-{agent-id}-{timestamp}.md` in the repo root with:
```markdown
## Blocked: {one-line summary}

**Agent:** {which crate you're working on}
**Wave:** {wave number}
**Issue:** {detailed description}
**Options I see:** {A, B, or C}
**My recommendation:** {which option and why}
**Blocked on:** {what you need to proceed}
```

Then **stop working on the blocked item** and continue with any unblocked tasks in your scope.

---

## Coordination rules

### Shared trait contracts

When a crate defines a trait that other crates will implement or consume:
1. Put the trait in `rune-core` if it's truly cross-cutting (e.g., entity IDs)
2. Put the trait in the defining crate's public API otherwise (e.g., `ModelProvider` lives in `rune-models`)
3. **Document the trait with a doc comment explaining the contract** — other agents will read this, not ask you

### Breaking changes

If you need to change a public type or trait that another crate depends on:
1. Make the change
2. Fix all compilation errors in crates you own
3. If other crates break, add a `BREAKING-CHANGE.md` note in the repo root:
   ```
   ## Breaking: {type/trait name} changed
   Crate: rune-{x}
   What changed: {description}
   Migration: {how to update consuming code}
   ```
4. The affected crate's agent picks this up and adapts

### Commit conventions

- One commit per logical unit of work (not per file)
- Message format: `rune-{crate}: {what changed}`
- Examples: `rune-core: add session and turn domain types`, `rune-store: initial Diesel schema and migrations`
- Agents working in parallel should work on **separate branches**: `wave-{n}/{crate-name}`
- Merge to `main` only after wave gate passes

### File organization within crates

```
crates/rune-{name}/
  Cargo.toml
  src/
    lib.rs          — public API re-exports
    {module}.rs     — one file per logical module
    {module}/       — subdirectory only if module has 3+ files
      mod.rs
      ...
  tests/
    {test_name}.rs  — integration tests
```

---

## What to read before starting

Every agent MUST read these files before writing code:

| File | Why |
|------|-----|
| `rune-plan.md` | Canonical goals, product direction, and confirmed stack constraints |
| `docs/reference/CRATE-LAYOUT.md` | Crate responsibilities and dependency rules |
| `docs/parity/PROTOCOLS.md` | Entity model, state machines, subsystem contracts |
| `docs/operator/DATABASES.md` | PostgreSQL decision, Diesel, embedded PG, FTS, pgvector |
| `docs/parity/PARITY-SPEC.md` | What parity means and what's in scope |
| `docs/IMPLEMENTATION-PHASES.md` | Phase acceptance criteria and sequencing rules |
| This file | Orchestration rules, wave model, escalation |

Read **only** the docs relevant to your crate. Don't read all 15 docs if you're building `rune-config`.

---

## Quality gates (every agent, every wave)

Before declaring your work done:

- [ ] `cargo check` passes for the entire workspace
- [ ] `cargo test` passes for your crate
- [ ] `cargo clippy` has no warnings for your crate
- [ ] `cargo doc --no-deps` builds without warnings for your crate
- [ ] No `unwrap()` or `expect()` in library code (use proper error types)
- [ ] No `println!()` — use `tracing` for all logging
- [ ] Public types and traits have doc comments
- [ ] No dependencies on crates that violate the dependency direction diagram

---

## Technology quick-reference

| Concern | Choice |
|---------|--------|
| Async runtime | Tokio |
| HTTP framework | Axum |
| Database ORM | Diesel + diesel-async |
| Database | PostgreSQL (embedded fallback via `postgresql_embedded`) |
| Serialization | serde + serde_json |
| CLI framework | Clap (derive) |
| Error handling | thiserror (libraries), anyhow (binaries) |
| Logging | tracing + tracing-subscriber |
| HTTP client | reqwest |
| Testing | built-in + wiremock for HTTP mocks |
| IDs | uuid v7 (time-sortable) |
| Config format | TOML |

---

## Anti-patterns to avoid

1. **Don't build what isn't in your wave.** If you see a gap that a future wave covers, leave a `// TODO(wave-N): description` comment and move on.
2. **Don't add channels/skills/media in early waves.** Phase 1-2 is daemon + runtime + tools. That's it.
3. **Don't abstract prematurely.** One provider implementation before a trait. One channel before a normalized model. One store backend before a repository trait.
4. **Don't mock the database in integration tests.** Use `TestDb` with real embedded PostgreSQL.
5. **Don't leak Azure specifics outside `rune-models`.** Azure is a provider detail, not a core concern.
6. **Don't add features not in the parity spec.** This is a rewrite, not a greenfield product.
7. **Don't use `Box<dyn Error>` in library crates.** Use typed errors with `thiserror`.
8. **Don't skip failure path tests.** Every success path needs a corresponding failure test.

---

## Summary for quick dispatch

```
Wave 0:  1 agent   — workspace bootstrap
Wave 1:  3 agents  — rune-core, rune-config, rune-testkit        (parallel)
Wave 2:  3 agents  — rune-store, rune-models, rune-tools          (parallel)
Wave 3:  1 agent   — rune-runtime                                 (critical path)
Wave 4:  3 agents  — rune-gateway, rune-cli, rune-channels        (parallel)
Wave 5:  2 agents  — apps/gateway, apps/cli                       (parallel)
         ─────────
Total:   ~6 waves, up to 3 agents concurrent
```

Each wave should take one focused work session. The full Phase 1 skeleton (daemon + runtime + tools + CLI) should be standing after all 6 waves.
