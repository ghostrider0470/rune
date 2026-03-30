
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
- `subagent_lifecycle=preempted` returns the last operator note as the next-task reason when
  available, making higher-priority takeovers visible without reading the raw transcript.

This closes an operator-visibility gap for approval pauses, delegated waits, and preempted work.
