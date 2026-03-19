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
| `rune models auth` | Manage provider auth/API keys | Not implemented — use `rune config set` for now |

### Output modes

All `models` subcommands support `--json` for machine-readable output alongside human-readable table output.

### Scan limitations

`models scan` currently probes Ollama providers only, using the native `/api/tags` endpoint. Non-Ollama providers are skipped. This is an intentional conservative scope — broader provider probing will follow when safe probe semantics are defined for cloud providers.

### Fallback chain behavior

`models fallbacks` and `models image-fallbacks` display chains configured in `config.toml` under `[models]`. At runtime, `RoutedModelProvider` walks the fallback chain on retriable errors only (rate-limit, transient 5xx, quota exhaustion, HTTP transport failure). Non-retriable errors (auth failure, model not found, invalid request) do not trigger fallback.

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
