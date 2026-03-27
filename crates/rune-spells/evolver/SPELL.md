---
name: evolver
namespace: rune.evolver
version: 0.1.0
kind: workflow
author: Horizon AI
description: Local self-evolution workflow spell for Rune with signal extraction, gene/capsule persistence, mutation validation, rollback, reflection, and human review. No outbound telemetry or publishing.
requires:
  - filesystem
  - process
  - git
tags:
  - evolution
  - self-improvement
  - local-only
  - safety
  - review
triggers:
  - evolve the system
  - run one evolution cycle
  - review pending mutations
  - show evolution status
match_rules:
  capabilities:
    any:
      - corrections
      - validation
      - git-checkpoint
      - local-persistence
  constraints:
    outbound_network: forbidden
    telemetry: forbidden
    publishing: forbidden
    external_access: read-only-opt-in
---

# Evolver

Local-only self-evolution spell for Rune.

## Guarantees

- No outbound telemetry, publishing, or device fingerprinting.
- Evolution stays local; git history is the audit trail.
- Uses validation and rollback before solidifying changes.
- Supports human review for pending mutations.

## Planned interfaces

- `evolve(strategy?)`
- `evolve_review(action)`
- `evolve_status()`

## Planned subsystems

1. Signal extraction
2. Gene and capsule selection
3. Mutation planning with blast radius estimation
4. Validation pipeline
5. Solidification with git checkpoint + rollback
6. Reflection engine
7. Narrative memory
8. Curriculum learning
