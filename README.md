# Rune

A high-performance personal AI gateway written in Rust. Drop-in replacement for [OpenClaw](https://github.com/openclaw/openclaw) with full Azure compatibility, Docker-first deployment, and PostgreSQL persistence.

## Why Rune?

- **Fast** — native Rust binary, no Node.js runtime overhead
- **Azure-native** — Azure AI Foundry provider handles all Azure-hosted models (OpenAI + Anthropic) through a single endpoint
- **Zero-config local dev** — embedded PostgreSQL starts automatically, no external DB needed
- **Docker-first** — mountable persistent storage at `/data/*`, `/config/*`, `/secrets/*`
- **OpenClaw compatible** — same user/operator experience, same channel integrations, same tool surface

## What It Does

Rune sits between your messaging channels (Telegram, Signal, Discord) and AI model providers (Azure OpenAI, Anthropic, OpenAI). It manages sessions, executes tools, persists conversations, runs scheduled jobs, and handles multi-agent orchestration.

```
Channels ──▶ Gateway ──▶ Session Engine ──▶ Model Provider
(Telegram)   (Axum)      (turns, tools,     (Azure AI Foundry,
                          memory, cron)       OpenAI, Anthropic)
                  │
            PostgreSQL
            (embedded or external)
```

## Architecture

| Crate | Purpose |
|-------|---------|
| `rune-config` | Configuration loading and validation |
| `rune-store` | PostgreSQL persistence via Diesel + embedded PG fallback |
| `rune-models` | Model providers — Azure AI Foundry, OpenAI, Anthropic |
| `rune-tools` | Tool registry + 15 built-in tool executors |
| `rune-runtime` | Session engine, turn executor, scheduler, memory loader |
| `rune-channels` | Channel adapters (Telegram live, Signal/Discord planned) |
| `rune-gateway` | Axum HTTP server, routes, auth, middleware |
| `rune-cli` | CLI interface |
| `rune-testkit` | Test utilities and fixtures |

**10 library crates, 2 binaries, ~75 Rust source files, and an actively growing parity test surface.**

## Model Providers

### Azure AI Foundry (recommended)

Single endpoint for all Azure-hosted models — routes automatically by model name:

```toml
[[models.providers]]
name = "azure-foundry"
kind = "azure-foundry"
base_url = "https://your-resource.services.ai.azure.com"
api_key = "your-key"

[models]
default_model = "gpt-5.4"  # or claude-sonnet-4-6, claude-opus-4-6, etc.
```

- `gpt-*`, `o1-*`, etc. → OpenAI Chat Completions API
- `claude-*` → Anthropic Messages API

Also supports `openai`, `anthropic`, and `azure-openai` provider kinds for non-Foundry setups.

## CLI

```bash
rune status                    # Gateway + system status
rune doctor                    # Diagnostic checks
rune gateway start|stop|restart|status|health

rune sessions list             # Active sessions
rune sessions show <id>        # Session details

rune cron list                 # Scheduled jobs
rune cron add --text "..." --at "2026-01-01T09:00:00"

rune config show               # Current config
rune config validate           # Validate config file
```

## HTTP API

Gateway exposes a REST API on the configured port:

| Endpoint | Description |
|----------|-------------|
| `GET /health` | Health check |
| `GET /status` | Gateway status |
| `GET /sessions` | List sessions |
| `POST /sessions` | Create session |
| `POST /sessions/{id}/messages` | Send message |
| `GET /sessions/{id}/transcript` | Get transcript |
| `POST /gateway/start\|stop\|restart` | Gateway lifecycle |

---

## Development

### Prerequisites

- Rust 1.80+ (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
- `build-essential`, `pkg-config` (Linux)

### Build

```bash
cargo build --release
# Binaries: target/release/rune, target/release/rune-gateway
```

### Configure

```bash
cp config.example.toml config.toml
# Fill in: API key, Telegram bot token, model name
```

### Run

```bash
# Foreground (Ctrl+C to stop)
cargo run --release --bin rune-gateway -- --config config.toml

# Or run the built binary directly
./target/release/rune-gateway --config config.toml
```

### Run as systemd service (recommended for dev)

```bash
# Start
systemctl --user start rune-gateway

# Stop
systemctl --user stop rune-gateway

# Restart (after rebuilding)
cargo build --release --bin rune-gateway && systemctl --user restart rune-gateway

# Logs (live tail)
journalctl --user -u rune-gateway -f

# Status
systemctl --user status rune-gateway
```

<details>
<summary>Install the systemd service (one-time)</summary>

```bash
mkdir -p ~/.config/systemd/user
cat > ~/.config/systemd/user/rune-gateway.service << 'EOF'
[Unit]
Description=Rune Gateway

[Service]
Type=simple
WorkingDirectory=%h/Development/rune
ExecStart=%h/Development/rune/target/release/rune-gateway --config config.toml
Restart=on-failure
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=default.target
EOF

systemctl --user daemon-reload
```
</details>

### Kill

```bash
# Graceful
systemctl --user stop rune-gateway

# Force kill (gateway + embedded postgres)
pkill -f rune-gateway && pkill postgres
```

### Test

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

### Release

Tag-driven via GitHub Actions — push a `v*` tag to build cross-compiled binaries:

```bash
git tag v0.5.0 && git push origin v0.5.0
```

---

## Docker

```bash
docker compose up -d
```

Persistent mounts: `./data` → `/data`, `./config` → `/config`

## Docs

| Doc | What |
|-----|------|
| [`PLAN.md`](docs/PLAN.md) | Scope, architecture, subsystem breakdown |
| [`PARITY-INVENTORY.md`](docs/PARITY-INVENTORY.md) | OpenClaw feature parity map |
| [`AZURE-COMPATIBILITY.md`](docs/AZURE-COMPATIBILITY.md) | Azure integration contract |
| [`DOCKER-DEPLOYMENT.md`](docs/DOCKER-DEPLOYMENT.md) | Docker deployment model |
| [`PROTOCOLS.md`](docs/PROTOCOLS.md) | API and protocol contracts |

## License

Private — [Horizon Tech d.o.o.](https://horizontech.ba)
