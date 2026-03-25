# Execution Engine — Design Spec

**Date:** 2026-03-25
**Goal:** Make Rune reliably autonomous — shipping code, reporting proactively, and self-updating — without depending on model prompt compliance.

## Problem Statement

Rune's agent stops to explain instead of executing, cron jobs were broken (now fixed), API calls grow unbounded per turn, jobs run sequentially blocking each other, Mem0 adds flaky latency, sessions leak, and there's no proactive delivery to Telegram. The result: Rune talks about work instead of doing it.

OpenClaw solved this with transcript pruning, queue awareness, duplicate suppression, and a tight heartbeat loop. Rune needs structural equivalents.

## Design

### 1. Wire Up Compaction

**Files:** `apps/gateway/src/main.rs`, `config.toml`

`TokenBudgetCompaction` already exists at `crates/rune-runtime/src/compaction.rs:255`. It estimates tokens per message, preserves a configurable tail of recent messages, and summarizes everything older into a single system message. It also supports flushing compacted summaries to daily memory files via `with_memory_flush()`.

**Changes:**
- In `apps/gateway/src/main.rs:594`, replace `Arc::new(NoOpCompaction)` with `TokenBudgetCompaction::new(context_window, preserve_tail).with_memory_flush(&workspace_root)`
- Default `context_window = 128_000` tokens (gpt-5.4), `preserve_tail = 20` messages
- Add `[runtime]` section to config schema:
  ```toml
  [runtime]
  context_window = 128000
  preserve_tail = 20
  ```
- Parse these in the gateway startup and pass to `TokenBudgetCompaction::new()`

**Effect:** API request bodies stay bounded at ~40-50KB instead of growing to 180KB+. Each tool iteration stays fast.

### 2. Parallel Cron Execution

**Files:** `crates/rune-gateway/src/supervisor.rs`

**Current:** Lines 331-336 run a sequential `for job in due_jobs { await run_job_lifecycle(...) }`. One slow job (roadmap-worker at ~2min) blocks email-check (3 seconds).

**Changes:**
- Replace the sequential for-loop with `tokio::spawn` per job
- Each spawned task owns its own copy of `SupervisorDeps` (it's already `Arc`-wrapped)
- The supervisor loop continues to the next tick without waiting
- The lane queue (`cron=0/1024`) already enforces concurrency — no additional limiting needed
- Each spawned task calls `release_claim` and `advance_next_run` on completion, same as today

**Sketch:**
```rust
let due_jobs = deps.scheduler.claim_due_jobs(CLAIM_LEASE_SECS).await;
for job in due_jobs {
    let deps = deps.clone();
    tokio::spawn(async move {
        let job_id = job.id;
        let _ = run_job_lifecycle(&deps, &job, true, SchedulerRunTrigger::Due).await;
        deps.scheduler.release_claim(&job_id).await;
    });
}
```

**Effect:** Fast jobs (email-check: 3s) aren't blocked by slow jobs (roadmap-worker: 2min). All jobs fire at their scheduled time.

### 3. Proactive Telegram Delivery

**Files:** `crates/rune-gateway/src/supervisor.rs`

**Current:** `deliver_result()` at line 195 calls `deliver_result_standalone()` which posts a `SessionEvent` to a broadcast channel. But for cron jobs with `delivery_mode = "none"`, nothing reaches Telegram.

**Changes:**
- In `deliver_result_standalone()`, when `delivery_mode` is `"announce"` and the job completed successfully, format the output and send it to the operator's Telegram chat
- The Telegram adapter is already initialized in the gateway. Pass a reference to the channel sender (or the operator delivery handle already used by heartbeat) into `SupervisorDeps`
- Update the 3 existing cron jobs' `delivery_mode` from `"none"` to `"announce"`:
  - `roadmap-worker`: reports what it shipped (commit hashes, files changed)
  - `email-calendar-check`: reports urgent items only (already in its prompt — "only notify if urgent")
  - `github-hygiene`: reports closed issues, status updates
- For failed jobs, also announce the failure so Hamza knows something broke

**Effect:** Rune proactively messages on Telegram when it ships code or finds something important — matching OpenClaw's behavior.

### 4. Skip Mem0 for Cron/Subagent Sessions

**Files:** `crates/rune-runtime/src/executor.rs`

**Current:** Lines 629-658 run Mem0 recall (embedding API call to Azure + vector query to Cosmos DB PG) before the first model call in every turn. For cron jobs, this adds latency and fails intermittently ("connection closed").

**Changes:**
- In `run_turn_loop()`, gate the Mem0 recall block on session kind:
  ```rust
  let mem0_prompt_section = if let Some(ref mem0) = self.mem0 {
      if matches!(session_kind, SessionKind::Scheduled | SessionKind::Subagent) {
          None  // skip for ephemeral sessions
      } else {
          // existing recall logic
      }
  };
  ```
- Mem0 capture (writing memories after a turn) can remain for all session types — it's the recall that's expensive and flaky

**Effect:** Cron jobs start faster (skip ~1-2s embedding + PG round-trip) and don't fail on flaky connections.

### 5. Session Hygiene

**Files:** `crates/rune-gateway/src/supervisor.rs`, `crates/rune-runtime/src/session_loop.rs` or session engine

**Current:** 170 scheduled sessions, 49 stuck in "running" forever. Each `isolated` cron run creates a new subagent session that's never cleaned up.

**Changes:**
- In `run_agent_turn()` for `SessionTarget::Isolated`: after `execute_in_session()` completes, mark the session as `completed` (this is already partially done — the `complete_when_done` flag exists at line 447 but needs verification)
- Add a stale session cleanup to the supervisor loop (run every 10 ticks = ~100s):
  - Find sessions with `status = 'running'` and `last_activity_at` older than 1 hour
  - Mark them as `completed`
- This is hygiene only — it doesn't affect execution correctness

**Effect:** DB stays clean, queries on sessions table stay fast.

### 6. Self-Update Loop

**Files:** `crates/rune-gateway/src/supervisor.rs`, cron job prompt

**Current:** `scripts/self-update.sh` exists with safety guards (build → smoke test → vite build → restart). The agent never calls it.

**Changes:**
- In the roadmap-worker cron prompt, add an explicit instruction: "If you pushed changes to ~/Development/rune, run `~/Development/rune/scripts/self-update.sh` as your last step before ending the turn."
- The self-update script already handles: cargo build failure (aborts), smoke test failure (aborts), and clean restart via systemctl
- No code change needed in Rune itself — this is a prompt-level instruction. The agent has `exec` tool access and `--yolo` approval mode.
- Future improvement: detect rune-repo pushes programmatically in the supervisor and auto-trigger self-update. But the prompt approach works now.

**Effect:** When Rune ships changes to its own codebase, it deploys them. Matches OpenClaw's self-update behavior.

### 7. Smarter Cron Prompts (Last-Run Context)

**Files:** `crates/rune-gateway/src/supervisor.rs`

**Current:** Each cron tick sends the same static message string. The agent starts fresh every 5 minutes with no memory of what the last run did.

**Changes:**
- Before constructing the agent turn message for a cron job, fetch the last completed `job_run` output for that job ID
- Prepend it to the message as context:
  ```
  ## Last run result (5 minutes ago):
  {last_output, truncated to 2000 chars}

  ## Your task:
  {original cron message}
  ```
- This gives the model continuity without persistent session state
- Truncate last_output to 2000 chars to avoid context bloat
- If no previous run exists or the last run failed, just send the original message

**Implementation location:** In `execute_job()` or `run_agent_turn()` in `supervisor.rs`, before calling `execute_in_session()`.

**Effect:** The agent knows what it did last time and can pick up where it left off, instead of re-reading the entire roadmap every 5 minutes.

## Files Changed (Summary)

| File | Changes |
|------|---------|
| `apps/gateway/src/main.rs` | Wire `TokenBudgetCompaction`, parse `[runtime]` config |
| `crates/rune-gateway/src/supervisor.rs` | Parallel cron spawn, Telegram delivery, session cleanup, last-run context |
| `crates/rune-runtime/src/executor.rs` | Skip Mem0 for scheduled/subagent sessions |
| `config.toml` | Add `[runtime]` section, update cron `delivery_mode` |
| Cron job prompts (DB update) | Add self-update instruction to roadmap-worker |

## Out of Scope

- Streaming API responses (significant change to model provider layer)
- Multi-step execution planner (Approach C — defer unless needed)
- New compaction strategy beyond `TokenBudgetCompaction`
- Transcript windowing (compaction handles this at the message level)

## Success Criteria

1. Cron jobs run in parallel — email-check completes in <10s regardless of roadmap-worker state
2. API request bodies stay under 60KB throughout a 200-iteration turn
3. Roadmap-worker ships at least 1 commit per run on average
4. Shipped commits are proactively reported on Telegram
5. After pushing rune changes, the agent self-updates and restarts
6. No "connection closed" errors from Mem0 during cron execution
7. Stale sessions are cleaned up automatically
