# Hook operations and troubleshooting

This runbook explains the runtime-visible behavior of Rune hooks.

## Canonical lifecycle phases

Rune exposes these hook phases and uses the same serialized names in manifests, diagnostics, and execution records:

- `pre_tool_call`
- `post_tool_call`
- `pre_turn`
- `post_turn`
- `session_created`
- `session_completed`
- `stop`
- `subagent_stop`
- `user_prompt_submit`
- `pre_compact`
- `notification`

Use only these values when declaring hook handlers.

## Execution contract

For a single emitted event:

- handlers run in registration order
- each handler receives mutable JSON event context
- handlers may add or update event-scoped fields in that context
- session-kind filters may skip handlers before execution
- suppression rules may suppress handlers before execution
- the runtime emits one structured execution record per handler attempt or pre-execution disposition

Hook context is event-scoped working state, not a durable plugin state store.

## Per-handler outcomes

Runtime execution records report one of these outcomes:

- `applied` — handler ran successfully
- `warned` — fail-open handler failed and execution continued
- `blocked` — fail-closed handler failed and execution stopped for the current event
- `suppressed` — handler was intentionally suppressed before execution
- `skipped` — handler did not apply, for example because session-kind filtering excluded it

## Failure semantics

Hook failures are isolated at the handler boundary.

- one handler failure does not crash registry dispatch by itself
- fail-open handlers continue sibling execution and record `warned`
- fail-closed handlers record `blocked`, set `hook_blocked = true`, set `hook_block_reason`, and stop later handlers for that event

When `hook_blocked = true`, treat the current event as intentionally terminated by hook policy rather than as a generic runtime crash.

## Isolation boundary

Current isolation is runtime-level, not process-level.

What Rune guarantees now:

- handler errors are caught and converted into structured outcomes
- handler bookkeeping is separated from mutable event context
- registry dispatch remains available even when individual handlers fail

What Rune does not guarantee yet:

- per-hook process sandboxing
- per-hook timeouts
- per-hook resource quotas

## Recursion prevention

Hooks are contractually non-reentrant.

Operator meaning:

- hooks should not recursively re-emit the same lifecycle event through the registry
- current boundedness comes from runtime-owned emission sites, fail-closed termination, session-kind filtering, and suppression rules
- future nested hook support must add an explicit depth bound and tracing metadata before it is considered safe

## Troubleshooting checklist

If hook behavior looks wrong:

1. confirm the handler declared a canonical lifecycle phase
2. inspect plugin discovery status for duplicate-name precedence, disable overrides, or manifest incompatibility
3. inspect execution records for `applied`, `warned`, `blocked`, `suppressed`, or `skipped`
4. if blocked, inspect `hook_block_reason`
5. if a handler never ran, check session-kind filtering and suppression rules before assuming registration failed

## Related docs

- [`CONFIGURATION.md`](CONFIGURATION.md) — configuration entrypoint and plugin discovery notes
- [`../reference/SUBSYSTEMS.md`](../reference/SUBSYSTEMS.md) — subsystem-level lifecycle contract summary
- [`../adr/ADR-0005-hook-lifecycle-contract-and-isolated-execution-boundaries.md`](../adr/ADR-0005-hook-lifecycle-contract-and-isolated-execution-boundaries.md) — canonical architecture decision
