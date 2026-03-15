# Phases 22-24: Implementation Specification

> Generated 2026-03-15. Authoritative reference for implementing phases 22 through 24.
> Every type, endpoint, wire example, error case, and acceptance criterion is defined
> here so that implementation can proceed without guessing.

---

## Table of Contents

1. [Phase 22 — Agent Modes & Orchestration](#phase-22--agent-modes--orchestration)
2. [Phase 23 — Git Worktree Isolation](#phase-23--git-worktree-isolation)
3. [Phase 24 — Intelligent Context Management](#phase-24--intelligent-context-management)

---

## Phase 22 — Agent Modes & Orchestration

### 22.1 Overview

Multiple specialized agent modes with an orchestrator that decomposes complex tasks
into coordinated sub-agent work. The orchestrator performs LLM-driven task
decomposition, builds a dependency graph, spawns sub-agent sessions with
mode-appropriate tool permissions, tracks progress, handles failure recovery, and
synthesises results into a coherent response.

### 22.2 Rust Types

#### File: `crates/rune-runtime/src/agent_mode.rs`

```rust
use std::collections::HashSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Built-in agent operating modes.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentMode {
    Orchestrator,
    Architect,
    Coder,
    Debugger,
    Ask,
    Custom(String),
}

impl AgentMode {
    /// Tool permission set for this mode.
    pub fn allowed_tools(&self) -> ToolPermissions {
        match self {
            Self::Orchestrator => ToolPermissions::all(),
            Self::Architect | Self::Ask => ToolPermissions::read_only(),
            Self::Coder => ToolPermissions::full(),
            Self::Debugger => ToolPermissions::read_exec(),
            Self::Custom(_) => ToolPermissions::default(),
        }
    }

    /// System-prompt template name looked up from embedded templates.
    pub fn system_prompt_template(&self) -> &str {
        match self {
            Self::Orchestrator => "orchestrator",
            Self::Architect => "architect",
            Self::Coder => "coder",
            Self::Debugger => "debugger",
            Self::Ask => "ask",
            Self::Custom(name) => name.as_str(),
        }
    }
}

impl Default for AgentMode {
    fn default() -> Self {
        Self::Coder
    }
}

/// Declarative tool permission set applied per mode.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolPermissions {
    /// Allowed tool name patterns (glob-style). Empty = deny all.
    pub allow: HashSet<String>,
    /// Explicitly denied tool names (takes precedence over allow).
    pub deny: HashSet<String>,
    /// Whether file-write tools are enabled.
    pub write_files: bool,
    /// Whether shell execution tools are enabled.
    pub exec_shell: bool,
}

impl Default for ToolPermissions {
    fn default() -> Self {
        Self::read_only()
    }
}

impl ToolPermissions {
    pub fn all() -> Self {
        Self {
            allow: HashSet::from(["*".into()]),
            deny: HashSet::new(),
            write_files: true,
            exec_shell: true,
        }
    }

    pub fn full() -> Self {
        Self {
            allow: HashSet::from(["*".into()]),
            deny: HashSet::new(),
            write_files: true,
            exec_shell: true,
        }
    }

    pub fn read_only() -> Self {
        Self {
            allow: HashSet::from([
                "read_file".into(),
                "list_files".into(),
                "search_files".into(),
                "grep".into(),
            ]),
            deny: HashSet::new(),
            write_files: false,
            exec_shell: false,
        }
    }

    pub fn read_exec() -> Self {
        Self {
            allow: HashSet::from([
                "read_file".into(),
                "list_files".into(),
                "search_files".into(),
                "grep".into(),
                "bash".into(),
            ]),
            deny: HashSet::new(),
            write_files: false,
            exec_shell: true,
        }
    }

    /// Returns true if the named tool is permitted.
    pub fn is_allowed(&self, tool_name: &str) -> bool {
        if self.deny.contains(tool_name) {
            return false;
        }
        self.allow.contains("*") || self.allow.contains(tool_name)
    }
}

/// Mode definition loaded from config or custom mode files.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModeDefinition {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub permissions: ToolPermissions,
    /// Source path for custom modes (None for built-in).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<PathBuf>,
}
```

#### File: `crates/rune-runtime/src/orchestrator.rs`

```rust
use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

use crate::agent_mode::AgentMode;
use crate::error::RuntimeError;

/// A single unit of work produced by task decomposition.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Subtask {
    pub id: Uuid,
    pub parent_task_id: Uuid,
    /// Human-readable slug used in branch naming.
    pub slug: String,
    pub description: String,
    /// Agent mode to execute this subtask.
    pub mode: AgentMode,
    /// IDs of subtasks that must complete before this one starts.
    pub depends_on: Vec<Uuid>,
    /// Optional context fragments forwarded from the parent.
    pub context_hints: Vec<String>,
    pub status: SubtaskStatus,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    /// Session ID of the sub-agent executing this subtask.
    pub session_id: Option<Uuid>,
    /// Number of retry attempts consumed.
    pub retry_count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubtaskStatus {
    Pending,
    Blocked,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

/// Result produced by a completed subtask.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SubtaskResult {
    pub subtask_id: Uuid,
    pub session_id: Uuid,
    pub status: SubtaskStatus,
    /// Summary of what the sub-agent accomplished.
    pub summary: String,
    /// Artifacts (file paths, snippets) produced.
    pub artifacts: Vec<String>,
    /// Error message if the subtask failed.
    pub error: Option<String>,
    pub completed_at: DateTime<Utc>,
}

/// DAG of subtask dependencies.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DependencyGraph {
    pub task_id: Uuid,
    pub subtasks: Vec<Subtask>,
    /// Adjacency list: subtask_id → set of successor subtask_ids.
    pub edges: HashMap<Uuid, Vec<Uuid>>,
}

impl DependencyGraph {
    /// Return subtask IDs whose dependencies have all succeeded.
    pub fn ready_subtasks(&self) -> Vec<Uuid> {
        let succeeded: std::collections::HashSet<Uuid> = self
            .subtasks
            .iter()
            .filter(|s| s.status == SubtaskStatus::Succeeded)
            .map(|s| s.id)
            .collect();

        self.subtasks
            .iter()
            .filter(|s| {
                s.status == SubtaskStatus::Pending
                    && s.depends_on.iter().all(|dep| succeeded.contains(dep))
            })
            .map(|s| s.id)
            .collect()
    }

    /// True when every subtask is in a terminal state.
    pub fn is_complete(&self) -> bool {
        self.subtasks.iter().all(|s| {
            matches!(
                s.status,
                SubtaskStatus::Succeeded | SubtaskStatus::Failed | SubtaskStatus::Cancelled
            )
        })
    }

    /// True when any required (non-cancelled) subtask has failed.
    pub fn has_failures(&self) -> bool {
        self.subtasks
            .iter()
            .any(|s| s.status == SubtaskStatus::Failed)
    }

    /// Detect cycles via DFS. Returns Err with the cycle path if found.
    pub fn validate_acyclic(&self) -> Result<(), Vec<Uuid>> {
        // Kahn's algorithm: if topo-sort does not consume all nodes, cycle exists.
        let mut in_degree: HashMap<Uuid, usize> = HashMap::new();
        for s in &self.subtasks {
            in_degree.entry(s.id).or_insert(0);
            for dep in &s.depends_on {
                *in_degree.entry(s.id).or_insert(0) += 1;
                let _ = in_degree.entry(*dep).or_insert(0);
            }
        }
        let mut queue: std::collections::VecDeque<Uuid> = in_degree
            .iter()
            .filter(|(_, &d)| d == 0)
            .map(|(&id, _)| id)
            .collect();
        let mut visited = 0usize;
        while let Some(node) = queue.pop_front() {
            visited += 1;
            if let Some(successors) = self.edges.get(&node) {
                for &succ in successors {
                    if let Some(d) = in_degree.get_mut(&succ) {
                        *d -= 1;
                        if *d == 0 {
                            queue.push_back(succ);
                        }
                    }
                }
            }
        }
        if visited == self.subtasks.len() {
            Ok(())
        } else {
            let stuck: Vec<Uuid> = in_degree
                .into_iter()
                .filter(|(_, d)| *d > 0)
                .map(|(id, _)| id)
                .collect();
            Err(stuck)
        }
    }
}

/// Orchestrator configuration.
#[derive(Clone, Debug, Deserialize)]
pub struct OrchestratorConfig {
    /// Maximum number of subtasks spawned from a single decomposition.
    pub max_subtasks: usize,
    /// Maximum parallel sub-agents.
    pub max_parallel: usize,
    /// Maximum retries per subtask before marking failed.
    pub max_retries: u32,
    /// Timeout per subtask in seconds.
    pub subtask_timeout_secs: u64,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            max_subtasks: 20,
            max_parallel: 5,
            max_retries: 2,
            subtask_timeout_secs: 300,
        }
    }
}

/// Progress event emitted on the orchestrator progress channel.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OrchestratorEvent {
    PlanCreated {
        task_id: Uuid,
        subtask_count: usize,
    },
    SubtaskStarted {
        task_id: Uuid,
        subtask_id: Uuid,
        mode: AgentMode,
    },
    SubtaskCompleted {
        task_id: Uuid,
        subtask_id: Uuid,
        status: SubtaskStatus,
    },
    SubtaskRetrying {
        task_id: Uuid,
        subtask_id: Uuid,
        attempt: u32,
    },
    SynthesisStarted {
        task_id: Uuid,
    },
    TaskCompleted {
        task_id: Uuid,
        success: bool,
    },
}

/// Main orchestrator that drives task decomposition and sub-agent coordination.
pub struct Orchestrator {
    config: OrchestratorConfig,
    graph: Arc<RwLock<Option<DependencyGraph>>>,
    results: Arc<RwLock<HashMap<Uuid, SubtaskResult>>>,
    event_tx: mpsc::Sender<OrchestratorEvent>,
}

impl Orchestrator {
    pub fn new(
        config: OrchestratorConfig,
        event_tx: mpsc::Sender<OrchestratorEvent>,
    ) -> Self {
        Self {
            config,
            graph: Arc::new(RwLock::new(None)),
            results: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
        }
    }

    /// Phase 1: Task Decomposition.
    ///
    /// Sends the user goal to the model with the orchestrator system prompt.
    /// The model returns a JSON plan (array of subtasks with dependencies).
    /// The plan is validated (acyclic, within limits) and stored.
    pub async fn decompose(
        &self,
        goal: &str,
        session_context: &str,
    ) -> Result<DependencyGraph, RuntimeError> {
        todo!("LLM planning call → parse → validate_acyclic → store graph")
    }

    /// Phase 2: Execution Loop.
    ///
    /// Algorithm:
    /// 1. Collect `ready_subtasks()` from the graph.
    /// 2. For each ready subtask (up to `max_parallel`):
    ///    a. Spawn a child session with the subtask's `AgentMode`.
    ///    b. Inject subtask description + context_hints as user message.
    ///    c. Run `TurnExecutor::execute_turn` in a spawned task.
    /// 3. Await completion of any running subtask.
    /// 4. On success: update graph, emit event, repeat from 1.
    /// 5. On failure: if retries < max_retries, re-queue with `retry_count + 1`
    ///    and optionally switch mode (e.g., Coder → Debugger); else mark Failed.
    /// 6. If `graph.has_failures()` and no more retries possible, cancel
    ///    remaining Pending/Blocked subtasks.
    /// 7. Repeat until `graph.is_complete()`.
    pub async fn execute(
        &self,
    ) -> Result<Vec<SubtaskResult>, RuntimeError> {
        todo!("main scheduling loop")
    }

    /// Phase 3: Result Synthesis.
    ///
    /// Collects all `SubtaskResult`s, feeds them to the model with a synthesis
    /// prompt, and returns a unified response. Failed subtask results are
    /// included so the model can note incomplete work.
    pub async fn synthesise(
        &self,
        results: &[SubtaskResult],
    ) -> Result<String, RuntimeError> {
        todo!("synthesis LLM call")
    }
}
```

#### Task Decomposition Algorithm (Detail)

1. Construct a planning prompt:
   - System prompt: orchestrator template with instructions to output JSON.
   - User message: the original goal + session context summary.
   - Required JSON schema: `{ "subtasks": [{ "slug", "description", "mode", "depends_on_slugs", "context_hints" }] }`.
2. Call the model with `temperature: 0.2` for determinism.
3. Parse the JSON response. If parsing fails, retry once with an error correction prompt.
4. Assign UUIDs, resolve `depends_on_slugs` to UUIDs, build `DependencyGraph`.
5. Validate: `validate_acyclic()`, `subtasks.len() <= max_subtasks`, no self-dependencies.
6. If validation fails, return `RuntimeError::InvalidPlan` with details.

#### Failure Recovery Strategy

| Condition | Action |
|---|---|
| Subtask fails, retries remaining | Re-queue with `retry_count + 1` |
| Coder subtask fails twice | Re-queue as Debugger mode on 3rd attempt |
| Subtask timeout | Cancel sub-agent session, mark Failed, attempt retry |
| All retries exhausted | Mark Failed, cancel dependents transitively |
| Critical-path failure (all successors also fail) | Emit `TaskCompleted { success: false }`, run synthesis with partial results |

#### File: `crates/rune-runtime/src/custom_mode.rs`

```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::Deserialize;
use tokio::sync::RwLock;

use crate::agent_mode::{ModeDefinition, ToolPermissions};
use crate::error::RuntimeError;

/// YAML frontmatter parsed from `modes/*.md` files.
#[derive(Debug, Deserialize)]
pub struct CustomModeFrontmatter {
    pub name: String,
    pub description: String,
    /// Tool names to allow. If omitted, defaults to read-only.
    #[serde(default)]
    pub tools: Vec<String>,
    /// If true, only read tools are allowed regardless of `tools` list.
    #[serde(default)]
    pub read_only: bool,
}

/// Manages loading and hot-reloading of custom mode definitions.
pub struct CustomModeLoader {
    modes_dir: PathBuf,
    modes: Arc<RwLock<HashMap<String, ModeDefinition>>>,
    _watcher: Option<RecommendedWatcher>,
}

impl CustomModeLoader {
    /// Scan `modes_dir` for `*.md` files and parse their frontmatter.
    pub async fn load(modes_dir: PathBuf) -> Result<Self, RuntimeError> {
        todo!("scan dir, parse YAML frontmatter, build ModeDefinition map")
    }

    /// Start a file-system watcher that reloads modes on change.
    pub fn watch(&mut self) -> Result<(), RuntimeError> {
        todo!("notify watcher → re-scan on Create/Modify/Remove events")
    }

    /// Retrieve a mode by name.
    pub async fn get(&self, name: &str) -> Option<ModeDefinition> {
        self.modes.read().await.get(name).cloned()
    }

    /// List all loaded custom modes.
    pub async fn list(&self) -> Vec<ModeDefinition> {
        self.modes.read().await.values().cloned().collect()
    }
}
```

### 22.3 Wire Protocol

#### `GET /modes`

List all available modes (built-in + custom).

**Response 200:**
```json
{
  "modes": [
    {
      "name": "orchestrator",
      "description": "Decomposes complex tasks into coordinated sub-agent work",
      "permissions": {
        "allow": ["*"],
        "deny": [],
        "write_files": true,
        "exec_shell": true
      },
      "source_path": null
    },
    {
      "name": "architect",
      "description": "Read-only analysis and design",
      "permissions": {
        "allow": ["read_file", "list_files", "search_files", "grep"],
        "deny": [],
        "write_files": false,
        "exec_shell": false
      },
      "source_path": null
    }
  ]
}
```

#### `GET /modes/{name}`

**Response 200:**
```json
{
  "name": "coder",
  "description": "Full read/write access for implementation",
  "system_prompt": "You are a senior software engineer...",
  "permissions": {
    "allow": ["*"],
    "deny": [],
    "write_files": true,
    "exec_shell": true
  },
  "source_path": null
}
```

**Response 404:**
```json
{ "error": "mode_not_found", "message": "No mode named 'foo'" }
```

#### `POST /sessions/{id}/mode`

Switch the active mode for a session.

**Request:**
```json
{ "mode": "architect" }
```

**Response 200:**
```json
{
  "session_id": "550e8400-e29b-41d4-a716-446655440000",
  "previous_mode": "coder",
  "current_mode": "architect"
}
```

**Response 404:**
```json
{ "error": "session_not_found", "message": "Session 550e... not found" }
```

**Response 400:**
```json
{ "error": "invalid_mode", "message": "Unknown mode 'xyz'" }
```

**Response 409:**
```json
{ "error": "turn_in_progress", "message": "Cannot switch mode while a turn is executing" }
```

#### Orchestrator Progress (WebSocket)

Events are streamed on the existing session WebSocket as frames with `"type": "orchestrator_event"`:

```json
{
  "type": "orchestrator_event",
  "payload": {
    "type": "subtask_started",
    "task_id": "...",
    "subtask_id": "...",
    "mode": "coder"
  }
}
```

### 22.4 Error Cases

```rust
/// Errors specific to mode and orchestration operations.
#[derive(Debug, thiserror::Error)]
pub enum ModeError {
    #[error("unknown mode: {name}")]
    UnknownMode { name: String },
    // HTTP 400

    #[error("cannot switch mode while turn is executing on session {session_id}")]
    TurnInProgress { session_id: Uuid },
    // HTTP 409

    #[error("custom mode file parse error at {path}: {reason}")]
    CustomModeParseError { path: PathBuf, reason: String },
    // HTTP 500 (internal, logged)

    #[error("orchestrator plan invalid: {reason}")]
    InvalidPlan { reason: String },
    // HTTP 422

    #[error("subtask limit exceeded: {count} > {max}")]
    SubtaskLimitExceeded { count: usize, max: usize },
    // HTTP 422

    #[error("dependency cycle detected involving subtasks: {cycle:?}")]
    DependencyCycle { cycle: Vec<Uuid> },
    // HTTP 422

    #[error("orchestrator timeout after {elapsed_secs}s")]
    OrchestratorTimeout { elapsed_secs: u64 },
    // HTTP 504

    #[error("all retries exhausted for subtask {subtask_id}")]
    RetriesExhausted { subtask_id: Uuid },
    // HTTP 500 (internal)

    #[error("plan decomposition LLM call failed: {source}")]
    PlanLlmError { source: String },
    // HTTP 502
}
```

| Variant | HTTP Status |
|---|---|
| `UnknownMode` | 400 |
| `TurnInProgress` | 409 |
| `CustomModeParseError` | 500 |
| `InvalidPlan` | 422 |
| `SubtaskLimitExceeded` | 422 |
| `DependencyCycle` | 422 |
| `OrchestratorTimeout` | 504 |
| `RetriesExhausted` | 500 |
| `PlanLlmError` | 502 |

### 22.5 Edge Cases

1. **Concurrent mode switch**: Two `POST /sessions/{id}/mode` arrive simultaneously. Guard with a per-session `RwLock`. First writer wins; second gets 409 if a turn started between check and write.
2. **Mode switch during orchestration**: Reject with 409 — the orchestrator owns the session until the task completes or is cancelled.
3. **Custom mode hot-reload race**: `RwLock<HashMap>` ensures readers see a consistent snapshot. A mode removed mid-session continues with its cached definition until the session ends.
4. **Subtask fan-out explosion**: The LLM may produce more subtasks than `max_subtasks`. Validate before accepting the plan. Return `SubtaskLimitExceeded`.
5. **Diamond dependencies**: Subtask C depends on A and B. Both A and B complete; C must not start twice. Use `compare_exchange` on `SubtaskStatus::Pending → Running`.
6. **Partial synthesis**: If 3 of 5 subtasks succeed and 2 fail after retries, synthesis runs with all 5 results (including error details) so the model can report partial progress.

### 22.6 SQL Migrations

```sql
-- 20260316000001_create_agent_modes.up.sql

CREATE TYPE subtask_status AS ENUM (
    'pending', 'blocked', 'running', 'succeeded', 'failed', 'cancelled'
);

ALTER TABLE sessions ADD COLUMN agent_mode TEXT NOT NULL DEFAULT 'coder';

CREATE TABLE orchestrator_tasks (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    session_id      UUID NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    goal            TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'planning',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at    TIMESTAMPTZ,
    result_summary  TEXT
);

CREATE TABLE subtasks (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id         UUID NOT NULL REFERENCES orchestrator_tasks(id) ON DELETE CASCADE,
    slug            TEXT NOT NULL,
    description     TEXT NOT NULL,
    mode            TEXT NOT NULL,
    status          subtask_status NOT NULL DEFAULT 'pending',
    depends_on      UUID[] NOT NULL DEFAULT '{}',
    context_hints   TEXT[] NOT NULL DEFAULT '{}',
    session_id      UUID REFERENCES sessions(id) ON DELETE SET NULL,
    retry_count     INT NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at      TIMESTAMPTZ,
    completed_at    TIMESTAMPTZ,
    result_summary  TEXT,
    error_message   TEXT
);

CREATE INDEX idx_subtasks_task_id ON subtasks(task_id);
CREATE INDEX idx_subtasks_status ON subtasks(status);
CREATE INDEX idx_orchestrator_tasks_session ON orchestrator_tasks(session_id);
```

```sql
-- 20260316000001_create_agent_modes.down.sql

DROP TABLE IF EXISTS subtasks;
DROP TABLE IF EXISTS orchestrator_tasks;
ALTER TABLE sessions DROP COLUMN IF EXISTS agent_mode;
DROP TYPE IF EXISTS subtask_status;
```

### 22.7 Integration Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// Verify all built-in modes have a system prompt template and valid permissions.
    #[test]
    fn test_builtin_modes_have_templates();

    /// ToolPermissions::is_allowed correctly applies allow/deny logic.
    #[test]
    fn test_tool_permissions_allow_deny();

    /// Read-only mode blocks write_file and bash tools.
    #[test]
    fn test_read_only_mode_blocks_writes();

    /// DependencyGraph::ready_subtasks returns only unblocked pending tasks.
    #[test]
    fn test_ready_subtasks_respects_dependencies();

    /// DependencyGraph::validate_acyclic detects a simple A→B→A cycle.
    #[test]
    fn test_cycle_detection();

    /// DependencyGraph::is_complete returns true only when all tasks terminal.
    #[test]
    fn test_graph_completion_check();

    /// Plans exceeding max_subtasks are rejected.
    #[test]
    fn test_subtask_limit_enforced();

    /// POST /sessions/{id}/mode returns 409 when a turn is running.
    #[tokio::test]
    async fn test_mode_switch_blocked_during_turn();

    /// GET /modes returns built-in + custom modes merged.
    #[tokio::test]
    async fn test_list_modes_includes_custom();

    /// Custom mode .md file with invalid YAML frontmatter logs error, skips file.
    #[tokio::test]
    async fn test_custom_mode_invalid_frontmatter_skipped();

    /// Orchestrator decomposes a simple two-step goal into sequential subtasks.
    #[tokio::test]
    async fn test_orchestrator_decompose_simple_plan();

    /// Orchestrator retries a failed subtask up to max_retries then marks failed.
    #[tokio::test]
    async fn test_orchestrator_retry_then_fail();

    /// Orchestrator runs independent subtasks in parallel up to max_parallel.
    #[tokio::test]
    async fn test_orchestrator_parallel_execution();

    /// Synthesis includes both succeeded and failed subtask results.
    #[tokio::test]
    async fn test_synthesis_includes_partial_results();
}
```

### 22.8 Acceptance Criteria

- [ ] `GET /modes` returns all 5 built-in modes + any custom modes from `modes/` dir
- [ ] `POST /sessions/{id}/mode` switches mode and subsequent turns use new system prompt
- [ ] Mode switch rejected (409) while a turn is executing
- [ ] Custom `.md` mode files are loaded on startup from configurable `modes_dir`
- [ ] Custom modes hot-reload within 2s of file change
- [ ] Invalid custom mode files are skipped with a warning log, not a crash
- [ ] Orchestrator decomposes a multi-step goal into a valid DAG of subtasks
- [ ] Subtasks with no dependencies start immediately in parallel (up to `max_parallel`)
- [ ] Dependent subtasks wait until all predecessors succeed
- [ ] Failed subtask is retried up to `max_retries` times
- [ ] Coder subtask failing twice is retried as Debugger on third attempt
- [ ] Timed-out subtask is cancelled and retried
- [ ] When all retries exhausted, dependent subtasks are transitively cancelled
- [ ] Synthesis produces a coherent response combining all subtask results
- [ ] Orchestrator events stream over session WebSocket in real time
- [ ] `agent_mode` column persisted on session row
- [ ] Subtask rows persisted to `subtasks` table with correct status transitions

### 22.9 Dependencies

| Crate | Version | Purpose |
|---|---|---|
| `notify` | 7 | File-system watcher for custom mode hot-reload |
| `serde_yaml` | 0.9 | Parse YAML frontmatter in custom mode `.md` files |
| `petgraph` | 0.6 | (Optional) Graph algorithms if needed beyond hand-rolled DAG |

All other dependencies already in workspace: `tokio 1`, `serde 1`, `uuid 1`, `chrono 0.4`, `thiserror 2`, `tracing 0.1`.

---

## Phase 23 — Git Worktree Isolation

### 23.1 Overview

Parallel agent execution in isolated git worktrees so multiple orchestrator subtasks
can work simultaneously without file conflicts. Each sub-agent gets its own working
directory backed by a git worktree with a dedicated branch. On completion, changes
are merged back to the parent branch with LLM-assisted conflict resolution.

### 23.2 Rust Types

#### File: `crates/rune-runtime/src/worktree.rs`

```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::error::RuntimeError;

/// Lifecycle state of a worktree.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeState {
    Creating,
    Active,
    Merging,
    Merged,
    Conflicted,
    Cleaning,
    Removed,
}

/// A tracked git worktree associated with a sub-agent session.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorktreeInfo {
    pub id: Uuid,
    pub session_id: Uuid,
    /// The subtask this worktree was created for.
    pub subtask_id: Option<Uuid>,
    /// Branch name: `rune/agent/<session-id>/<task-slug>`
    pub branch: String,
    /// Absolute path to the worktree directory.
    pub path: PathBuf,
    /// Branch this worktree was forked from.
    pub base_branch: String,
    pub state: WorktreeState,
    pub created_at: DateTime<Utc>,
    pub merged_at: Option<DateTime<Utc>>,
}

/// Conflict detected during merge.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MergeConflict {
    pub file_path: String,
    /// Unified diff showing the conflict markers.
    pub conflict_diff: String,
    /// LLM-suggested resolution (if available).
    pub suggested_resolution: Option<String>,
}

/// Result of a merge attempt.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum MergeResult {
    /// Clean merge, no conflicts.
    Success { merged_commit: String },
    /// Conflicts detected; needs human or LLM resolution.
    Conflict { conflicts: Vec<MergeConflict> },
    /// Nothing to merge (no changes in worktree branch).
    NoChanges,
}

/// Configuration for the worktree subsystem.
#[derive(Clone, Debug, Deserialize)]
pub struct WorktreeConfig {
    /// Base directory for worktrees. Default: `.rune/worktrees/`
    pub base_dir: PathBuf,
    /// Whether to auto-exclude worktree dirs from git.
    pub auto_exclude: bool,
    /// Whether to attempt LLM-assisted merge for conflicts.
    pub llm_conflict_resolution: bool,
    /// Auto-cleanup worktrees after merge. Default: true.
    pub auto_cleanup: bool,
}

impl Default for WorktreeConfig {
    fn default() -> Self {
        Self {
            base_dir: PathBuf::from(".rune/worktrees"),
            auto_exclude: true,
            llm_conflict_resolution: true,
            auto_cleanup: true,
        }
    }
}

/// Manages git worktree lifecycle.
pub struct WorktreeManager {
    config: WorktreeConfig,
    /// Root of the main git repository.
    repo_root: PathBuf,
    worktrees: Arc<RwLock<HashMap<Uuid, WorktreeInfo>>>,
}

impl WorktreeManager {
    pub fn new(repo_root: PathBuf, config: WorktreeConfig) -> Self {
        Self {
            config,
            repo_root,
            worktrees: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new worktree for a sub-agent session.
    ///
    /// Steps:
    /// 1. Compute branch name: `rune/agent/{session_id}/{slug}`.
    /// 2. Compute path: `{base_dir}/{session_id}/`.
    /// 3. Run `git worktree add -b {branch} {path} {base_branch}`.
    /// 4. If `auto_exclude`, append path to `.git/info/exclude`.
    /// 5. Store `WorktreeInfo` with `state: Active`.
    pub async fn create(
        &self,
        session_id: Uuid,
        subtask_id: Option<Uuid>,
        slug: &str,
        base_branch: &str,
    ) -> Result<WorktreeInfo, RuntimeError> {
        todo!()
    }

    /// List all tracked worktrees.
    pub async fn list(&self) -> Vec<WorktreeInfo> {
        self.worktrees.read().await.values().cloned().collect()
    }

    /// Get a worktree by session ID.
    pub async fn get_by_session(&self, session_id: Uuid) -> Option<WorktreeInfo> {
        self.worktrees
            .read()
            .await
            .values()
            .find(|w| w.session_id == session_id)
            .cloned()
    }

    /// Merge the worktree branch back to its base branch.
    ///
    /// Steps:
    /// 1. Set state to `Merging`.
    /// 2. `git checkout {base_branch}` in repo_root.
    /// 3. `git merge --no-ff {worktree_branch}`.
    /// 4. If clean: set state `Merged`, return `MergeResult::Success`.
    /// 5. If conflicts:
    ///    a. Parse conflict markers from each conflicted file.
    ///    b. If `llm_conflict_resolution` enabled, send each conflict to the model
    ///       with both sides + context; apply suggested resolution.
    ///    c. If LLM resolution succeeds for all files, commit and return Success.
    ///    d. Otherwise `git merge --abort`, set state `Conflicted`,
    ///       return `MergeResult::Conflict` with details.
    /// 6. If auto_cleanup, call `self.cleanup(id)`.
    pub async fn merge(&self, id: Uuid) -> Result<MergeResult, RuntimeError> {
        todo!()
    }

    /// Remove a worktree and delete its branch.
    ///
    /// Steps:
    /// 1. Set state to `Cleaning`.
    /// 2. `git worktree remove --force {path}`.
    /// 3. `git branch -D {branch}`.
    /// 4. Remove entry from `.git/info/exclude` if present.
    /// 5. Set state to `Removed`.
    pub async fn cleanup(&self, id: Uuid) -> Result<(), RuntimeError> {
        todo!()
    }

    /// Cleanup all worktrees in terminal states (Merged, Removed).
    pub async fn cleanup_stale(&self) -> Result<usize, RuntimeError> {
        todo!()
    }
}
```

### 23.3 Branch Naming & Paths

| Component | Pattern | Example |
|---|---|---|
| Branch | `rune/agent/{session_id_short}/{slug}` | `rune/agent/550e8400/fix-auth-bug` |
| Worktree path | `{base_dir}/{session_id}/` | `.rune/worktrees/550e8400-e29b-41d4-a716-446655440000/` |
| Exclude entry | `{worktree_path}` appended to `.git/info/exclude` | `.rune/worktrees/550e8400-*/` |

`session_id_short` = first 8 hex chars of the UUID to keep branch names manageable.

### 23.4 Worktree Lifecycle

```
  create()          sub-agent work         merge()          cleanup()
     │                    │                    │                 │
 Creating ──► Active ─────────────────► Merging ──► Merged ──► Removed
                                            │
                                            ├──► Conflicted (manual resolution needed)
                                            │        │
                                            │        └──► merge() retry ──► Merged
                                            │
                                            └──► (merge --abort on unresolvable conflict)
```

### 23.5 Wire Protocol

No new REST endpoints. Worktree lifecycle is internal to the orchestrator. Status is
visible via the existing session metadata and orchestrator events.

Orchestrator events include worktree state:

```json
{
  "type": "orchestrator_event",
  "payload": {
    "type": "subtask_started",
    "task_id": "...",
    "subtask_id": "...",
    "mode": "coder",
    "worktree": {
      "branch": "rune/agent/550e8400/fix-auth-bug",
      "path": ".rune/worktrees/550e8400-e29b-41d4-a716-446655440000/"
    }
  }
}
```

Merge result surfaced via `subtask_completed` event:

```json
{
  "type": "orchestrator_event",
  "payload": {
    "type": "subtask_completed",
    "task_id": "...",
    "subtask_id": "...",
    "status": "succeeded",
    "merge": {
      "status": "success",
      "merged_commit": "abc1234"
    }
  }
}
```

Conflict case:

```json
{
  "type": "orchestrator_event",
  "payload": {
    "type": "subtask_completed",
    "task_id": "...",
    "subtask_id": "...",
    "status": "succeeded",
    "merge": {
      "status": "conflict",
      "conflicts": [
        {
          "file_path": "src/main.rs",
          "conflict_diff": "<<<<<<< HEAD\n...\n=======\n...\n>>>>>>>",
          "suggested_resolution": "// merged version..."
        }
      ]
    }
  }
}
```

### 23.6 Error Cases

```rust
#[derive(Debug, thiserror::Error)]
pub enum WorktreeError {
    #[error("git worktree creation failed: {reason}")]
    CreateFailed { reason: String },

    #[error("worktree not found: {id}")]
    NotFound { id: Uuid },

    #[error("worktree {id} is in state {state:?}, expected {expected:?}")]
    InvalidState {
        id: Uuid,
        state: WorktreeState,
        expected: Vec<WorktreeState>,
    },

    #[error("merge conflicts in {count} files")]
    MergeConflicts { count: usize },

    #[error("git command failed: {command} — {stderr}")]
    GitCommandFailed { command: String, stderr: String },

    #[error("worktree path already exists: {path}")]
    PathExists { path: PathBuf },

    #[error("branch already exists: {branch}")]
    BranchExists { branch: String },

    #[error("worktree cleanup failed: {reason}")]
    CleanupFailed { reason: String },

    #[error("base branch {branch} not found")]
    BaseBranchNotFound { branch: String },

    #[error("repository has uncommitted changes on {branch}")]
    DirtyWorkingTree { branch: String },
}
```

### 23.7 Edge Cases

1. **Concurrent merges to same base branch**: Serialize merges through a per-repo `Mutex<()>`. Second merge sees the first's changes and may get additional conflicts.
2. **Worktree for a cancelled subtask**: `cleanup()` is called even if the subtask never produced commits — `git worktree remove` handles empty worktrees.
3. **Orphaned worktrees after crash**: On startup, `WorktreeManager` runs `git worktree list --porcelain`, compares against DB, and cleans up untracked worktrees.
4. **Branch name collision**: If `rune/agent/{id}/{slug}` already exists (from a previous failed run), append a numeric suffix: `{slug}-2`.
5. **Base branch moves during subtask**: The worktree was forked from `main@abc123`. While the subtask runs, `main` advances. Merge uses `--no-ff` to create a merge commit; conflicts are expected and handled by LLM-assisted resolution.
6. **Large repo, slow worktree creation**: `git worktree add` is O(checkout), not O(clone). For very large repos, this can still be slow. The `Creating` state prevents premature use. The orchestrator waits for the `Active` callback.
7. **File tools scoping**: `TurnExecutor` receives a `working_dir: PathBuf` field. When a worktree is active, this is set to the worktree path. All file tools (read, write, list, grep, bash) resolve relative paths from this root. Absolute paths outside the worktree are rejected by a path-prefix guard.

### 23.8 SQL Migrations

```sql
-- 20260317000001_create_worktrees.up.sql

CREATE TYPE worktree_state AS ENUM (
    'creating', 'active', 'merging', 'merged', 'conflicted', 'cleaning', 'removed'
);

CREATE TABLE worktrees (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    session_id      UUID NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    subtask_id      UUID REFERENCES subtasks(id) ON DELETE SET NULL,
    branch          TEXT NOT NULL,
    path            TEXT NOT NULL,
    base_branch     TEXT NOT NULL,
    state           worktree_state NOT NULL DEFAULT 'creating',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    merged_at       TIMESTAMPTZ,
    merge_commit    TEXT
);

CREATE INDEX idx_worktrees_session ON worktrees(session_id);
CREATE INDEX idx_worktrees_state ON worktrees(state);
CREATE UNIQUE INDEX idx_worktrees_branch ON worktrees(branch);
```

```sql
-- 20260317000001_create_worktrees.down.sql

DROP TABLE IF EXISTS worktrees;
DROP TYPE IF EXISTS worktree_state;
```

### 23.9 Integration Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// Create a worktree, verify branch exists, path is a valid git checkout.
    #[tokio::test]
    async fn test_create_worktree();

    /// Create, then cleanup — verify branch and directory are removed.
    #[tokio::test]
    async fn test_cleanup_removes_branch_and_dir();

    /// Create worktree, commit a file, merge back — verify file on base branch.
    #[tokio::test]
    async fn test_merge_clean();

    /// Create two worktrees editing the same file differently — verify conflict detection.
    #[tokio::test]
    async fn test_merge_conflict_detected();

    /// Merge with LLM resolution enabled — verify auto-resolved commit.
    #[tokio::test]
    async fn test_llm_conflict_resolution();

    /// Duplicate branch name gets numeric suffix.
    #[tokio::test]
    async fn test_branch_name_collision_suffix();

    /// Worktree in wrong state for merge returns InvalidState error.
    #[tokio::test]
    async fn test_merge_rejects_non_active_worktree();

    /// On startup, orphaned worktrees are detected and cleaned up.
    #[tokio::test]
    async fn test_orphan_cleanup_on_startup();

    /// File tools reject absolute paths outside worktree root.
    #[tokio::test]
    async fn test_path_guard_blocks_escape();

    /// Concurrent merges are serialized and both succeed.
    #[tokio::test]
    async fn test_concurrent_merge_serialization();
}
```

### 23.10 Acceptance Criteria

- [ ] `WorktreeManager::create` produces a valid git worktree with correct branch name
- [ ] Branch follows pattern `rune/agent/{session_id_short}/{slug}`
- [ ] Worktree path is under `.rune/worktrees/{session_id}/`
- [ ] Worktree directory added to `.git/info/exclude`
- [ ] Sub-agent file tools are scoped to worktree root; paths outside are rejected
- [ ] Clean merge commits changes to base branch with `--no-ff`
- [ ] Conflicting merge is detected and returns conflict details
- [ ] LLM-assisted conflict resolution resolves simple conflicts automatically
- [ ] Unresolvable conflicts abort merge and set state to `Conflicted`
- [ ] `cleanup` removes worktree directory, branch, and exclude entry
- [ ] Orphaned worktrees are cleaned on startup
- [ ] Branch name collision appends numeric suffix
- [ ] Concurrent merges to the same base branch are serialized
- [ ] Worktree rows persisted with correct state transitions
- [ ] Orchestrator automatically creates worktrees for subtasks and merges on completion

### 23.11 Dependencies

No new crate dependencies. Git operations use `tokio::process::Command` to invoke the `git` CLI, which is already available in the runtime environment.

---

## Phase 24 — Intelligent Context Management

### 24.1 Overview

Replace the current static context assembly in `TurnExecutor` with a priority-based
context manager that dynamically allocates a token budget, compresses older turns
progressively, and shares relevant context between parent and child sessions.

### 24.2 Rust Types

#### File: `crates/rune-runtime/src/context_manager.rs`

```rust
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Token budget allocation across context categories.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContextBudget {
    /// Total tokens available (model context window minus safety margin).
    pub total: usize,
    /// Tokens reserved for the system prompt.
    pub system_prompt: usize,
    /// Tokens reserved for memory bank content.
    pub memory: usize,
    /// Tokens reserved for tool definitions.
    pub tools: usize,
    /// Tokens reserved for transcript (conversation history).
    pub transcript: usize,
    /// Tokens reserved for the current user message + response headroom.
    pub user_message: usize,
}

impl ContextBudget {
    /// Compute allocation given a model's context window size.
    ///
    /// Default split (adjustable via `AllocationStrategy`):
    /// - system_prompt: 10%
    /// - memory: 10%
    /// - tools: 15%
    /// - transcript: 50%
    /// - user_message: 15%
    pub fn allocate(total_tokens: usize, strategy: &AllocationStrategy) -> Self {
        let usable = (total_tokens as f64 * 0.95) as usize; // 5% safety margin
        Self {
            total: usable,
            system_prompt: (usable as f64 * strategy.system_prompt_pct) as usize,
            memory: (usable as f64 * strategy.memory_pct) as usize,
            tools: (usable as f64 * strategy.tools_pct) as usize,
            transcript: (usable as f64 * strategy.transcript_pct) as usize,
            user_message: (usable as f64 * strategy.user_message_pct) as usize,
        }
    }

    /// Remaining tokens not yet allocated.
    pub fn remaining(&self) -> usize {
        self.total
            .saturating_sub(
                self.system_prompt
                    + self.memory
                    + self.tools
                    + self.transcript
                    + self.user_message,
            )
    }
}

/// Configurable allocation percentages.
#[derive(Clone, Debug, Deserialize)]
pub struct AllocationStrategy {
    pub system_prompt_pct: f64,
    pub memory_pct: f64,
    pub tools_pct: f64,
    pub transcript_pct: f64,
    pub user_message_pct: f64,
}

impl Default for AllocationStrategy {
    fn default() -> Self {
        Self {
            system_prompt_pct: 0.10,
            memory_pct: 0.10,
            tools_pct: 0.15,
            transcript_pct: 0.50,
            user_message_pct: 0.15,
        }
    }
}

/// A scored context item ready for budget packing.
#[derive(Clone, Debug)]
pub struct ScoredContextItem {
    /// Source identifier (turn ID, memory key, etc.).
    pub source: String,
    /// Priority score in [0.0, 1.0]. Higher = more important.
    pub priority: f64,
    /// Estimated token count.
    pub tokens: usize,
    /// The content to include.
    pub content: String,
    /// Category for budget allocation.
    pub category: ContextCategory,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextCategory {
    SystemPrompt,
    Memory,
    Tool,
    Transcript,
    UserMessage,
}

/// Priority scoring factors.
#[derive(Clone, Debug)]
pub struct PriorityFactors {
    /// Recency: turns closer to now score higher. Weight: 0.4.
    pub recency: f64,
    /// Semantic similarity to the current user message. Weight: 0.3.
    pub relevance: f64,
    /// Explicit user references (@-mentions, quoted text). Weight: 0.2.
    pub explicit_ref: f64,
    /// Tool output importance (errors score higher). Weight: 0.1.
    pub tool_importance: f64,
}

impl PriorityFactors {
    pub fn score(&self) -> f64 {
        self.recency * 0.4
            + self.relevance * 0.3
            + self.explicit_ref * 0.2
            + self.tool_importance * 0.1
    }
}

/// Assembled context ready to be passed to the model.
#[derive(Clone, Debug)]
pub struct AssembledContext {
    pub system_prompt: String,
    pub messages: Vec<ContextMessage>,
    pub tool_definitions: Vec<serde_json::Value>,
    pub budget_used: ContextBudget,
    /// Items that were dropped due to budget constraints.
    pub dropped_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContextMessage {
    pub role: String,
    pub content: String,
    /// Whether this message was compressed from the original.
    pub compressed: bool,
    /// Original turn ID if available.
    pub turn_id: Option<Uuid>,
}

/// Main context manager that replaces static context assembly.
pub struct ContextManager {
    strategy: AllocationStrategy,
    model_context_window: usize,
}

impl ContextManager {
    pub fn new(model_context_window: usize, strategy: AllocationStrategy) -> Self {
        Self {
            strategy,
            model_context_window,
        }
    }

    /// Assemble context for a turn.
    ///
    /// Algorithm:
    /// 1. Compute `ContextBudget` from model window + strategy.
    /// 2. Score all transcript items by `PriorityFactors`.
    /// 3. Sort by priority descending.
    /// 4. Greedily pack items into their category budget.
    ///    - System prompt is always included (hard requirement).
    ///    - Most recent N turns are always included (hard requirement, N=3).
    ///    - Remaining transcript packed by score until budget exhausted.
    /// 5. If transcript budget exceeded, trigger compression on oldest included turns.
    /// 6. Return `AssembledContext`.
    pub async fn assemble(
        &self,
        session_id: Uuid,
        current_message: &str,
        transcript_items: &[ScoredContextItem],
        memory_items: &[ScoredContextItem],
        tool_defs: &[serde_json::Value],
        system_prompt: &str,
    ) -> Result<AssembledContext, crate::error::RuntimeError> {
        todo!()
    }

    /// Re-allocate budget dynamically based on session characteristics.
    ///
    /// - Conversation-heavy (many short turns): increase transcript to 60%, decrease tools to 10%.
    /// - Knowledge-heavy (long tool outputs): increase tools to 25%, decrease transcript to 40%.
    /// - Detection: ratio of tool-output tokens to user-message tokens.
    pub fn adapt_strategy(
        &mut self,
        transcript_token_ratio: f64,
        tool_output_token_ratio: f64,
    ) {
        if tool_output_token_ratio > 0.5 {
            self.strategy.tools_pct = 0.25;
            self.strategy.transcript_pct = 0.40;
        } else if transcript_token_ratio > 0.7 {
            self.strategy.transcript_pct = 0.60;
            self.strategy.tools_pct = 0.10;
        }
        // Normalize to ensure sum ≤ 1.0
        let sum = self.strategy.system_prompt_pct
            + self.strategy.memory_pct
            + self.strategy.tools_pct
            + self.strategy.transcript_pct
            + self.strategy.user_message_pct;
        if sum > 1.0 {
            let scale = 1.0 / sum;
            self.strategy.system_prompt_pct *= scale;
            self.strategy.memory_pct *= scale;
            self.strategy.tools_pct *= scale;
            self.strategy.transcript_pct *= scale;
            self.strategy.user_message_pct *= scale;
        }
    }
}
```

#### File: `crates/rune-runtime/src/context_compression.rs`

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::RuntimeError;

/// Detail level for compressed transcript items.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetailLevel {
    /// Full original content.
    Full,
    /// Summarized to 2-3 sentences per turn.
    Summary,
    /// Collapsed to a single bullet point per turn.
    Bullet,
    /// Omitted entirely (only metadata retained).
    Omitted,
}

/// A compressed representation of one or more transcript turns.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompressedSegment {
    /// ID of this compressed segment.
    pub id: Uuid,
    pub session_id: Uuid,
    /// Turn IDs covered by this segment.
    pub turn_ids: Vec<Uuid>,
    /// The compressed text content.
    pub content: String,
    /// Detail level of this segment.
    pub detail: DetailLevel,
    /// Approximate token count of the compressed content.
    pub token_count: usize,
    /// Original token count before compression.
    pub original_token_count: usize,
    pub created_at: DateTime<Utc>,
}

/// Checkpoint saved to the database for session resumption.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompressionCheckpoint {
    pub id: Uuid,
    pub session_id: Uuid,
    /// Ordered compressed segments forming the session history.
    pub segments: Vec<CompressedSegment>,
    /// Turn ID of the newest turn included in the checkpoint.
    pub up_to_turn_id: Uuid,
    /// Total token count of the checkpoint.
    pub total_tokens: usize,
    pub created_at: DateTime<Utc>,
}

/// Checkpoint wire format for DB JSON column.
///
/// ```json
/// {
///   "id": "uuid",
///   "session_id": "uuid",
///   "segments": [
///     {
///       "id": "uuid",
///       "turn_ids": ["uuid", "uuid"],
///       "content": "User asked about auth. Agent suggested JWT with refresh tokens.",
///       "detail": "summary",
///       "token_count": 42,
///       "original_token_count": 850
///     },
///     {
///       "id": "uuid",
///       "turn_ids": ["uuid"],
///       "content": "- Implemented JWT middleware",
///       "detail": "bullet",
///       "token_count": 8,
///       "original_token_count": 1200
///     }
///   ],
///   "up_to_turn_id": "uuid",
///   "total_tokens": 50,
///   "created_at": "2026-03-15T10:00:00Z"
/// }
/// ```

/// Progressive compression strategy.
///
/// Token distance from the most recent turn determines detail level:
/// - Turns 0..N (recent): `Full` (no compression)
/// - Turns N..2N: `Summary` (2-3 sentence summaries)
/// - Turns 2N..4N: `Bullet` (single bullet point each)
/// - Turns > 4N: `Omitted` (metadata only)
///
/// N is computed from `transcript_budget / estimated_tokens_per_full_turn`.
#[derive(Clone, Debug, Deserialize)]
pub struct CompressionConfig {
    /// Number of most-recent turns to keep at Full detail.
    pub full_detail_turns: usize,
    /// Model used for summarization. Uses the session's model by default.
    pub summarization_model: Option<String>,
    /// Trigger compression when transcript tokens exceed this fraction of budget.
    pub trigger_threshold: f64,
    /// Target token count after compression (as fraction of budget).
    pub target_ratio: f64,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            full_detail_turns: 6,
            summarization_model: None,
            trigger_threshold: 0.85,
            target_ratio: 0.60,
        }
    }
}

/// Compresses transcript segments and manages checkpoints.
pub struct ContextCompressor {
    config: CompressionConfig,
}

impl ContextCompressor {
    pub fn new(config: CompressionConfig) -> Self {
        Self { config }
    }

    /// Compress a batch of transcript turns into a `CompressedSegment`.
    ///
    /// Uses the model to generate a summary at the requested `DetailLevel`.
    /// For `Bullet` level, the prompt requests a single bullet point.
    /// For `Summary` level, 2-3 sentences.
    pub async fn compress(
        &self,
        turns: &[(Uuid, String)],
        target_detail: DetailLevel,
    ) -> Result<CompressedSegment, RuntimeError> {
        todo!("LLM summarization call")
    }

    /// Apply progressive compression to a full transcript.
    ///
    /// Returns an ordered list of segments at varying detail levels,
    /// plus any existing checkpoint segments that are reused.
    pub async fn compress_progressive(
        &self,
        session_id: Uuid,
        transcript: &[(Uuid, String, usize)], // (turn_id, content, tokens)
        existing_checkpoint: Option<&CompressionCheckpoint>,
        transcript_budget: usize,
    ) -> Result<Vec<CompressedSegment>, RuntimeError> {
        todo!("progressive detail-level assignment + compress")
    }

    /// Save a checkpoint to the database.
    pub async fn save_checkpoint(
        &self,
        checkpoint: &CompressionCheckpoint,
    ) -> Result<(), RuntimeError> {
        todo!("persist to context_checkpoints table")
    }

    /// Load the most recent checkpoint for a session.
    pub async fn load_checkpoint(
        &self,
        session_id: Uuid,
    ) -> Result<Option<CompressionCheckpoint>, RuntimeError> {
        todo!("query context_checkpoints table")
    }
}
```

#### File: `crates/rune-runtime/src/context_sharing.rs`

```rust
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::context_compression::CompressedSegment;
use crate::context_manager::ScoredContextItem;
use crate::error::RuntimeError;

/// Context package prepared for a sub-agent.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SharedContext {
    /// ID of the parent session.
    pub parent_session_id: Uuid,
    /// ID of the child session receiving this context.
    pub child_session_id: Uuid,
    /// Subtask description providing task focus.
    pub task_description: String,
    /// Compressed parent transcript (scoped to relevant segments).
    pub transcript_segments: Vec<CompressedSegment>,
    /// Memory bank items inherited from parent.
    pub memory_items: Vec<SharedMemoryItem>,
    /// Explicit context fragments selected by the orchestrator.
    pub context_hints: Vec<String>,
    /// Total token count of shared context.
    pub total_tokens: usize,
}

/// A memory item shared from parent to child.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SharedMemoryItem {
    pub key: String,
    pub content: String,
    pub tokens: usize,
}

/// Result produced by a sub-agent to propagate back to parent.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PropagatedResult {
    pub child_session_id: Uuid,
    pub subtask_id: Uuid,
    /// Summary of the child's work (compressed).
    pub summary: String,
    /// Key findings or artifacts to inject into parent context.
    pub findings: Vec<String>,
    /// New memory items discovered by the child.
    pub new_memory: Vec<SharedMemoryItem>,
    pub tokens: usize,
}

/// Manages context sharing between parent and child sessions.
pub struct ContextSharing;

impl ContextSharing {
    /// Prepare context for a sub-agent session.
    ///
    /// Algorithm:
    /// 1. Load parent's compression checkpoint.
    /// 2. Score parent transcript segments by relevance to `task_description`
    ///    using keyword overlap + semantic similarity (if embedding available).
    /// 3. Select top-K segments that fit within the child's context budget
    ///    allocation for inherited context (default: 20% of child's transcript budget).
    /// 4. Load parent memory bank, filter to items relevant to the task.
    /// 5. Package into `SharedContext`.
    pub async fn prepare_for_child(
        parent_session_id: Uuid,
        child_session_id: Uuid,
        task_description: &str,
        context_hints: Vec<String>,
        child_budget_tokens: usize,
    ) -> Result<SharedContext, RuntimeError> {
        todo!()
    }

    /// Propagate a child's results back to the parent's context.
    ///
    /// Algorithm:
    /// 1. Compress the child's transcript into a summary.
    /// 2. Extract key findings (files modified, errors encountered, decisions made).
    /// 3. Extract any new memory items the child created.
    /// 4. Package into `PropagatedResult`.
    /// 5. Inject into parent's next context assembly as high-priority items.
    pub async fn propagate_to_parent(
        child_session_id: Uuid,
        subtask_id: Uuid,
    ) -> Result<PropagatedResult, RuntimeError> {
        todo!()
    }
}
```

### 24.3 Priority Scoring (Detail)

Each transcript item is scored:

```
priority = recency * 0.4 + relevance * 0.3 + explicit_ref * 0.2 + tool_importance * 0.1
```

| Factor | Computation | Range |
|---|---|---|
| `recency` | `1.0 - (turn_index / total_turns)` | [0.0, 1.0] |
| `relevance` | Jaccard similarity of turn tokens vs current message tokens (or cosine similarity if embeddings available) | [0.0, 1.0] |
| `explicit_ref` | 1.0 if user quoted or @-mentioned content from this turn, else 0.0 | {0.0, 1.0} |
| `tool_importance` | 1.0 if tool output contains "error"/"fail"/"panic", 0.7 for tool calls, 0.3 for assistant text, 0.0 for omitted | [0.0, 1.0] |

Hard constraints (never dropped regardless of score):
- System prompt
- Most recent 3 turns
- Current user message

### 24.4 Token Budget Allocation (Detail)

For a 128k-token model with default strategy:

| Category | Percentage | Tokens |
|---|---|---|
| Safety margin | 5% | 6,400 |
| System prompt | 10% | 12,160 |
| Memory | 10% | 12,160 |
| Tools | 15% | 18,240 |
| Transcript | 50% | 60,800 |
| User message | 15% | 18,240 |
| **Usable total** | 95% | **121,600** |

Dynamic adaptation thresholds:
- If `tool_output_tokens / total_tokens > 0.5`: shift tools to 25%, transcript to 40%.
- If `user_message_tokens / total_tokens > 0.7`: shift transcript to 60%, tools to 10%.
- After adaptation, percentages are normalized to sum to 1.0.

### 24.5 Compression Checkpoint Format

Stored as JSONB in the `context_checkpoints` table:

```json
{
  "id": "a1b2c3d4-...",
  "session_id": "550e8400-...",
  "segments": [
    {
      "id": "seg-001",
      "turn_ids": ["turn-1", "turn-2", "turn-3"],
      "content": "User requested auth system. Agent designed JWT flow with refresh tokens and implemented middleware.",
      "detail": "summary",
      "token_count": 35,
      "original_token_count": 2400
    },
    {
      "id": "seg-002",
      "turn_ids": ["turn-4"],
      "content": "- Fixed token expiry bug in refresh endpoint",
      "detail": "bullet",
      "token_count": 12,
      "original_token_count": 900
    },
    {
      "id": "seg-003",
      "turn_ids": ["turn-5", "turn-6"],
      "content": "User: Can you add rate limiting?\nAssistant: I'll add a token bucket rate limiter...\n[full content]",
      "detail": "full",
      "token_count": 1800,
      "original_token_count": 1800
    }
  ],
  "up_to_turn_id": "turn-6",
  "total_tokens": 1847,
  "created_at": "2026-03-15T14:30:00Z"
}
```

Progressive detail bands (for `full_detail_turns = 6`):
- Turns 1-6 (most recent): `Full`
- Turns 7-12: `Summary`
- Turns 13-24: `Bullet`
- Turns 25+: `Omitted`

### 24.6 Wire Protocol

No new REST endpoints. Context management is internal to `TurnExecutor`. Diagnostic
information is exposed via the existing debug/status endpoints.

Session metadata includes context budget usage:

```json
{
  "context": {
    "budget": {
      "total": 121600,
      "system_prompt": 12160,
      "memory": 12160,
      "tools": 18240,
      "transcript": 60800,
      "user_message": 18240
    },
    "used": {
      "system_prompt": 850,
      "memory": 2400,
      "tools": 5600,
      "transcript": 45000,
      "user_message": 1200
    },
    "compression": {
      "checkpoint_exists": true,
      "segments": 12,
      "compression_ratio": 0.35,
      "oldest_detail_level": "bullet"
    },
    "dropped_items": 3
  }
}
```

### 24.7 Error Cases

```rust
#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    #[error("context budget exceeded: {used} tokens > {budget} total")]
    BudgetExceeded { used: usize, budget: usize },

    #[error("system prompt exceeds its budget: {tokens} > {budget}")]
    SystemPromptTooLarge { tokens: usize, budget: usize },

    #[error("compression failed for session {session_id}: {reason}")]
    CompressionFailed { session_id: Uuid, reason: String },

    #[error("checkpoint not found for session {session_id}")]
    CheckpointNotFound { session_id: Uuid },

    #[error("checkpoint corrupted for session {session_id}: {reason}")]
    CheckpointCorrupted { session_id: Uuid, reason: String },

    #[error("context sharing failed from {parent} to {child}: {reason}")]
    SharingFailed {
        parent: Uuid,
        child: Uuid,
        reason: String,
    },

    #[error("token counting failed: {reason}")]
    TokenCountError { reason: String },

    #[error("model context window unknown for model {model}")]
    UnknownContextWindow { model: String },
}
```

### 24.8 Edge Cases

1. **System prompt exceeds its budget**: System prompt is a hard requirement. If it exceeds its allocation, steal tokens from memory and transcript proportionally. If it exceeds 30% of total, return `SystemPromptTooLarge` error.
2. **Zero transcript turns**: First turn in a new session. Skip compression, allocate full transcript budget to the current exchange.
3. **Checkpoint deserialization failure**: If the stored checkpoint JSON is corrupted, log a warning, discard the checkpoint, and recompress from the raw transcript. Set `CheckpointCorrupted` as a soft error (non-fatal).
4. **Concurrent checkpoint writes**: Two turns for the same session race to save a checkpoint. Use `INSERT ... ON CONFLICT (session_id) DO UPDATE` with a `created_at` check — newest wins.
5. **Sub-agent inherits stale context**: The parent's checkpoint was created N turns ago. When preparing `SharedContext`, load both checkpoint + raw turns since the checkpoint to get the latest state.
6. **Token count estimation drift**: Token counts are estimated (not exact tiktoken). Maintain a 5% safety margin (already in `ContextBudget::allocate`). If the model returns a token-limit error, reduce the transcript budget by 10% and retry the turn once.
7. **Summarization model unavailable**: If the configured summarization model fails, fall back to simple truncation (keep first and last sentence of each turn) rather than blocking the session.

### 24.9 SQL Migrations

```sql
-- 20260318000001_create_context_checkpoints.up.sql

CREATE TABLE context_checkpoints (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    session_id      UUID NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    up_to_turn_id   UUID NOT NULL,
    segments        JSONB NOT NULL,
    total_tokens    INT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX idx_context_checkpoints_session
    ON context_checkpoints(session_id);

CREATE TABLE shared_contexts (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    parent_session_id   UUID NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    child_session_id    UUID NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    subtask_id          UUID REFERENCES subtasks(id) ON DELETE SET NULL,
    context_payload     JSONB NOT NULL,
    total_tokens        INT NOT NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_shared_contexts_parent ON shared_contexts(parent_session_id);
CREATE INDEX idx_shared_contexts_child ON shared_contexts(child_session_id);
```

```sql
-- 20260318000001_create_context_checkpoints.down.sql

DROP TABLE IF EXISTS shared_contexts;
DROP TABLE IF EXISTS context_checkpoints;
```

### 24.10 Integration Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// ContextBudget::allocate produces correct splits for a 128k model.
    #[test]
    fn test_budget_allocation_128k();

    /// Budget remaining is zero when all categories sum to total.
    #[test]
    fn test_budget_remaining_zero_when_full();

    /// PriorityFactors::score computes weighted sum correctly.
    #[test]
    fn test_priority_scoring();

    /// Most recent 3 turns are always included regardless of score.
    #[tokio::test]
    async fn test_hard_constraint_recent_turns();

    /// Items are packed greedily by score, respecting category budgets.
    #[tokio::test]
    async fn test_greedy_packing_by_score();

    /// When transcript exceeds budget, oldest turns are compressed.
    #[tokio::test]
    async fn test_compression_triggered_on_threshold();

    /// Progressive compression assigns correct detail levels.
    #[tokio::test]
    async fn test_progressive_detail_levels();

    /// Checkpoint is saved and loaded correctly (round-trip).
    #[tokio::test]
    async fn test_checkpoint_round_trip();

    /// Corrupted checkpoint is discarded with warning, recompression occurs.
    #[tokio::test]
    async fn test_corrupted_checkpoint_recovery();

    /// Concurrent checkpoint writes — newest wins via ON CONFLICT.
    #[tokio::test]
    async fn test_concurrent_checkpoint_newest_wins();

    /// adapt_strategy shifts budget toward tools for tool-heavy sessions.
    #[test]
    fn test_adapt_strategy_tool_heavy();

    /// adapt_strategy shifts budget toward transcript for conversation-heavy sessions.
    #[test]
    fn test_adapt_strategy_conversation_heavy();

    /// Strategy percentages are normalized after adaptation.
    #[test]
    fn test_strategy_normalization();

    /// SharedContext is prepared with relevant parent segments only.
    #[tokio::test]
    async fn test_shared_context_relevance_filtering();

    /// PropagatedResult flows back to parent context assembly.
    #[tokio::test]
    async fn test_result_propagation_to_parent();

    /// System prompt exceeding 30% of total triggers error.
    #[tokio::test]
    async fn test_system_prompt_too_large_error();

    /// Token count estimation drift: retry on model token-limit error.
    #[tokio::test]
    async fn test_retry_on_token_limit_error();

    /// Summarization model failure falls back to truncation.
    #[tokio::test]
    async fn test_compression_fallback_on_model_failure();
}
```

### 24.11 Acceptance Criteria

- [ ] `ContextManager` replaces static context assembly in `TurnExecutor`
- [ ] Token budget is computed from model's context window with 5% safety margin
- [ ] Budget allocation splits across 5 categories with configurable percentages
- [ ] Priority scoring ranks transcript items by recency, relevance, explicit refs, tool importance
- [ ] Most recent 3 turns and system prompt are always included (hard constraints)
- [ ] Greedy packing fills categories by score until budget exhausted
- [ ] Dynamic adaptation shifts budget for tool-heavy vs conversation-heavy sessions
- [ ] Percentages are normalized after adaptation
- [ ] Compression triggers at 85% of transcript budget
- [ ] Progressive detail: Full → Summary → Bullet → Omitted based on turn age
- [ ] Compression checkpoint saved to `context_checkpoints` table as JSONB
- [ ] Checkpoint round-trips correctly (save + load produces identical segments)
- [ ] Corrupted checkpoint is discarded gracefully, recompression occurs
- [ ] Concurrent checkpoint writes resolve via newest-wins
- [ ] Sub-agent receives scoped context from parent (relevant segments + memory only)
- [ ] Sub-agent results propagate back to parent context as high-priority items
- [ ] System prompt exceeding 30% of total returns error
- [ ] Token-limit model error triggers 10% transcript budget reduction and single retry
- [ ] Summarization model failure falls back to truncation (first + last sentence)
- [ ] `dropped_count` in `AssembledContext` accurately reflects omitted items

### 24.12 Dependencies

| Crate | Version | Purpose |
|---|---|---|
| `tiktoken-rs` | 0.6 | Accurate token counting for OpenAI-compatible models |

All other dependencies already in workspace.

---

## Cross-Phase Integration Notes

### Execution Order

Phase 22 is implemented first. Phase 23 depends on the orchestrator's subtask
spawning. Phase 24 depends on Phase 22 for context sharing between orchestrator
and sub-agents.

```
Phase 22 (Modes + Orchestrator)
    │
    ├──► Phase 23 (Worktree Isolation) — uses orchestrator subtask lifecycle
    │
    └──► Phase 24 (Context Management) — uses orchestrator context sharing
```

### Modified Files Summary

| File | Phase | Change |
|---|---|---|
| `crates/rune-runtime/src/executor.rs` | 22, 23, 24 | Route through mode system prompt + tool filter; set working_dir per worktree; replace static context with `ContextManager` |
| `crates/rune-config/src/lib.rs` | 22 | Add `[modes]` config section (default_mode, modes_dir) |
| `crates/rune-tools/src/lib.rs` | 23 | Scope file tools to worktree root |
| `crates/rune-store/src/repos.rs` | 22, 23, 24 | Add `SubtaskRepo`, `WorktreeRepo`, `ContextCheckpointRepo`, `SharedContextRepo` |
| `crates/rune-gateway/src/routes.rs` | 22 | Add `/modes`, `/sessions/{id}/mode` routes |

### New Repo Traits (for `rune-store/src/repos.rs`)

```rust
#[async_trait]
pub trait SubtaskRepo: Send + Sync {
    async fn create(&self, subtask: NewSubtask) -> Result<SubtaskRow, StoreError>;
    async fn find_by_id(&self, id: Uuid) -> Result<SubtaskRow, StoreError>;
    async fn list_by_task(&self, task_id: Uuid) -> Result<Vec<SubtaskRow>, StoreError>;
    async fn update_status(
        &self,
        id: Uuid,
        status: &str,
        started_at: Option<DateTime<Utc>>,
        completed_at: Option<DateTime<Utc>>,
    ) -> Result<SubtaskRow, StoreError>;
}

#[async_trait]
pub trait WorktreeRepo: Send + Sync {
    async fn create(&self, worktree: NewWorktree) -> Result<WorktreeRow, StoreError>;
    async fn find_by_id(&self, id: Uuid) -> Result<WorktreeRow, StoreError>;
    async fn find_by_session(&self, session_id: Uuid) -> Result<Option<WorktreeRow>, StoreError>;
    async fn update_state(
        &self,
        id: Uuid,
        state: &str,
        merged_at: Option<DateTime<Utc>>,
        merge_commit: Option<&str>,
    ) -> Result<WorktreeRow, StoreError>;
    async fn list_by_state(&self, state: &str) -> Result<Vec<WorktreeRow>, StoreError>;
}

#[async_trait]
pub trait ContextCheckpointRepo: Send + Sync {
    async fn upsert(&self, checkpoint: NewContextCheckpoint) -> Result<ContextCheckpointRow, StoreError>;
    async fn find_by_session(&self, session_id: Uuid) -> Result<Option<ContextCheckpointRow>, StoreError>;
    async fn delete_by_session(&self, session_id: Uuid) -> Result<bool, StoreError>;
}

#[async_trait]
pub trait SharedContextRepo: Send + Sync {
    async fn create(&self, ctx: NewSharedContext) -> Result<SharedContextRow, StoreError>;
    async fn find_by_child(&self, child_session_id: Uuid) -> Result<Option<SharedContextRow>, StoreError>;
    async fn list_by_parent(&self, parent_session_id: Uuid) -> Result<Vec<SharedContextRow>, StoreError>;
}
```
