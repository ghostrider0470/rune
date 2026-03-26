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
cargo run --release --bin rune -- setup --path ~/.rune --api-key "$OPENAI_API_KEY"
```

Or choose a different provider/model explicitly:

```bash
cargo run --release --bin rune -- setup \
  --path ~/.rune \
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

For non-interactive CI/script usage, add `--non-interactive`. `rune setup` defaults to WebChat enabled, gateway startup enabled, and browser open enabled, so the happy path lands in chat immediately. If you skip automatic startup, the wizard prints the exact `rune-gateway --config ...`, `rune service install --target systemd --name rune-gateway --workdir ... --config ... --enable --start`, `rune service install --target launchd --name rune-gateway --workdir ... --config ... --enable --start`, plus `rune --gateway-url http://127.0.0.1:8787 health` and `rune --gateway-url http://127.0.0.1:8787 doctor run` verification commands to finish setup.

## Create config

```bash
cp config.example.toml config.toml
```

Then set the provider/channel/auth values needed for your environment.

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

The command now prints the exact follow-up activation/status commands as part of its output.

Check status on Linux:

```bash
systemctl --user status rune-gateway.service
journalctl --user -u rune-gateway -f
```

On macOS, use `--target launchd --enable` instead. The command writes `~/Library/LaunchAgents/rune-gateway.plist`, adds stdout/stderr log files next to it, prints the exact `launchctl bootstrap|enable|kickstart|print` commands, bootstraps it with `launchctl bootstrap`, enables it, and kickstarts it when `--start` is set.


## Zero-config Docker Compose

For a fast local evaluation with persisted state and no manual config, use the bundled Compose file:

```bash
docker compose -f docker-compose.zero-config.yml up --build -d
```

This starts Rune on `http://127.0.0.1:8787/chat` (with `/dashboard` also available) and persists state in the `rune-data` volume. The compose file explicitly enables the browser UI + WebChat and passes through `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, and `OLLAMA_HOST` when provided.

Useful follow-ups:

```bash
docker compose -f docker-compose.zero-config.yml logs -f
docker compose -f docker-compose.zero-config.yml down
```

To point the container at a real provider instead of local Ollama auto-detect, pass env vars through Compose, for example:

```bash
OPENAI_API_KEY=... docker compose -f docker-compose.zero-config.yml up --build -d
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
