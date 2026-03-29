# Quick Start

This is the fastest path to a local Rune browser chat.

## Option A — one-command install

```bash
curl -fsSL "$(rune update install-script 2>/dev/null || echo https://raw.githubusercontent.com/ghostrider0470/rune/main/scripts/install.sh)" | sh
```

Then run the built-in first-run wizard:

```bash
rune setup --api-key "$OPENAI_API_KEY"
# or: rune onboard --api-key "$OPENAI_API_KEY"
```

Or install + configure + background the gateway in one flow:

```bash
rune setup --api-key "$OPENAI_API_KEY" --install-service --service-target systemd
```

On macOS, swap the service target:

```bash
rune setup --api-key "$OPENAI_API_KEY" --install-service --service-target launchd
```

If Ollama is already running locally, this also works without an API key:

```bash
rune setup
```

That flow uses the safe `setup` alias for the first-run wizard: it writes a local SQLite-backed config, enables the embedded UI + WebChat, starts the gateway or installs it as a background service, and opens browser chat at `http://127.0.0.1:8787/webchat`.

WebChat now resumes browser-scoped sessions automatically. Share or bookmark `http://127.0.0.1:8787/webchat?session_id=...` to reopen a specific session, or pass `session_token=team-browser` so multiple browsers keep isolated session lists under distinct `webchat:<token>` channels. If gateway auth is enabled, append `api_key=...` (or `auth=...` to drive the WebSocket bearer subprotocol from a browser link) to test the protected flow from a browser without extra tooling.

## Option B — source checkout

```bash
cargo build --release --bin rune --bin rune-gateway
cargo run --release --bin rune -- setup --api-key "$OPENAI_API_KEY"
```


## Option C — zero-config Docker

```bash
git clone --depth 1 --branch main https://github.com/ghostrider0470/rune ~/Development/rune
cd ~/Development/rune
docker compose up --build -d
```

This uses the bundled zero-config Compose profile with durable Docker volumes mapped to Rune's `/data`, `/config`, and `/secrets` paths, so container restarts keep the same local SQLite database, sessions, memory, logs, generated config, and secret material. Fill `config.example.env` into `.env` first if you want OpenAI/Anthropic credentials injected on first boot.

## Manual fallback

If you want to wire config yourself:

```bash
cp config.example.toml config.toml
# optional: set [instance] name / advertised_addr / peers for multi-instance discovery
./target/release/rune-gateway --config config.toml
```

If you are wiring multiple Rune instances together, set `[instance]` in `config.toml` and check `http://127.0.0.1:8787/api/v1/instance/health` after startup to confirm identity, capability manifest, and peer reachability. Then run `rune gateway instance-health` locally for the same summary over the CLI, and `rune gateway delegation-plan --strategy least_busy` (or `--strategy named --peer-id <peer>`) to inspect the sender/receiver contract before turning on cross-instance delegation.

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
