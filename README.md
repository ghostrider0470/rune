# OpenClaw Rust Rewrite

Goal: design and build a personal Rust rewrite of OpenClaw with near-identical end-user functionality, stronger performance characteristics, and an extension model that supports Rust-native skills/plugins.

This repo is now in active implementation. The planning docs remain the execution authority for parity, sequencing, and acceptance criteria.

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
