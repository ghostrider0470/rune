## Next-task explainability and approval-aware resume

`GET /sessions/{id}/status` and `session.status` now expose `next_task_reason` alongside
`status_reason` and `resume_hint`.

Use these fields together:

- `status_reason` explains why the current state exists.
- `next_task_reason` explains why Rune will pick the next action it reports.
- `resume_hint` tells the operator how to safely resume blocked or preempted work.

Current behavior:

- `waiting_for_approval` returns an approval-blocked next-task reason so operators can see that
  approval handling is the only valid next step.
- `waiting_for_subagent` returns delegated-work follow-up context, including lifecycle-specific
  reasons like `queued`.
- `subagent_lifecycle=preempted` returns the last operator note as the `status_reason` and
  `next_task_reason` when available, plus a concrete parked-work `resume_hint`, making
  higher-priority takeovers visible without reading the raw transcript.
- `status=cancelled` with `subagent_lifecycle=cancelled` now explains that delegated work was
  explicitly cancelled, preserves the operator note as the next-task reason when present, and
  gives a concrete recovery hint: either spawn a replacement subagent or steer the parent session
  with a new plan.

This closes an operator-visibility gap for approval pauses, delegated waits, preempted work, and
explicitly cancelled delegated work.


## Goal lease visibility on session status

`GET /sessions/{id}/status` and session-tree payloads now expose `goal_lease` when a session carries durable orchestration lease metadata. This gives operators a compact control-plane snapshot without reading raw session metadata.

`goal_lease` currently includes:

- `goal_key` — the durable objective identifier
- `owner_agent_id` — the agent/session currently holding the lease
- `state` — `active` or `expired`, derived from `goal_lease_expires_at`
- `leased_at` and `lease_expires_at` — lease timing for takeover/debug decisions
- `recovered_at` and `recovered_from_agent_id` — present when ownership was recovered from a stale worker

Operational guidance:

- treat `state=expired` as audit information, not current ownership truth
- use `goal_lease` together with `status_reason` / `next_task_reason` to distinguish an actively owned objective from a stuck or abandoned one
- when recovery fields are present, the current owner already replaced a stale worker; use that history before manually reassigning work

## Lane queue visibility and priority routing

`GET /status` exposes `lane_stats` so operators can verify that high-priority work is isolated from normal background contention. The payload includes independent utilisation/capacity counters for `main`, `priority`, `subagent`, `cron`, and `heartbeat`, plus global/per-project tool concurrency.

Practical reading guide:

- `priority_*` reflects control/comms traffic that should cut ahead of background work.
- `heartbeat_*` reflects watchdog/health traffic that should remain immediate even when other lanes are busy.
- `subagent_*` shows delegated workload saturation separately from operator-facing direct sessions.
- `tool_active`, `tool_capacity`, and `project_tool_capacity` help explain cross-project throttling during heavy tool use.

Current routing contract:

- background cron load does not block the `priority` lane
- `heartbeat` traffic bypasses normal priority contention
- scheduled and subagent sessions use independent lanes
- lane stats are intended as the operator-facing explainability surface for task picking and preemption pressure

This is the fast diagnostic surface to check before assuming Rune is stalled: if `main` is free but `subagent` is saturated, the bottleneck is delegated work; if `priority` or `heartbeat` are active, control-plane traffic is intentionally bypassing background queues.
