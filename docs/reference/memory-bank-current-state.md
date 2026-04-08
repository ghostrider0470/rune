# Memory Bank: current shipped surface

Status: current implementation reference.

This document describes the Memory Bank functionality that is actually shipped in Rune today.
It exists to prevent confusion with broader planned Memory Bank work described in the Phase 25 spec.

## Current implementation

Rune currently exposes two Memory Bank tools in `crates/rune-tools/src/memory_tool.rs`:

- `memory_bank_list`
- `memory_bank_get`

Current behavior:

- `memory_bank_list` and `memory_bank_get` currently operate on the workspace `.rune/knowledge/` directory
- only Markdown files are discoverable/readable through those two tools
- `memory_bank_list` lists Markdown documents under `.rune/knowledge/`
- `memory_bank_get` reads a specific Markdown file under `.rune/knowledge/`
- path traversal is explicitly rejected for both tools
- separate runtime prompt injection now also seeds and loads `.rune/knowledge/` via `MemoryBankLoader` in `crates/rune-runtime/src/memory_bank.rs`
- that `.rune/knowledge/` scaffold currently contains `ARCHITECTURE.md`, `DECISIONS.md`, `CONVENTIONS.md`, and `DEPENDENCIES.md`

## Not yet implemented from the Phase 25 spec

The Phase 25 specification in `docs/specs/phases-25-27.md` describes a larger future subsystem that is not fully shipped today, including:

- the currently shipped tools already use `.rune/knowledge/` as their on-disk source; the remaining future work is broader API/search/staleness/onboarding support
- a unified `memory_bank` tool with read/update/search operations
- persistence via `knowledge_docs` store models and migrations
- staleness detection/reporting
- `/onboard` project briefing generation
- automatic memory-bank context injection from that subsystem beyond the currently shipped runtime-side scaffold loader

Treat that Phase 25 document as the target design, not as proof of current parity.

## Contributor guidance

When reasoning about current behavior, use the implementation in:

- `crates/rune-tools/src/memory_tool.rs`
- tool registration in `crates/rune-tools/src/stubs.rs`
- related tests in `crates/rune-tools/src/tests.rs`
- runtime scaffold loader in `crates/rune-runtime/src/memory_bank.rs`

When planning future work, use the Phase 25 spec as the intended end state.

## Source-of-truth rule

If implementation and planning docs diverge, current shipped behavior is defined by the code and tests, not by the future-phase design doc.


# Mem0 dedup/update policy

Rune's mem0 capture path now uses a conservative policy:

- approximate embedding similarity alone does **not** overwrite an existing fact
- exact normalized fact matches may update the existing memory row
- near-duplicate but text-distinct facts are preserved as new memories
- capture/store APIs return a per-fact decision trail so operators can inspect what happened

Decision actions currently exposed:

- `inserted` — a new memory row was created
- `updated_exact` — an existing memory was updated because the normalized fact text matched exactly
- `skipped_duplicate` — reserved for future explicit duplicate suppression flows
- `merged` — reserved for future explainable merge flows

The `/api/v1/memory/capture` and `/api/v1/memory/store` endpoints now return `decisions[]` with:

- `action`
- `reason`
- `matched_memory_id`
- `matched_fact`
- `similarity`
- `memory` (when a row was inserted/updated)
