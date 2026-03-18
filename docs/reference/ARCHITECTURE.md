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

## Reference entrypoints

- [`CRATE-LAYOUT.md`](CRATE-LAYOUT.md)
- [`SUBSYSTEMS.md`](SUBSYSTEMS.md)
- [`../parity/PROTOCOLS.md`](../parity/PROTOCOLS.md)
- [`../parity/PARITY-CONTRACTS.md`](../parity/PARITY-CONTRACTS.md)
- [`../INDEX.md`](../INDEX.md)
