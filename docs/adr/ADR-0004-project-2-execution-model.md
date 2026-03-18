# ADR-0004: Project 2 Execution Model

- **Status:** Accepted
- **Date:** 2026-03-18

## Context

Rune needs one live execution control plane for active work.

Before this decision, execution state could be inferred from too many places:
- repo docs
- roadmap/planning files
- branch names
- issue comments
- pull requests
- chat context

That made it too easy for active priority, status, and sequencing to drift.

The docs reorganization proposal already points toward GitHub Project 2 as the place for live execution truth, and Hamza's operating preference is explicit:
- the project board should be the control plane
- the desired hierarchy is Epic → Feature → Story
- docs should explain the system, not replace the execution queue
- issue/PR trails should show visible movement, but not become the sole planning surface

## Decision

Rune will use **GitHub Project 2 as the live execution control plane**.

### 1. Project 2 owns active execution state
Use Project 2 for:
- what is active now
- execution order
- status flow
- batch progress
- linkage between issues and PRs

Repo docs must not try to mirror the board as a second live queue.

### 2. Default hierarchy is Epic → Feature → Story
Use:
- **Epic** for broad delivery lanes
- **Feature** for coherent capability groups within an epic
- **Story** for atomic executable slices

This keeps strategy, execution grouping, and actionable work separated.

### 3. One active PR at a time per current batch lane
Within the active lane:
- keep one active PR
- accumulate coherent reviewed work on the active batch branch
- merge when green and mergeable
- continue immediately after merge

This aligns Project 2 execution with the accepted batch-branch workflow model.

### 4. Issues and PRs are execution artifacts, not replacements for the board
Use issues and PRs to provide:
- acceptance scope
- milestone evidence
- commit/validation trail
- review and merge record

But do not rely on issue threads alone to determine the full current queue.

### 5. Repo docs describe roles, not live status
Repo docs may explain:
- where execution truth lives
- how hierarchy works
- what each planning surface is for

Repo docs should not be treated as the canonical source for current in-flight status.

## Consequences

### Positive
- lower ambiguity about current priorities
- easier handoff between chat, issues, PRs, and docs
- stronger visible execution trail without duplicating the queue in docs
- better alignment with autonomous batch execution

### Negative / tradeoffs
- Project hygiene matters more; stale board state becomes an operational problem
- contributors must keep issue/PR linkage disciplined
- some readers may still look in repo docs first and need clear pointers back to Project 2

## Alternatives considered

### Use repo docs as the live execution queue
Rejected because docs go stale too easily and are worse for active status flow.

### Use issues only with no Project 2 structure
Rejected because hierarchy and queue visibility become weaker as work expands.

### Let branch names and PRs imply the current plan
Rejected because that hides queue intent and makes prioritization harder to inspect.

## Follow-up

This ADR should be reflected in:
- `docs/INDEX.md`
- `docs/OPENCLAW-COVERAGE-MAP.md`
- `rune-plan.md`
- issue/PR workflow habits
- future contributor workflow docs
