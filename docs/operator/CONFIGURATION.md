# Configuration

This is the operator-facing entry doc for Rune configuration behavior.

## Scope

Rune configuration covers:
- gateway settings
- model/provider configuration
- channel configuration
- auth and security-related settings
- storage/runtime path choices

## Current canonical references

Use these docs for the current contract picture:
- [`OPERATOR-POLICY.md`](OPERATOR-POLICY.md) — operator-side runtime rules and autonomy guidance
- [`DEPLOYMENT.md`](DEPLOYMENT.md) — deployment/storage/runtime layout expectations
- [`DATABASES.md`](DATABASES.md) — durable storage model and database choices
- [`../parity/PROTOCOLS.md`](../parity/PROTOCOLS.md) — config-related runtime/state concepts
- [`../INDEX.md`](../INDEX.md) — docs front door

## Current operator use

Use this doc as the configuration entrypoint for:
- where config concerns live
- which deeper docs cover deployment/storage/runtime config semantics
- how config relates to provider, channel, and auth behavior


## Plugin discovery and decision reporting

Rune scans plugin directories in configured order. Discovery is deterministic:
- earlier scan directories have higher precedence
- first plugin name wins across directories
- later duplicates are skipped, not merged
- disabled plugins are skipped by config override before activation
- incompatible Claude plugin manifests are rejected before registration

Gateway plugin status surfaces runtime-visible discovery metadata per plugin:
- `kind` — `native` or `claude`
- `last_decision` — `loaded`, `skipped_duplicate`, `disabled_by_override`, `rejected_incompatible`, or `parse_failed`
- `last_detail` — human-readable reason for the decision

Use this when troubleshooting why a plugin did not activate after startup or reload.

## Hook execution behavior

Plugin hook execution is lifecycle-based and reports per-handler outcomes.

Operational contract:
- hooks run for explicit lifecycle phases only
- handlers execute in registration order
- hook failures are isolated to the handler boundary
- fail-open failures are reported as `warned` and do not stop sibling handlers
- fail-closed failures are reported as `blocked`, set `hook_blocked`/`hook_block_reason`, and stop further handlers for that event
- suppressed and filtered handlers are reported as `suppressed` or `skipped` rather than silently disappearing

Use hook execution records and block markers when diagnosing runtime-visible tool or turn behavior changes caused by plugins.

Detailed operator guidance lives in [`HOOKS.md`](HOOKS.md).

For the canonical architecture decision, see [`../adr/ADR-0005-hook-lifecycle-contract-and-isolated-execution-boundaries.md`](../adr/ADR-0005-hook-lifecycle-contract-and-isolated-execution-boundaries.md).

Operational notes:
- duplicate plugin names across scan roots are an override mechanism; only the highest-precedence copy loads
- Claude plugin manifests must declare a non-empty manifest version
- native `PLUGIN.md` manifests use schema version `1` today and are rejected if they declare another version
- native manifests preserve declared `capabilities` and `hooks` order for display, while also producing deterministic canonical sets for runtime comparison and auditing
- native manifests may optionally declare `author` and `homepage`; if present they must be non-empty
- `/api/plugins` and `/api/plugins/{name}` return the latest discovery decision alongside component counts
- `/api/plugins` and `/api/plugins/{name}` also return `registered_commands`, exposing each plugin-provided slash command name/description/prompt body so operators can audit the effective dynamic tool surface without rescanning plugin files
- `/api/plugins/reload` returns the full registration summary, including `hooks` and `mcp_servers`, so operators can confirm reload outcomes for the complete plugin runtime surface in one call

## Read next

- use [`DEPLOYMENT.md`](DEPLOYMENT.md) and [`DATABASES.md`](DATABASES.md) when configuration questions are really about runtime/storage layout
- use [`PROVIDERS.md`](PROVIDERS.md) and [`CHANNELS.md`](CHANNELS.md) when configuration questions are really about model or channel setup
- use [`../parity/PROTOCOLS.md`](../parity/PROTOCOLS.md) when you need deeper runtime semantics behind config behavior

## Further detail still missing

Deeper follow-up documentation is still useful for:
- config file shape and precedence
- secrets and env override guidance
- gateway/auth/runtime configuration pointers
- channel/model/provider configuration navigation

- native plugin manifest reports now preserve manifest_path and emit explicit warnings when runtime defaults name/version/description/binary
