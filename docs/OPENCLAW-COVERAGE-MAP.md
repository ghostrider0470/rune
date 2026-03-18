# OpenClaw Coverage Map

> Status: active parity-navigation document.
>
> Use this file to answer: "which Rune document or artifact proves coverage for this part of OpenClaw?"
>
> This file is a navigation map, not the exhaustive parity inventory.
> - Use [`PARITY-INVENTORY.md`](parity/PARITY-INVENTORY.md) for the full surface census.
> - Use [`PARITY-SPEC.md`](parity/PARITY-SPEC.md) for the release rule.
> - Use [`PARITY-CONTRACTS.md`](parity/PARITY-CONTRACTS.md) and [`PROTOCOLS.md`](parity/PROTOCOLS.md) for subsystem invariants.
> - Use [`IMPLEMENTATION-PHASES.md`](IMPLEMENTATION-PHASES.md) for sequencing and acceptance criteria.
> - Use GitHub Project 2 and issue comments for live execution state.

## Relationship to the planning docs

| Question | Canonical source |
|---|---|
| What is Rune trying to become? | [`../rune-plan.md`](../rune-plan.md) |
| What parity phases exist and what must each phase prove? | [`IMPLEMENTATION-PHASES.md`](IMPLEMENTATION-PHASES.md) |
| What OpenClaw surfaces must be covered overall? | [`PARITY-INVENTORY.md`](parity/PARITY-INVENTORY.md) |
| Where do I start if I need the parity-related docs front door? | `OPENCLAW-COVERAGE-MAP.md` |

## Coverage map by surface

| OpenClaw surface | Primary Rune coverage doc(s) | Use when you need |
|---|---|---|
| CLI commands and operator workflows | [`PARITY-INVENTORY.md`](parity/PARITY-INVENTORY.md), [`FUNCTIONALITY-CHECKLIST.md`](FUNCTIONALITY-CHECKLIST.md) | command-family parity and evidence status |
| Gateway / daemon / control plane | [`PARITY-CONTRACTS.md`](parity/PARITY-CONTRACTS.md), [`PROTOCOLS.md`](parity/PROTOCOLS.md), [`IMPLEMENTATION-PHASES.md`](IMPLEMENTATION-PHASES.md) | runtime behavior and phase acceptance criteria |
| Sessions / turns / transcripts | [`PROTOCOLS.md`](parity/PROTOCOLS.md), [`PARITY-CONTRACTS.md`](parity/PARITY-CONTRACTS.md) | state model and behavioral invariants |
| Tools / approvals / process execution | [`PARITY-INVENTORY.md`](parity/PARITY-INVENTORY.md), [`PARITY-CONTRACTS.md`](parity/PARITY-CONTRACTS.md) | tool-surface coverage and gating behavior |
| Scheduler / reminders / heartbeat | [`PARITY-INVENTORY.md`](parity/PARITY-INVENTORY.md), [`IMPLEMENTATION-PHASES.md`](IMPLEMENTATION-PHASES.md) | automation semantics and sequencing |
| Memory / retrieval | [`PARITY-INVENTORY.md`](parity/PARITY-INVENTORY.md), [`IMPLEMENTATION-PHASES.md`](IMPLEMENTATION-PHASES.md), [`operator/MEMORY.md`](operator/MEMORY.md) | memory surface and active implementation state |
| Channels / adapters / messaging | [`PARITY-INVENTORY.md`](parity/PARITY-INVENTORY.md), [`PROTOCOLS.md`](parity/PROTOCOLS.md), crate-level channel tests | adapter behavior and inbound/outbound semantics |
| Media / browser / OCR / TTS / STT | [`PARITY-INVENTORY.md`](parity/PARITY-INVENTORY.md), [`IMPLEMENTATION-PHASES.md`](IMPLEMENTATION-PHASES.md) | adjacent parity surfaces and phase placement |
| Config / secrets / precedence | [`PROTOCOLS.md`](parity/PROTOCOLS.md), [`PARITY-CONTRACTS.md`](parity/PARITY-CONTRACTS.md), operator docs | runtime config behavior and operator-facing expectations |
| Deployment / Docker / persistence | [`operator/DEPLOYMENT.md`](operator/DEPLOYMENT.md), [`operator/DATABASES.md`](operator/DATABASES.md), [`PARITY-SPEC.md`](parity/PARITY-SPEC.md) | persistent storage model and deployment contract |
| Admin UI / operator visibility | [`FUNCTIONALITY-CHECKLIST.md`](FUNCTIONALITY-CHECKLIST.md), relevant feature issues/PRs | visibility, diagnostics, and UI-facing evidence |

## How to use this file

1. Start here if you know the OpenClaw surface but not the right Rune doc.
2. Jump to the primary coverage doc for that surface.
3. Use GitHub Project 2 and issue comments to find the current active implementation slice.
4. Use `FUNCTIONALITY-CHECKLIST.md` when you need the implementation/evidence view rather than the planning view.

## Non-goal

This file is not intended to replace:

- the exhaustive parity inventory
- the phase acceptance document
- the live GitHub execution queue
- the strategy/positioning docs

It exists to keep those surfaces connected and discoverable.
