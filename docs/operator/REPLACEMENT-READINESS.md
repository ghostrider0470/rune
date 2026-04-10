# Replacement Readiness

This is the canonical operator-facing answer to one question:

**Can Rune honestly replace OpenClaw yet?**

Current answer: **not yet**.

As of 2026-04-07, the remaining scoped child issues under epic #893 (#896, #898, #899) are closed. The remaining blocker categories below are readiness-proof gates tracked directly in the readiness surfaces rather than open feature-gap tickets.

Rune already ships substantial OpenClaw-facing parity work, but replacement claims must stay narrower than feature existence. A feature can be implemented and still not be sufficient evidence for an honest replacement claim.

## Canonical verdict

Treat this file as the authoritative readiness view for operator questions.

Current verdict: **not ready**.

Why:
- major parity foundations are shipped across runtime, tools, channels, scheduler, UI, and diagnostics
- replacement-readiness still has explicit remaining blockers
- those blockers are documented here and reflected in doctor/readiness surfaces

## Readiness state model

Use these labels consistently across docs and issue discussions:

- **implemented** — the capability exists in shipped Rune behavior
- **partially evidenced** — the capability or claim exists, but the black-box/operator evidence is still incomplete for a full replacement claim
- **not replacement-ready** — Rune must not be presented as a full OpenClaw replacement on that surface yet

A surface can be implemented without being replacement-ready.

## What is already shipped

These statements reflect shipped Rune reality, not roadmap intent:

- runtime/session/tooling foundations are implemented
- anti-thrash protection and bounded recovery loops are implemented
- delegated-session baseline and operator-visible subagent audit surfaces are implemented
- operator dashboard, doctor, and status surfaces are implemented
- broad channel/provider support is implemented
- OpenClaw-replacement verdict plumbing is implemented in doctor/readiness responses
- channel authenticity checks are implemented for relevant webhook-style adapters

These shipped capabilities are necessary, but they do not by themselves justify a full replacement claim.

## Remaining blockers

### 1. Channel sender trust-boundary parity

State: **not replacement-ready**

Tracked by: **readiness surfaces and canonical docs**

Current shipped truth:
- Rune distinguishes provider authenticity from sender trust policy
- provider/webhook authenticity checks exist for relevant adapters
- operator-configurable sender allowlist parity is not shipped yet

Practical meaning:
- deployments that require sender-level trust boundaries must still treat Rune as blocked on this surface
- documentation must not describe provider authenticity as if it were sender allowlist parity

## Current operator guidance

You may say the following today:
- Rune is a parity-targeted OpenClaw replacement
- large portions of the product surface are already implemented
- doctor/status surfaces expose an explicit replacement-readiness verdict
- Rune is **not yet** an honest full replacement claim while readiness evidence remains bounded to the current documented surfaces and the doctor/status blocker categories below remain unresolved

You should **not** say the following today:
- Rune is already a full OpenClaw replacement
- Rune has sender trust-boundary parity across channels
- Rune is replacement-ready merely because major features are implemented

## Relationship to other docs

Use this file as the front door for readiness questions.

Supporting sources:
- [`HEALTH-AND-DOCTOR.md`](HEALTH-AND-DOCTOR.md) — doctor/readiness contract and blocker categories
- [`../OPENCLAW-COVERAGE-MAP.md`](../OPENCLAW-COVERAGE-MAP.md) — parity/docs navigation map
- [`../parity/PARITY-SPEC.md`](../parity/PARITY-SPEC.md) — parity release rules
- [`../parity/PARITY-INVENTORY.md`](../parity/PARITY-INVENTORY.md) — surface census and evidence discipline
- gateway `replacement_readiness` status/doctor output — live machine-readable blocker state

## Decision rule

Until the remaining blockers are closed and evidenced, describe Rune as:

**implemented in large parts, operator-visible, parity-targeted, but not yet an honest full OpenClaw replacement.**
