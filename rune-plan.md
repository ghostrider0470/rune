# Rune Plan

> Status: Canonical product strategy and planning summary as of 2026-03-17.
>
> Use this file for Rune's goals, product direction, confirmed stack choices, and high-level delivery map.
> Use `docs/IMPLEMENTATION-PHASES.md` for parity-phase acceptance criteria and sequencing.
> Use GitHub Project 2 for live execution state.
> Legacy planning files in `docs/PLAN.md`, `docs/STACK.md`, and `docs/WORKPLAN.md` remain as provenance during the transition.

## Source of truth

| Concern | Canonical source | Notes |
|---|---|---|
| Product strategy | `rune-plan.md` | Goals, constraints, product shape, stack direction |
| Parity execution phases | `docs/IMPLEMENTATION-PHASES.md` | Acceptance criteria and sequencing rules |
| Live execution state | GitHub Project 2 | Current epics, features, stories, and batch progress |
| Runtime orchestration rules | `docs/AGENT-ORCHESTRATION.md` | Agent workflow and implementation guardrails |
| Detailed parity contracts | `docs/PARITY-SPEC.md`, `docs/PARITY-CONTRACTS.md`, `docs/PROTOCOLS.md` | Subsystem invariants and observable behavior |

## Objective

Build a Rust-based system that replicates OpenClaw's practical functionality while improving durability, observability, and maintainability.

Non-negotiable constraints:

- functionally identical behavior to OpenClaw where parity is claimed
- full Azure compatibility across model, runtime, and infrastructure integrations
- Docker-first deployment with mountable persistent storage

Rune's target product surface includes:

- multi-channel personal assistant runtime
- agent and session orchestration
- gateway daemon and control plane
- tool execution and approvals
- cron and heartbeat automation
- memory and retrieval
- media understanding and TTS flows
- skill and plugin installation paths
- frontend and admin UI

Not the goal:

- line-by-line source compatibility
- preserving TypeScript internals
- implementing the entire surface in one phase

The delivery bias is behavioral parity first, then performance and maintainability gains.

## Rewrite principles

1. **Protocol-first**
   Freeze durable contracts before broad feature work.
2. **Daemon-first**
   Treat the Rust gateway and service layer as the operational core.
3. **Async everywhere**
   Prefer structured concurrency and bounded work queues.
4. **Isolate untrusted extensions**
   Skills and plugins need explicit capability boundaries.
5. **State machines over ad hoc orchestration**
   Sessions, turns, approvals, jobs, and channel flows must be modeled explicitly.
6. **Parity before optimization**
   Preserve operator-visible behavior before tuning internals.
7. **Frontend decoupled from runtime**
   UI must depend on stable APIs rather than runtime internals.

## Product and architecture direction

Rune is intended to be a messaging-first AI gateway with a durable local control plane.

Core architecture direction:

- a long-running gateway daemon exposing HTTP and WebSocket control surfaces
- durable relational state for sessions, turns, jobs, approvals, tool executions, and channel metadata
- human-editable file state for memory, config overlays, logs, exports, and skill bundles
- explicit provider abstractions for model, media, and channel integrations
- inspectable runtime behavior with health, status, diagnostics, and audit trails

The target subsystem shape remains:

- `rune-core` for domain types, events, IDs, and policies
- `rune-runtime` for sessions, turns, tools, and background jobs
- `rune-gateway` for daemon, APIs, auth, health, and control plane
- `rune-cli` for operator workflows
- supporting crates for channels, models, memory, media, storage, and UI-facing contracts

## Confirmed stack direction

Azure compatibility is mandatory and must be first-class rather than handled as a generic-provider afterthought.

Confirmed implementation choices:

- Rust for the backend/runtime
- Tokio for async execution
- Axum for HTTP and WebSocket transport
- Clap for the CLI
- Serde plus TOML/Figment for config and serialization
- `tracing` for logs and diagnostics
- PostgreSQL via Diesel and `diesel-async` as the primary durable store
- `postgresql_embedded` as the zero-config local fallback when no external database is configured
- PostgreSQL FTS and `pgvector` for search and retrieval support
- remote embeddings only in early phases

Provider and interface direction:

- Azure OpenAI / Azure AI Foundry is a hard requirement
- OpenAI, Anthropic, and AWS Bedrock remain first-class provider targets
- Telegram is the first channel priority, with Discord and other adapters following by parity need
- REST plus WebSocket is the operator-facing control plane shape

Frontend and extension direction:

- React 19 + Vite SPA for the admin surface
- TanStack tooling for routing, forms, queries, and tables
- Tailwind CSS 4 with shadcn/ui and Radix UI for the operator UI system
- prompt skills plus isolated executable helpers as the initial extension model

Deployment direction:

- Docker and bare-metal/systemd are both first-class
- durable paths must remain mountable under `/data`, `/config`, and `/secrets`
- runtime design must not block Azure Container Apps, AKS, App Service, or VM-based hosting later

## High-level delivery map

This summary exists to keep the strategic shape visible. The execution source of truth remains `docs/IMPLEMENTATION-PHASES.md`.

1. Phase 0: freeze parity inventory, protocols, contracts, and open questions.
2. Phase 1: stand up the daemon, CLI control plane, health surfaces, and durable stores.
3. Phase 2: land session and turn lifecycle minimum parity.
4. Phase 3: reach first-class tool loop and approvals parity.
5. Phase 4: reproduce scheduling, reminders, heartbeats, and isolated jobs.
6. Phase 5: preserve workspace memory and retrieval behavior.
7. Later phases: channels, skills/plugins, broader parity coverage, and richer operator UX.

## Planning boundaries

Use this file when the question is "what is Rune trying to become?"

Do not use this file for:

- day-to-day task tracking
- per-batch execution logs
- issue workflow status
- fine-grained acceptance evidence

Those belong in GitHub Project 2, issue comments, and the phase-specific parity docs.
