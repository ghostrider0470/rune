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


## Session creation and subagent delegation handoff

`POST /sessions` supports first-class subagent handoff fields so orchestrators can preload relevant context instead of forcing delegated agents to rediscover it.

### Session creation request fields

| Field | Type | Notes |
|---|---|---|
| `kind` | string | Defaults to `"direct"`; use `"subagent"` for delegated agent sessions |
| `workspace_root` | string? | Optional workspace root for the session |
| `requester_session_id` | UUID? | Parent/orchestrator session ID |
| `channel_ref` | string? | Optional channel or orchestrator reference |
| `mode` | string? | Optional mode hint persisted in session metadata |
| `project_id` | string? | Optional project-scoped context selector |
| `delegation_context` | object? | Preloaded context slice for a subagent |
| `shared_scratchpad_path` | string? | Stable handoff file path shared by orchestrator and subagent |
| `delegation_plan` | object? | Optional upstream sender/receiver/routing contract embedded into subagent handoff metadata |

### Delegation context intent

When `kind = "subagent"` and `delegation_context` is provided, Rune stores the payload under session metadata so the subagent prompt can start from the orchestrator-selected slice.

Typical `delegation_context` contents:
- task summary/objective
- token budget or context budget
- relevant memory chunks
- relevant file summaries
- constraints, expected output, or execution notes

### Shared scratchpad intent

`shared_scratchpad_path` lets the orchestrator and delegated subagent coordinate through a stable workspace file path for structured findings and incremental handoff.

### Example request

```json
{
  "kind": "subagent",
  "requester_session_id": "11111111-1111-1111-1111-111111111111",
  "channel_ref": "orchestrator:acme",
  "mode": "isolated",
  "delegation_context": {
    "task": "Implement retry budget fix",
    "budget": { "token_budget": 1536 },
    "file_summaries": [
      {
        "path": "src/retry.rs",
        "summary": "retry budget enforcement"
      }
    ]
  },
  "shared_scratchpad_path": "agents/acme/scratchpads/retry-fix.md",
  "delegation_plan": {
    "strategy": "named",
    "sender": { "instance_id": "origin-a" },
    "receiver": { "instance_id": "peer-b" }
  }
}
```

### Current behavior notes

- Delegation metadata sections are currently rendered into subagent system context.
- This ships the context-handoff substrate for delegated sessions.
- Subagent creation also persists `shared_scratchpad_path` even when no explicit `delegation_context` payload is supplied; Rune stores an empty object for `delegation_context` in that case so prompt assembly has a stable metadata shape.
- `delegation_plan` can be supplied alongside `delegation_context`; Rune embeds it under `delegation_context.delegation_plan` so delegated sessions retain the upstream sender/receiver/routing contract.
- Shared scratchpad support is path-level metadata today; higher-level bidirectional scratchpad workflows can build on top of it.

## Multi-instance delegation and failover contracts

Rune already exposes the core multi-instance operator surfaces needed to reason about federation health and delegation safety.

### Key HTTP endpoints

| Method | Path | Purpose |
|---|---|---|
| `GET /api/v1/instance/health` | | Local instance identity, capability manifest, peer reachability snapshot |
| `GET /api/v1/instance/delegation-plan` | | Compute sender/receiver/task-contract metadata before handing work to a peer |
| `POST /api/v1/instance/delegations` | | Submit a delegated task envelope |
| `GET /api/v1/instance/delegations/{task_id}` | | Poll delegated task lifecycle state/result envelope |
| `GET /api/v1/instance/peer-health-alerts` | | Summarize peer health alerts, failover readiness, and split-brain guard state |

### Instance config baseline

Configure `[instance]` in `config.toml` for every node participating in federation:

```toml
[instance]
# id defaults to a persisted UUID under ~/.rune/instance-id
name = "rune-local"
# advertised_addr = "http://10.0.0.15:18790"
# roles = ["gateway"]
# peers = [
#   { id = "rune-laptop", health_url = "http://10.0.0.16:18790/api/v1/instance/health" },
# ]
```

Operational expectations:
- `id` is stable across restarts because Rune persists it locally if you do not set one explicitly.
- `name` is the operator-facing identity shown in health/delegation surfaces.
- `advertised_addr` should resolve from peer instances; otherwise delegation plans will fall back to the peer health URL origin when possible.
- `peers[*].health_url` is the explicit discovery list today. Federation is config-driven, not gossip-based.

### Delegation-plan safety checks

`GET /api/v1/instance/delegation-plan` returns a preflight contract instead of forcing operators to guess whether a peer is safe to target.

The response includes:
- sender identity and `capability_hash`
- selected peer identity/addressing
- capability compatibility evaluation (`compatible`, model overlap, missing roles, missing projects, detail)
- task submission/result polling URLs
- branch-reservation and file-lock expectations for delegated coding work
- timeout/lifecycle metadata for the delegated task contract

Use it before enabling real handoff so operators can verify:
- the chosen peer is healthy
- the chosen peer is capability-compatible
- the routing URLs are reachable from the sender
- the conflict-prevention requirements are understood by the orchestrator

### Failover and split-brain boundary

`GET /api/v1/instance/peer-health-alerts` is the current operator surface for failover posture.

Important semantics already enforced by Rune:
- unhealthy peers are classified separately from degraded peers
- `work_absorption_required = true` only when at least one peer is unreachable
- `failover_ready = true` only when no degraded peers are present
- `network_partition_suspected = true` when unreachable and degraded peers coexist
- `split_brain_guard` explicitly documents that failover absorption does **not** activate for merely degraded peers

That means Rune intentionally prefers under-failing over duplicate execution during suspected partitions.

### Rejoin and compatibility guidance

Current guidance for peer restarts/rejoin events:
- keep instance IDs stable across restarts so peers observe the same logical node coming back
- re-check `/api/v1/instance/health` after restart to confirm identity, capability hash, and peer visibility
- re-run `rune gateway delegation-plan` before resuming delegated work if model/project/role capability changed during rollout
- treat `capability_hash` drift as a rollout signal: compatible peers can still be selected, but operators should verify the reported compatibility details before resuming automation

### CLI verification flow

The CLI mirrors the same contracts for local operator checks:
- `rune gateway instance-health`
- `rune gateway delegation-plan --strategy least_busy`
- `rune gateway delegation-plan --strategy named --peer-id <peer>`

Recommended rollout sequence:
1. bring up each node with `[instance]` configured
2. verify every node reports the expected identity and peers via `instance-health`
3. inspect a delegation plan from the sender node
4. only then enable real delegated work between instances

## Further detail still missing

Deeper follow-up documentation is still useful for:
- auth expectations
- dashboard/API shape pointers
- session and control-plane resource summaries
- heartbeat status/enable/disable endpoints

Until a fuller API reference is split out, treat the parity docs as the detailed contract source.
