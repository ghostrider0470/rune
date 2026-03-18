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

## Verify startup

After Rune starts, check:
- dashboard at `http://127.0.0.1:8787/dashboard`
- status/health surfaces through the gateway
- local logs/console output for startup failures

## If you want more than local bring-up

Use these next:
- use [`../operator/DEPLOYMENT.md`](../operator/DEPLOYMENT.md) and [`../operator/DATABASES.md`](../operator/DATABASES.md) when the next question is deployment/storage shape
- use [`../contributor/DEVELOPMENT.md`](../contributor/DEVELOPMENT.md) when the next step is active development rather than local trial use
- use [`../INDEX.md`](../INDEX.md) if you need the wider docs front door
