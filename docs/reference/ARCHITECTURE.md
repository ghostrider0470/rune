# Architecture

This is the reference-level architecture overview for Rune.

## Runtime shape

Rune is a messaging-first AI runtime with:
- a long-running gateway daemon
- a session/turn execution engine
- tool execution and approvals
- durable storage
- provider abstractions
- operator-facing control-plane surfaces

## Core layers

### Gateway / control plane
Responsible for:
- HTTP and dashboard surfaces
- health/status/diagnostics
- auth and access control
- process supervision and runtime hosting

### Runtime engine
Responsible for:
- sessions
- turns
- context assembly
- tool loop orchestration
- scheduled execution
- transcript lifecycle

### Persistence
Responsible for durable storage of:
- sessions and transcripts
- cron jobs and runs
- approvals and execution records
- channel/device-related state

### Provider layer
Responsible for:
- model/provider routing
- Azure-oriented provider behavior
- model capability abstraction
- future media capability routing

### Channel layer
Responsible for:
- inbound normalization
- outbound delivery
- reply/reaction/media semantics
- adapter-specific integration behavior

## Agent-team orchestration

Rune's multi-agent model is lead/worker based: a parent session acts as the lead, delegated child sessions act as workers, and durable session/subagent metadata is the control-plane backbone for inspection, steering, cancellation, and result collection. See [`AGENT-TEAMS.md`](AGENT-TEAMS.md) for the concrete model and current implementation reality.

## Current reference use

Use this doc as the architecture entrypoint for:
- understanding Rune's current runtime shape and subsystem boundaries
- navigating from high-level architecture questions into deeper protocol, crate-layout, and subsystem references

## Read next

- use [`CRATE-LAYOUT.md`](CRATE-LAYOUT.md) and [`SUBSYSTEMS.md`](SUBSYSTEMS.md) when you need implementation-structure detail
- use [`../parity/PROTOCOLS.md`](../parity/PROTOCOLS.md) and [`../parity/PARITY-CONTRACTS.md`](../parity/PARITY-CONTRACTS.md) when the question is really about runtime semantics and invariants
- use [`../INDEX.md`](../INDEX.md) when you need to jump back out to the wider docs front door

## Further detail still missing

Deeper follow-up documentation is still useful for:
- cross-cutting runtime flows and boundaries
- storage/provider/channel relationship details
- architecture-level invariants and tradeoffs
- diagrams or richer deployment/control-plane context if that becomes useful

## Anti-thrash runtime guardrails

Rune now carries a first anti-thrash foundation in the runtime session layer:
- repeated failures are fingerprinted against the normalized failing message and error shape
- failure state is persisted in session metadata under `anti_thrash`
- repeated retries on the same failing fingerprint are backoff-suppressed before they can keep re-entering the executor
- retry budgets eventually exhaust, marking the objective/session state as suppressed instead of pretending the runtime is still productively shipping

This is intentionally session-scoped foundation work for M10 issue #754 / feature #756. It establishes durable failure fingerprints, retry counters, suppression reasons, and next-retry timestamps that later scheduler/control-plane surfaces can consume.

## Orchestrator goal leases and duplicate suppression

Multi-agent orchestration state now persists goal ownership explicitly alongside file locks and merge-queue state. `OrchestratorState` carries durable `goal_leases` and `goal_conflicts` records so agents can suppress duplicate execution of the same goal, recover stale ownership when a lease expires, and leave an operator-visible audit trail for both suppression and reassignment decisions.

Shipped behavior:
- active leases suppress duplicate claims from other agents and append a `duplicate_suppressed` conflict record
- expired leases can be reclaimed by a new agent without hidden concurrent ownership; the recovered lease records `recovered_at` and `recovered_from_agent_id`
- agent entries can carry the currently owned `goal_key` so state snapshots explain which agent owns which delegated objective
- claim operations now return the durable lease snapshot directly, and state helpers can answer the current owner for a goal or expire stale leases while clearing dangling agent goal assignments

This is the orchestration-state slice for issue #779 under feature #766. It does not replace higher-level runtime routing yet; it establishes the durable ownership/audit primitive that later gateway and scheduler surfaces can expose directly.
