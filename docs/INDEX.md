# Rune Docs Index

This is the docs front door for Rune.

Use it to find the right source of truth by audience and concern.

## Source of truth map

| Concern | Canonical source | Use when you need |
|---|---|---|
| Public product entry | [`README.md`](../README.md) | quick understanding, quick start, project positioning |
| Product strategy | [`rune-plan.md`](../rune-plan.md) | goals, direction, confirmed stack choices, high-level delivery map |
| Parity execution phases | [`IMPLEMENTATION-PHASES.md`](IMPLEMENTATION-PHASES.md) | phase sequencing and acceptance criteria |
| Live execution queue | GitHub Project 2 | what is active now |
| Runtime orchestration rules | [`AGENT-ORCHESTRATION.md`](AGENT-ORCHESTRATION.md) | agent workflow and execution guardrails |
| Parity docs front door | [`OPENCLAW-COVERAGE-MAP.md`](OPENCLAW-COVERAGE-MAP.md) | where to start for OpenClaw-surface coverage and parity navigation |
| Parity contracts | [`PARITY-SPEC.md`](PARITY-SPEC.md), [`PARITY-CONTRACTS.md`](PARITY-CONTRACTS.md), [`PROTOCOLS.md`](PROTOCOLS.md) | observable runtime behavior and subsystem invariants |
| Operator deployment/runtime docs | [`operator/DEPLOYMENT.md`](operator/DEPLOYMENT.md), [`operator/DATABASES.md`](operator/DATABASES.md), [`operator/OPERATOR-POLICY.md`](operator/OPERATOR-POLICY.md) | deployment, storage, health, operational rules |
| Contributor/reference docs | [`CRATE-LAYOUT.md`](CRATE-LAYOUT.md), [`SUBSYSTEMS.md`](SUBSYSTEMS.md), [`FUNCTIONALITY-CHECKLIST.md`](FUNCTIONALITY-CHECKLIST.md) | implementation reference and verification detail |
| Long-form strategy / rationale | [`strategy/COMPETITIVE-RESEARCH.md`](strategy/COMPETITIVE-RESEARCH.md), [`strategy/AZURE-DATA-OPTIONS.md`](strategy/AZURE-DATA-OPTIONS.md), [`DOCS-README-PLAN-REORG.md`](DOCS-README-PLAN-REORG.md) | design rationale and planning context |

## Transitional structure scaffold

The approved audience-based folder layout now exists in scaffold form:

- [`getting-started/`](getting-started/README.md)
- [`operator/`](operator/README.md)
- [`contributor/`](contributor/README.md)
- [`reference/`](reference/README.md)
- [`parity/`](parity/README.md)
- [`strategy/`](strategy/README.md)
- [`adr/`](adr/README.md)

The current top-level docs remain authoritative until content is intentionally moved.

## Recommended reading paths

### If you are evaluating Rune
1. [`README.md`](../README.md)
2. [`rune-plan.md`](../rune-plan.md)
3. [`AZURE-COMPATIBILITY.md`](AZURE-COMPATIBILITY.md)
4. [`strategy/COMPETITIVE-RESEARCH.md`](strategy/COMPETITIVE-RESEARCH.md)

### If you are operating Rune
1. [`operator/DEPLOYMENT.md`](operator/DEPLOYMENT.md)
2. [`operator/DATABASES.md`](operator/DATABASES.md)
3. [`operator/OPERATOR-POLICY.md`](operator/OPERATOR-POLICY.md)
4. [`IMPLEMENTATION-PHASES.md`](IMPLEMENTATION-PHASES.md) for parity-phase context

### If you are building Rune
1. [`AGENT-ORCHESTRATION.md`](AGENT-ORCHESTRATION.md)
2. [`rune-plan.md`](../rune-plan.md)
3. [`IMPLEMENTATION-PHASES.md`](IMPLEMENTATION-PHASES.md)
4. [`PROTOCOLS.md`](PROTOCOLS.md)
5. [`CRATE-LAYOUT.md`](CRATE-LAYOUT.md)

## Transitional note

Legacy planning files still exist during the docs cleanup transition:

- [`PLAN.md`](PLAN.md)
- [`STACK.md`](STACK.md)
- [`WORKPLAN.md`](WORKPLAN.md)

Treat those as provenance and historical context unless a file explicitly says otherwise.
