---
name: context-budgeting
description: Partition-aware context budgeting with checkpointing and heartbeat-driven garbage collection.
namespace: horizon.context-budgeting
version: 0.1.0
author: Horizon AI
kind: tool
requires:
  - filesystem
  - memory
tags:
  - context
  - memory
  - gc
  - checkpoint
triggers:
  - context budget
  - checkpoint context
  - compact context
  - gc context
enabled: true
---

# Context Budgeting

Provides partition-aware context budgeting primitives for Rune runtime sessions.

## Capabilities
- Reports partition usage across objective, history, decision log, background, and reserve
- Creates persistent checkpoints before compaction
- Runs heartbeat-driven garbage collection above budget thresholds
- Preserves objective context and recent history during compaction

## Tools
- `context_budget()`
- `context_checkpoint()`
- `context_gc(aggressive)`
