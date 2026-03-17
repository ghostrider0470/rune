# ADR-0002: Standalone-First Product Shape

- **Status:** Accepted
- **Date:** 2026-03-17

## Context

Rune needs a product shape that preserves OpenClaw-style assistant behavior while avoiding unnecessary infrastructure coupling.

Two competing tendencies were visible in earlier planning:
- treating Rune primarily as a server/platform product
- treating Rune primarily as a standalone personal runtime

The current implementation and operator goals point toward a standalone-first system with optional server-grade deployment paths rather than a server-first architecture.

This is also consistent with the docs reorganization proposal language:
- "standalone-first with optional server-grade deployment paths"
- durable local control plane
- Docker-first deployment without requiring cluster-only assumptions

## Decision

Rune will be designed as a **standalone-first personal AI runtime**.

That means:

### 1. Local-first default experience
- a single operator can run Rune locally with minimal infrastructure
- local or single-node deployments are first-class, not "dev mode only"
- zero-config fallbacks are acceptable when they do not compromise inspectability or durability

### 2. Server-grade deployment remains supported
- hosted or team-facing deployments are valid
- Docker and service-manager deployment paths remain first-class
- architecture should not block future Azure Container Apps, AKS, App Service, or VM-based hosting

### 3. Product complexity should justify itself
- do not force distributed-system complexity into the default user experience
- keep operator-facing behavior understandable on one machine before expanding outward

### 4. Durable control plane still matters
Standalone-first does **not** mean throwaway or toy mode.
Rune must still provide:
- durable state
- health and status visibility
- inspectable logs and diagnostics
- reproducible operator workflows

### 5. Cloud compatibility is additive, not identity-defining
Azure support remains a hard requirement, but Rune should not require Azure-native infrastructure to make sense as a product.

## Consequences

### Positive
- clearer product identity
- simpler default deployment story
- less pressure to over-engineer early orchestration/deployment layers
- better alignment with local assistant and self-hosted use cases

### Negative / tradeoffs
- some enterprise/server narratives become secondary rather than primary
- docs and positioning must be careful not to imply "cloud optional" means "cloud unimportant"
- later scaling/distribution work must be added intentionally instead of assumed from day one

## Alternatives considered

### Server-first platform shape
Rejected as the default because it pushes unnecessary infrastructure complexity into the product identity too early.

### Dual-first positioning with no default bias
Rejected because it produces ambiguous documentation and inconsistent operator expectations.

## Follow-up

This ADR should be reflected in:
- `README.md` positioning language
- `rune-plan.md`
- deployment and operator docs
- future ADRs about execution model and source-of-truth boundaries
