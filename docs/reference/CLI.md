# CLI Reference Entry

This document is the stable reference entry for Rune's CLI surface.

## Current scope

Rune's CLI is part of the parity-seeking control-plane surface.

Use these docs for the current contract picture:
- [`../parity/PARITY-INVENTORY.md`](../parity/PARITY-INVENTORY.md) — command-family coverage map
- [`../OPENCLAW-COVERAGE-MAP.md`](../OPENCLAW-COVERAGE-MAP.md) — parity-navigation front door
- [`../parity/PROTOCOLS.md`](../parity/PROTOCOLS.md) — resource/state concepts behind CLI operations

## Current reference use

Use this doc as the CLI entrypoint for:
- understanding where current command-family coverage lives
- navigating from CLI questions into parity inventory and protocol references

---

## `message` command family

Operator surface for channel-targeted outbound messaging and message-level actions.

### Shipped subcommands

| Subcommand | Purpose | Status |
|---|---|---|
| `rune message send --channel <adapter> --text <body> [--session <id>] [--thread <id>]` | Send a message through a configured channel adapter | Shipped |
| `rune message read --message-id <id> --channel <adapter> [--session <id>]` | Read/fetch a specific message by provider id | Shipped |
| `rune message edit --message-id <id> --channel <adapter> --text <body> [--session <id>]` | Edit a sent message where supported | Shipped |
| `rune message delete --message-id <id> --channel <adapter> [--session <id>]` | Delete a message where supported | Shipped |
| `rune message react --message-id <id> --emoji <emoji> [--remove] [--channel <adapter>] [--session <id>]` | Add/remove a reaction | Shipped |
| `rune message pin --message-id <id> [--unpin] [--channel <adapter>] [--session <id>]` | Pin or unpin a message | Shipped |
| `rune message search <query> [--channel <adapter>] [--session <id>] [--limit <n>]` | Search message history | Shipped |
| `rune message broadcast --text <body> [--channels a,b,c] [--session <id>]` | Broadcast to multiple channel adapters | Shipped |
| `rune message thread list --thread-id <id> [...]` / `reply --thread-id <id> ...` | List/reply in a thread | Shipped |
| `rune message voice send --text <body> --channel <adapter> [--voice <voice>] [--model <model>] [--output <file>]` | TTS-assisted voice message generation/send flow | Shipped |
| `rune message voice status` | Inspect TTS/provider readiness for voice messaging | Shipped |

### Deferred / not-yet-shipped breadth

The broader `message` family still has explicit gaps tracked in `docs/parity/CLI-MATRIX.md`, including `reactions`, `poll`, `channel`, `event`, `member/role`, `emoji`, `permissions`, `sticker`, and moderation actions.

---

## `models` command family

Operator surface for provider inventory, model selection, fallback chains, and provider scanning.

### Shipped subcommands

| Subcommand | Purpose | Status |
|---|---|---|
| `rune models list` | List configured providers with kind, base URL, default model, credential source, and readiness | Shipped |
| `rune models status` | Show resolved default text and image models plus per-provider credential readiness | Shipped |
| `rune models set <model>` | Set default text model in local `config.toml` (validates against provider inventory) | Shipped |
| `rune models set-image <model>` | Set default image model in local `config.toml` | Shipped |
| `rune models aliases` | Show configured alias-to-provider/model mappings with credential status | Shipped |
| `rune models fallbacks` | List configured text fallback chains | Shipped |
| `rune models image-fallbacks` | List configured image fallback chains | Shipped |
| `rune models scan` | Probe locally reachable providers for available models (Ollama only currently) | Shipped |

### Not yet shipped

| Subcommand | Purpose | Status |
|---|---|---|
| `rune models auth` | Inspect provider auth/API-key configuration status and management hints | Implemented as inspectable auth-status surface; mutation still uses `rune config set` |

### Output modes

All `models` subcommands support `--json` for machine-readable output alongside human-readable table output.

### Scan limitations

`models scan` currently probes Ollama providers only, using the native `/api/tags` endpoint. Non-Ollama providers are skipped. This is an intentional conservative scope — broader provider probing will follow when safe probe semantics are defined for cloud providers.

### Fallback chain behavior

`models fallbacks` and `models image-fallbacks` display chains configured in `config.toml` under `[models]`. At runtime, `RoutedModelProvider` walks the fallback chain on retriable errors only (rate-limit, transient 5xx, quota exhaustion, HTTP transport failure). Non-retriable errors (auth failure, model not found, invalid request) do not trigger fallback.

---

## `cron` command family

Operator surface for scheduled job management, execution, and inspection.

### Shipped subcommands

| Subcommand | Purpose | Status |
|---|---|---|
| `rune cron status` | Show scheduler status (total, enabled, due counts) | Shipped |
| `rune cron list [--include-disabled]` | List cron jobs | Shipped |
| `rune cron add --text "<text>" --at "<iso-8601>" [--session-target main\|isolated] [--delivery-mode none\|announce\|webhook] [--webhook-url <url>]` | Create one-shot system_event job | Shipped |
| `rune cron show <id>` | Show job details | Shipped |
| `rune cron edit <id> [--name <name>] [--delivery-mode <mode>] [--webhook-url <url>]` | Edit job name or delivery mode | Shipped |
| `rune cron enable <id>` | Enable job | Shipped |
| `rune cron disable <id>` | Disable job | Shipped |
| `rune cron rm <id>` | Remove job | Shipped |
| `rune cron run <id>` | Trigger job immediately (manual run) | Shipped |
| `rune cron runs <id>` | Show run history for job | Shipped |
| `rune cron wake --text "<text>" [--mode next-heartbeat\|now] [--context-messages <n>]` | Queue wake event | Shipped |

### CLI surface gaps

- `cron add` creates one-shot `system_event` jobs only; the gateway API accepts the full schedule and payload schema including `every`, `cron`, and `agent_turn`
- `cron edit` mutates name and delivery mode only; schedule and payload edits require the gateway API

### Delivery mode behavior

- `none` — silent execution, no outbound delivery
- `announce` — broadcasts `cron_run_completed` event via the session event channel
- `webhook` — POSTs result payload to configured webhook URL (30 s timeout)

### Claim/lease semantics

Due jobs are claimed atomically before execution. Stale claims older than 300 s expire for crash recovery. Concurrent supervisor ticks cannot duplicate execution.

---

## `reminders` command family

Operator surface for one-shot reminder management.

### Shipped subcommands

| Subcommand | Purpose | Status |
|---|---|---|
| `rune reminders add <message> --in <duration> [--target <target>]` | Create reminder (duration: "30m", "2h", "1d", etc.) | Shipped |
| `rune reminders list [--include-delivered]` | List reminders | Shipped |
| `rune reminders cancel <id>` | Cancel reminder | Shipped |

### Target routing

- `"main"` (default) — executes in the stable `system:scheduled-main` session
- `"isolated"` — creates a one-shot subagent session under the main scheduled session
- unknown values — fall back to `"main"` with a warning

### Reminder outcomes

Reminders resolve to one of four terminal states: `pending`, `delivered`, `cancelled`, or `missed`. Failed delivery attempts produce `missed` with inspectable error context. Cancellation produces an explicit `cancelled` outcome rather than silent deletion.

---

## Read next

- use [`../parity/PARITY-INVENTORY.md`](../parity/PARITY-INVENTORY.md) when you need the full command/surface census
- use [`../OPENCLAW-COVERAGE-MAP.md`](../OPENCLAW-COVERAGE-MAP.md) when you need broader docs navigation by OpenClaw surface
- use [`../parity/PROTOCOLS.md`](../parity/PROTOCOLS.md) when you need the runtime/state model behind a CLI behavior
- use [`../operator/PROVIDERS.md`](../operator/PROVIDERS.md) for provider configuration and setup details

## Further detail still missing

Deeper follow-up documentation is still useful for:
- top-level command families beyond `models`
- operator mental model
- lifecycle/status/config/doctor command pointers
- links to deeper command-specific docs if those split later

Until a fuller CLI reference is split out, treat the parity inventory as the authoritative command census.
