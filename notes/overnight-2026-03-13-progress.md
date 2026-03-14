# Overnight Progress — 2026-03-13

Execution authority: `docs/AGENT-ORCHESTRATION.md`

## What was verified

- Existing repo baseline is real, not planning-only.
- Workspace/crate graph already matches Wave 0 expectations.
- `cargo check` passes.
- `cargo test` passes.
- `cargo clippy --workspace --all-targets -- -D warnings` passes.
- Embedded PostgreSQL fallback is working in tests and app wiring.

## Doc reconciliation findings

Primary authority remains `docs/AGENT-ORCHESTRATION.md`.

Safe reconciliations identified during execution:
- `docs/CRATE-LAYOUT.md` is explicitly an earlier planning draft and should not be treated as sequencing authority.
- `docs/IMPLEMENTATION-PHASES.md` contains older phase framing, but its current implementation evidence sections are still useful as status notes.
- `docs/FUNCTIONALITY-CHECKLIST.md` is the practical parity tracker and is currently the most accurate status surface besides the code.

No hard architecture conflict found yet that requires escalation.

## Concrete code change completed this pass

### Persist turn usage totals durably

Problem:
- turn usage was returned to callers but not persisted back into the `turns` table after execution.
- this weakened later parity work for status/usage-cost/operator inspection.

Change made:
- added `TurnRepo::update_usage(...)`
- implemented it in `PgTurnRepo`
- updated `TurnExecutor` to persist prompt/completion token totals even for failed turns before final status update
- extended runtime and store tests to verify persisted usage values

Files touched:
- `crates/rune-store/src/repos.rs`
- `crates/rune-store/src/pg.rs`
- `crates/rune-runtime/src/executor.rs`
- `crates/rune-runtime/src/tests.rs`
- `crates/rune-store/tests/pg_integration.rs`

Validation run:
- targeted runtime/store tests passed
- `cargo check` passed after the change

### Persist heartbeat state across gateway restarts

Problem:
- `HeartbeatState` claimed to represent persisted runner state, but the active gateway wiring used an in-memory `HeartbeatRunner::new(...)` only.
- enable/disable state, last run timestamp, run count, and suppression count were lost on restart.

Change made:
- added file-backed `HeartbeatRunner::with_state_file(...)`
- heartbeat state now auto-loads from JSON on startup and rewrites on enable/disable/interval changes/ticks/suppression updates
- gateway binary now stores heartbeat state under `paths.logs_dir/heartbeat-state.json`

Files touched:
- `crates/rune-runtime/src/heartbeat.rs`
- `apps/gateway/src/main.rs`

Validation status:
- added restart-persistence coverage with `state_file_persists_across_runner_reloads`
- `cargo test -p rune-runtime heartbeat -- --nocapture` passed
- `cargo check` passed after the change

## Additional concrete code change completed this pass

### Persist cron run history durably

Problem:
- cron job definitions were already persisted, but `cron runs` history still lived only in process memory.
- gateway restart dropped execution history, weakening parity for operator auditability and restart durability.

Change made:
- added durable `job_runs` storage with a new Diesel migration
- added `JobRunRow` / `NewJobRun` models plus `JobRunRepo` and `PgJobRunRepo`
- updated `rune-runtime::scheduler::Scheduler` to persist run start/completion and read `cron runs` from the durable repo when available
- updated gateway production wiring to use `Scheduler::new_with_repos(...)`
- added PostgreSQL integration coverage for create/complete/list job-run flows

Files touched:
- `crates/rune-store/migrations/2026-03-14-000002_add_job_runs/up.sql`
- `crates/rune-store/migrations/2026-03-14-000002_add_job_runs/down.sql`
- `crates/rune-store/src/schema.rs`
- `crates/rune-store/src/models.rs`
- `crates/rune-store/src/repos.rs`
- `crates/rune-store/src/pg.rs`
- `crates/rune-store/src/lib.rs`
- `crates/rune-store/tests/pg_integration.rs`
- `crates/rune-runtime/src/scheduler.rs`
- `apps/gateway/src/main.rs`

Validation status:
- targeted `rune-store` integration test path passed
- workspace `cargo check` passed
- workspace `cargo clippy --workspace -- -D warnings` passed

## Additional concrete code change completed this pass

### Separate scheduled main-session vs isolated-run semantics

Problem:
- scheduled `main` and `isolated` agent-turn jobs still executed through effectively the same session path.
- this blurred parity-critical semantics and weakened auditability for descendant scheduled runs.

Change made:
- introduced `SessionEngine::get_session_by_channel_ref(...)` for restart-safe lookup of stable channel/scheduler sessions
- updated gateway supervisor execution so scheduled `main` work reuses a stable `system:scheduled-main` scheduled session
- updated scheduled `isolated` work to create a fresh `subagent` session linked back to that stable main scheduled session via `requester_session_id`
- kept reminders and system events on the stable main scheduled session instead of creating a fresh scheduled session every time
- extended runtime tests to verify the stable scheduled-main lookup and isolated descendant linkage model

Files touched:
- `crates/rune-runtime/src/engine.rs`
- `crates/rune-runtime/src/tests.rs`
- `crates/rune-gateway/src/supervisor.rs`

Validation status:
- targeted `rune-runtime` and `rune-gateway` tests passed

## Highest-leverage remaining execution gaps observed

1. **Heartbeat durability gap**
   - file-backed state now exists for runner status/counters, but heartbeat run history and richer suppression/no-op evidence are not yet durable job records.

3. **Scheduled isolated run semantics are only partially real**
   - supervisor executes jobs, but broader gateway cron storage/execution still needs to unify around the durable store path.

4. **Session status parity surface is still shallow**
   - usage is now stored per turn, but `/status` / `session_status` parity still needs a real aggregate read model (model, usage, cost, flags, timing).

5. **Approval persistence is partial**
   - tool-level allow-always/deny persistence exists, but exact-call allow-once lifecycle remains runtime-local rather than durable.

## Recommended next slice

If continuing immediately, prioritize this order:

1. introduce durable scheduler/job repositories into gateway/supervisor wiring
2. persist reminder state under the same restart-safe model
3. route manual cron run requests through the same execution path as due jobs
4. build a persisted session-status aggregate endpoint/tool surface on top of stored turns + session metadata

## Additional concrete code change completed this pass

### Unify manual and due cron execution lifecycle

Problem:
- due jobs in the background supervisor and manual `POST /cron/{id}/run` requests executed through duplicated lifecycle logic.
- that invited semantic drift in run history persistence, status recording, next-run advancement, and future scheduler changes.

Change made:
- introduced shared gateway supervisor helper `run_job_lifecycle(...)`
- moved manual cron execution route to reuse the same lifecycle path as due jobs
- kept manual-run event emission in the route layer while centralizing actual scheduler state transitions in one place
- added gateway route coverage for `cron run` -> durable run history -> incremented run metadata

Files touched:
- `crates/rune-gateway/src/supervisor.rs`
- `crates/rune-gateway/src/routes.rs`
- `crates/rune-gateway/src/lib.rs`
- `crates/rune-gateway/tests/route_tests.rs`

Validation status:
- targeted `rune-gateway` test `cron_run_executes_and_records_run_history` passed
- `cargo check --workspace` passed
- `cargo clippy -p rune-gateway -p rune-cli --tests -- -D warnings` passed

### Fix CLI session detail channel-field parity drift

Problem:
- gateway session detail responses expose `channel_ref`, but the CLI client was reading `channel` only.
- operator `rune sessions show` therefore silently lost channel/routing visibility for valid gateway responses.

Change made:
- updated CLI session-detail parsing to accept `channel` or fallback to `channel_ref`
- repaired affected output test fixture drift after recent session aggregate fields were added
- added direct unit coverage for `channel_ref` parsing

Files touched:
- `crates/rune-cli/src/client.rs`
- `crates/rune-cli/src/output.rs`

Validation status:
- targeted `rune-cli` tests passed
- shared clippy pass above stayed clean

## Constraint reminder

Do not expand scope into speculative frontend/channel/plugin work from this thread until the durability/status parity path above is closed further.
