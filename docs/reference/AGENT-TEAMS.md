# Agent Teams

This document defines the Rune-native agent-team model: how a parent session delegates work to multiple child workers, how context and artifacts flow, what lifecycle states mean, and how this integrates with Rune's existing subagent/session/runtime primitives.

It is intentionally grounded in **current Rune implementation reality** while pulling useful lessons from Claude Code's **subagents** and **agent teams** concepts:
- use specialized delegated workers instead of overloading one long context
- keep the parent as the planner / synthesizer
- give each worker bounded scope, explicit tasking, and isolated context
- return compact results and artifacts back to the parent
- preserve operator visibility into delegation, status, and outcomes

Rune should adopt those ideas, but in Rune's own architecture and parity constraints.

---

## 1. Why agent teams exist

A single session is good for linear work. It is bad at:
- parallelizing independent tasks
- keeping multiple long-running investigations isolated
- separating planner context from worker context
- auditing delegated work in a durable, inspectable way

Rune already has the base primitives for delegation:
- `subagent` session kind
- `requester_session_id` parent/child linkage
- `sessions_spawn`-style child creation
- optional `delegation_context`, `delegation_plan`, and `shared_scratchpad_path`
- steering and kill/cancel surfaces for child sessions
- kill/cancel responses include previous status, cancellation timestamp, and explicit non-auto-resume semantics for auditability
- durable subagent lifecycle metadata on session status/tree APIs
- orchestration state with file locks, goal leases, and merge queue tracking

Agent teams are the higher-level contract that turns those primitives into a predictable execution model.

---

## 2. Core model

An **agent team** is a parent-led delegation graph rooted in one session.

### Roles

#### Lead agent
The parent session is the lead.

Responsibilities:
- understand the user objective
- break work into bounded sub-tasks
- decide which tasks can run in parallel
- create and track worker sessions
- synthesize worker results into the final answer or next action
- own external communication and final operator-visible output

The lead is not just another worker. It is the control plane for the team.

#### Worker agent
A worker is typically a `subagent` session created by the lead.

Responsibilities:
- execute one bounded delegated task
- stay within the assigned scope
- emit compact summaries and artifacts back to the lead
- avoid mutating overlapping files or branches unless orchestration locks permit it

Workers should be disposable and specialized, not general long-lived authorities.

---

## 3. Team topology

Rune should support these topologies. The design intentionally favors shallow, inspectable teams over opaque recursion.

### 3.1 Single-worker delegation
One lead delegates one bounded task to one child.

Use when:
- the lead needs focused research, code review, testing, or implementation help
- the delegated task benefits from context isolation

This maps directly to today's `subagent` session model.

### 3.2 Parallel fan-out
One lead delegates multiple independent tasks to sibling workers.

Use when:
- tasks do not share mutable state
- tasks can be merged by synthesis rather than shared live editing
- the lead can coordinate final consolidation

Examples:
- one worker audits docs, another audits tests, another audits runtime gaps
- one worker explores issue references while another inspects code paths

### 3.3 Hierarchical delegation
A worker may spawn its own worker.

Rune already allows parent/requester linkage deeply enough for trees. But this should remain constrained.

Recommended rule:
- default to **one explicit lead with shallow workers**
- allow deeper delegation only when necessary and inspectable

Why:
- deep delegation trees are harder to audit
- status explanations degrade quickly
- file/branch collision risk grows with depth

---

## 4. Delegation contract

Every worker creation should carry a structured delegation contract, whether surfaced as explicit JSON metadata or encoded through equivalent runtime fields.

Minimum contract:
- `goal`: what the worker must accomplish
- `scope`: explicit boundaries, including files, systems, and forbidden side quests
- `deliverable`: what the lead expects back
- `constraints`: tests, safety rails, time/effort budget, non-goals
- `artifacts`: where to write any shared notes or result files
- `success_criteria`: what counts as completion

Rune already has partial fields for this:
- `delegation_context`
- `delegation_plan`
- `shared_scratchpad_path`
- session metadata

Recommended shape:

```json
{
  "goal": "Audit the current session status/subagent lifecycle surfaces for agent-team readiness",
  "scope": {
    "paths": [
      "crates/rune-gateway/src/routes.rs",
      "crates/rune-gateway/src/ws_rpc.rs",
      "docs/reference"
    ],
    "disallow_mutation": true
  },
  "deliverable": {
    "summary": "Compact findings list",
    "artifacts": [
      "memory://subagents/<id>/readiness-notes.md"
    ]
  },
  "constraints": {
    "max_parallelism": 1,
    "must_reference_existing_behavior": true,
    "must_not_claim_unimplemented_runtime_attachment": true
  },
  "success_criteria": [
    "List what exists today",
    "List highest-value gaps",
    "Recommend next implementation slices"
  ]
}
```

The exact serialized format can evolve. The invariant should not.

Recommended API/tooling direction:
- `sessions_spawn` should grow a first-class `delegation_contract` field instead of relying on loosely-shaped metadata blobs
- gateway and WS session-create surfaces should mirror that field so lead agents, UI clients, and automation all speak the same delegation language
- status/tree payloads should echo the normalized contract summary back for inspection without forcing operators to inspect raw transcript text

---

## 5. Context handoff

Claude Code's subagent/team model is useful here: workers should not inherit the entire parent context blindly.

Rune should prefer **selective handoff** over full transcript cloning.

### Handoff should include
- the delegated task statement
- the minimal relevant transcript slice
- explicit repo/project/workspace context
- references to relevant files/issues/docs
- any required locks/goal ownership information
- artifact destinations and return contract

### Handoff should avoid
- dumping full parent history by default
- mixing unrelated objectives into worker context
- making the worker infer hidden success criteria

### Current Rune fit
Current Rune already supports preloaded delegation context and shared scratchpad references on subagent creation. The next maturity step is to formalize what the handoff must contain and expose it more cleanly in API/tool surfaces.

Recommended handoff envelope fields:
- `task_summary` — one-paragraph worker brief
- `selected_transcript_range` — explicit transcript excerpts or message ids included on purpose
- `relevant_paths` — repo paths the worker may inspect or mutate
- `linked_issues` / `linked_docs` — issue numbers and docs references that define done-ness
- `lock_context` — file locks, branch reservations, or goal leases already granted
- `return_contract` — summary shape, artifact paths, and required verification evidence

---

## 6. Results and artifact flow

A worker should return two things:

### 6.1 Summary
A compact, parent-readable summary suitable for synthesis.

Example:
- what was done
- what was verified
- blockers
- recommended next step

### 6.2 Artifacts
Optional durable outputs:
- notes
- patch references
- test logs
- review findings
- generated files
- memory URIs

Rune already has durable `subagent_result` transcript items and a `latest_subagent_result` summary on session status surfaces. That is the right primitive.

Agent-team expectation:
- every completed worker should emit a result record
- summaries should be compact enough for parent synthesis
- artifacts should be durable and inspectable
- parent transcript should record when results were received and acted on

---

## 7. Lifecycle model

Rune already exposes subagent lifecycle metadata like:
- `queued`
- `attached`
- `steered`
- `cancelled`
- `preempted`
- runtime attachment/status fields

Agent teams need a clearer semantic model on top.

### Recommended team lifecycle phases

#### Parent / lead
- `planning` — lead is decomposing work
- `delegating` — lead is spawning or assigning workers
- `waiting_for_subagent` — lead is blocked on delegated progress
- `synthesizing` — lead is merging worker outputs
- `completed` / `failed` / `cancelled`

#### Worker
- `queued` — created, not yet attached to runtime
- `attached` — runtime attached and actively executing
- `running` — task in progress
- `steered` — operator/lead sent updated guidance
- `blocked` — worker hit a real blocker
- `completed` — worker emitted final result
- `failed` — worker failed without valid output
- `cancelled` — explicitly stopped
- `preempted` — superseded by higher-priority routing

### Important honesty rule
Rune must keep distinguishing:
- **durable lifecycle knowledge**
- **actual runtime attachment parity**

Current code already does this honestly by surfacing that subagent runtime execution remains conservative and full remote/runtime attachment parity is incomplete. Agent-team UX must preserve that honesty instead of papering it over.

---

## 8. Safety and concurrency constraints

Parallel delegation is only safe when mutable state is controlled.

Rune already has the right foundational ideas:
- orchestrator file locks
- goal leases / duplicate suppression
- merge queue state
- branch reservation guidance in gateway policy surfaces

Agent teams should explicitly rely on those primitives.

### 8.1 File mutation rule
A worker must not mutate repo paths unless it holds the relevant orchestrator locks or is assigned a disjoint scope.

### 8.2 Goal ownership rule
If a delegated task corresponds to a durable goal, only one active worker should own that goal at a time unless the plan explicitly defines a read-only split.

### 8.3 Branch rule
If a delegated coding task produces branch-based output, branch names should be reserved before work begins, and workers should not share a mutable branch accidentally.

### 8.4 Parallelism rule
Fan-out should be bounded by:
- task independence
- runtime capacity
- operator visibility
- consolidation cost

More workers is not automatically better.

---

## 9. Relationship to Rune tools and APIs

### `sessions_spawn`
This is the session-level worker creation primitive.

Agent-team expectation:
- tool input should capture structured delegation intent, not just a free-form task string
- spawned workers should inherit requester linkage and project scoping cleanly
- returned metadata should make the child immediately inspectable

### `subagents` controls
These are worker control primitives.

Agent-team expectation:
- steer for scope correction or tighter objectives
- cancel for invalid, duplicate, or superseded work
- list/status for parent control loops and operator inspection

### `session_status` / session-tree surfaces
These are the observability backbone.

Agent-team expectation:
- show parent/child tree clearly
- show lifecycle, attachment state, last note, latest result
- explain whether the parent is waiting, synthesizing, or stalled
- surface team-level rollups such as active worker count, blocked worker count, and last team event

### `acp_dispatch`
This is a sibling but not identical concept.

Use it when:
- work is better delegated to an external coding agent runtime
- the task needs its own large context window and full tool autonomy

Agent teams should treat `acp_dispatch` as a **worker backend option**, not the only delegation model. A Rune agent team can include:
- native Rune subagent sessions
- external coding-agent subprocesses
- potentially both, if observability and contracts remain clear

---

## 10. Recommended operator-visible invariants

Rune should make these true for every agent team:

1. **A parent always remains accountable.**
   There is one lead session responsible for the final answer.

2. **Every worker has bounded scope.**
   No vague "go help with this" delegation.

3. **Every worker is inspectable.**
   Parent linkage, lifecycle, notes, and latest result must be queryable.

4. **Every mutation path is coordinated.**
   File locks, goals, and branches prevent hidden collisions.

5. **Every completion is artifact-backed.**
   A worker should return a summary and optional artifacts, not disappear silently.

6. **Unimplemented parity stays visible.**
   If runtime attachment is conservative/incomplete, status surfaces must say so.

---

## 11. What Rune has today vs what is still missing

### Exists today
- `subagent` session kind
- parent/requester linkage
- child creation with delegation context / scratchpad fields
- steer and kill controls
- durable subagent lifecycle/status metadata
- session tree and session status exposure of subagent fields
- durable latest subagent result summarization
- orchestrator state with file locks, goal leases, and merge queue primitives

### Missing or incomplete
- first-class documented **agent-team** contract in the runtime/control plane
- structured delegation payloads exposed consistently across tools/APIs
- explicit lead/team lifecycle semantics beyond raw subagent fields
- richer parent-side synthesis/explainability for multiple workers
- stronger runtime parity for attached/live subagent execution across all modes
- better bounded policies for nested delegation and large fan-out
- unified treatment of native subagents and `acp_dispatch` workers under one team abstraction

---

## 12. Recommended implementation sequence

### Phase 1 — design/documentation contract
- land this doc
- cross-link architecture and planning docs
- align terminology: lead, worker, delegation contract, result artifact, synthesis

### Phase 2 — structured worker creation
- extend `sessions_spawn` and related session creation surfaces to accept a first-class delegation contract
- normalize delegation metadata fields instead of relying on ad hoc blobs
- keep backward-compatible support for existing `delegation_context` / `delegation_plan` payloads during migration

### Phase 3 — team observability
Status: partially landed on current `main`
- session status already exposes durable orchestration explainability including status reasons, next-task reasons, approval-aware resume hints, unresolved parity notes, and latest delegated-result summaries
- remaining work is parent-side multi-worker rollups on status/tree surfaces: explicit team counts, waiting/synthesizing lead state, and latest team events across more than one worker
- keep making it obvious when fields reflect durable persisted state versus live runtime attachment

### Phase 4 — safety integration
- wire delegation flows more tightly to goal leases, file locks, and branch reservation primitives
- reject unsafe fan-out instead of merely documenting it

### Phase 5 — backend unification
- define how Rune-native subagents and `acp_dispatch` workers report into one consistent parent/team result model

---


## 12.1 Goal lease inspection surfaces

Feature #766 now includes a compact inspection layer for durable goal ownership state so parent agents, dashboards, and future gateway routes can answer "who owns what right now?" without re-implementing filtering rules.

State helpers available on `OrchestratorState`:
- `goal_lease(goal_key)` — fetch the persisted lease record for a specific goal key
- `agent_goal_lease(agent_id)` — fetch the lease currently held by a specific agent
- `active_goal_leases(now)` — return only unexpired leases suitable for operator-visible current-state views
- `stale_goal_leases(now)` — list expired leases without mutating state, useful for diagnostics before recovery
- `current_goal_owners(now)` — reduce active leases into a lightweight `goal_key -> owner_agent_id` map
- `goal_lease_summary(now, recent_conflict_limit)` — produce one inspection payload with total/active/stale counts, active owners, stale goal keys, and recent duplicate-suppression conflict records

Operational guidance:
- use `goal_lease_summary(...)` for overview/status surfaces that need one truthful snapshot
- use the more specific helpers for targeted enforcement or per-goal diagnostics
- inspection helpers intentionally exclude expired leases from "active owner" views so stale ownership is never presented as current execution
- stale leases remain persisted until explicit recovery/expiry handling runs, preserving auditability while keeping active views honest

## 13. Practical guidance for Rune agents

When deciding whether to use an agent team:

Use a worker when:
- the task is independent and bounded
- the task benefits from isolated context
- you need parallel investigation or implementation
- you can define clear success criteria and return format

Do not use a worker when:
- the work requires constant shared mutable state with the parent
- the task is so small that delegation overhead dominates
- the scope is too vague to contract cleanly
- observability or locking is not sufficient to keep the work safe

Lead behavior rule:
- decompose
- delegate only independent slices
- monitor honestly
- synthesize compactly
- never lose ownership of the final answer

---

## 14. Design note on Claude Code learnings

Useful lessons taken from Claude Code subagents / agent teams:
- specialization beats one overloaded context
- delegation needs explicit boundaries
- parent synthesis matters as much as worker execution
- compact result return is critical
- inspectability is part of the product, not a debug afterthought

What Rune should not copy blindly:
- any behavior that hides current parity limits
- any delegation flow that bypasses Rune's durable session/status model
- any parallelism model that ignores file locks, goal ownership, or branch safety
- any assumption that all worker backends share identical attachment, streaming, or cancellation semantics

Rune's version should remain:
- durable
- inspectable
- Azure-friendly
- operator-honest
- consistent with existing session and orchestration architecture
