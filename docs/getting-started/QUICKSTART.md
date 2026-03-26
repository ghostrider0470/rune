# Quick Start

This is the fastest path to a local Rune browser chat.

## Option A — one-command install

```bash
curl -fsSL https://raw.githubusercontent.com/ghostrider0470/rune/main/scripts/install.sh | sh
```

Then run:

```bash
rune setup --path ~/.rune --api-key "$OPENAI_API_KEY"
```

If Ollama is already running locally, this also works without an API key:

```bash
rune setup --path ~/.rune
```

That flow uses the safe `setup` alias for the first-run wizard: it writes a local SQLite-backed config, enables the embedded UI + WebChat, starts the gateway, and opens browser chat at `http://127.0.0.1:8787/webchat`.

## Option B — source checkout

```bash
cargo build --release --bin rune --bin rune-gateway
cargo run --release --bin rune -- setup --path ~/.rune --api-key "$OPENAI_API_KEY"
```

## Manual fallback

If you want to wire config yourself:

```bash
cp config.example.toml config.toml
./target/release/rune-gateway --config config.toml
```

Open:

```text
http://127.0.0.1:8787/webchat
```

Dashboard remains available at:

```text
http://127.0.0.1:8787/dashboard
```

If `gateway.auth_token` is configured, use the same bearer token expected by the protected gateway routes.

## What this gets you

This path is meant for a quick local bring-up of:
- the gateway process
- browser chat over the embedded WebSocket-backed WebChat
- dashboard/status visibility
- local config-driven runtime behavior
- a standalone-first operator workflow

## Next docs

- use [`INSTALL.md`](INSTALL.md) if the quick path is not enough and you want the fuller local setup path
- use [`../operator/README.md`](../operator/README.md) if you are now thinking like an operator rather than just trying the runtime once
- use [`../contributor/DEVELOPMENT.md`](../contributor/DEVELOPMENT.md) if you are moving from trying Rune into building or changing it
- use [`../INDEX.md`](../INDEX.md) if you need the wider docs front door
