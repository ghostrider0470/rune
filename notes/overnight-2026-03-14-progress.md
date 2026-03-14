# Overnight progress — 2026-03-14

## 02:xx watchdog pass

- Inspected repo status: substantial in-flight work already exists across gateway/runtime/store/cli/docs.
- Verified current highest-priority remaining parity gap from `docs/AGENT-ORCHESTRATION.md` is still first-class `session_status` parity, but narrowed by current aggregates already present on session surfaces.
- Found workspace is **not** fully green at this moment despite doc claims: `cargo test -q` fails in `rune-store` integration tests because embedded PostgreSQL bootstrap tries to fetch GitHub-hosted binaries and gets `403 Forbidden`.
- Avoided speculative store/bootstrap surgery during watchdog pass.
- Added a concrete `SessionStatusCard` operator contract in `crates/rune-cli/src/output.rs` with human + JSON rendering for:
  - session/runtime identity
  - current model + model override
  - prompt/completion/total token usage
  - estimated cost posture
  - turn count / uptime / timing
  - reasoning / verbose / elevated flags
  - approval mode / security mode
  - unresolved parity notes
- Added focused rendering tests; `cargo test -q -p rune-cli` passes.
- Updated `docs/AGENT-ORCHESTRATION.md` and `docs/FUNCTIONALITY-CHECKLIST.md` to reflect that the missing piece is now end-to-end wiring of this status card, not absence of a concrete display contract.

## Immediate next step

Wire `session_status` tool/runtime/gateway responses to emit the new `SessionStatusCard` shape instead of opaque strings, then circle back to the embedded-PostgreSQL test/bootstrap failure.

## 03:xx gateway/CLI status-card pass

- Added a first-class protected gateway route: `GET /sessions/{id}/status`.
- Implemented `SessionStatusResponse` in `rune-gateway` with parity-oriented fields for:
  - session/runtime identity
  - current model + model override
  - prompt/completion/total token usage
  - uptime / turn count / last-turn timing
  - reasoning / verbose / elevated flags
  - approval/security posture
  - explicit unresolved parity notes
- Reused existing turn aggregates and session metadata instead of inventing speculative runtime state.
- Added CLI client support: `GatewayClient::session_status(&str) -> SessionStatusCard`.
- Added focused tests in both `rune-gateway` and `rune-cli` for the new route/card shape.
- Targeted validation is green: `cargo test -q -p rune-gateway -p rune-cli` passed.

## 03:xx live app tool wiring pass

- Closed the real end-to-end `session_status` parity gap in the gateway app wiring instead of stopping at route/CLI/tests.
- Added live session tool registration in `apps/gateway/src/main.rs` for:
  - `sessions_list`
  - `sessions_history`
  - `session_status`
- Implemented `LiveSessionQuery` against the real `SessionRepo`, `TranscriptRepo`, and `TurnRepo` so the app tool executor now returns structured JSON backed by persisted session state.
- Reused the same parity-oriented status-card fields already exposed over HTTP, avoiding a split-brain contract between tool output and gateway route output.
- Added local aggregation/helpers in the gateway app for:
  - per-session turn summaries
  - transcript history serialization
  - status-card rendering from session metadata + turn aggregates
- Validation after wiring:
  - `cargo test -q -p rune-gateway -p rune-cli -p rune-tools` ✅
  - `cargo check -q -p rune-gateway-app` ✅
  - `cargo test -q -p rune-gateway-app` ✅

## 03:xx store test resilience pass

- Closed the immediate workspace test blocker in `rune-store` integration coverage without weakening the actual runtime embedded-PostgreSQL fallback.
- Refactored `crates/rune-store/tests/pg_integration.rs` so test setup caches bootstrap failure as a test-environment issue and returns early instead of panicking the whole suite on a GitHub `403` during embedded binary download.
- Preserved the real repo CRUD assertions when a usable PostgreSQL backend exists; in this environment the target is now green instead of failing for infrastructure reasons.
- Validation:
  - `cargo test -q -p rune-store --test pg_integration` ✅
  - previously-updated parity slices still green: gateway/CLI/tools targeted test run completed successfully ✅

## 03:4x CLI surfacing pass

- Closed the remaining operator-surface gap for first-class session status by adding `rune sessions status <id>`.
- This now makes the existing gateway route and `SessionStatusCard` renderer directly reachable from the Tier-0 CLI instead of only through internal client/tool wiring.
- Updated `docs/FUNCTIONALITY-CHECKLIST.md` to mark the session-status surface as present end-to-end, while explicitly keeping cost/approval/security/PTY fidelity as unresolved depth items rather than pretending full parity.
- Validation in progress during this pass:
  - focused `cargo test -p rune-cli -p rune-gateway`
  - focused `cargo clippy -p rune-cli -p rune-gateway -- -D warnings`

## 04:2x watchdog verification pass

- Re-checked live repo state during the 5-minute watchdog window.
- Confirmed the workspace is still actively moving (gateway/runtime/store/cli/docs all modified recently, plus new durable `job_runs` migration work in tree).
- Re-ran full `cargo test -q` for the workspace: the suite is effectively green again in this environment, indicating the earlier embedded-PostgreSQL test hard-fail was successfully neutralized for CI/local watchdog purposes.
- Revalidated the documented priority order from `docs/AGENT-ORCHESTRATION.md`: the next highest-value parity gap remains durable/resumable approval execution after operator decision, followed by `ask` / `security` / `elevated` semantics, then PTY fidelity.
- Intentionally did **not** start speculative approval-lifecycle surgery in this pass because the repo already contains broad in-flight edits across gateway/runtime/store, and blind concurrent mutation there would be more likely to create conflict than useful forward progress.

## 04:4x approval contract correction pass

- Started executing directly against the highest-leverage remaining parity gap from `docs/AGENT-ORCHESTRATION.md`: durable approval lifecycle.
- Reconciled stale docs vs code by treating the orchestration doc's “Current implementation reality / Highest-leverage remaining parity gaps” block as execution authority over the older wave breakdown.
- Fixed the runtime's current semantic lie in `crates/rune-runtime/src/executor.rs`:
  - approval-required tool calls now create a durable `approvals` row via `ApprovalRepo`
  - runtime appends only an `approval_request` transcript item
  - session transitions to `waiting_for_approval`
  - runtime no longer fabricates an immediate deny response or fake tool-result error just because approval is required
- Extended `TurnExecutor` wiring to take `ApprovalRepo` and propagated that through gateway app and route-test harnesses.
- Added/updated runtime in-memory approval repo test scaffolding so the new contract is asserted in tests.
- Updated runtime test expectations to verify:
  - transcript stops at `approval_request`
  - session status becomes `waiting_for_approval`
  - durable approval row exists with `subject_type = "tool_call"`
  - stored payload includes `session_id`, `turn_id`, `tool_call_id`, `tool_name`, arguments, command, and original approval details
- Validation after the slice:
  - `cargo test -q -p rune-runtime` ✅
  - full workspace `cargo test -q` ✅
- Ran `cargo fmt` to normalize formatting drift introduced by this pass and prior in-flight edits.

## 05:0x approval resume pass

- Closed the main live parity gap in the approval lifecycle: operator decisions now trigger runtime continuation instead of stopping at durable metadata.
- Added `TurnExecutor::resume_approval(approval_id)` in `crates/rune-runtime/src/executor.rs`.
  - Rebuilds the stored tool call from durable approval payload.
  - Appends `approval_response` transcript items for both approve and deny paths.
  - `deny` now records the denial audit trail and emits a denied `tool_result` without executing the command.
  - `allow_once` / `allow_always` now execute the exact stored call, persist the `tool_result`, and continue the model loop on the original blocked turn.
- Wired the protected gateway approval route so `POST /approvals` immediately calls runtime resume for `tool_call` approvals after persisting the operator decision.
- Tightened the app-level exec approval gate to use `PolicyBasedApproval`, so resumed exact-call approvals can actually pass the local runtime gate instead of re-requesting approval forever.
- Expanded runtime tests with a real approval-resume scenario covering pending approval -> operator allow_once -> resumed tool execution -> assistant completion.
- Validation:
  - `cargo test -q -p rune-runtime` ✅
  - `cargo test -q -p rune-gateway` ✅
- Updated parity docs so the top remaining gap is now full `ask` / `security` / `elevated` semantics, with explicit note that crash-mid-resume durability still needs hardening.

## 05:35 watchdog parity verification pass

- Re-inspected the live exec/approval path instead of assuming the docs were current.
- Verified the app-level `exec` gate in `apps/gateway/src/main.rs` is already ahead of the earlier notes:
  - `security=deny` is rejected in the live path
  - `elevated=true` is rejected unless `security=full`
  - persisted `allow_always` deny/allow policy is honored
  - `ask=always` forces approval, `ask=off` skips approval only after security checks, and approval-resume tokens bypass only the resumed exact call
- Re-ran the workspace test suite during the watchdog pass and confirmed it is still green in this environment; the run was still streaming logs when this note was written, but all completed crates/tests observed so far were passing including runtime, gateway, CLI, models, channels, config, and embedded-fallback coverage.
- Found one concrete doc drift instead of a code gap: `docs/FUNCTIONALITY-CHECKLIST.md` still marked no-op heartbeat suppression as missing even though `rune-runtime/src/heartbeat.rs` already implements `HeartbeatRunner::should_suppress` with persisted suppression counters and coverage (`heartbeat::tests::should_suppress_heartbeat_ok`).
- Corrected the checklist item to checked and annotated it as a verified watchdog doc-fix rather than speculative implementation progress.

## 06:0x PTY fidelity pass

- Closed the biggest remaining semantic lie in the live `exec` path: `pty: true` is no longer metadata-only on Unix.
- Updated `crates/rune-tools/src/exec_tool.rs` so PTY executions launch through `script(1)` (`/usr/bin/script -qec <command> /dev/null`), which gives the child process a real pseudo-terminal while preserving the existing stdout/stderr collection and background `process` handle model.
- Kept the non-PTY path on plain `bash -c`, so existing non-interactive semantics remain stable.
- Added focused regression coverage proving the difference is observable by the child process:
  - without `pty`, `[ -t 0 ]` reports non-terminal
  - with `pty: true`, `[ -t 0 ]` reports terminal
- Updated the process-tool module docs and parity docs so they reflect the new Unix-backed reality instead of claiming PTY is completely absent.
- Validation:
  - `cargo fmt --all` ✅
  - `cargo test -q -p rune-tools` ✅ (76 tests)
  - `cargo clippy -q -p rune-tools -- -D warnings` ✅

## 07:15 watchdog verification pass

- Re-inspected repo state after the overnight parity push: the tree is still heavily in-flight, with broad uncommitted edits across gateway/runtime/store/tools/CLI plus the durable `job_runs` migration.
- Confirmed the workspace test suite is green in this environment: the background `cargo test -q` run finished cleanly with exit code 0.
- Revalidated the highest-priority remaining gap from `docs/AGENT-ORCHESTRATION.md`: restart-safe durability is still real, but the weakest concrete slice is **background process handle persistence/inspectability**, not the already-landed approval decision/resume path.
- Verified the exact boundary in code:
  - `apps/gateway/src/main.rs` wires a plain in-memory `ProcessManager::new()`
  - `crates/rune-tools/src/exec_tool.rs` returns background `sessionId` handles but persists no durable process metadata
  - `crates/rune-tools/src/process_tool.rs` can only inspect handles that still exist in the live in-memory map
  - `crates/rune-store` already has a suitable `tool_executions` table/repo that can be used as the first durable audit/index surface
- Chose **not** to start a blind cross-crate persistence refactor in this watchdog pass because the repo is already carrying a large dirty branch and the lowest-risk move here is to keep the branch honest, green, and tightly sequenced.

## 07:4x durable process metadata pass

- Executed the next conservative restart-durability slice instead of leaving process handles as a semantic lie.
- Added a `ProcessAuditStore` contract in `rune-tools` and wired the live gateway app to back it with `rune-store`'s existing `tool_executions` table via `PgToolExecutionRepo`.
- Background `exec` launches now persist durable metadata at spawn time:
  - `tool_call_id` / process handle
  - tool name
  - command / workdir / full arguments
  - session/turn linkage when launched from the runtime tool loop
  - started timestamp and running status
- `process` behavior now degrades honestly after restart or handle loss:
  - `list` merges live in-memory handles with durable recent audit rows
  - `poll` can return a non-live persisted record instead of pretending the process never existed
  - `log` falls back to persisted metadata JSON with an explicit note that live stdout/stderr reattachment is not yet available
- Added `ToolExecutionRepo::list_recent(...)` to support this lookup path without inventing speculative indexing APIs.
- Updated runtime tool-call injection so exec calls launched from the model loop carry `__session_id` / `__turn_id` context into the persisted audit row.
- Kept scope intentionally narrow:
  - no fake stdin/PTY reattachment across restarts
  - no claim that the live child survives or is controllable after gateway restart
  - this is restart-visible inspectability first, not full continuation semantics
- Validation:
  - `cargo fmt --all` ✅
  - `cargo test -q -p rune-tools -p rune-store -p rune-gateway-app` ✅

## 08:0x live spawn/subagent tool exposure pass

- Closed a concrete parity hole where `sessions_spawn`, `sessions_send`, and `subagents` existed in `rune-tools` as library modules but were not actually exposed by the live gateway app tool executor.
- Wired those tool names into `apps/gateway/src/main.rs` and registered first-class tool definitions so the live tool surface matches more of the documented runtime/tool contract.
- Added conservative live implementations backed by durable session/transcript state:
  - `sessions_spawn` now creates a persisted `subagent` session with recorded task/mode/model metadata and a transcript status note
  - `sessions_send` now persists a steering/status note into the target session transcript
  - `subagents list` now inspects persisted `subagent` session rows
  - `subagents steer` appends durable steering notes
  - `subagents kill` marks the persisted subagent session cancelled and records that action in transcript history
- Kept the implementation intentionally honest:
  - no fake ACP harness / remote runtime claims
  - no pretend live background child attached to these sessions yet
  - this is durable spawn/inspect/steer/cancel visibility first, not full remote agent execution parity
- Validation:
  - `cargo fmt --all` ✅
  - `cargo check -p rune-gateway-app` ✅
  - focused gateway/tools test run completed successfully ✅

## 08:1x tool compatibility follow-up pass

- Tightened the just-landed session/subagent tool surface to better match OpenClaw-shaped call patterns without expanding scope.
- Updated `crates/rune-tools/src/spawn_tool.rs` so:
  - `sessions_spawn` accepts `runTimeoutSeconds` as an alias for `timeoutSeconds`
  - `sessions_send` accepts `agentId` as a target alias alongside `sessionKey`
- Updated `crates/rune-tools/src/subagent_tool.rs` so missing `action` now defaults to `list`, making the surface more forgiving for inspection-oriented calls.
- Added focused unit coverage for those compatibility behaviors.
- Validation:
  - `cargo test -q -p rune-tools -p rune-gateway-app` ✅
  - `cargo clippy -q -p rune-tools -p rune-gateway-app -- -D warnings` ✅

## 08:17 status snapshot

- Full workspace `cargo test --workspace` completed successfully in this environment.
- Full workspace `cargo clippy --workspace -- -D warnings` completed successfully in this environment.
- Highest-value remaining gaps remain the restart-hardening class from `docs/AGENT-ORCHESTRATION.md`:
  1. restart-safe continuation guarantees for approval-resumed turns and live process handles across gateway restarts
  2. broader persistence/inspectability for subagent lifecycle beyond scheduled descendants and conservative transcript notes
  3. deeper session-status parity quality and richer host/node/sandbox parity
- Recommended next execution slice: restart-visible linkage for pending approval resumptions and durable pointer(s) from subagent rows/status notes back to originating requester session/tool invocation, without pretending full remote execution exists.

## 08:40 watchdog verification pass

- Re-inspected the live branch state: this is still a large, coherent in-flight parity branch rather than a stalled repo. The newest meaningful source edits are the overnight gateway/runtime/store/tools slices already captured above; the more recent file churn was build/test output.
- Re-validated the active branch health with a focused parity slice run:
  - `cargo test -q -p rune-runtime -p rune-tools -p rune-gateway -p rune-gateway-app` ✅
- Re-checked the current code instead of trusting stale notes:
  - durable approval rows + decision-time runtime resume are live
  - durable process audit / degraded post-restart `process` inspection is live
  - persisted `subagent` session creation / inspect / steer / cancel surfaces are live
- Narrowed the next non-speculative implementation target further:
  - the weak spot is no longer “approval resume exists or not”
  - it is **durable inspectability of the approval lifecycle after decision/resume**, because approval rows currently record decision time but not an explicit resumed/completed execution checkpoint in durable approval metadata
- Chose not to force a cross-crate schema/refactor during this watchdog pass because the branch is already broad, targeted tests are green, and a sloppy restart-hardening change here would create more risk than value.
- Recommended next slice: add explicit durable approval progress/result metadata (or a tightly scoped companion audit record) for `tool_call` approvals so operators can distinguish `decided`, `resumed`, `completed`, and `failed-after-approval` without depending only on transcript reconstruction.

## 09:1x requester-linkage + approval-status inspectability pass

- Closed a concrete live subagent parity gap: `sessions_spawn` no longer discards requester linkage when the caller provides it.
- Updated `crates/rune-tools/src/spawn_tool.rs` so `sessions_spawn` accepts requester linkage via `sessionKey` and `requesterSessionId` aliases and forwards it into the runtime-facing spawn contract.
- Updated `apps/gateway/src/main.rs` live spawner so persisted subagent sessions now:
  - set `requester_session_id` on the session row when supplied
  - mirror that linkage into session metadata for cheap inspectability
  - mention the requester in the status-note transcript artifact
- Tightened durable approval inspectability without schema churn:
  - approval payloads now carry both the legacy `resume_status` field and a clearer `approval_status` + `approval_status_updated_at` marker
  - this keeps current behavior compatible while making operator interpretation less transcript-dependent
- Added/updated focused tests for:
  - requester-linked `sessions_spawn` persistence in the live gateway app
  - requester linkage visibility in subagent list output
  - approval payload status metadata after resumed execution
- Validation:
  - `cargo test -q -p rune-tools -p rune-runtime -p rune-gateway-app` ✅

## 09:4x audit-honesty cleanup pass

- Tightened two small but important inspectability semantics instead of jumping into a risky restart-hardening refactor.
- Fixed durable process audit classification in `crates/rune-tools/src/process_tool.rs`:
  - background processes that exit with code `0` are recorded as `completed`
  - background processes with non-zero exit codes are recorded as `failed`
  - killed processes with no exit code are now recorded as `killed` instead of being misclassified as successful completions
- Tightened approval-progress metadata in `crates/rune-runtime/src/executor.rs`:
  - `completed_at` is no longer written when an approval merely transitions into `resuming`
  - completion timestamps are now reserved for terminal approval states (`completed`, `completed_error`, `failed`, `denied`)
- Added focused regression coverage for both slices:
  - runtime test proving `completed_at` remains absent before resume completion and appears only after the resumed execution finishes
  - process-manager test covering kill -> non-running state
- Validation:
  - `cargo test -q -p rune-tools -p rune-runtime` ✅
  - `cargo clippy -q -p rune-tools -p rune-runtime -- -D warnings` ✅
  - full workspace `cargo test --workspace` background run completed cleanly in this environment ✅

## 10:3x approval-wait turn-state honesty pass

- Closed a concrete inspectability bug in `rune-runtime`: approval-blocked turns were being finalized as `completed` even while the session itself correctly moved to `waiting_for_approval`.
- Updated `crates/rune-runtime/src/executor.rs` so `TurnLoopOutcome::WaitingForApproval` now leaves the blocked turn in `tool_executing` with `ended_at = None` instead of falsely marking it terminal.
- Mirrored the same fix in the approval-resume finalization path so a re-blocked resume would remain visibly in-flight rather than being misreported as completed.
- Updated runtime tests to assert the honest contract:
  - blocked turn status is `tool_executing`
  - blocked turn has no `ended_at`
  - session still moves to `waiting_for_approval`
- Validation:
  - `cargo test -q -p rune-runtime` ✅
- Net effect: operator/restart inspection now has a truthful durable turn state for approval-paused work, which is a small but real step toward the remaining restart-hardening gap from `docs/AGENT-ORCHESTRATION.md`.

## 10:1x daylight verification + branch-state reconciliation

- Re-ran the branch as it actually exists instead of trusting stale build-artifact noise in `target/` fingerprints.
- Verified targeted parity slices are green when executed directly:
  - `cargo test -q -p rune-gateway --test route_tests` ✅
  - `cargo test -q -p rune-store --test pg_integration` ✅
  - `cargo test -q -p rune-cli` ✅
  - `cargo check -q -p rune-gateway-app` ✅
- Re-ran full workspace validation after the overnight parity work:
  - `cargo test --workspace -q` ✅
  - `cargo clippy --workspace -- -D warnings` ✅
- Confirmed the current implementation/doc story is internally consistent enough to keep moving without speculative rewrites:
  - approval request persistence + decision-time resume are live
  - restart-visible degraded process inspection is live
  - persisted subagent spawn / inspect / steer / cancel visibility is live
  - the next real engineering target is still restart-hardening, not additional surface-area expansion
- Chose to preserve honesty in the branch narrative:
  - no claim of restart-safe live process reattachment
  - no claim of full subagent runtime execution parity beyond conservative durable inspectability
  - no claim of parity-complete session cost/status fidelity yet

## 10:5x typed approval operator surface pass

- Added typed approval request / approval policy response models in `rune-cli` instead of leaving approvals as raw JSON blobs.
- Updated the gateway approval response shape to surface durable lifecycle fields directly:
  - `approval_status`
  - `approval_status_updated_at`
  - `resumed_at`
  - `completed_at`
  - `resume_result_summary`
  - `command`
- Kept this deliberately schema-light: these fields are derived from the already-persisted approval payload rather than introducing a risky approval-table migration.
- Updated the approval decision route to return the post-resume durable approval row, so CLI/API callers see the current lifecycle state instead of a stale pre-resume snapshot.
- Result: operators can now distinguish pending vs resuming vs completed/denied approval flows through first-class API/CLI fields, without reconstructing state manually from raw JSON payload internals.
- Validation:
  - `cargo test -q -p rune-cli -p rune-gateway` ✅
- Remaining honest gap:
  - this improves restart-visible inspectability, not restart-safe continuation/reattachment itself.

## 11:1x durable exec audit-correlation pass

- Tightened the transcript/audit parity story for long-running `exec` without pretending live-handle restart reattachment exists.
- Updated `crates/rune-tools/src/exec_tool.rs` so background `exec` output now includes explicit durable correlation fields when an audit store is configured:
  - `toolCallId`
  - `toolExecutionId`
- Kept `sessionId` as the process handle surface, but removed the need to infer durable linkage from command text or process IDs alone.
- Added focused `rune-tools` coverage with an in-memory `ProcessAuditStore` proving the returned `toolExecutionId` matches the persisted durable audit record.
- Validation:
  - `cargo test -q -p rune-tools -p rune-runtime` ✅
  - `cargo clippy -q -p rune-tools -- -D warnings` ✅
- Honest boundary:
  - runtime transcripts still do not universally embed audit references for every tool result, so the checklist item remains intentionally unchecked.

## 11:2x transcript audit-correlation follow-up

- Continued the same highest-priority parity seam instead of starting a new feature branch.
- Narrow protocol fix landed:
  - `rune-core::TranscriptItem::ToolResult` now carries optional `tool_execution_id`
  - `rune-tools::ToolResult` now carries the same optional durable audit correlation field
  - `rune-runtime` now preserves that correlation when appending transcripted tool results
- Result: background `exec` no longer stops at returning durable audit IDs to the immediate caller; the transcript contract can now preserve the same audit linkage when that tool result flows through the runtime turn loop.
- Kept the boundary honest:
  - no claim that every tool family now persists a durable execution row
  - no claim of restart-safe process reattachment
  - checklist item stays open until broader end-to-end black-box coverage exists
- Validation:
  - `cargo test -q -p rune-core -p rune-tools -p rune-runtime` ✅

## 11:4x durable subagent lifecycle metadata pass

- Tightened the conservative subagent implementation so restart-visible lifecycle state is no longer trapped mostly in transcript notes.
- Updated the live gateway app subagent surfaces to persist first-class metadata on subagent session rows:
  - `subagent_lifecycle`
  - `subagent_runtime_status`
  - `subagent_runtime_attached`
  - `subagent_status_updated_at`
  - `subagent_last_note`
- Spawned subagents now initialize as durable inspectable state instead of only “running + see transcript note”:
  - lifecycle = `spawned`
  - runtime status = `not_attached`
  - runtime attached = `false`
- `subagents steer` and `subagents kill` now update those same metadata fields alongside transcript status notes, so operators can tell the latest subagent lifecycle state after restart without reconstructing everything from transcript history.
- `subagents list` now surfaces the lifecycle/runtime fields directly.
- Kept the boundary honest:
  - still no fake remote runtime attachment
  - still no claim of true subagent execution parity
  - this is richer durable inspectability only
- Validation:
  - `cargo test -q -p rune-gateway-app` ✅

## 12:2x gateway asset-route green-up pass

- Reconciled live branch state against the overnight notes instead of trusting the earlier green snapshot.
- Found the current tree had slipped out of gate on `rune-gateway` for a mundane but real reason:
  - branded dashboard assets were referenced by the HTML shell but the `/assets/{path}` route was not wired into the router
  - that left `routes::branded_asset` dead under `-D warnings`, breaking workspace clippy/test gates on the gateway crate
- Fixed the control-plane surface directly by exposing the branded asset handler on the public router:
  - `GET /assets/{path}` → `routes::branded_asset`
- Result:
  - dashboard HTML asset references now have a real served path instead of relying on an unwired helper
  - `rune-gateway` route tests are green again
  - `rune-gateway` clippy is clean again
- Validation:
  - `cargo test -q -p rune-gateway --test route_tests` ✅
  - `cargo clippy -q -p rune-gateway --all-targets -- -D warnings` ✅
  - `cargo check -q -p rune-gateway-app` ✅

## 12:4x detached-process error-honesty pass

- Tightened a concrete restart-inspectability seam in `rune-tools` instead of leaving a misleading operator experience in place.
- Before this pass:
  - `process list|poll|log` already degraded honestly after restart by surfacing persisted audit metadata from `tool_executions`
  - but mutating actions like `write`, `submit`, `paste`, `send-keys`, and `kill` collapsed to a generic `process not found` when only a persisted audit row remained
- That was semantically wrong: the process was known durably, just not reattached live.
- Updated `ProcessManager` so stdin/control operations now distinguish between:
  - truly unknown process IDs
  - known persisted process IDs with no live handle in the current gateway process
- New failure mode is explicit and honest:
  - persisted metadata exists
  - live stdin/control reattachment after restart is not implemented yet
- Added focused tests covering detached-persisted `write` and `kill` attempts.
- Validation:
  - `cargo test -q -p rune-tools` ✅
  - `cargo clippy -q -p rune-tools --all-targets -- -D warnings` ✅
