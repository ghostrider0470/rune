<p align="center">
  <img src="assets/core_rune_midnight.svg" alt="Rune" width="120" />
</p>

# Rune

A Rust-based personal AI runtime designed as a high-performance OpenClaw-style gateway with strong Azure-oriented provider support, durable state, and standalone-first operation.

## Current status

Rune is an active parity-seeking runtime buildout, not a parity-complete replacement claim.

Today, the project already has:
- standalone and service-style runtime operation
- durable PostgreSQL-backed state with embedded local fallback
- first-class Cosmos DB for NoSQL backend support for Azure-native document deployments
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

### Install in one command

```bash
curl -fsSL "$(rune update install-script 2>/dev/null || echo https://raw.githubusercontent.com/ghostrider0470/rune/main/scripts/install.sh)" | sh
```

The installer builds `rune` and `rune-gateway`, places them in `~/.local/bin` by default, then you can run:

```bash
rune setup --api-key "$OPENAI_API_KEY"
# or: rune onboard --api-key "$OPENAI_API_KEY"
```

If Ollama is already running locally, `rune setup` auto-detects it and skips the API key requirement. `rune setup` is a safe alias for the first-run wizard: it writes a zero-config local SQLite state path, enables WebChat/UI, starts the gateway, and opens browser chat by default.

### Local dev / source checkout

```bash
cargo build --release --bin rune-gateway --bin rune
cargo run --release --bin rune -- setup --api-key "$OPENAI_API_KEY"
```

Or wire config manually:

```bash
cp config.example.toml config.toml
# fill in your provider + channel settings
# optional: set [instance] name/advertised_addr/peers for multi-instance discovery
./target/release/rune-gateway --config config.toml
```

For multi-instance federation bring-up, configure `[instance]` in `config.toml` and inspect `GET /api/v1/instance/health`. Example:

```toml
[instance]
name = "rune-desktop"
advertised_addr = "http://192.168.1.10:18790"
roles = ["gateway", "scheduler"]
peers = [
  { id = "rune-laptop", health_url = "http://192.168.1.20:18790/api/v1/instance/health" },
]
```

That endpoint returns instance identity, capability manifest, live load counters, and peer health snapshots so operators can verify discovery wiring before delegation lands.

You can inspect the same surface from the CLI with `rune gateway instance-health` and preview delegation targeting with `rune gateway delegation-plan --strategy least_busy`.

Then open `http://127.0.0.1:8787/webchat` for browser chat or `http://127.0.0.1:8787/dashboard` for the admin UI. The legacy `/chat` path redirects into WebChat so old bookmarks keep working.

WebChat keeps browser sessions isolated by channel token: use `?session_token=my-browser` to keep one browser's session list separate from another, and `?session_id=<uuid>` to reopen a specific thread directly. When gateway auth is enabled, the browser flow also accepts `?api_key=...` (and `?auth=...` for WebSocket bearer-header compatibility) so the embedded chat can be used without a custom proxy.

To generate and install a user service for unattended operation:

```bash
./target/release/rune service install \
  --target systemd \
  --name rune-gateway \
  --workdir "$PWD" \
  --config "$PWD/config.toml" \
  --enable \
  --start
```

On macOS, swap `--target launchd` to write a LaunchAgent plist instead. The install command also wires stdout/stderr logs next to the plist, bootstraps the agent with `launchctl bootstrap`, and kickstarts it when `--start` is passed. Add `--no-bootstrap` when you want to generate/install the unit file but handle `systemctl`/`launchctl` activation yourself.

If you skip `--start`, Rune prints the exact manual `rune-gateway --config ...` command, ready-to-run `rune service install --target systemd --name rune-gateway --workdir ... --config ... --enable --start` and `rune service install --target launchd --name rune-gateway --workdir ... --config ... --enable --start` follow-ups, plus `rune --gateway-url http://127.0.0.1:8787 health` and `rune --gateway-url http://127.0.0.1:8787 doctor run` verification commands so the setup path still ends in a durable background service without hunting through docs.

### Zero-config Docker bring-up

Rune also ships a zero-config Docker Compose path for fast evaluation with persisted local state:

```bash
git clone --depth 1 --branch main https://github.com/ghostrider0470/rune ~/Development/rune
cd ~/Development/rune
docker compose up --build -d
```

The default Compose file now ships the zero-config path: it mounts durable state under the named `rune-data`, `rune-config`, and `rune-secrets` volumes, maps them onto Rune's Docker-first `/data`, `/config`, and `/secrets` paths, explicitly enables the browser UI + WebChat, reads optional provider credentials from a local `.env` file (see `config.example.env`), and exposes chat on `http://127.0.0.1:8787/webchat` (`/chat` redirects into WebChat and `/dashboard` is also available).

Next docs from there:
- [`docs/getting-started/QUICKSTART.md`](docs/getting-started/QUICKSTART.md)
- [`docs/getting-started/INSTALL.md`](docs/getting-started/INSTALL.md)
- [`docs/operator/README.md`](docs/operator/README.md)
- [`docs/contributor/DEVELOPMENT.md`](docs/contributor/DEVELOPMENT.md)

## What Rune does

Rune sits between messaging channels and model providers. It manages:
- sessions and turn execution
- tool calls and approvals
- durable approval queue inspection and decisioning via `rune approvals ...`
- cron jobs, reminders, and automation
- memory and retrieval workflows
- provider routing and model invocation
- operator-facing control-plane visibility

## Core features (current-state view)

- **Gateway + dashboard** — health/status surfaces and an operator dashboard are live
- **Durable storage** — SQLite, PostgreSQL (external or embedded), and Azure Cosmos DB for NoSQL backends are shipped today; Azure SQL Database config is reserved but intentionally unsupported pending issue #782
- **Tool runtime** — built-in file, exec/process, cron, session, and memory-oriented tools are implemented
- **Provider layer** — Azure AI Foundry, Azure OpenAI, OpenAI, and Anthropic provider paths are part of the active runtime shape
- **Docs + execution discipline** — ADR trail, source-of-truth boundaries, and Project 2 execution model are now explicit

## Native spells shipped today

Rune also ships a growing set of Rust-native spells:
- `rust-patterns` — query production Rust patterns by topic, tags, imports, or validation scan
- `security-audit` — run a baseline host security audit for ports, sensitive permissions, SSH hardening, and firewall status
- `code-review` — perform structured multi-dimensional review on a file, diff, or PR target
- `evolver` — current local scaffold for the self-evolution workflow lane

The canonical inventory lives in [`docs/parity/PARITY-INVENTORY.md#77-native-spell-inventory`](docs/parity/PARITY-INVENTORY.md#77-native-spell-inventory).

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
- use `rune gateway instance-health` via the CLI after configuring `[instance]` to inspect identity/capabilities/load, and `rune gateway delegation-plan --strategy least_busy` to preview the current cross-instance delegation contract
- use [`docs/contributor/README.md`](docs/contributor/README.md) for the contributor docs hub and related execution references
- use [`docs/AGENT-ORCHESTRATION.md`](docs/AGENT-ORCHESTRATION.md) for deeper runtime/repo execution context
- use [`docs/reference/README.md`](docs/reference/README.md), [`docs/reference/ARCHITECTURE.md`](docs/reference/ARCHITECTURE.md), [`docs/reference/CRATE-LAYOUT.md`](docs/reference/CRATE-LAYOUT.md), and [`docs/reference/SUBSYSTEMS.md`](docs/reference/SUBSYSTEMS.md) for technical reference material

## License

Private — [Horizon Tech d.o.o.](https://horizontech.ba)
