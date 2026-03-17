# ADR-0001: Execution Workflow and Speed Model

- **Status:** Accepted
- **Date:** 2026-03-17

## Context

Rune development was slowed and destabilized by mixing incompatible execution modes:

- direct local `main` development
- PR-driven work
- temporary side branches
- worktrees
- background autonomous execution
- inconsistent merge timing

This caused:
- branch/worktree clutter
- delayed merges even when PRs were green
- confusion about the active source of truth
- extra cleanup work instead of forward progress

A clearer operating model is needed to keep development both fast and safe.

## Decision

Rune development will use the following default workflow model:

### 1. Batch-branch mode is the default
Development happens on:
- one active coding branch
- one active PR
- one coherent milestone batch

### 2. `main` stays stable and synced
- `main` is the integration target
- `main` should remain clean and synced to `origin/main`
- implementation should not happen directly on `main` by default

### 3. No worktrees by default
- no git worktrees
- no temp clones
- no alternate repo copies

Unless explicitly approved as an exception.

### 4. Prefer larger coherent milestone PRs
- fewer PRs
- larger but still coherent milestone batches
- avoid unnecessary micro-PR fragmentation

### 5. Merge immediately when green
If the active PR is green and mergeable:
1. merge immediately
2. sync `main`
3. create the next batch branch
4. continue work

### 6. Parallelism is asymmetric
Use:
- one coding agent on the active batch branch
- support agents for planning/review/CI/docs analysis in parallel

Do not use multiple overlapping coding agents by default.

## Consequences

### Positive
- cleaner repo state
- less branch churn
- faster progress through larger coherent batches
- less idle waiting between merge and next work
- lower operational ambiguity

### Negative / tradeoffs
- less fine-grained PR history than strict one-story-one-PR mode
- parallel coding throughput is intentionally limited to reduce collisions
- explicit planning is needed to keep milestone batches coherent

## Alternatives considered

### Direct `main` mode
Rejected as default because it becomes ambiguous and messy too quickly.

### Strict one-story-one-PR mode
Rejected as default because it is cleaner but slower than desired.

### Worktree-per-task parallel coding
Rejected as default because it increases hidden state and operational confusion.

## Follow-up

This ADR should be reflected in:
- execution policy docs
- Project 2 workflow expectations
- agent operating rules
