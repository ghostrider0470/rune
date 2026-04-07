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
- `anti_thrash.stall_reason`
- `anti_thrash.operator_note`
- `anti_thrash.last_error`

Current semantics:
- when the same inbound message keeps triggering the same failure fingerprint, Rune records retry/backoff metadata instead of blindly re-entering the executor
- while `next_retry_at` is still in the future, repeated retries for that same fingerprint are suppressed
- once the retry budget is exhausted, the session remains alive but is explicitly marked as suppressed for that fingerprint
- terminal suppression now persists both a machine-readable `stall_reason` and a human-facing `operator_note`, so route/status surfaces can explain whether the session is waiting for backoff expiry or needs intervention for an exhausted retry budget

This is the first operator-visible M10 anti-thrash foundation rather than the full final status surface. For now, operators can inspect persisted session metadata to distinguish a degraded-but-alive lane from one that is still actively shipping.

- Anti-thrash metadata now persists objective fingerprints and objective snapshots for repeated-failure diagnosis across reloads/restarts.
- `GET /api/dashboard/sessions` now lifts the anti-thrash fields into top-level operator-visible session diagnostics so dashboards do not need to parse raw metadata blobs.

Dashboard session anti-thrash fields:
- `stall_reason` — machine-readable/current operator-visible reason the lane is stalled or suppressed
- `operator_note` — human-facing remediation hint for exhausted budget or active backoff
- `next_retry_at` — retry release timestamp when backoff is still active
- `retry_budget_exhausted` — whether the repeated-failure budget is terminally exhausted
- `suppression_reason` — normalized suppression code such as `backoff_active` or `retry_budget_exhausted`
- `last_error` — latest recorded failure string associated with the fingerprint
- `failure_fingerprint` — normalized repeated-failure key
- `objective_fingerprint` — normalized objective key for the work that keeps failing
- `objective_snapshot` — structured objective summary captured when suppression was recorded


## Readiness SLO contract

`/api/doctor/run`, `/api/doctor/results`, and `rune doctor` now declare the current responsiveness readiness contract explicitly.

Current target SLOs:
- interactive response latency: `<= 2000ms`
- queue delay before execution starts: `<= 500ms`
- stuck-turn rate: `<= 1.0%`
- recovery time after a detected stuck turn: `<= 60s`

Current readiness semantics:
- doctor surfaces `readiness_status=slo_defined_evidence_pending` until Rune exposes live evidence for queue delay, stuck-turn rate, and recovery-time compliance
- defined targets without live evidence are **not** treated as ready
- operators should treat `readiness_summary` as the canonical explanation for why readiness is blocked or satisfied
- doctor also emits a `replacement_readiness` section with a direct `verdict`, concise `summary`, and explicit blocker categories (`operational`, `product-surface`, `runtime-resilience`, `documentation`) mapped to canonical GitHub issues

This keeps readiness claims honest: the SLO target exists now, but replacement-readiness remains blocked until the runtime publishes those signals.

## Replacement-readiness verdict

`/api/doctor/run`, `/api/doctor/results`, and `rune doctor` now answer the operator question directly: is Rune an honest OpenClaw replacement yet?

Current contract:
- `replacement_readiness.verdict=not_ready` means Rune must not be presented as a full replacement yet
- `replacement_readiness.summary` is the short operator-facing verdict sentence
- `replacement_readiness.blockers[]` is the machine-readable blocker list for automation and dashboards
- each blocker includes a category, normalized status, concise detail, and canonical issue reference when the blocker maps to tracked roadmap work

Current blocker mapping:
- `operational` → readiness evidence still pending direct live queue-delay / stuck-turn / recovery-time publication in status/doctor surfaces
- `documentation` → parity/operator evidence reconciliation (`#896`)

## Channel trust-boundary truth

Rune now documents channel trust-boundary behavior explicitly rather than implying OpenClaw parity where it does not exist yet.

Current shipped truth by surface:
- **Telegram** — webhook authenticity can be enforced through the bot secret-token verification path, but Rune does **not** ship a sender allowlist for inbound Telegram users/chats. Any message delivered by Telegram to an enabled bot can enter the session routing path.
- **Slack** — webhook authenticity is enforced when the local Events API listener is enabled via `slack_signing_secret`, but Rune does **not** ship a sender/user/channel allowlist decision layer for accepted inbound events.
- **WhatsApp** — webhook verification and app-secret signature validation are enforced when configured, but Rune does **not** ship a sender/phone allowlist decision layer beyond provider authenticity.
- **Discord / Signal / Teams** — provider credentials and transport setup gate connectivity, but Rune does **not** currently expose an operator-configurable trusted-sender allowlist for inbound acceptance decisions.
- **Web/operator surfaces** — operator/API authorization is a separate control plane concern and is not equivalent to channel sender allowlisting.

Operator interpretation:
- Rune currently distinguishes **provider authenticity** from **sender trust policy**.
- Provider authenticity checks are shipped for relevant webhook surfaces.
- Sender allowlist parity with OpenClaw is **not shipped** yet and remains an explicit gap, not hidden behavior.

Current inspectability:
- The parity matrix records this as a gap instead of implying parity.
- Doctor/readiness documentation now states the divergence plainly so operators can make an informed deployment decision.
- If sender-level trust boundaries are required for a deployment, treat that as a blocking product gap until issue `#898` lands a real policy surface.
