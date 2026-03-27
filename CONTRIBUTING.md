# Contributing to Rune

## Workflow

1. **Every change goes through a PR** — no direct commits to `main`
2. **Every PR references a GitHub issue** — no orphan work
3. **Branch naming**: `agent/<role>/<scope>` or `feature/<scope>`
4. **Squash-only merges** — clean linear history
5. **One active PR at a time** preferred — merge before starting next

## Issue Hierarchy

- **Epic** (`type:epic`) — Top-level outcome area
- **Feature** (`type:feature`) — Capability group under an epic
- **Story** (`type:story`) — Single executable deliverable

## Labels

### Type
- `type:epic` / `type:feature` / `type:story`
- `bug` / `enhancement` / `documentation`

### Priority
- `priority:p0` — Drop everything
- `priority:p1` — Important, do next
- `priority:p2` — Useful but not urgent

### Area
- `area:runtime` / `area:memory` / `area:channels` / `area:tools`
- `area:automation` / `area:media` / `area:mobile`

## Definition of Done

A task is done when:
- Code compiles (`cargo build --release`)
- Tests pass (`cargo test`)
- PR is merged to `main`
- Related issue is updated/closed
- No secrets committed

## Development Setup

```bash
# Clone
git clone https://github.com/ghostrider0470/rune.git
cd rune

# Build
cargo build --release -p rune-gateway-app

# Run
./target/release/rune-gateway --config config.toml

# Test
cargo test
```

## Architecture

Rune is a multi-crate Rust workspace:
- `rune-gateway` / `rune-gateway-app` — HTTP gateway and entry point
- `rune-runtime` — Session management, agent loop, tool dispatch
- `rune-store` — Storage backends (SQLite, LanceDB, pgvector, Cosmos DB)
- `rune-models` — LLM provider abstraction (OpenAI, Anthropic, Azure)
- `rune-channels` — Channel adapters (Telegram, Discord, WebChat)
- `rune-tools` — Built-in tool implementations
- `rune-cli` — CLI interface and diagnostics
