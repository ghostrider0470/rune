# Hook operations and troubleshooting

This runbook explains the runtime-visible behavior of Rune hooks.

## Canonical lifecycle phases

Rune exposes these hook phases and uses the same serialized names in manifests, diagnostics, execution records, and registration metadata:

## Deterministic registration and execution order

Hook ordering is deterministic. Rune preserves handler registration order per event and executes handlers in that exact order. In practice, the ordering contract is:

1. plugin discovery order is deterministic from configured scan directory order
2. duplicate plugin names are resolved with first-directory-wins semantics
3. manifest-declared hook order is preserved when handlers are registered
4. runtime execution order for a given event matches handler registration order exactly

This means repeated startups with the same plugin directories and manifests produce the same per-event hook ordering. Registration metadata can be exported from the runtime for auditability and troubleshooting.

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
- pre-tool and post-tool execution records are appended to the session transcript as `status_note` entries prefixed with `hook_pre_tool_call ` or `hook_post_tool_call ` followed by a JSON array of execution records, each carrying deterministic `order` metadata
- fail-closed pre-tool handlers stop the tool call before execution and the recorded block reason is surfaced as the tool result error text

Hook context is event-scoped working state, not a durable plugin state store.

## Per-handler outcomes

Runtime execution records report one of these outcomes and include `order`, the zero-based handler position for that event execution:

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
