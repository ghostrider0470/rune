# Quick Start

This is the fastest path to a local Rune gateway run.

## 1. Build the gateway

```bash
cargo build --release --bin rune-gateway
```

## 2. Create local config

```bash
cp config.example.toml config.toml
```

Fill in the provider, auth, and channel values you actually need for your local run.

## 3. Start Rune

```bash
./target/release/rune-gateway --config config.toml
```

## 4. Open the dashboard

Go to:

```text
http://127.0.0.1:8787/dashboard
```

If `gateway.auth_token` is configured, use the same bearer token expected by the protected gateway routes.

## What this gets you

This path is meant for a quick local bring-up of:
- the gateway process
- dashboard/status visibility
- local config-driven runtime behavior
- a standalone-first operator workflow

## Next docs

- use [`INSTALL.md`](INSTALL.md) if the quick path is not enough and you want the fuller local setup path
- use [`../operator/README.md`](../operator/README.md) if you are now thinking like an operator rather than just trying the runtime once
- use [`../contributor/DEVELOPMENT.md`](../contributor/DEVELOPMENT.md) if you are moving from trying Rune into building or changing it
- use [`../INDEX.md`](../INDEX.md) if you need the wider docs front door
