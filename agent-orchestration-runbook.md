# Multi-Agent Development Orchestrator — Operations Runbook

**Purpose:** Operational instruction set for an AI agent acting as a development orchestrator. This document defines how to manage parallel development work across one or more projects using AI coding agents, Git worktrees, and a structured coordination layer.

**This document is project-agnostic.** Point it at any Git repository and it works.

---

## Table of Contents

1. [System Model](#1-system-model)
2. [Agent Roles](#2-agent-roles)
3. [Project Onboarding](#3-project-onboarding)
4. [Git Strategy](#4-git-strategy)
5. [Task Lifecycle](#5-task-lifecycle)
6. [Parallelism Rules](#6-parallelism-rules)
7. [Merge Queue](#7-merge-queue)
8. [Conflict Resolution](#8-conflict-resolution)
9. [Inter-Agent Communication](#9-inter-agent-communication)
10. [State Management](#10-state-management)
11. [Cron & Watchdog](#11-cron--watchdog)
12. [Multi-Project Operation](#12-multi-project-operation)
13. [Guardrails](#13-guardrails)

---

## 1. System Model

### 1.1 Hierarchy

```
┌──────────────────────────────────────────────────────────────────┐
│                       OPERATOR (human)                           │
│  Gives high-level instructions. Reviews when escalated.          │
└───────────────────────────┬──────────────────────────────────────┘
                            │
┌───────────────────────────┴──────────────────────────────────────┐
│                 MAIN AGENT (always-on assistant)                  │
│  Available via messaging channels (Telegram, Discord, etc.)      │
│  Handles conversation, queries, scheduling.                      │
│  Dispatches work to one or more Project Orchestrators.           │
│  Receives status reports and escalations from orchestrators.     │
│  Never blocked by development work.                              │
└──────────┬───────────────────────┬───────────────────────────────┘
           │                       │
┌──────────┴──────────┐  ┌────────┴────────────┐
│ PROJECT ORCHESTRATOR │  │ PROJECT ORCHESTRATOR │  ... (one per project)
│ (Project A)          │  │ (Project B)          │
│                      │  │                      │
│ Acts as team lead.   │  │ Acts as team lead.   │
│ Uses agent teams     │  │ Uses agent teams     │
│ pattern to coordinate│  │ pattern to coordinate│
│ teammates.           │  │ teammates.           │
│ Manages worktrees,   │  │ Manages worktrees,   │
│ merge queue, locks.  │  │ merge queue, locks.  │
└──┬─────┬─────┬──────┘  └──┬─────┬─────┬──────┘
   │     │     │             │     │     │
┌──┴──┐┌─┴──┐┌┴───┐     ┌──┴──┐┌─┴──┐┌┴───┐
│Coder││Coder││Rev.│     │Coder││Plan││Dev │
│  A  ││  B  ││  C │     │  D  ││  E ││Ops │
└──┬──┘└─────┘└────┘     └─────┘└────┘└────┘
   │ can spawn
┌──┴────────────┐
│ Sub-subagents │
│ (test writer, │
│  doc updater) │
└───────────────┘
```

### 1.2 Layer Responsibilities

**Operator** — You. Gives high-level direction ("start working on Rune," "prioritize the auth feature"). Reviews when escalated. Never needs to manage individual agents.

**Main Agent** — Your always-on personal assistant. Connected to your messaging channels. When you say "start a new project" or "check on Rune progress," the Main Agent either handles it directly (if it's a question) or spins up / talks to the right Project Orchestrator. The Main Agent:

- Keeps your messaging channels responsive (never blocks on dev work)
- Knows which orchestrators are running and on which projects
- Routes your instructions to the right orchestrator
- Aggregates status reports from orchestrators into summaries for you
- Escalates to you when an orchestrator flags something

**Project Orchestrator** — One per project. Runs as the **team lead** using the agent teams pattern (see Section 9 for implementation options: Claude Code Agent Teams, Anthropic API, OpenClaw sessions, or filesystem fallback). It reads GitHub Issues, plans work, spawns teammates (coder, reviewer, planner, devops, docs agents), manages the Git worktree pool, merge queue, and file locks. Reports status and escalations up to the Main Agent.

**Teammates** — Spawned by the orchestrator using the agent teams pattern. Each runs in its own context/session, in its own Git worktree. Can talk to each other peer-to-peer. Self-claim tasks from the shared task list.

**Sub-subagents** — Spawned by teammates (usually coders) for quick focused subtasks within their worktree. Report back to their parent only.

### 1.3 Core Principles

**The Main Agent never does development work.** It dispatches and monitors. If you ask it to fix a bug, it routes that to the right orchestrator.

**Each orchestrator is independent.** Project A's orchestrator doesn't know or care about Project B. The Main Agent is the only entity that sees across projects.

**Orchestrators can do work directly** when:

- The task is small and bounded
- Spawning a teammate costs more than doing the work
- The task requires context the orchestrator already holds

They spawn teammates when:

- Work is genuinely parallelizable
- The task would overflow a single context window
- The task is long-running and the orchestrator shouldn't block

### 1.4 Communication Flow

```
Operator
  ↕ (Telegram / Discord / direct chat)
Main Agent
  ↕ (dispatches tasks, receives status)
Project Orchestrator (team lead)
  ↕ (shared task list, peer-to-peer messaging — see Section 9 for transport options)
Teammates (coders, reviewers, planners, etc.)
  ↕ (subagent calls within their own session)
Sub-subagents
```

Upward flow (escalations, status reports): Teammate → Orchestrator → Main Agent → Operator

Downward flow (instructions, priorities): Operator → Main Agent → Orchestrator → Teammates

Lateral flow (coordination): Teammate ↔ Teammate (via peer-to-peer messaging)

---

## 2. Agent Roles

### 2.1 Coder Agent

**Does:** Writes code, creates commits, pushes branches, opens PRs.
**Isolation:** Always runs in its own Git worktree on its own branch.
**Constraints:**
- Cannot work on file paths reserved by another active coder
- Must run tests before pushing (`cargo test`, `npm test`, etc. — whatever the project uses)
- Must create atomic, meaningful commits
- Can spawn sub-subagents (test writer, doc updater) within its own worktree

### 2.2 Review Agent

**Does:** Reads diffs, checks for bugs, security issues, performance problems, style violations. Leaves review comments on PRs.
**Isolation:** Reads from the coder's branch. Does not write code.
**Constraints:**
- Multiple review agents can review different PRs simultaneously
- Multiple review agents can review the same PR simultaneously (security + perf + docs)
- Never pushes commits (review only)

### 2.3 Planner Agent

**Does:** Analyzes a milestone or epic, breaks it into tasks, identifies file paths each task will touch, determines dependencies between tasks, produces a task list with ordering constraints.
**Isolation:** Reads `main`. Does not write code.
**Constraints:**
- Must complete before coders start on the planned work
- Output is a structured task list (see Section 5.2)
- Runs sequentially, not in parallel with coders for the same batch

### 2.4 DevOps Agent

**Does:** CI log triage, infrastructure-as-code generation, incident response analysis, dependency audit, secrets rotation reminders.
**Isolation:** Operates on CI/IaC files in its own worktree.
**Constraints:**
- Never touches application source code
- File paths typically disjoint from coder agents

### 2.5 Docs Agent

**Does:** Updates documentation, README, changelogs, API docs.
**Isolation:** Own worktree.
**Constraints:**
- Documentation changes batch into a single daily branch (see Section 7.3)
- Does not create one PR per file

---

## 3. Project Onboarding

When the operator says "start working on project X," execute this sequence:

### 3.1 Initial Setup

```bash
# 1. Clone the repo (if not already present)
git clone <repo_url> ~/repos/<project_name>
cd ~/repos/<project_name>

# 2. Verify the default branch
DEFAULT_BRANCH=$(git symbolic-ref refs/remotes/origin/HEAD | sed 's@^refs/remotes/origin/@@')
echo "Default branch: $DEFAULT_BRANCH"

# 3. Create the worktrees directory
mkdir -p ~/repos/<project_name>.worktrees

# 4. Add .worktrees to .gitignore if using in-repo layout
# (Skip if using sibling directory layout as above)

# 5. Enable branch protection on default branch (if you have admin access)
# - Require CI passing
# - Require squash merge
# - Require linear history
```

### 3.2 Project Discovery

Before doing any work, understand the project:

1. **Read the README** — understand what it is, how to build, how to test
2. **Read open GitHub Issues** — understand what's planned and prioritized
3. **Read the GitHub Project board** (if one exists) — understand epic/feature/story hierarchy
4. **Identify the build system** — Cargo, npm, pip, make, etc.
5. **Identify the test command** — `cargo test`, `npm test`, `pytest`, etc.
6. **Identify CI configuration** — `.github/workflows/`, `.gitlab-ci.yml`, etc.
7. **Check for existing branch protection rules**
8. **Check for existing contribution guidelines** — `CONTRIBUTING.md`, PR templates

### 3.3 Create Project State File

Create a state file for the orchestrator to track ongoing work:

```json
{
  "project": "<project_name>",
  "repo": "<repo_url>",
  "default_branch": "main",
  "build_command": "cargo build",
  "test_command": "cargo test",
  "lint_command": "cargo clippy -- -D warnings",
  "format_command": "cargo fmt --check",
  "worktree_root": "~/repos/<project_name>.worktrees",
  "active_agents": [],
  "file_locks": {},
  "merge_queue": [],
  "last_merge_to_default": null,
  "daily_doc_branch": null
}
```

Store this at `~/repos/<project_name>.worktrees/.orchestrator-state.json`.

---

## 4. Git Strategy

### 4.1 Branching Model: Trunk-Based Development

The default branch (`main` or `master`) is always deployable. Every merge must pass CI. No long-lived feature branches. No `develop` branch. No Gitflow.

```
main ─────●─────●─────●─────●─────●──── (always green)
           \   /  \   /       \   /
            ●─●    ●─●         ●─●
          agent/   agent/     agent/
          coder/   coder/     reviewer/
          issue-1  issue-2    pr-3
```

Target branch lifetime: hours, not days. If a branch lives >24 hours, flag it for review or splitting.

### 4.2 Worktree-Per-Agent Isolation

Every agent gets its own Git worktree. Each worktree is a separate directory checking out a different branch but sharing the same `.git` database.

**Directory layout:**

```
~/repos/
├── <project>/                              # Main worktree (default branch)
│   └── .git/                               # Shared git database
├── <project>.worktrees/
│   ├── .orchestrator-state.json            # Orchestrator state
│   ├── agent-coder-issue-42/               # Coder worktree
│   ├── agent-coder-issue-43/               # Coder worktree
│   ├── agent-reviewer-pr-15/               # Reviewer worktree
│   └── agent-docs-batch-2026-03-18/        # Docs worktree
```

**Worktree lifecycle commands:**

```bash
# Create worktree + branch for an agent
cd ~/repos/<project>
git worktree add \
  ~/repos/<project>.worktrees/agent-coder-issue-42 \
  -b agent/coder/issue-42 \
  $DEFAULT_BRANCH

# Agent works in isolation
cd ~/repos/<project>.worktrees/agent-coder-issue-42
# ... write code, run tests, commit ...

# Push and open PR
git push origin agent/coder/issue-42
# Open PR via gh CLI or API

# After merge, clean up
cd ~/repos/<project>
git worktree remove ~/repos/<project>.worktrees/agent-coder-issue-42
git branch -d agent/coder/issue-42
```

**Critical rule:** Git does not allow the same branch checked out in two worktrees. This is a safety feature.

### 4.3 Branch Naming Convention

```
agent/<role>/<scope>

Roles: coder, reviewer, planner, devops, docs
Scope: issue number, PR number, milestone, or date

Examples:
  agent/coder/issue-42-add-fts5
  agent/coder/issue-43-refactor-gateway
  agent/reviewer/pr-15-security-audit
  agent/planner/milestone-4-batch
  agent/devops/ci-fix-lint
  agent/docs/batch-2026-03-18
```

Human branches (operator's manual work) use no prefix or `human/` prefix. This lets you filter agent branches easily:

```bash
git branch --list 'agent/*'
```

### 4.4 Squash Merges Only

Every agent branch becomes a single commit on the default branch. This keeps history clean and makes reverts trivial. Individual agent commits are preserved in the PR.

### 4.5 Recommended Tooling

- **Worktrunk** (`wt` CLI) — Purpose-built for parallel AI agent worktree management. Three commands: `wt switch`, `wt list`, `wt merge`. Supports hooks and squash merge.
- **GitHub Merge Queue** — Enable on the repo for sequential merge with automatic rebasing.
- **Branch protection** — Require: CI passing, squash merge only, linear history. Optionally require review (can be automated reviewer agent).

---

## 5. Task Lifecycle

### 5.1 How Work Arrives

Work arrives in one of three ways:

1. **Operator tells you directly:** "Fix the login bug" or "Implement feature X"
2. **GitHub Issues:** The operator creates issues; you read them and execute
3. **Planner agent output:** A planner produces a structured task list from a milestone

In all cases, the **source of truth for what to build is GitHub Issues**. If the operator gives a verbal instruction, create an issue first (or ask the operator to), then work from the issue.

### 5.2 Planner Output Format

When a planner agent breaks down a milestone, it produces:

```json
{
  "milestone": "Milestone name or issue number",
  "tasks": [
    {
      "id": "task-1",
      "title": "Short description",
      "github_issue": "#42",
      "role": "coder",
      "estimated_files": [
        "src/module_a/*.rs",
        "tests/test_module_a.rs"
      ],
      "depends_on": [],
      "estimated_size": "small | medium | large"
    },
    {
      "id": "task-2",
      "title": "Another task",
      "github_issue": "#43",
      "role": "coder",
      "estimated_files": [
        "src/module_b/*.rs"
      ],
      "depends_on": ["task-1"],
      "estimated_size": "medium"
    }
  ]
}
```

The `estimated_files` field is critical — it drives the file path locking system.

### 5.3 Task Assignment Decision Tree

When you have tasks to execute, follow this:

```
Is the task small and bounded (< ~100 lines of change)?
├─ YES → Do it yourself directly. No subagent needed.
└─ NO
   ├─ Does this task have dependencies on incomplete tasks?
   │  ├─ YES → Queue it. Wait for dependencies.
   │  └─ NO
   │     ├─ Do the estimated_files overlap with any active agent's file locks?
   │     │  ├─ YES → Queue it. Wait for the conflicting agent to finish.
   │     │  └─ NO → Spawn a subagent.
   │     │     ├─ Create worktree + branch
   │     │     ├─ Register file locks in state
   │     │     ├─ Launch agent with task context
   │     │     └─ Monitor progress
```

### 5.4 Spawning a Subagent

When spawning a coder agent:

1. **Create worktree:**
   ```bash
   git worktree add \
     <worktree_root>/agent-coder-issue-<N> \
     -b agent/coder/issue-<N> \
     $DEFAULT_BRANCH
   ```

2. **Register in state file:**
   ```json
   {
     "agent_id": "agent-coder-issue-42",
     "role": "coder",
     "branch": "agent/coder/issue-42",
     "worktree_path": "<worktree_root>/agent-coder-issue-42",
     "github_issue": "#42",
     "file_locks": ["src/module_a/*", "tests/test_module_a.rs"],
     "started_at": "2026-03-18T10:00:00Z",
     "status": "active"
   }
   ```

3. **Provide the agent with:**
   - The GitHub Issue content (title, description, acceptance criteria)
   - The worktree path to work in
   - The build/test/lint commands from project state
   - Instruction to commit, push, and open a PR when done
   - Instruction to not touch files outside its reserved paths

4. **On agent completion:**
   - Verify the agent pushed and opened a PR
   - Add the PR to the merge queue
   - Release file locks
   - Update agent status to `completed`
   - Do NOT remove the worktree yet (reviewer may need it)

### 5.5 Completing a Task

After the PR is merged:

```bash
# Remove worktree
git worktree remove <worktree_path>

# Delete branch locally
git branch -d agent/coder/issue-<N>

# Delete remote branch
git push origin --delete agent/coder/issue-<N>

# Update state file: remove agent entry, clear file locks
```

---

## 6. Parallelism Rules

### 6.1 When to Parallelize

```
Parallel = SAFE when:
  - Tasks touch different file paths (no overlapping file locks)
  - Tasks have no dependency relationship
  - Outputs don't feed into each other

Sequential = REQUIRED when:
  - Task B depends on Task A's output
  - Both tasks modify the same files
  - One task is "plan" and the other is "execute that plan"

Examples of safe parallelism:
  - Coder A on src/auth/* + Coder B on src/payments/*
  - Security reviewer + Performance reviewer on the same PR
  - Docs agent updating README + Coder fixing a bug in src/

Examples requiring sequential execution:
  - Planner → then Coders (planner must finish first)
  - Coder commits → then Reviewer reviews (code must exist first)
  - Two coders both need to modify Cargo.toml or package.json
```

### 6.2 Shared File Conflict Handling

Some files are touched by nearly every task (lock files, module registries, config manifests like `Cargo.toml`, `package.json`, `go.mod`). When a task needs to modify a shared file:

1. If no other coder is active → proceed normally
2. If another coder is active on different paths but might also touch the shared file → run sequentially. Add to merge queue. The second task runs after the first merges.
3. Never let two coders modify a shared manifest file in parallel. The merge conflict will be semantic, not textual, and auto-resolution is unreliable.

### 6.3 Maximum Concurrent Agents

Start with 2-3 parallel coders. Scale to 5+ only after the workflow is stable and you've observed clean merges. The bottleneck will be review, not coding — having 10 PRs waiting for review creates pressure, not productivity.

---

## 7. Merge Queue

### 7.1 Sequential Merge

The orchestrator merges one PR at a time to the default branch:

```
Merge Queue:
┌────────────────────────────────────────────────┐
│  #  │ Branch                 │ Status          │
│  1  │ agent/coder/issue-42   │ CI ✅ Review ✅  │ ← merge next
│  2  │ agent/coder/issue-43   │ CI ✅ Review ✅  │ ← wait
│  3  │ agent/docs/batch-0318  │ CI ✅            │ ← wait
└────────────────────────────────────────────────┘
```

After merging #1:

1. Pull updated default branch
2. Rebase #2 onto new default branch
3. Re-run CI on #2
4. If green → merge #2
5. Repeat

### 7.2 Why Sequential

Parallel merges cause semantic conflicts that CI cannot catch. Two agents might both add entries to a config file in ways that compile individually but break together. Sequential merging with rebase catches this.

### 7.3 PR Batching Rules

These rules prevent the "too many tiny PRs" problem:

- **Minimum PR size:** Don't open a PR for fewer than ~10 meaningful lines (excluding generated files). Batch small changes.
- **Maximum PR size:** Keep under ~400 lines of diff. If larger, the planner should split the task.
- **Documentation batching:** Doc-only changes accumulate in a single `agent/docs/batch-YYYY-MM-DD` branch. Merge once daily, not per-file.
- **Formatting/lint fixes:** Batch into a single PR, not one per file.

### 7.4 Merge Queue State

Track in the state file:

```json
{
  "merge_queue": [
    {
      "branch": "agent/coder/issue-42",
      "pr_number": 15,
      "ci_status": "passing",
      "review_status": "approved",
      "queued_at": "2026-03-18T11:00:00Z"
    }
  ]
}
```

---

## 8. Conflict Resolution

### 8.1 Resolution Hierarchy

When merge conflicts occur:

**Level 1 — Trivial (auto-resolve):**
Both agents added entries to a list, import block, or registry. A resolver agent (or the orchestrator itself) reads both changes, understands intent, merges the additions. Run build + tests to verify.

**Level 2 — Mechanical (tool-resolve):**
Import ordering, formatting, whitespace. Run the project's formatter (`cargo fmt`, `prettier`, `black`, etc.) and linter after merge. If it passes, accept.

**Level 3 — Semantic (escalate):**
Two agents made different architectural decisions, changed the same function differently, or modified a shared interface incompatibly. **Escalate to the operator.** Do not auto-resolve. Pause the merge queue and report:

```
CONFLICT ESCALATION
PR #15 (agent/coder/issue-42) conflicts with PR #16 (agent/coder/issue-43)
Files: src/gateway/router.rs
Nature: Both PRs restructured the route registration logic differently.
Action needed: Operator must decide which approach to keep.
```

### 8.2 Prevention Over Resolution

The file path locking system (Section 5.3) should prevent most conflicts. If you're seeing frequent conflicts, the planner is not splitting tasks well enough along file boundaries.

---

## 9. Inter-Agent Communication

### 9.1 The Pattern

Agents need to talk to each other — not just report to the orchestrator. This is the **agent teams** pattern, inspired by Anthropic's implementation in Claude Code (released with Opus 4.6, February 2026) but applicable to any agent runtime.

The pattern has five components:

1. **Team lead** — The orchestrator. Decomposes work, maintains the shared task list, broadcasts updates, synthesizes results.
2. **Shared task list** — All teammates can see what's assigned, what's available, and what's blocked. Teammates self-claim the next unassigned task when they finish one.
3. **Peer-to-peer messaging** — Any agent can message any other agent directly. A coder tells another coder "I changed this interface." A coder tells a reviewer "PR is ready." No need to route through the lead.
4. **Independent context** — Each agent has its own context/session. They don't share conversation history. Task-specific details must be included when spawning.
5. **Lifecycle management** — The lead can check if agents are idle, reassign work, request graceful shutdown, and enforce quality gates before marking tasks complete.

```
┌─────────────────────────────────────────────────┐
│              Team Lead (Orchestrator)            │
│  Shared task list. Broadcasts. Synthesizes.     │
└──────┬──────────┬──────────┬────────────────────┘
       │          │          │
  ┌────┴────┐ ┌───┴───┐ ┌───┴───┐
  │Teammate │ │Teammate│ │Teammate│
  │ (Coder) │ │(Coder) │ │(Review)│
  └────┬────┘ └───┬───┘ └───┬───┘
       │          │          │
       └──── peer-to-peer ───┘
            messaging
```

### 9.2 Teams vs Subagents

| | Agent Teams (peer-to-peer) | Subagents (hub-and-spoke) |
|---|---|---|
| Communication | Any agent ↔ any agent | Child → parent only |
| Context | Each has own session | Runs within parent's session |
| Coordination | Can challenge, share, collaborate | Isolated — siblings can't talk |
| Cost | Higher (multiple sessions) | Lower (shared session) |
| Best for | Parallel features, cross-layer work, debugging with competing hypotheses | Quick subtasks, research feeding back to one agent |

**Decision tree:**

```
Do agents need to talk to each other?
├─ YES → Team pattern
└─ NO
   ├─ Is the subtask quick and focused?
   │  ├─ YES → Subagent
   │  └─ NO → Team pattern (for independence)
   └─ Could agents benefit from challenging each other?
      ├─ YES → Team pattern
      └─ NO → Subagent
```

### 9.3 Communication Patterns

These patterns apply regardless of which implementation you use.

**Pattern 1: Interface change notification (coder → coders)**

Coder A changes a trait, API endpoint, or shared type. Coder A messages all other active coders immediately:

> "I added `find_by_tag()` to the StorageService trait in `src/traits.rs`. If you're implementing a storage backend, you'll need to add this method."

**Pattern 2: Ready for review (coder → reviewer)**

> "PR #15 is ready for review. Focus areas: the query builder in `fts5.rs` and the migration in `migrations/003_fts5.sql`."

**Pattern 3: Review feedback (reviewer → coder)**

> "PR #15 has two issues: (1) tokenizer config doesn't handle Unicode — see `fts5.rs:42`. (2) Missing error handling in the migration rollback. Comments on the PR."

**Pattern 4: Dependency unblock (coder → coder)**

> Coder B → Coder A: "I need the SessionRepo trait changes from issue #42 before I can implement the gateway routes. Are you close?"
> Coder A → Coder B: "Just pushed. Rebase onto my latest or wait for merge to main."

**Pattern 5: Competing hypotheses (debug team)**

> Agent A: "I think the issue is in the connection pool — timeout is too aggressive."
> Agent B: "I tested that. Pool timeout is fine. The problem is the query planner choosing a seq scan. Look at EXPLAIN output."
> Agent A: "You're right, the index on `session_id` is missing after the migration."

**Pattern 6: Orchestrator broadcast**

> Lead → all: "main was updated (PR #14 merged). All active branches must rebase before pushing."

### 9.4 Spawning a Teammate

Regardless of implementation, every teammate spawn needs this information:

```
AGENT IDENTITY:
  Name: coder-issue-42
  Role: coder
  Project: <project_name>

WORKSPACE:
  Worktree: ~/repos/<project>.worktrees/agent-coder-issue-42
  Branch: agent/coder/issue-42

TASK:
  GitHub Issue: #42
  Description: [from the issue]
  Acceptance criteria: [from the issue]

FILE OWNERSHIP (do not modify files outside these paths):
  - src/module_a/*
  - tests/test_module_a.rs

COMMANDS:
  Build: [build_command]
  Test: [test_command]
  Lint: [lint_command]

WHEN DONE:
  1. Run tests. All must pass.
  2. Commit with a meaningful message.
  3. Push your branch.
  4. Open a PR against the default branch.
  5. Message the reviewer teammate that your PR is ready.
  6. Pick up the next unassigned task from the shared task list.

IF YOU CHANGE A SHARED INTERFACE:
  Message all other active coder teammates about what changed.

IF YOU'RE BLOCKED:
  Message the teammate whose work you depend on.
  If no response or the block is architectural, message the lead.
```

### 9.5 Implementation Options

The communication pattern is the same. The transport differs. Choose based on your environment:

#### Option A: Claude Code Agent Teams

Use when: You're running Claude Code as the agent execution environment.

Anthropic's native implementation of the team pattern. One Claude Code session is the team lead, teammates are spawned as separate sessions with their own context windows. Peer-to-peer messaging, shared task list, and self-claiming are built in.

```bash
# Enable
export CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1
# Or in ~/.claude/settings.json: {"env": {"CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS": "1"}}
```

Features you get for free: shared task list with file-locking on claims, peer-to-peer messaging, `TeammateIdle` and `TaskCompleted` lifecycle hooks, graceful shutdown negotiation, tmux split-pane monitoring.

Limitations: experimental, no session resumption, no nested teams.

#### Option B: Anthropic API with Conversation Threads

Use when: Agents are API calls, not CLI sessions. You're building custom orchestration.

Each agent is a separate `/v1/messages` conversation. The orchestrator maintains agent state and routes messages between them by injecting received messages into each agent's conversation history.

```
Orchestrator keeps:
  - One conversation thread per agent (list of messages)
  - A shared task list (JSON file or in-memory)
  - A message queue per agent

When Agent A sends a message to Agent B:
  1. Agent A's response includes a structured message (e.g., in a tool call or tagged block)
  2. Orchestrator extracts the message
  3. Orchestrator injects it into Agent B's next API call as context
  4. Agent B sees it as part of its conversation
```

This gives you full control over routing, persistence, and cross-session continuity. You can persist conversations to disk and resume them.

#### Option C: OpenClaw as Communication Backbone

Use when: OpenClaw is your running runtime and you want agents to communicate through its session and channel infrastructure.

Each agent is an OpenClaw session. The orchestrator is a session with a system prompt defining its role. Agents communicate by sending messages through OpenClaw's internal messaging — either via the HTTP API (if available) or through dedicated channels.

```
Orchestrator session → dispatches tasks → creates agent sessions
Agent sessions → post messages to a shared "agent-comms" channel or thread
All agents read the shared channel for broadcasts
Direct messages route through the session API

Agent A → OpenClaw API → Agent B's session
```

Advantages: agents get all of OpenClaw's capabilities (tool execution, memory, provider routing) for free. The Main Agent is also an OpenClaw session, so the entire hierarchy lives in one runtime. Communication is persistent — messages survive restarts because OpenClaw stores session history.

#### Option D: Filesystem Mailboxes (Fallback)

Use when: No native agent teams support, no API orchestration layer, or you need the simplest possible implementation.

Each agent has an inbox directory. Messages are JSON files. Works across any agent runtime.

```
<project>.worktrees/
├── .comms/
│   ├── .board/                    # Broadcast (all agents read)
│   ├── agent-coder-issue-42/      # Agent A's inbox
│   │   ├── 2026-03-18T10-02Z_from-coder-43_heads-up.json    # Unread
│   │   └── .read/                                             # Processed
│   ├── agent-coder-issue-43/      # Agent B's inbox
│   └── agent-reviewer-pr-15/      # Agent C's inbox
```

Message format:

```json
{
  "id": "msg-20260318T100200Z-a42-to-a43",
  "timestamp": "2026-03-18T10:02:00Z",
  "from": "agent-coder-issue-42",
  "to": "agent-coder-issue-43",
  "type": "info | question | request | answer | alert | ready-for-review",
  "subject": "Short summary",
  "body": "Full message content.",
  "requires_response": false
}
```

Agents check their inbox at the start of each work cycle. Move processed messages to `.read/`. Check `.board/` for broadcasts.

### 9.6 Quality Gates

Regardless of implementation, enforce these before marking any task complete:

1. **Tests pass** — Run the project's test command in the agent's worktree
2. **Lint clean** — Run the project's lint command
3. **PR opened** — The branch is pushed and a PR exists
4. **Reviewer notified** — A message was sent to the reviewer agent

If using Claude Code Agent Teams, use `TaskCompleted` hooks to enforce these automatically. If using other implementations, the orchestrator checks these conditions before accepting a task as done.

---

## 10. State Management

### 10.1 State File Location

```
~/repos/<project>.worktrees/.orchestrator-state.json
```

One state file per project.

### 10.2 State File Schema

```json
{
  "project": "string — project name",
  "repo": "string — git remote URL",
  "default_branch": "string — main or master",
  "build_command": "string",
  "test_command": "string",
  "lint_command": "string | null",
  "format_command": "string | null",
  "worktree_root": "string — absolute path",

  "active_agents": [
    {
      "agent_id": "string — unique identifier",
      "role": "coder | reviewer | planner | devops | docs",
      "branch": "string — full branch name",
      "worktree_path": "string — absolute path",
      "github_issue": "string | null — e.g. '#42'",
      "file_locks": ["string — glob patterns"],
      "started_at": "string — ISO 8601",
      "status": "active | completed | failed | stale"
    }
  ],

  "file_locks": {
    "src/module_a/*": "agent-coder-issue-42",
    "tests/test_module_a.rs": "agent-coder-issue-42"
  },

  "merge_queue": [
    {
      "branch": "string",
      "pr_number": "integer",
      "ci_status": "pending | passing | failing",
      "review_status": "pending | approved | changes_requested",
      "queued_at": "string — ISO 8601"
    }
  ],

  "last_merge_to_default": "string | null — ISO 8601",
  "daily_doc_branch": "string | null — current day's doc batch branch"
}
```

### 10.3 State Update Rules

- **Read state before every decision.** Never assume state from memory — always read the file.
- **Write state after every action.** Create worktree → update state. Merge PR → update state. Release lock → update state.
- **Atomic updates.** Read, modify, write as a single operation. Don't hold stale state across long operations.

---

## 11. Cron & Watchdog

### 11.1 Worker Tick (runs on schedule or on-demand)

The worker tick is the orchestrator's main loop:

```
1. Read state file for each managed project
2. Pull latest default branch
3. Check GitHub Issues for new/updated work items
4. For each project:
   a. Are there completed agents? → Process their PRs, add to merge queue
   b. Is the merge queue non-empty? → Attempt to merge the next item
   c. Are there queued tasks waiting for dependencies? → Check if deps are met
   d. Are there tasks ready to start? → Check file locks, spawn agents
   e. Are there stale agents (no commits in >6 hours)? → Flag for review
5. Run daily cleanup if needed (see 10.3)
```

### 11.2 Watchdog Checks

Run these checks continuously or on each tick:

```
For each active agent:
  - Has it committed in the last 6 hours?
    NO → Mark as stale. Alert operator.
  - Is its worktree still valid?
    NO → Clean up orphaned state entry.
  - Are its file locks still needed?
    Agent completed → Release locks.

For the merge queue:
  - Is the next PR's CI still passing after rebase?
    NO → Re-trigger CI or alert operator.
  - Has any PR been in the queue for >24 hours?
    YES → Alert operator.

For worktrees:
  - Run: git worktree list
  - Cross-reference with state file active_agents
  - Orphaned worktrees (in git but not in state) → prune
  - Ghost agents (in state but worktree missing) → clean up state
```

### 11.3 Daily Cleanup

Run once per day:

```bash
# For each managed project:
cd ~/repos/<project>

# Remove merged branches
git fetch --prune
git branch --merged $DEFAULT_BRANCH | grep 'agent/' | xargs -r git branch -d

# Prune worktree references
git worktree prune

# Garbage collect
git gc --auto

# Reset daily doc branch tracking in state file
# (doc batch for yesterday is either merged or abandoned)
```

---

## 12. Multi-Project Operation

### 12.1 One Orchestrator Per Project

Each project gets its own Project Orchestrator — a separate agent teams session acting as team lead. The **Main Agent** manages all orchestrators. Orchestrators do not know about each other.

```
Main Agent
├── Orchestrator: Project A (+ its teammates)
├── Orchestrator: Project B (+ its teammates)
└── Orchestrator: Project C (+ its teammates)
```

Each project has:

- Its own repo clone in `~/repos/<project>/`
- Its own worktree directory `~/repos/<project>.worktrees/`
- Its own state file `~/repos/<project>.worktrees/.orchestrator-state.json`
- Its own team session with its own teammates

Projects are fully isolated. An agent working on Project A never touches Project B's repo. Teammates within a project can talk to each other. Teammates across projects cannot.

### 12.2 Adding a New Project

When the operator tells the Main Agent "start working on <new_project>":

1. Main Agent spins up a new Project Orchestrator for that project
2. Orchestrator runs the Project Onboarding sequence (Section 3)
3. Orchestrator creates the state file
4. Orchestrator reads GitHub Issues to understand current priorities
5. Orchestrator reports back to the Main Agent: "Project onboarded. Found N open issues. Top priorities appear to be X, Y, Z."
6. Main Agent relays the summary to the operator and asks for direction

### 12.3 Cross-Project Work

When the operator requests work spanning two projects (e.g., "update the client library in Project A to match the API change in Project B"), the **Main Agent** coordinates:

1. Main Agent tells Orchestrator B to complete the API change first
2. Orchestrator B's team finishes and merges to Project B's default branch
3. Main Agent then tells Orchestrator A to start the client update
4. Never have agents in two different repos depending on each other's uncommitted work

The Main Agent is the only entity that sees across projects. Orchestrators never talk to each other directly.

### 12.4 Status Aggregation

The Main Agent maintains an overview of all active projects:

- Which orchestrators are running
- How many active teammates each has
- Merge queue depth per project
- Any pending escalations

When the operator asks "what's the status?" or "how's everything going?", the Main Agent queries each orchestrator and synthesizes a cross-project summary.

### 12.5 Resource Allocation

When managing multiple projects, the Main Agent prioritizes based on operator instruction. Default: each orchestrator operates independently at whatever pace it can. The operator can say "focus on Project A" — the Main Agent can either pause other orchestrators or reduce their teammate count to free up capacity.

---

## 13. Guardrails

### 13.1 Things You Must Always Do

1. **Read state before acting.** Never assume.
2. **Run tests before pushing.** Every coder agent must run the project's test command in its worktree before pushing. A push with failing tests is a bug in the orchestrator.
3. **Squash merge only.** Never fast-forward or regular merge to the default branch.
4. **Sequential merge queue.** Never merge two PRs simultaneously.
5. **Rebase after every merge.** All queued branches rebase onto the new default branch head.
6. **File lock before assigning.** Check file locks before giving a task to a coder. If paths overlap, queue the task.
7. **Clean up after merge.** Remove worktree, delete branch (local + remote), release file locks, update state.
8. **Report conflicts, don't guess.** If a semantic conflict occurs, escalate upward. Never auto-resolve architectural disagreements.
9. **Main Agent stays responsive.** The Main Agent never blocks on development work. It dispatches and monitors. If the operator messages, it replies immediately — it doesn't wait for an orchestrator to finish.
10. **Orchestrators stay within their project.** An orchestrator never touches another project's repo, issues, or state.

### 13.2 Things You Must Never Do

1. **Never commit directly to the default branch.** All changes go through branches + PRs.
2. **Never let two coders work on overlapping file paths.**
3. **Never merge a PR with failing CI.**
4. **Never delete the default branch.**
5. **Never force-push to the default branch.**
6. **Never create one PR per small file change.** Batch.
7. **Never leave orphaned worktrees.** Clean up after every completed or failed task.
8. **Never hold stale file locks.** If an agent is done (completed or failed), release its locks immediately.
9. **Never start coding before the planner finishes** (when a planning phase is active).
10. **Never auto-resolve semantic conflicts.** Only trivial (list additions) and mechanical (formatting) conflicts may be auto-resolved.
11. **Main Agent never does dev work.** It doesn't write code, run tests, or manage worktrees. It dispatches to orchestrators.
12. **Orchestrators never talk to each other.** Cross-project coordination goes through the Main Agent.
13. **Teammates never work across projects.** A teammate belongs to one orchestrator, one project.

### 13.3 Escalation Chain

Escalations flow upward through the hierarchy:

```
Teammate → Orchestrator → Main Agent → Operator
```

**Teammate escalates to Orchestrator when:**
- Blocked on another teammate's work and messaging didn't resolve it
- Unsure which approach to take on an architectural question
- Tests pass locally but the task feels wrong
- File path conflict with another teammate

**Orchestrator escalates to Main Agent when:**
- A semantic merge conflict it can't auto-resolve
- An agent has been stale for >6 hours with no commits
- CI has been failing on the default branch for >1 hour
- The merge queue has been blocked for >24 hours
- A task requires access to secrets, credentials, or infrastructure it doesn't have
- Unsure whether two tasks' file paths will overlap
- The operator's instruction conflicts with these guardrails

**Main Agent escalates to Operator when:**
- An orchestrator reports an unresolvable conflict
- Cross-project dependency decision is needed
- Resource allocation question ("Project A and B both need attention, which first?")
- Any security or credential issue
- Anything the Main Agent isn't confident handling autonomously

Escalation format (used at every level):

```
ESCALATION: [category]
Project: <project_name>
Issue: <brief description>
Context: <what happened>
Options: <what you think the choices are>
Recommendation: <what you'd do if you could decide>
Waiting for: <operator decision>
```

---

*This runbook is project-agnostic and version-controlled. Update it as the orchestration process evolves.*
