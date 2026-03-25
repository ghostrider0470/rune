# Execution Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Rune reliably autonomous — bounded context, parallel cron, proactive Telegram delivery, no Mem0 latency for crons, session cleanup, and last-run context injection.

**Architecture:** Seven independent changes to the execution pipeline. Each task is self-contained and can be committed independently. The compaction fix includes a critical bug-prevention change (system message preservation). Parallel cron uses `tokio::spawn` with the existing lane queue for concurrency control. Telegram delivery reuses the existing `OperatorDelivery` trait already used by heartbeat.

**Tech Stack:** Rust, tokio, SQLite, Telegram Bot API

---

### Task 1: Fix Compaction System Message Sweep Bug

The existing `TokenBudgetCompaction::compact()` sweeps ALL messages before the preserved tail into a summary — including the system prompt at index 0. This would destroy system instructions, workspace context, and memory. Fix this before wiring compaction.

**Files:**
- Modify: `crates/rune-runtime/src/compaction.rs:255-292`
- Test: `crates/rune-runtime/src/compaction.rs` (inline tests)

- [ ] **Step 1: Write failing test for system message preservation**

Add this test to the `mod tests` block in `crates/rune-runtime/src/compaction.rs`:

```rust
#[test]
fn compact_preserves_system_message_at_index_zero() {
    let compaction = TokenBudgetCompaction::new(100, 2);
    let mut messages = vec![msg(Role::System, "You are a helpful assistant. This is the system prompt.")];
    for i in 0..30 {
        messages.push(msg(
            Role::User,
            &format!("message {} with padding to inflate token count for compaction", i),
        ));
    }
    let result = compaction.compact(messages);
    // Should have: original system msg + summary + 2 preserved tail = 4
    assert!(result.len() >= 3);
    assert_eq!(result[0].role, Role::System);
    assert!(
        result[0].content.as_ref().unwrap().contains("helpful assistant"),
        "original system message must be preserved, got: {}",
        result[0].content.as_ref().unwrap()
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rune-runtime compact_preserves_system_message -- --nocapture`
Expected: FAIL — the system message gets replaced by the summary.

- [ ] **Step 3: Fix compact() to preserve system message at index 0**

In `crates/rune-runtime/src/compaction.rs`, replace the `compact` method (lines 256-292) with:

```rust
fn compact(&self, messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    let total_tokens: usize = messages.iter().map(Self::estimate_tokens).sum();
    let threshold = (self.context_window * 80) / 100;

    if total_tokens <= threshold {
        return messages;
    }

    let len = messages.len();
    if len <= self.preserve_tail {
        return messages;
    }

    // Always preserve the system message at index 0 (contains system prompt,
    // workspace context, memory, skills). Only compact user/assistant/tool
    // messages between the system prompt and the preserved tail.
    let has_system_prefix = messages
        .first()
        .is_some_and(|m| m.role == Role::System);

    let compactable_start = if has_system_prefix { 1 } else { 0 };
    let compactable = &messages[compactable_start..];

    if compactable.len() <= self.preserve_tail {
        return messages;
    }

    let split_at = compactable.len() - self.preserve_tail;
    let (old, recent) = compactable.split_at(split_at);

    // Memory flush: persist a structured summary before dropping messages
    if let Some(workspace_root) = &self.workspace_root {
        let flush_note = Self::build_flush_note(old);
        Self::flush_to_memory(workspace_root, &flush_note);
    }

    let summary_text = Self::summarize(old);
    let summary_msg = ChatMessage {
        role: Role::System,
        content: Some(summary_text),
        name: None,
        tool_call_id: None,
        tool_calls: None,
    };

    let mut result = Vec::with_capacity(2 + recent.len());
    if has_system_prefix {
        result.push(messages[0].clone());
    }
    result.push(summary_msg);
    result.extend_from_slice(recent);
    result
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p rune-runtime compact_preserves_system_message -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run all existing compaction tests**

Run: `cargo test -p rune-runtime compaction -- --nocapture`
Expected: ALL PASS (the existing `over_budget_compacts_old_messages` test may need its assertion updated from `len() == 3` to `len() == 4` since we now keep system + summary + 2 tail)

- [ ] **Step 6: Fix any broken assertions in existing tests**

The `over_budget_compacts_old_messages` test (line 326) asserts `result.len() == 3`. With the fix, messages that don't start with a System message still produce 3 (summary + 2 tail), but messages starting with System produce 4. The existing test uses `Role::User` messages only (no system prefix), so it should still pass as-is. Verify and fix if needed.

The `compaction_flushes_to_daily_memory` test (line 419) also asserts `result.len() == 3` with User-only messages. Same — should pass.

- [ ] **Step 7: Commit**

```bash
git add crates/rune-runtime/src/compaction.rs
git commit -m "fix(runtime): preserve system message during compaction

TokenBudgetCompaction.compact() was sweeping the system prompt (index 0)
into the summary when messages exceeded the token budget. This would
destroy system instructions, workspace context, and memory sections.

Now always preserves the system message at index 0 and only compacts
user/assistant/tool messages between the system prompt and the preserved
tail."
```

---

### Task 2: Add Compaction Config Fields

**Files:**
- Modify: `crates/rune-config/src/lib.rs:530-535`

- [ ] **Step 1: Add CompactionConfig to RuntimeConfig**

In `crates/rune-config/src/lib.rs`, add a `CompactionConfig` struct and a field on `RuntimeConfig`:

```rust
/// Compaction controls for context window management.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionConfig {
    /// Model context window size in tokens. Default: 128000.
    #[serde(default = "default_context_window")]
    pub context_window: usize,
    /// Number of recent messages to always preserve verbatim. Default: 20.
    #[serde(default = "default_preserve_tail")]
    pub preserve_tail: usize,
}

fn default_context_window() -> usize { 128_000 }
fn default_preserve_tail() -> usize { 20 }

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            context_window: default_context_window(),
            preserve_tail: default_preserve_tail(),
        }
    }
}
```

Then add to `RuntimeConfig`:

```rust
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeConfig {
    #[serde(default)]
    pub lanes: LaneQueueConfig,
    #[serde(default)]
    pub compaction: CompactionConfig,
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p rune-config`
Expected: success

- [ ] **Step 3: Commit**

```bash
git add crates/rune-config/src/lib.rs
git commit -m "feat(config): add compaction settings to [runtime] section"
```

---

### Task 3: Wire TokenBudgetCompaction in Gateway

**Files:**
- Modify: `apps/gateway/src/main.rs:592-594`

- [ ] **Step 1: Replace NoOpCompaction with TokenBudgetCompaction**

In `apps/gateway/src/main.rs`, change the import to include `TokenBudgetCompaction`:

```rust
use rune_runtime::{
    ContextAssembler, LaneQueue, Mem0Engine, NoOpCompaction, SessionEngine, TelegramFileDownloader,
    TurnExecutor, TokenBudgetCompaction,
    // ... rest of imports
};
```

Then at line ~592, replace `Arc::new(NoOpCompaction)` with:

```rust
    let compaction: Arc<dyn rune_runtime::CompactionStrategy> = Arc::new(
        TokenBudgetCompaction::new(
            config.runtime.compaction.context_window,
            config.runtime.compaction.preserve_tail,
        )
        .with_memory_flush(&workspace_root),
    );

    let mut turn_executor = TurnExecutor::new(
        session_repo.clone(),
        turn_repo.clone(),
        transcript_repo.clone(),
        approval_repo.clone(),
        model_provider.clone(),
        tool_executor,
        tool_registry,
        ContextAssembler::new(&system_prompt),
        compaction,
    );
```

- [ ] **Step 2: Build the full gateway**

Run: `cargo build --release --bin rune-gateway`
Expected: success

- [ ] **Step 3: Commit**

```bash
git add apps/gateway/src/main.rs
git commit -m "feat(runtime): wire TokenBudgetCompaction in gateway

Replaces NoOpCompaction with TokenBudgetCompaction using config values
from [runtime.compaction]. Enables memory flush to daily notes so
compacted context is not permanently lost."
```

---

### Task 4: Parallel Cron Execution

**Files:**
- Modify: `crates/rune-gateway/src/supervisor.rs:329-336`

- [ ] **Step 1: Replace sequential cron loop with tokio::spawn**

In `crates/rune-gateway/src/supervisor.rs`, replace lines 329-336:

```rust
        // --- Scheduled jobs (durable claim prevents duplicate execution) ---
        let due_jobs = deps.scheduler.claim_due_jobs(CLAIM_LEASE_SECS).await;
        for job in due_jobs {
            let job_id = job.id;
            debug!(job_id = %job_id, name = ?job.name, "executing claimed job");
            let _ = run_job_lifecycle(&deps, &job, true, SchedulerRunTrigger::Due).await;
            deps.scheduler.release_claim(&job_id).await;
        }
```

With:

```rust
        // --- Scheduled jobs (spawned in parallel, claim lease is the safety net) ---
        let due_jobs = deps.scheduler.claim_due_jobs(CLAIM_LEASE_SECS).await;
        for job in due_jobs {
            let deps = deps.clone();
            tokio::spawn(async move {
                let job_id = job.id;
                debug!(job_id = %job_id, name = ?job.name, "executing claimed job");
                let _ = run_job_lifecycle(&deps, &job, true, SchedulerRunTrigger::Due).await;
                deps.scheduler.release_claim(&job_id).await;
            });
        }
```

- [ ] **Step 2: Build and verify**

Run: `cargo build -p rune-gateway`
Expected: success. `run_job_lifecycle` and `deps` types are already `Send + 'static`.

- [ ] **Step 3: Commit**

```bash
git add crates/rune-gateway/src/supervisor.rs
git commit -m "feat(supervisor): parallelize cron job execution

Spawn each claimed cron job as an independent tokio task instead of
running sequentially. Fast jobs (email-check: 3s) are no longer blocked
by slow jobs (roadmap-worker: 2min). The lane queue (cron=1024) enforces
concurrency limits. The 300s claim lease is the safety net if a task
panics before release_claim."
```

---

### Task 5: Proactive Telegram Delivery for Cron Jobs

**Files:**
- Modify: `crates/rune-gateway/src/supervisor.rs:195-202` and `206-272`

- [ ] **Step 1: Pass operator_delivery into deliver_result and use it for Announce mode**

In `crates/rune-gateway/src/supervisor.rs`, change `deliver_result` to pass the operator delivery:

```rust
async fn deliver_result(
    deps: &SupervisorDeps,
    job: &Job,
    status: &rune_runtime::scheduler::JobRunStatus,
    output: &str,
    trigger: SchedulerRunTrigger,
) {
    deliver_result_standalone(
        &deps.event_tx,
        deps.operator_delivery.as_deref(),
        job,
        status,
        output,
        trigger,
    )
    .await;
}
```

Then update `deliver_result_standalone` signature and `Announce` arm:

```rust
async fn deliver_result_standalone(
    event_tx: &broadcast::Sender<SessionEvent>,
    operator_delivery: Option<&dyn OperatorDelivery>,
    job: &Job,
    status: &rune_runtime::scheduler::JobRunStatus,
    output: &str,
    trigger: SchedulerRunTrigger,
) {
    match job.delivery_mode {
        SchedulerDeliveryMode::None => {
            // Silent execution — no additional delivery.
        }
        SchedulerDeliveryMode::Announce => {
            // Broadcast to WebSocket subscribers (existing behavior)
            let _ = event_tx.send(SessionEvent {
                session_id: job.id.to_string(),
                kind: "cron_run_completed".to_string(),
                payload: json!({
                    "job_id": job.id.to_string(),
                    "job_name": job.name,
                    "delivery_mode": "announce",
                    "trigger": trigger.as_str(),
                    "status": status,
                    "output": output,
                }),
                state_changed: true,
            });

            // Also deliver to operator's Telegram (proactive updates)
            if let Some(delivery) = operator_delivery {
                let status_str = match status {
                    rune_runtime::scheduler::JobRunStatus::Completed => "completed",
                    rune_runtime::scheduler::JobRunStatus::Failed => "FAILED",
                    _ => "finished",
                };
                let truncated = if output.len() > 3000 {
                    &output[..3000]
                } else {
                    output
                };
                let msg = format!(
                    "*[{}]* {} ({})\n\n{}",
                    job.name.as_deref().unwrap_or("cron"),
                    status_str,
                    trigger.as_str(),
                    truncated,
                );
                if let Err(e) = delivery.deliver(&msg).await {
                    warn!(job_id = %job.id, error = %e, "operator delivery for cron result failed");
                }
            }

            debug!(job_id = %job.id, "announce delivery sent");
        }
        SchedulerDeliveryMode::Webhook => {
            // ... existing webhook code unchanged ...
```

- [ ] **Step 2: Fix any test compilation issues**

Check if `deliver_result_standalone` is called in tests. Search for it:

Run: `grep -rn "deliver_result_standalone" crates/rune-gateway/`

If called in tests, update the call sites to pass the new `operator_delivery` parameter as `None`.

- [ ] **Step 3: Build**

Run: `cargo build -p rune-gateway`
Expected: success

- [ ] **Step 4: Update existing cron jobs delivery_mode in DB**

```bash
python3 -c "
import sqlite3
conn = sqlite3.connect('/home/hamza/.rune/db/rune.db')
cur = conn.cursor()
cur.execute(\"UPDATE jobs SET delivery_mode = 'announce' WHERE job_type = 'cron' AND enabled = 1\")
print(f'Updated {cur.rowcount} jobs to announce delivery')
conn.commit()
conn.close()
"
```

- [ ] **Step 5: Commit**

```bash
git add crates/rune-gateway/src/supervisor.rs
git commit -m "feat(supervisor): deliver cron results to Telegram via operator_delivery

When delivery_mode is 'announce', cron job results are now sent to the
operator's Telegram chat using the existing OperatorDelivery trait
(same path heartbeat uses). Truncates output to 3000 chars to stay
within Telegram message limits."
```

---

### Task 6: Skip Mem0 Recall for Cron/Subagent Sessions

**Files:**
- Modify: `crates/rune-runtime/src/executor.rs:629-658`

- [ ] **Step 1: Gate Mem0 recall on session kind**

In `crates/rune-runtime/src/executor.rs`, around line 629, wrap the existing Mem0 recall block:

Replace:
```rust
        let mem0_prompt_section = if let Some(ref mem0) = self.mem0 {
```

With:
```rust
        let mem0_prompt_section = if let Some(ref mem0) = self.mem0 {
            if matches!(session_kind, SessionKind::Scheduled | SessionKind::Subagent) {
                debug!("skipping mem0 recall for ephemeral session kind={:?}", session_kind);
                None
            } else {
```

And close the extra `if` with an additional `}` before the final `else { None }` and closing braces. The structure becomes:

```rust
        let mem0_prompt_section = if let Some(ref mem0) = self.mem0 {
            if matches!(session_kind, SessionKind::Scheduled | SessionKind::Subagent) {
                debug!("skipping mem0 recall for ephemeral session kind={:?}", session_kind);
                None
            } else {
                // existing recall logic (lines 630-656 unchanged)
                let transcript_rows = self.transcript_repo.list_by_session(session_id).await?;
                let user_msg = transcript_rows
                    .iter()
                    .rev()
                    .find_map(|row| {
                        let item: rune_core::TranscriptItem =
                            serde_json::from_value(row.payload.clone()).ok()?;
                        if let rune_core::TranscriptItem::UserMessage { message } = item {
                            Some(message.content)
                        } else {
                            None
                        }
                    })
                    .unwrap_or_default();

                if !user_msg.is_empty() {
                    let memories = mem0.recall(&user_msg).await;
                    let section = Mem0Engine::format_for_prompt(&memories);
                    if section.is_empty() {
                        None
                    } else {
                        Some(section)
                    }
                } else {
                    None
                }
            }
        } else {
            None
        };
```

- [ ] **Step 2: Build and verify**

Run: `cargo build -p rune-runtime`
Expected: success

- [ ] **Step 3: Commit**

```bash
git add crates/rune-runtime/src/executor.rs
git commit -m "fix(runtime): skip Mem0 recall for scheduled and subagent sessions

Cron jobs and subagent sessions are ephemeral — semantic memory recall
adds 1-2s latency (embedding API + PG vector query) and fails
intermittently on flaky Azure Cosmos DB connections. Mem0 capture
(writing memories) still runs for all session types."
```

---

### Task 7: Session Hygiene — Stale Session Cleanup

**Files:**
- Modify: `crates/rune-gateway/src/supervisor.rs:274` (supervisor_loop)
- Modify: `crates/rune-store/src/repos.rs` (add trait method)
- Modify: `crates/rune-store/src/sqlite/mod.rs` (implement)

- [ ] **Step 1: Add `mark_stale_completed` to SessionRepo trait**

In `crates/rune-store/src/repos.rs`, add to the `SessionRepo` trait:

```rust
    /// Mark sessions stuck in 'running' with no activity for over `stale_secs` as completed.
    async fn mark_stale_completed(&self, stale_secs: i64) -> Result<u64, StoreError>;
```

- [ ] **Step 2: Implement in SQLite**

In `crates/rune-store/src/sqlite/mod.rs`, add the implementation to `SqliteSessionRepo`:

```rust
    async fn mark_stale_completed(&self, stale_secs: i64) -> Result<u64, StoreError> {
        let cutoff = (Utc::now() - chrono::Duration::seconds(stale_secs))
            .format("%Y-%m-%dT%H:%M:%S%.6fZ")
            .to_string();
        self.conn
            .call(move |conn| {
                let count = conn.execute(
                    "UPDATE sessions SET status = 'completed' WHERE status = 'running' AND last_activity_at < ?1",
                    [&cutoff],
                )?;
                Ok(count as u64)
            })
            .await
            .map_err(StoreError::from)
    }
```

- [ ] **Step 3: Implement in PG (if postgres feature exists)**

Check if `crates/rune-store/src/pg.rs` has a `SessionRepo` impl. If so, add:

```rust
    async fn mark_stale_completed(&self, stale_secs: i64) -> Result<u64, StoreError> {
        let cutoff = Utc::now() - chrono::Duration::seconds(stale_secs);
        let mut conn = self.pool.get().await.map_err(pool_err)?;
        let count = diesel::update(
            sessions::table
                .filter(sessions::status.eq("running"))
                .filter(sessions::last_activity_at.lt(cutoff)),
        )
        .set(sessions::status.eq("completed"))
        .execute(&mut conn)
        .await?;
        Ok(count as u64)
    }
```

- [ ] **Step 4: Add cleanup call to supervisor loop**

In `crates/rune-gateway/src/supervisor.rs`, inside `supervisor_loop`, add a tick counter and periodic cleanup. Add before the loop:

```rust
    let mut tick_count: u64 = 0;
```

Then inside the loop, after the reminders section (~line 376):

```rust
        // --- Stale session cleanup (every ~100s / 10 ticks) ---
        tick_count += 1;
        if tick_count % 10 == 0 {
            let stale_secs = 3600; // 1 hour
            match deps.session_engine.session_repo().mark_stale_completed(stale_secs).await {
                Ok(0) => {}
                Ok(n) => info!(count = n, "cleaned up stale running sessions"),
                Err(e) => warn!(error = %e, "stale session cleanup failed"),
            }
        }
```

Note: Check if `session_engine` exposes `session_repo()`. If not, add a public accessor or call the repo directly via state.

- [ ] **Step 5: Build**

Run: `cargo build -p rune-gateway`
Expected: success (may need to add the session_repo accessor if missing)

- [ ] **Step 6: Commit**

```bash
git add crates/rune-store/src/repos.rs crates/rune-store/src/sqlite/mod.rs crates/rune-gateway/src/supervisor.rs
git commit -m "feat(supervisor): periodic cleanup of stale running sessions

Every ~100s the supervisor marks sessions stuck in 'running' with no
activity for over 1 hour as 'completed'. Prevents accumulation of
leaked sessions from crashed cron jobs or interrupted turns."
```

---

### Task 8: Last-Run Context Injection for Cron Jobs

**Files:**
- Modify: `crates/rune-gateway/src/supervisor.rs:425-450` (run_agent_turn)

- [ ] **Step 1: Inject last-run context into cron message**

In `crates/rune-gateway/src/supervisor.rs`, modify `run_agent_turn` (around line 425). Before the `match target` block, fetch the last run output and prepend it:

```rust
async fn run_agent_turn(
    deps: &SupervisorDeps,
    message: &str,
    model: Option<&str>,
    target: SessionTarget,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    // Inject last-run context for continuity across cron ticks.
    // This is a no-op on first run or if no completed runs exist.
    let enriched_message = message.to_string();

    match target {
```

Actually, we need the job_id to fetch the last run. The job is not passed to `run_agent_turn`. Instead, inject context at the `execute_job` level where we have the full `Job`. Modify `execute_job`:

```rust
pub(crate) async fn execute_job(
    deps: &SupervisorDeps,
    job: &Job,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    match &job.payload {
        JobPayload::SystemEvent { text } => run_system_event(deps, text).await,
        JobPayload::AgentTurn { message, model, .. } => {
            let enriched = enrich_with_last_run(deps, job, message).await;
            run_agent_turn(deps, &enriched, model.as_deref(), job.session_target).await
        }
    }
}
```

Add the `enrich_with_last_run` helper:

```rust
/// Prepend last-run output to a cron message for continuity across ticks.
async fn enrich_with_last_run(deps: &SupervisorDeps, job: &Job, message: &str) -> String {
    let runs = deps.scheduler.get_runs(&job.id, Some(1)).await;
    let last_output = runs.first().and_then(|r| {
        if r.status == rune_runtime::scheduler::JobRunStatus::Completed {
            r.output.as_deref()
        } else {
            None
        }
    });

    match last_output {
        Some(output) if !output.is_empty() => {
            let truncated = if output.len() > 2000 {
                &output[..2000]
            } else {
                output
            };
            format!("## Last run result:\n{truncated}\n\n## Your task:\n{message}")
        }
        _ => message.to_string(),
    }
}
```

- [ ] **Step 2: Build and verify**

Run: `cargo build -p rune-gateway`
Expected: success

- [ ] **Step 3: Commit**

```bash
git add crates/rune-gateway/src/supervisor.rs
git commit -m "feat(supervisor): inject last-run context into cron job messages

Before executing a cron agent_turn, fetch the last completed run's
output and prepend it to the message. This gives the model continuity
across 5-minute boundaries without needing persistent session state.
Truncates to 2000 chars to avoid context bloat."
```

---

### Task 9: Update Cron Prompts (Self-Update + Delivery Mode)

This task updates the roadmap-worker cron prompt in the database and the `config.toml` for reference.

**Files:**
- Modify: SQLite database (runtime update)
- Modify: `config.toml` (add `[runtime.compaction]`)

- [ ] **Step 1: Update roadmap-worker prompt with self-update instruction**

```bash
python3 -c "
import sqlite3, json
conn = sqlite3.connect('/home/hamza/.rune/db/rune.db')
cur = conn.cursor()

# Get current roadmap-worker payload
cur.execute(\"SELECT id, payload FROM jobs WHERE job_type='cron' AND payload LIKE '%roadmap-worker%'\")
row = cur.fetchone()
if row:
    job_id, payload_str = row
    payload = json.loads(payload_str)
    payload['payload']['message'] = (
        'Continue roadmap work. Read ROADMAP.md, pick up where you left off, and SHIP CODE. '
        'Do not explain — execute. Build, test, commit, push, next item. Do not stop until you ship or hit a real blocker. '
        'If you pushed changes to ~/Development/rune, run ~/Development/rune/scripts/self-update.sh as your last step.'
    )
    cur.execute('UPDATE jobs SET payload = ? WHERE id = ?', (json.dumps(payload), job_id))
    print(f'Updated roadmap-worker prompt')

conn.commit()
conn.close()
"
```

- [ ] **Step 2: Add compaction config to config.toml**

Add to `/home/hamza/Development/rune/config.toml`:

```toml
[runtime.compaction]
context_window = 128000
preserve_tail = 20
```

- [ ] **Step 3: Commit config**

```bash
git add config.toml
git commit -m "feat(config): add runtime compaction settings"
```

---

### Task 10: Build, Deploy, and Verify

- [ ] **Step 1: Full build**

```bash
cargo build --release --bin rune-gateway
```

Expected: success with no warnings

- [ ] **Step 2: Run all tests**

```bash
cargo test -p rune-runtime && cargo test -p rune-gateway && cargo test -p rune-store
```

Expected: all pass

- [ ] **Step 3: Restart gateway**

```bash
systemctl --user restart rune-gateway && sleep 3 && systemctl --user status rune-gateway
```

Expected: active (running)

- [ ] **Step 4: Clear stale claims and verify cron firing**

```bash
python3 -c "
import sqlite3
from datetime import datetime, timezone
conn = sqlite3.connect('/home/hamza/.rune/db/rune.db')
now = datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%S.000000Z')
cur = conn.cursor()
cur.execute('UPDATE jobs SET claimed_at = NULL, next_run_at = ? WHERE job_type = \"cron\" AND enabled = 1 AND next_run_at <= ?', (now, now))
print(f'Reset {cur.rowcount} stale jobs')
conn.commit()
conn.close()
"
```

- [ ] **Step 5: Wait 20s and verify parallel execution**

```bash
sleep 20 && python3 -c "
import sqlite3
conn = sqlite3.connect('/home/hamza/.rune/db/rune.db')
conn.row_factory = sqlite3.Row
cur = conn.cursor()
cur.execute('SELECT json_extract(payload, \"$.name\") as name, claimed_at, last_run_at, delivery_mode FROM jobs WHERE job_type=\"cron\"')
for r in cur.fetchall():
    print(dict(r))
cur.execute('SELECT job_id, status, started_at FROM job_runs ORDER BY created_at DESC LIMIT 5')
for r in cur.fetchall():
    print(dict(r))
conn.close()
"
```

Expected: all 3 jobs claimed with close timestamps (parallel), delivery_mode = "announce"

- [ ] **Step 6: Check Telegram for proactive delivery**

Verify on Telegram that cron job results appear as messages from the bot.

- [ ] **Step 7: Push all changes**

```bash
git push origin main
```

- [ ] **Step 8: Commit plan as completed**

```bash
git add docs/superpowers/plans/2026-03-26-execution-engine.md
git commit -m "docs: mark execution engine plan as complete"
git push origin main
```
