# OpenClaw Rust Rewrite

Goal: design and build a personal Rust rewrite of OpenClaw with near-identical end-user functionality, stronger performance characteristics, and an extension model that supports Rust-native skills/plugins.

This repo is now in active implementation. The planning docs remain the execution authority for parity, sequencing, and acceptance criteria.

## Current implementation snapshot

- Phase-1 skeleton is in place across the initial `rune-*` workspace and both app binaries.
- `cargo test` and `cargo clippy --all-targets --all-features -- -D warnings` are currently green at the workspace root.
- `apps/gateway` is no longer a stub exit path: it now boots a zero-config in-memory gateway/runtime stack so the current HTTP/WS control-plane surface is executable during development.
- Current smoke-tested gateway surface:
  - `GET /health`
  - `GET /status`
  - `GET /gateway/health`
  - `POST /gateway/start`
  - `POST /gateway/stop`
  - `POST /gateway/restart`
  - `GET /sessions`
  - `POST /sessions`
  - `GET /sessions/{id}`
  - `POST /sessions/{id}/messages`
  - `GET /sessions/{id}/transcript`
- Current runnable CLI parity slice now includes:
  - `rune gateway status`
  - `rune gateway health`
  - `rune gateway start`
  - `rune gateway stop`
  - `rune gateway restart`
  - `rune status`
  - `rune health`
  - `rune doctor`
  - `rune cron status`
  - `rune cron list`
  - `rune cron add --text ... --at ...`
  - `rune cron edit <id> --name ...`
  - `rune cron enable <id>` / `rune cron disable <id>`
  - `rune cron rm <id>`
  - `rune cron run <id>`
  - `rune cron runs <id>`
  - `rune sessions list`
  - `rune sessions show <id>`
  - `rune config show`
  - `rune config validate`
- Current smoke-tested runtime flow: create session -> send message -> receive assistant reply -> inspect persisted transcript.
- This runnable path is transitional and intentionally zero-config. Release-target persistence remains PostgreSQL via Diesel + `diesel-async`, with embedded PostgreSQL fallback for local dev.

## Current deliverables

- `docs/PLAN.md` — rewrite scope, architecture, subsystem breakdown, migration strategy
- `docs/PARITY-SPEC.md` — governing parity definition and release gate
- `docs/PARITY-CONTRACTS.md` — subsystem-by-subsystem implementation-grade parity contracts
- `docs/PARITY-INVENTORY.md` — anchor OpenClaw surface inventory and parity map, including the evidence-tiered local CLI command-family census, sampled subcommand breadth, legacy-alias notes, minimum control-plane resource/event matrix, config-domain census, storage/database posture guardrails, and operator-visible parity priorities
- `docs/FUNCTIONALITY-CHECKLIST.md` — detailed parity checklist
- `docs/PROTOCOLS.md` — canonical entities, state machines, command/resource/event matrices, and runtime/control-plane protocol expectations
- `docs/CRATE-LAYOUT.md` — Rust workspace/crate boundaries and dependency rules
- `docs/AZURE-COMPATIBILITY.md` — Azure request semantics, storage mappings, hosting expectations, and release-blocker compatibility contract
- `docs/DOCKER-DEPLOYMENT.md` — Docker-first persistent-state deployment model, restart/probe expectations, and mount-failure parity requirements
- `docs/DATABASES.md` — storage/database options and phased recommendations
- `docs/IMPLEMENTATION-PHASES.md` — parity-first sequencing and acceptance milestones
- `docs/COMPETITIVE-RESEARCH.md` — external references and what to borrow/avoid
- `notes/open-questions.md` — unresolved design decisions

## Ground rules

- Treat OpenClaw behavior as the compatibility target
- Prefer protocol and behavior parity over implementation parity
- Optimize for performance, observability, safety, and future extensibility
- Keep code aligned to the docs, and reconcile stale contradictions explicitly when implementation overtakes old planning text
