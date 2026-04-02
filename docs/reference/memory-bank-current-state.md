# Memory Bank: current shipped surface

Status: current implementation reference.

This document describes the Memory Bank functionality that is actually shipped in Rune today.
It exists to prevent confusion with broader planned Memory Bank work described in the Phase 25 spec.

## Current implementation

Rune currently exposes two Memory Bank tools in `crates/rune-tools/src/memory_tool.rs`:

- `memory_bank_list`
- `memory_bank_get`

Current behavior:

- data source is the workspace `memory-bank/` directory
- only Markdown files are discoverable/readable
- `memory_bank_list` lists Markdown documents under `memory-bank/`
- `memory_bank_get` reads a specific Markdown file under `memory-bank/`
- path traversal is explicitly rejected

## Not yet implemented from the Phase 25 spec

The Phase 25 specification in `docs/specs/phases-25-27.md` describes a larger future subsystem that is not fully shipped today, including:

- `.rune/knowledge/` as the canonical storage location
- a unified `memory_bank` tool with read/update/search operations
- persistence via `knowledge_docs` store models and migrations
- staleness detection/reporting
- `/onboard` project briefing generation
- automatic memory-bank context injection from that subsystem

Treat that Phase 25 document as the target design, not as proof of current parity.

## Contributor guidance

When reasoning about current behavior, use the implementation in:

- `crates/rune-tools/src/memory_tool.rs`
- tool registration in `crates/rune-tools/src/stubs.rs`
- related tests in `crates/rune-tools/src/tests.rs`

When planning future work, use the Phase 25 spec as the intended end state.

## Source-of-truth rule

If implementation and planning docs diverge, current shipped behavior is defined by the code and tests, not by the future-phase design doc.
