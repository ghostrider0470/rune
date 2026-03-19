# API Reference Entry

This document is the stable reference entry for Rune's operator-facing API surface.

## Current scope

Rune exposes operator-facing HTTP endpoints and dashboard/API surfaces through the gateway.

Use these docs for the current contract picture:
- [`../parity/PROTOCOLS.md`](../parity/PROTOCOLS.md) — protocol and entity boundaries
- [`../parity/PARITY-CONTRACTS.md`](../parity/PARITY-CONTRACTS.md) — behavioral expectations and invariants
- [`../OPENCLAW-COVERAGE-MAP.md`](../OPENCLAW-COVERAGE-MAP.md) — where to find parity coverage by surface

## Current reference use

Use this doc as the API entrypoint for:
- understanding where the current HTTP/dashboard/control-plane contract picture lives
- navigating from API questions into parity contracts and coverage docs

## Read next

- use [`../parity/PROTOCOLS.md`](../parity/PROTOCOLS.md) when you need entity/protocol structure behind an API question
- use [`../parity/PARITY-CONTRACTS.md`](../parity/PARITY-CONTRACTS.md) when you need behavioral invariants and response expectations
- use [`../OPENCLAW-COVERAGE-MAP.md`](../OPENCLAW-COVERAGE-MAP.md) when you need the broad docs/parity navigation view by surface

---

## Scheduler and reminder endpoints

Shipped operator-facing HTTP surface for cron jobs and reminders.

### Cron job endpoints

| Method | Path | Purpose |
|---|---|---|
| `GET /cron/status` | | Scheduler status (total, enabled, due job counts) |
| `GET /cron` | | List cron jobs (query: `include_disabled`) |
| `POST /cron` | | Create cron job |
| `GET /cron/{id}` | | Get job details |
| `POST /cron/{id}` | | Update job (name, delivery mode, webhook URL, enabled) |
| `DELETE /cron/{id}` | | Remove job |
| `POST /cron/{id}/run` | | Trigger job immediately (manual run) |
| `GET /cron/{id}/runs` | | List run history for job |
| `POST /cron/wake` | | Queue wake event for heartbeat system |

### Cron job request shape

```
{
  "name": string?,
  "schedule": { "kind": "at" | "every" | "cron", ... },
  "payload": { "kind": "system_event" | "agent_turn", ... },
  "sessionTarget": "main" | "isolated",
  "deliveryMode": "none" | "announce" | "webhook",
  "webhookUrl": string?,
  "enabled": bool?
}
```

### Delivery mode behavior

- `none` — silent execution, no outbound delivery
- `announce` — broadcasts `cron_run_completed` event via the session event channel
- `webhook` — POSTs result payload to configured `webhookUrl` (30 s timeout, no retry)

### Reminder endpoints

| Method | Path | Purpose |
|---|---|---|
| `GET /reminders` | | List reminders (query: `include_delivered`) |
| `POST /reminders` | | Create reminder |
| `DELETE /reminders/{id}` | | Cancel reminder |

### Reminder request shape

```
{
  "message": string,
  "fire_at": "ISO-8601 datetime",
  "target": string  // default: "main"
}
```

### Reminder target routing

- `"main"` (default) — executes in the stable `system:scheduled-main` session
- `"isolated"` — creates a one-shot subagent session under the main scheduled session
- unknown values — fall back to `"main"` with a warning

### Reminder response fields

Responses include `id`, `message`, `target`, `fire_at`, `status` (`pending` / `delivered` / `cancelled` / `missed`), `delivered`, `created_at`, `delivered_at`, `outcome_at`, and `last_error`.

### Claim/lease semantics

Due jobs and reminders are claimed atomically before execution via a `claimed_at` column. Stale claims older than the lease duration (default 300 s) expire and become reclaimable for crash recovery. This prevents concurrent supervisor ticks from executing the same job or reminder twice.

---

## Further detail still missing

Deeper follow-up documentation is still useful for:
- auth expectations
- dashboard/API shape pointers
- session and control-plane resource summaries
- heartbeat status/enable/disable endpoints

Until a fuller API reference is split out, treat the parity docs as the detailed contract source.
