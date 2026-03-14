# Approval resume gap — 2026-03-14

## What the watchdog verified

The next highest-priority parity gap in `docs/AGENT-ORCHESTRATION.md` is real and precisely located:

- `apps/gateway/src/main.rs` currently forces `exec` into `ToolError::ApprovalRequired` unless `ask=off`
- `crates/rune-runtime/src/executor.rs` catches `ToolError::ApprovalRequired` but **converts it into an immediate synthetic deny**:
  - appends `approval_request`
  - appends `approval_response` with `decision = Deny`
  - appends `tool_result` error text
  - then loops back to the model
- `crates/rune-gateway/src/routes.rs` and `rune-store` already expose durable approval list/decide surfaces
- therefore the durable operator approval API exists, but runtime execution is not yet wired to create/use those durable approval rows

## Exact parity break

Current behavior is inspectable but not resumable:

1. model asks for `exec`
2. app tool layer returns `ApprovalRequired`
3. runtime writes transcript artifacts only
4. no `approvals` row is created from the live turn
5. later operator decision cannot resume the blocked execution because there is no execution checkpoint to resume

This matches the docs: approval visibility exists, durable resumption does not.

## Lowest-risk implementation slice

Implement in this order to minimize conflict with existing in-flight gateway/store/runtime edits:

### Slice 1 — persist real approval requests from runtime

Extend `TurnExecutor` wiring so approval-required tool calls create an `approvals` row instead of only transcript artifacts.

Needed payload per approval row:
- `subject_type = "tool_call"`
- `subject_id = <tool_call_id>`
- `reason = <tool name>`
- `presented_payload = structured ApprovalRequest JSON + resume context`

Resume context should include at least:
- `session_id`
- `turn_id`
- `tool_call_id`
- `tool_name`
- exact canonicalized tool arguments
- optional extracted `command`

### Slice 2 — stop fabricating deny on mere approval requirement

When approval is required, runtime should not append an automatic deny response.
Instead:
- append `approval_request`
- mark the turn/session as waiting
- stop the current turn with a dedicated waiting outcome

This is the semantic bridge needed for later resumption.

### Slice 3 — operator decision should map to execution intent

When `POST /approvals` decides:
- `allow_once`: authorize the exact stored call once
- `allow_always`: persist tool policy and authorize the stored call
- `deny`: append approval response + denied tool result to transcript

The current gateway route only records the decision row; it does not trigger runtime continuation.

### Slice 4 — resume executor path

Add a resume path that reconstructs the stored tool call and continues the pending turn:
- load approval row
- rebuild `ToolCall`
- re-run execution under the approved scope
- persist `tool_result`
- continue model loop

This likely wants a dedicated runtime entrypoint rather than overloading `execute(session_id, user_message, ...)`.

## Suggested acceptance checks

1. approval-required `exec` creates a durable pending approval row
2. no synthetic deny is written before operator decision
3. `allow_once` resumes exactly one stored call and does not widen scope
4. `allow_always` resumes stored call and persists future tool policy
5. `deny` writes transcript denial/audit trail without executing the command
6. restart-safe resumption is explicit: either implemented or documented as remaining gap

## Recommendation for next watchdog pass

Start with Slice 1 + Slice 2 only.
That closes the biggest semantic lie without forcing the full resume orchestrator in one risky pass.
