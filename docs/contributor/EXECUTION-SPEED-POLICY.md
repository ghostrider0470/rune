# Execution Speed Policy

This document captures how Rune development should move fast **without** collapsing into repo chaos.

## Core rule

Speed comes from:
- coherent milestone batches
- immediate merges when green
- low state ambiguity
- parallel support work

Not from:
- multiple coding branches at once
- dirty `main`
- worktrees by default
- tiny fragmented PRs for every minor sub-slice

## Default operating mode

### Batch-branch mode

Use:
- **one active coding branch**
- **one active PR**
- **one coherent milestone batch**

That is the default speed/safety balance for Rune.

## Branch rules

- `main` stays stable and synced to `origin/main`
- do not code on `main` by default
- create one batch branch from clean `main`
- keep all implementation for that milestone on the batch branch
- merge when the batch is coherent and validated
- delete the branch after merge
- immediately create the next batch branch

## PR sizing rule

Prefer **larger coherent milestone PRs** over many tiny PRs.

Good PR shape:
- one lane
- one milestone
- one reviewer story
- multiple related commits allowed

Bad PR shape:
- unrelated browser + docs + memory + channels mixed together
- tiny one-commit PRs when the work obviously belongs in one larger milestone

## Validation rule

Use tiered validation:

### During implementation
- targeted tests
- targeted lint
- focused local validation

### Before PR / merge
- broader validation sweep appropriate to the batch

### In CI
- final gating checks

Do not run the whole universe on every tiny local change if targeted validation is sufficient.

## Merge rule

If the active PR is:
- green
- mergeable

then:
1. merge immediately
2. sync `main` immediately
3. create the next batch immediately
4. continue immediately

Do not stop at a status checkpoint when the next safe execution step is obvious.

## Parallelism rule

Use:
- **one coding agent** on the active batch branch
- additional support agents for:
  - issue decomposition
  - docs analysis
  - CI/log inspection
  - parity-gap identification

Do **not** use multiple coding agents on overlapping repo state by default.

## Repository safety rule

- no git worktrees by default
- no temp clones by default
- no alternate repo copies by default
- use the canonical repo unless an explicit exception is approved

## Interruption rule

Only interrupt Hamza for:
- real blockers
- real milestones
- meaningful decisions

Do not generate chatter for routine intermediate state.

## Cleanup rule

Periodically clean:
- merged local branches
- stale temp branches
- stale worktree metadata
- stale partial branch clutter

Low state complexity is a speed feature.
