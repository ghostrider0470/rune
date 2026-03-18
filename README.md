<p align="center">
  <img src="assets/hero.png" alt="Rune Hero" width="800" />
</p>

# Rune

<p align="left">
  <img src="assets/rune-logo-wordmark-dark.svg" alt="Rune Logo" height="48" />
</p>

A Rust-based personal AI runtime designed as a high-performance OpenClaw-style gateway with strong Azure-oriented provider support, durable state, and standalone-first operation.

## Current status

Rune is an active parity-seeking runtime buildout, not a parity-complete replacement claim.

Today, the project already has:
- standalone and service-style runtime operation
- durable PostgreSQL-backed state with embedded local fallback
- gateway, dashboard, sessions, cron, and tool-execution surfaces
- strong Azure-oriented model-provider support
- GitHub Project 2 as the live execution control plane

## Why Rune

- **Rust runtime core** — durable, inspectable, and built for long-running gateway operation
- **Standalone-first** — runs well on one machine without forcing distributed-system complexity
- **Server-grade path** — still designed to scale into Docker, VM, and broader hosted deployments
- **Azure-oriented** — Azure OpenAI / Azure AI Foundry support is a first-class requirement, not an afterthought
- **Operator-visible** — health, status, diagnostics, logs, and persistent state matter as product features

## Quick start

```bash
cargo build --release --bin rune-gateway
cp config.example.toml config.toml
# fill in your provider + channel settings
./target/release/rune-gateway --config config.toml
```

Then open `http://127.0.0.1:8787/dashboard`.

For fuller development setup and service-style local operation, see [`docs/contributor/DEVELOPMENT.md`](docs/contributor/DEVELOPMENT.md).

## What Rune does

Rune sits between messaging channels and model providers. It manages:
- sessions and turn execution
- tool calls and approvals
- cron jobs, reminders, and automation
- memory and retrieval workflows
- provider routing and model invocation
- operator-facing control-plane visibility

## Core features (current-state view)

- **Gateway + dashboard** — health/status surfaces and an operator dashboard are live
- **Durable storage** — PostgreSQL-backed persistence with embedded local fallback for zero-config development
- **Tool runtime** — built-in file, exec/process, cron, session, and memory-oriented tools are implemented
- **Provider layer** — Azure AI Foundry, Azure OpenAI, OpenAI, and Anthropic provider paths are part of the active runtime shape
- **Docs + execution discipline** — ADR trail, source-of-truth boundaries, and Project 2 execution model are now explicit

## Standalone vs server runtime modes

Rune is **standalone-first** by default:
- one operator
- one machine
- clear local control plane
- durable local state

It also supports a broader server-grade path:
- Docker deployment
- service-manager operation
- external PostgreSQL
- future Azure-hosted deployment options

That means local-first is the default experience, not a throwaway dev-only mode.

## Documentation

- [`docs/INDEX.md`](docs/INDEX.md) — docs front door by audience and concern
- [`rune-plan.md`](rune-plan.md) — canonical product strategy and planning summary
- [`docs/OPENCLAW-COVERAGE-MAP.md`](docs/OPENCLAW-COVERAGE-MAP.md) — OpenClaw-surface parity navigation
- [`docs/operator/DEPLOYMENT.md`](docs/operator/DEPLOYMENT.md) — deployment model
- [`docs/operator/DATABASES.md`](docs/operator/DATABASES.md) — storage model and database choices
- [`docs/parity/PROTOCOLS.md`](docs/parity/PROTOCOLS.md) — runtime and protocol contracts
- [`docs/adr/README.md`](docs/adr/README.md) — durable architecture decision trail

## For contributors

Start here:
- [`docs/contributor/DEVELOPMENT.md`](docs/contributor/DEVELOPMENT.md)
- [`docs/AGENT-ORCHESTRATION.md`](docs/AGENT-ORCHESTRATION.md)
- [`docs/reference/CRATE-LAYOUT.md`](docs/reference/CRATE-LAYOUT.md)
- [`docs/reference/SUBSYSTEMS.md`](docs/reference/SUBSYSTEMS.md)

## License

Private — [Horizon Tech d.o.o.](https://horizontech.ba)
