# Subsystems to replicate

## 1. Gateway / control plane
Long-running daemon with local/remote access, status, logs, auth, pairing, and tool invocation.

## 2. Agent runtime
Session engine, prompt assembly, turn execution, tool loops, retries, and compaction.

## 3. Channels
Provider adapters normalizing inbound/outbound messaging, media, group routing, and reactions.

## 4. Automation
Heartbeats, cron jobs, wake events, reminders, and periodic tasks.

## 5. Memory
Workspace memory files, semantic retrieval, indexing, and safe recall APIs.

## 6. Tools
Filesystem tools, process tools, scheduler tools, session tools, memory tools, ACP dispatch (Claude Code / Codex CLI), and future browser/media tools.

## 7. Skills / plugins
Prompt-triggered skills plus executable extensions.

## Hook lifecycle contract

Rune hook execution uses explicit lifecycle phases with per-handler policy outcomes.

Canonical phases:
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

Execution model:
- handlers run in registration order for the emitted phase
- handlers receive mutable JSON event context
- handlers may modify context for the active event
- session-kind filters may skip handlers before execution
- suppression rules may suppress handlers before execution

Per-handler outcomes reported by the runtime:
- `applied` — handler ran successfully
- `warned` — handler failed fail-open; execution continued
- `blocked` — handler failed fail-closed; event execution stopped
- `suppressed` — handler was intentionally suppressed before execution
- `skipped` — handler did not apply, for example due to session-kind filtering

Failure boundary:
- hook handler failures do not crash registry dispatch by default
- fail-closed handlers set `hook_blocked = true` and `hook_block_reason`
- once blocked, remaining handlers for that event are not executed

For the architecture decision and contract rationale, see [`../adr/ADR-0005-hook-lifecycle-contract-and-isolated-execution-boundaries.md`](../adr/ADR-0005-hook-lifecycle-contract-and-isolated-execution-boundaries.md).


## 8. UI
Operator dashboard, session browser, logs, approvals, skills, config, and channel health.

## 9. Security / approvals
Auth, secrets, approval gates, sandbox boundaries, and audit trails.

## 10. Operations
Health checks, doctor flows, updates, backups, service lifecycle, and diagnostics.
