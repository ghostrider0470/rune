# Rune

A Rust rewrite of [OpenClaw](https://github.com/openclaw/openclaw) — personal AI gateway with full Azure compatibility, Docker-first deployment, and PostgreSQL persistence.

## Quick Start

### Prerequisites

- Rust 1.80+ (`rustup install stable`)
- `build-essential`, `pkg-config`, `libssl-dev` (or use `rustls-tls` — default)

### Build

```bash
cargo build --release
```

Binaries land in `target/release/`:
- `rune` — CLI
- `rune-gateway` — gateway server

### Configure

```bash
cp config.example.toml config.toml
# Edit config.toml with your API keys and Telegram bot token
```

Key config sections:
- `[models]` — model provider (Azure AI Foundry recommended for Azure)
- `[channels]` — Telegram bot token
- `[gateway]` — host/port
- `[paths]` — workspace, data directories

### Run (Development)

**Foreground (see logs directly):**
```bash
cargo run --release --bin rune-gateway -- --config config.toml
# Ctrl+C to stop
```

**Background via systemd (recommended — survives shell exits):**
```bash
# Install service (one-time)
mkdir -p ~/.config/systemd/user
cat > ~/.config/systemd/user/rune-gateway.service << 'EOF'
[Unit]
Description=Rune Gateway

[Service]
Type=simple
WorkingDirectory=/home/YOU/Development/rune
ExecStart=/home/YOU/Development/rune/target/release/rune-gateway --config config.toml
Restart=on-failure
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=default.target
EOF

systemctl --user daemon-reload

# Start / stop / restart
systemctl --user start rune-gateway
systemctl --user stop rune-gateway
systemctl --user restart rune-gateway

# Check status
systemctl --user status rune-gateway

# Tail logs
journalctl --user -u rune-gateway -f

# Enable on boot
systemctl --user enable rune-gateway
```

**Quick kill (any method):**
```bash
# If running in foreground: Ctrl+C
# If systemd:
systemctl --user stop rune-gateway
# Nuclear option (kills gateway + embedded postgres):
pkill -f rune-gateway; pkill postgres
```

### Run CLI

```bash
# Gateway management
rune gateway status
rune gateway health

# Sessions
rune sessions list
rune sessions show <id>

# Cron jobs
rune cron list
rune cron add --text "reminder" --at "2026-01-01T09:00:00"

# Config
rune config show
rune config validate

# Diagnostics
rune doctor
rune status
rune health
```

### Database

Rune uses **embedded PostgreSQL** by default — zero config needed. It downloads and manages its own PG instance in `.data/db/`.

To use an external PostgreSQL instead:
```toml
[database]
url = "postgres://user:pass@localhost/rune"
```

## Architecture

```
┌─────────────┐    ┌──────────────┐    ┌─────────────┐
│  Telegram    │───▶│   Gateway    │───▶│   Model     │
│  (channels)  │◀───│  (session    │◀───│  Provider   │
│              │    │   loop)      │    │  (Azure AI) │
└─────────────┘    └──────┬───────┘    └─────────────┘
                          │
                   ┌──────┴───────┐
                   │  PostgreSQL  │
                   │  (embedded   │
                   │   or remote) │
                   └──────────────┘
```

### Crate Layout

| Crate | Purpose |
|-------|---------|
| `rune-config` | Configuration loading/validation |
| `rune-store` | PostgreSQL persistence (Diesel) + embedded PG |
| `rune-models` | Model providers (Azure AI Foundry, OpenAI, Anthropic) |
| `rune-tools` | Tool registry + built-in tool executors |
| `rune-runtime` | Session engine, turn executor, scheduler |
| `rune-channels` | Channel adapters (Telegram, more planned) |
| `rune-gateway` | Axum HTTP server, routes, middleware |
| `rune-cli` | CLI commands |
| `rune-testkit` | Test utilities |

### Model Providers

**Azure AI Foundry** (recommended) — single endpoint for all Azure-hosted models:
```toml
[[models.providers]]
name = "azure-foundry"
kind = "azure-foundry"
base_url = "https://your-resource.services.ai.azure.com"
api_key = "your-key"
```

Routes automatically: `claude-*` → Anthropic API, everything else → OpenAI API.

Also supports: `openai`, `anthropic`, `azure-openai` provider kinds.

## Development

```bash
# Run tests
cargo test --workspace

# Clippy (must pass with zero warnings)
cargo clippy --workspace -- -D warnings

# Check compilation
cargo check --workspace
```

## Releases

Tag-driven via GitHub Actions. Push a `v*` tag to build cross-compiled binaries:
```bash
git tag v0.4.0
git push origin v0.4.0
```

## Docs

- `docs/PLAN.md` — scope and architecture
- `docs/PARITY-INVENTORY.md` — OpenClaw feature parity map
- `docs/AZURE-COMPATIBILITY.md` — Azure integration details
- `docs/DOCKER-DEPLOYMENT.md` — Docker deployment model
- `docs/PROTOCOLS.md` — protocol and API contracts

## License

Private — Horizon Tech d.o.o.
