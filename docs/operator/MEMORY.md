# Memory

This is the operator-facing entry doc for Rune's memory surface.

## Scope

Rune's memory model includes:
- curated long-term memory in `MEMORY.md`
- daily/raw memory notes under `memory/*.md`
- retrieval/search over those file-oriented memory surfaces
- a privacy boundary where curated memory remains main-session-only

## Current canonical references

Use these docs for the current contract picture:
- [`../parity/PROTOCOLS.md`](../parity/PROTOCOLS.md)
- [`../parity/PARITY-INVENTORY.md`](../parity/PARITY-INVENTORY.md)
- [`../parity/PARITY-CONTRACTS.md`](../parity/PARITY-CONTRACTS.md)
- [`../FUNCTIONALITY-CHECKLIST.md`](../FUNCTIONALITY-CHECKLIST.md)
- [`../INDEX.md`](../INDEX.md)

## Current operator-visible surface

The current operator CLI already exposes a read-only memory surface with:
- `memory status`
- `memory search`
- `memory get`

The runtime also enforces the curated-memory privacy boundary: `MEMORY.md` is main-session-only, while non-direct contexts use the daily/raw memory surfaces without loading curated long-term memory.

## Current operator use

Use this doc as the memory entrypoint for:
- understanding the current file-oriented memory model
- locating the privacy-boundary and retrieval-surface references that already exist

## Next depth to add

This file can still grow into deeper reference for:
- memory storage conventions
- privacy-boundary explanation
- operator inspection/troubleshooting of memory retrieval
- links to deeper memory-specific docs if split later
