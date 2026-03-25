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
cargo run --release --bin rune -- setup   --provider anthropic   --model claude-3-7-sonnet-latest   --api-key "$ANTHROPIC_API_KEY"
```

What this does:

- initializes the workspace if needed
- writes `config.toml`
- enables browser WebChat
- starts the gateway locally
- opens `http://127.0.0.1:8787/chat`

For non-interactive CI/script usage, add `--non-interactive`.

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
./target/release/rune service install   --target systemd   --name rune-gateway   --workdir "$PWD"   --config "$PWD/config.toml"   --enable   --start
```

Check status on Linux:

```bash
systemctl --user status rune-gateway
```

On macOS, use `--target launchd --enable` instead. The command writes `~/Library/LaunchAgents/rune-gateway.plist` and loads it with `launchctl load -w`.

## Verify startup

After Rune starts, check:
- dashboard at `http://127.0.0.1:8787/dashboard`
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
