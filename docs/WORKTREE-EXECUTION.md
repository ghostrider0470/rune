# Workflow Transition Note

This document exists to prevent stale worktree-era guidance from conflicting with Rune's current execution model.

## Current default

Rune does **not** use a worktree-per-task model by default.

The accepted default is:
- one active coding branch
- one active PR
- one coherent milestone batch
- `main` kept stable and synced to `origin/main`
- no git worktrees by default
- no temp clones by default
- no alternate repo copies by default

## Why this changed

The earlier worktree-oriented approach increased hidden state and branch/worktree clutter.

Rune now prefers a simpler batch-branch workflow because it reduces:
- branch drift
- merge-path ambiguity
- cleanup overhead
- overlap between parallel coding surfaces

## Canonical current references

Use these docs instead of treating this file as separate workflow authority:
- [`contributor/WORKFLOW.md`](contributor/WORKFLOW.md)
- [`contributor/EXECUTION-SPEED-POLICY.md`](contributor/EXECUTION-SPEED-POLICY.md)
- [`adr/ADR-0001-execution-workflow-and-speed.md`](adr/ADR-0001-execution-workflow-and-speed.md)
- [`adr/ADR-0004-project-2-execution-model.md`](adr/ADR-0004-project-2-execution-model.md)
- [`INDEX.md`](INDEX.md)

## Status of the old model

The worktree-per-task model should be treated as historical context only, not current instruction.

Unless an explicit exception is approved, contributors and agents should follow the batch-branch model above.
