# Install / Local Setup

This document is the lightweight install/setup path for getting Rune running on one machine.

## Prerequisites

- Rust 1.80+
- common Linux build tooling such as `build-essential` and `pkg-config`

Rust install example:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

## Build

```bash
cargo build --release
```

## Fast local first run

If you just want Rune working with an API key and browser chat, use the built-in first-run wizard:

```bash
cargo run --release --bin rune -- setup --api-key "$OPENAI_API_KEY"
```

Or choose a different provider/model explicitly:

```bash
cargo run --release --bin rune -- setup \
  --provider anthropic \
  --model claude-3-7-sonnet-latest \
  --api-key "$ANTHROPIC_API_KEY"
```

What this does:

- initializes the workspace if needed
- writes `config.toml` with local SQLite storage and UI/WebChat enabled
- enables browser WebChat
- starts the gateway locally
- opens `http://127.0.0.1:8787/webchat`

For non-interactive CI/script usage, add `--non-interactive`. `rune setup` defaults to `~/.rune`, WebChat enabled, gateway startup enabled, and browser open enabled, so the happy path lands in chat immediately. If you skip automatic startup, the wizard prints the exact `rune-gateway --config ...`, `rune service install --target systemd --name rune-gateway --workdir ... --config ... --enable --start`, `rune service install --target launchd --name rune-gateway --workdir ... --config ... --enable --start`, plus `rune --gateway-url http://127.0.0.1:8787 health` and `rune --gateway-url http://127.0.0.1:8787 doctor run` verification commands to finish setup. For a one-shot install + configure + background-service flow, use `rune setup --api-key "$OPENAI_API_KEY" --install-service --service-target systemd` on Linux or swap `--service-target launchd` on macOS.

## Create config

```bash
cp config.example.toml config.toml
```

Then set the provider/channel/auth values needed for your environment. If you are testing federation, also fill `[instance]` with a human-readable `name`, optional `advertised_addr`, and `peers = [{ id, health_url }]`.
For storage, shipped backends today are SQLite, PostgreSQL (including Azure Database for PostgreSQL), Azure Cosmos DB for NoSQL, and Azure SQL Database. Azure SQL currently rides Rune's SQL-family backend path and uses the `azure_sql_*` config fields documented in `docs/operator/DATABASES.md`.

## Run

```bash
./target/release/rune-gateway --config config.toml
```

Or during active development:

```bash
cargo run --release --bin rune-gateway -- --config config.toml
```

## Install as a user service

Build the CLI binary too:

```bash
cargo build --release --bin rune-gateway --bin rune
```

Generate a service definition with the built-in CLI:

```bash
./target/release/rune service install \
  --target systemd \
  --name rune-gateway \
  --workdir "$PWD" \
  --config "$PWD/config.toml" \
  --enable \
  --start
```

The command now prints the exact follow-up activation/status commands as part of its output. Add `--no-bootstrap` if you only want the generated unit/plist written to disk without immediately running `systemctl` or `launchctl`.

Check status on Linux:

```bash
systemctl --user status rune-gateway.service
journalctl --user -u rune-gateway -f
```

On macOS, use `--target launchd --enable` instead. The command writes `~/Library/LaunchAgents/rune-gateway.plist`, adds stdout/stderr log files next to it, prints the exact `launchctl bootstrap|enable|kickstart|print` commands, bootstraps it with `launchctl bootstrap`, enables it, and kickstarts it when `--start` is set.


## Zero-config Docker Compose

For a fast local evaluation with persisted state and no manual config, use the default bundled Compose file from a repo checkout:

```bash
git clone --depth 1 --branch main https://github.com/ghostrider0470/rune ~/Development/rune
cd ~/Development/rune
docker compose up --build -d
```

This starts Rune on `http://127.0.0.1:8787/webchat` (with `/dashboard` also available; `/chat` redirects into WebChat) and persists state/config/secrets in the named `rune-data`, `rune-config`, and `rune-secrets` volumes. The mounted paths line up with Rune's Docker-first layout: `/data/*` holds SQLite/session/media/log state, `/config` holds generated config, and `/secrets` holds provider credentials or future secret-store material. The default `docker-compose.yml` now ships the zero-config profile directly: it explicitly enables the browser UI + WebChat, reads optional provider credentials from a local `.env` file, and still allows shell-exported `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, and `OLLAMA_HOST` overrides when provided.

Useful follow-ups:

```bash
docker compose logs -f
docker compose down
```

To point the container at a real provider instead of local Ollama auto-detect, either export env vars inline or copy `config.example.env` to `.env` and fill it in. The `.env` file only seeds container environment variables; durable runtime state still lives in the mounted Docker volumes so restarts and image rebuilds keep the same SQLite DB, sessions, memory, logs, config, and secrets:

```bash
cp config.example.env .env
$EDITOR .env
docker compose up --build -d
```

Inline env vars still work too:

```bash
OPENAI_API_KEY=... docker compose up --build -d
```

## Verify startup

After Rune starts, check:
- dashboard at `http://127.0.0.1:8787/dashboard`
- browser chat at `http://127.0.0.1:8787/webchat`
- status/health surfaces through the gateway
- local logs/console output for startup failures

## Shell completions

Enable tab-completion for all Rune commands and flags:

```bash
# Example for bash — see full shell matrix in the CLI reference:
rune completion generate bash > ~/.local/share/bash-completion/completions/rune
```

For zsh, fish, elvish, and PowerShell installation instructions, see [`../reference/CLI.md` § completion command family](../reference/CLI.md#completion-command-family).

## If you want more than local bring-up

Use these next:
- use [`../operator/DEPLOYMENT.md`](../operator/DEPLOYMENT.md) and [`../operator/DATABASES.md`](../operator/DATABASES.md) when the next question is deployment/storage shape
- use [`../contributor/DEVELOPMENT.md`](../contributor/DEVELOPMENT.md) when the next step is active development rather than local trial use
- use [`../INDEX.md`](../INDEX.md) if you need the wider docs front door


If Ollama is already running locally, `rune setup` now auto-detects it and defaults to `--provider ollama` without requiring an API key.
