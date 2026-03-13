# Worktree Execution Model

Rune now uses a worktree-per-task execution model for parallel implementation.

## Rule

- one subagent = one task
- one subagent = one git branch
- one subagent = one dedicated git worktree

## Current convention

Worktrees live under:

- `/home/hamza/Development/rune-worktrees/`

Branch naming:

- `wave-0/bootstrap`
- `wave-1/rune-core`
- `wave-1/rune-config`
- `wave-1/rune-testkit`
- future waves follow the same pattern

## Why

This preserves:
- atomic commits
- regular pushes
- reduced file collisions
- cleaner subagent parallelism
- easier rollback and review

## Merge discipline

A wave is considered complete only when:
- each worktree branch has committed/pushed changes
- compilation/tests for the wave pass
- changes are reconciled back into the mainline intentionally
