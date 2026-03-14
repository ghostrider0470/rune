# Afternoon verification — 2026-03-14

## Scope

Reconciled stale docs against the live runtime path before starting any speculative new implementation slice.

## What was verified in code

- `rune-runtime` already wires `WorkspaceLoader` into the live turn path (`crates/rune-runtime/src/executor.rs`).
- Session-kind-specific startup file loading is already real, not planned-only:
  - direct/channel/subagent sessions load standard workspace context files
  - scheduled sessions additionally load `HEARTBEAT.md`
- `rune-runtime` already wires `MemoryLoader` into the live turn path.
- The curated-memory privacy boundary is already enforced in code:
  - `SessionKind::Direct` loads `MEMORY.md` + daily notes
  - `SessionKind::Channel`, `Subagent`, and `Scheduled` exclude `MEMORY.md` and load only daily notes
- Runtime tests already prove these behaviors in the prompt actually sent to the model:
  - `direct_session_prompt_includes_workspace_and_memory_context`
  - `channel_session_prompt_excludes_long_term_memory`

## Repo updates made

- Marked the following checklist items as complete in `docs/FUNCTIONALITY-CHECKLIST.md` with concrete evidence notes:
  - startup file loading rules by session type
  - main-session-only curated-memory boundary
  - main-session-only privacy boundary for curated memory
- Removed a few stale crate/route doc comments that still described live surfaces as skeletons/placeholders/stubs even though the repo has moved beyond that state.

## Why this matters

The current branch already contains more live parity behavior than some docs/checklists were admitting. Fixing that drift reduces the risk of future agents redoing implemented work or regressing back into planning-mode assumptions.

## Next likely execution targets

Highest-value remaining non-speculative gaps still align with `docs/AGENT-ORCHESTRATION.md`:
1. restart-safe continuation/inspectability across gateway restarts
2. richer subagent lifecycle/runtime parity beyond durable inspectability
3. deeper session-status parity quality
4. broader host/node/sandbox parity
5. cross-platform PTY fidelity beyond current Unix coverage
