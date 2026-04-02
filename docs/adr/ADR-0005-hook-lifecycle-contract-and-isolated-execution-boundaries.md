# ADR-0005: Hook lifecycle contract and isolated execution boundaries

- Status: Accepted
- Date: 2026-04-03
- Related issues: #776, #765

## Context

Rune supports plugin hook handlers that participate in runtime lifecycle events such as tool calls, turn execution, session lifecycle, compaction, and notifications. That extension point needs a stable contract so plugins can extend behavior without destabilizing the runtime.

Issue #776 requires explicit hook phases, isolated execution boundaries, documented input/output contracts, and explicit recursion prevention or bounds.

## Decision

Rune adopts the following hook lifecycle contract.

### 1. Explicit hook phases

The runtime exposes these canonical hook phases:

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

These phase names are the canonical serialized contract used by manifests, runtime records, diagnostics, and operator-facing reporting.

### 2. Execution order and scope

For a given event:

- handlers run in registration order
- handlers receive a mutable JSON context object
- handlers are isolated at the error boundary: one handler failure does not crash the hook registry or the runtime event loop by itself
- execution records are produced per handler with `plugin`, `event`, `outcome`, and optional `reason`

### 3. Failure semantics

Handlers declare policy through `fail_closed()`:

- `fail_closed = false` → fail open
  - handler error is recorded as `warned`
  - execution continues to the next eligible handler
- `fail_closed = true` → fail closed
  - handler error is recorded as `blocked`
  - runtime context is marked with:
    - `hook_blocked = true`
    - `hook_block_reason = "hook \`<plugin>\` failed: <error>"`
  - further handlers for the same event are not executed

This makes hook failures observable while preventing extension failures from silently corrupting control flow.

### 4. Input/output contract

Hook input/output is a mutable JSON object owned by the runtime for the duration of a single event emission.

Contract rules:

- hooks may inspect existing keys and may add or update keys in the provided context
- hooks must treat absent keys as normal and avoid assuming a fully fixed schema unless documented by the emitting site
- runtime-owned block markers (`hook_blocked`, `hook_block_reason`) are reserved for hook policy enforcement
- callers must treat hook-produced context as event-scoped data, not as a durable plugin state store
- handler outcomes are emitted separately as structured execution records and should be used for diagnostics/auditing

### 5. Isolation boundary

Isolation is logical/runtime isolation, not process isolation.

Current guarantees:

- handler errors are caught and converted into policy outcomes
- a handler cannot directly stop execution of sibling handlers unless it is configured fail-closed and fails
- hook bookkeeping is separated from handler mutation through structured `HookExecutionRecord` output
- registry-level dispatch remains available even when individual handlers fail

Non-goals of the current contract:

- process sandboxing per hook
- timeouts per hook handler
- resource quotas per hook handler

Those may be added later without changing the phase names or policy vocabulary.

### 6. Recursion prevention and bounded behavior

Hook handlers must not recursively re-emit the same lifecycle event through the registry. Rune's current contract treats hook execution as non-reentrant runtime extension logic.

Practical boundary in the current implementation:

- the registry only executes handlers explicitly registered for the emitted event
- handlers are invoked by runtime-owned emission sites, not by chained automatic redispatch inside the registry
- fail-closed blocking stops further processing for the current event
- session-kind filters and suppression rules provide additional bounded skipping behavior before execution

Operator and contributor guidance:

- plugin authors must not implement self-triggering hook loops
- future nested hook emission support, if added, must include an explicit depth bound and tracing metadata

## Consequences

### Positive

- plugins now have a documented contract for lifecycle events
- operators can distinguish applied, warned, blocked, suppressed, and skipped outcomes
- runtime-visible failures are auditable without collapsing the full execution path
- the current implementation is aligned with documented semantics

### Tradeoffs

- JSON context remains flexible rather than fully schema-typed
- recursion prevention is currently contract-driven and architecture-bounded rather than enforced by a depth counter
- stronger isolation such as per-hook sandboxing/timeouts remains future work

## Operational notes

When troubleshooting hook behavior:

- inspect hook execution records for outcome and reason
- if `hook_blocked = true`, treat the event as fail-closed terminated
- if a handler is missing, check session-kind filtering, suppression rules, disable overrides, duplicate plugin precedence, and manifest compatibility decisions
