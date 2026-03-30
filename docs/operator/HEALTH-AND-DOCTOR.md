# Health and Doctor

This is the operator-facing entry doc for runtime health, diagnostics, and doctor-style checks.

## Scope

Operator-visible runtime health includes:
- gateway health/status surfaces
- startup/runtime diagnostics
- dependency-state visibility
- troubleshooting entrypoints

## Current canonical references

Use these docs for the current contract picture:
- [`OPERATOR-POLICY.md`](OPERATOR-POLICY.md)
- [`DEPLOYMENT.md`](DEPLOYMENT.md)
- [`DATABASES.md`](DATABASES.md)
- [`../parity/PARITY-CONTRACTS.md`](../parity/PARITY-CONTRACTS.md)
- [`../INDEX.md`](../INDEX.md)

## Current operator use

Use this doc as the health/diagnostics entrypoint for:
- where health/status expectations live now
- how to navigate to deployment, database, and parity contract troubleshooting references

## Read next

- use [`DEPLOYMENT.md`](DEPLOYMENT.md) and [`DATABASES.md`](DATABASES.md) when the issue looks like runtime/storage layout or persistence health
- use [`../parity/PARITY-CONTRACTS.md`](../parity/PARITY-CONTRACTS.md) when you need deeper runtime invariants behind a failing behavior
- use [`OPERATOR-POLICY.md`](OPERATOR-POLICY.md) when the question is operational guardrails rather than runtime failure analysis

## Further detail still missing

Deeper follow-up documentation is still useful for:
- health/status endpoint pointers
- doctor command/navigation
- common failure-mode triage
- dependency troubleshooting flow

## Anti-thrash diagnostics

Rune now persists early anti-thrash diagnostics in session metadata:
- `anti_thrash.failure_fingerprint`
- `anti_thrash.retry_count`
- `anti_thrash.budget_exhausted`
- `anti_thrash.suppression_reason`
- `anti_thrash.next_retry_at`
- `anti_thrash.last_error`

Current semantics:
- when the same inbound message keeps triggering the same failure fingerprint, Rune records retry/backoff metadata instead of blindly re-entering the executor
- while `next_retry_at` is still in the future, repeated retries for that same fingerprint are suppressed
- once the retry budget is exhausted, the session remains alive but is explicitly marked as suppressed for that fingerprint

This is the first operator-visible M10 anti-thrash foundation rather than the full final status surface. For now, operators can inspect persisted session metadata to distinguish a degraded-but-alive lane from one that is still actively shipping.
