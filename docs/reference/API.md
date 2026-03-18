# API Reference Entry

This document is the stable reference entry for Rune's operator-facing API surface.

## Current scope

Rune exposes operator-facing HTTP endpoints and dashboard/API surfaces through the gateway.

Use these docs for the current contract picture:
- [`../parity/PROTOCOLS.md`](../parity/PROTOCOLS.md) — protocol and entity boundaries
- [`../parity/PARITY-CONTRACTS.md`](../parity/PARITY-CONTRACTS.md) — behavioral expectations and invariants
- [`../OPENCLAW-COVERAGE-MAP.md`](../OPENCLAW-COVERAGE-MAP.md) — where to find parity coverage by surface

## Current reference use

Use this doc as the API entrypoint for:
- understanding where the current HTTP/dashboard/control-plane contract picture lives
- navigating from API questions into parity contracts and coverage docs

## Next depth to add

This file can still grow into deeper reference for:
- core HTTP route families
- auth expectations
- dashboard/API shape pointers
- session and control-plane resource summaries

Until a fuller API reference is split out, treat the parity docs as the detailed contract source.
