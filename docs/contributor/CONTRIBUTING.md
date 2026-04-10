# Contributing

This is the contributor-facing entry doc for how work should move through Rune.

## Scope

Contributors should use Rune's docs and issue trail in a way that keeps strategy, execution, and durable decisions separated.

## Open-source baseline

Rune is now being prepared for external contributors. Keep issues scoped, document user-visible changes, and prefer small PRs with clear validation evidence.

## Start here

- [`DEVELOPMENT.md`](DEVELOPMENT.md)
- [`WORKFLOW.md`](WORKFLOW.md)
- [`TESTING.md`](TESTING.md)
- [`EXECUTION-SPEED-POLICY.md`](EXECUTION-SPEED-POLICY.md)
- [`../AGENT-ORCHESTRATION.md`](../AGENT-ORCHESTRATION.md)
- [`../INDEX.md`](../INDEX.md)

## Core rule

Use repo docs to understand the system.
Use GitHub Project 2, issues, and PRs to understand active execution.
Use ADRs for durable decisions.
Documentation is part of delivery, not optional cleanup after shipping code.

## Documentation as a delivery requirement

Every non-trivial Rune change should explicitly answer:
- what durable docs changed with this work?
- what operator/contributor/reference surface is now different?
- does this decision deserve an ADR or equivalent canonical record?

Issue scopes and PRs should treat docs the same way they treat validation: as part of done. If the user-visible or operator-visible surface changed, the same PR should carry the matching docs update.

## Current contributor use

Use this doc as the contributor entrypoint for:
- understanding how docs, issues, PRs, and ADRs relate today
- navigating to the right development, testing, and workflow references first

## Read next

- use [`DEVELOPMENT.md`](DEVELOPMENT.md) for local setup and build/run flow
- use [`TESTING.md`](TESTING.md) when you need validation guidance
- use [`WORKFLOW.md`](WORKFLOW.md) and [`EXECUTION-SPEED-POLICY.md`](EXECUTION-SPEED-POLICY.md) when you need the accepted execution model and merge discipline

## Further detail still missing

Deeper follow-up documentation is still useful for:
- contribution expectations
- doc/issue/PR relationship
- review hygiene
- where to find the right contributor references first
