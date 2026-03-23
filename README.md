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
cargo build --release --bin rune-gateway --bin rune
cp config.example.toml config.toml
# fill in your provider + channel settings
./target/release/rune-gateway --config config.toml
```

Then open `http://127.0.0.1:8787/dashboard`.

To generate a user service for unattended operation:

```bash
./target/release/rune service install   --target systemd   --name rune-gateway   --workdir "$PWD"   --config "$PWD/config.toml"

systemctl --user daemon-reload
systemctl --user enable --now rune-gateway
```

On macOS, swap `--target launchd` to write a LaunchAgent plist instead.

Next docs from there:
- [`docs/getting-started/QUICKSTART.md`](docs/getting-started/QUICKSTART.md)
- [`docs/getting-started/INSTALL.md`](docs/getting-started/INSTALL.md)
- [`docs/operator/README.md`](docs/operator/README.md)
- [`docs/contributor/DEVELOPMENT.md`](docs/contributor/DEVELOPMENT.md)

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

- use [`docs/INDEX.md`](docs/INDEX.md) if you want the docs front door by audience and concern
- use [`docs/getting-started/README.md`](docs/getting-started/README.md) for local bring-up guidance
- use [`docs/operator/README.md`](docs/operator/README.md) for operator-facing runtime/deployment/navigation questions
- use [`docs/contributor/README.md`](docs/contributor/README.md) for contributor workflow and implementation-entry guidance
- use [`docs/reference/README.md`](docs/reference/README.md) for architecture/API/CLI/subsystem reference navigation
- use [`docs/parity/README.md`](docs/parity/README.md) for parity contracts, coverage, and sequencing navigation
- use [`docs/strategy/README.md`](docs/strategy/README.md) for product rationale and positioning
- use [`docs/adr/README.md`](docs/adr/README.md) for durable architecture decisions
- use [`rune-plan.md`](rune-plan.md) for the canonical product strategy and planning summary
- use [`docs/OPENCLAW-COVERAGE-MAP.md`](docs/OPENCLAW-COVERAGE-MAP.md) if you need the OpenClaw-surface parity front door

## For contributors

Start here:
- use [`docs/contributor/DEVELOPMENT.md`](docs/contributor/DEVELOPMENT.md) for local setup and day-to-day build/run flow
- use [`docs/contributor/README.md`](docs/contributor/README.md) for the contributor docs hub and related execution references
- use [`docs/AGENT-ORCHESTRATION.md`](docs/AGENT-ORCHESTRATION.md) for deeper runtime/repo execution context
- use [`docs/reference/README.md`](docs/reference/README.md), [`docs/reference/ARCHITECTURE.md`](docs/reference/ARCHITECTURE.md), [`docs/reference/CRATE-LAYOUT.md`](docs/reference/CRATE-LAYOUT.md), and [`docs/reference/SUBSYSTEMS.md`](docs/reference/SUBSYSTEMS.md) for technical reference material

## License

Private — [Horizon Tech d.o.o.](https://horizontech.ba)
