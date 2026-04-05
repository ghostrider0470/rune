# Workflow

This is the contributor-facing entry doc for Rune execution flow.

## Current workflow model

Rune currently uses a batch-branch workflow model:
- one active coding branch
- one active PR
- one coherent milestone batch
- immediate merge only after the required CI gates are green and the PR is mergeable

## Current canonical references

- [`EXECUTION-SPEED-POLICY.md`](EXECUTION-SPEED-POLICY.md)
- [`../adr/ADR-0001-execution-workflow-and-speed.md`](../adr/ADR-0001-execution-workflow-and-speed.md)
- [`../adr/ADR-0004-project-2-execution-model.md`](../adr/ADR-0004-project-2-execution-model.md)
- [`../AGENT-ORCHESTRATION.md`](../AGENT-ORCHESTRATION.md)
- [`../INDEX.md`](../INDEX.md)

## Current contributor use

Use this doc as the workflow entrypoint for:
- the accepted batch-branch / single-PR model
- where execution authority lives now
- how contributor flow relates to issues, PRs, and Project 2

## Definition of done

Rune work is not done when code exists locally. Rune work is done when the shipped slice includes:
- code changes
- validation evidence appropriate to the change
- canonical documentation updates for any operator-visible, contributor-visible, runtime-significant, or architecture-significant behavior

If a change introduces new behavior, config, workflow, failure mode, or decision surface, the matching docs update is part of the same PR rather than follow-up backlog residue.

## Documentation expectations

Before merging non-trivial work, contributors should decide which durable docs must move with the code:
- ADRs for architecture-significant decisions
- operator docs/runbooks for runtime-visible behavior and troubleshooting
- contributor/reference docs for development workflow, subsystem contracts, or config surfaces
- README/docs index updates when navigation or first-stop guidance changes

## Read next

- use [`EXECUTION-SPEED-POLICY.md`](EXECUTION-SPEED-POLICY.md) when you need the speed/safety operating rules behind this workflow
- use [`../adr/ADR-0001-execution-workflow-and-speed.md`](../adr/ADR-0001-execution-workflow-and-speed.md) and [`../adr/ADR-0004-project-2-execution-model.md`](../adr/ADR-0004-project-2-execution-model.md) when you need the durable rationale behind the current model
- use [`../AGENT-ORCHESTRATION.md`](../AGENT-ORCHESTRATION.md) when you need deeper execution context for the repo/runtime itself

## Further detail still missing

Deeper follow-up documentation is still useful for:
- branch/PR workflow expectations
- issue/PR/project-board relationship
- merge discipline
- contributor execution cadence and review flow
