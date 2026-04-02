# ADR-0001: Tool Surfaces Must Be Runtime-Readable

- Status: Accepted
- Date: 2026-03-30

## Context

Rune already exposes operational knowledge through tools such as `memory_search`, `memory_get`, `read`, `git`, `sessions_list`, and gateway RPC surfaces. Durable architecture decisions existed in `docs/adr/`, but they were not exposed through a first-class runtime tool surface. That meant models and operators could only discover them by knowing repo paths in advance or by using generic file tools.

Phase 15 requires a Memory Bank foundation that makes project knowledge discoverable and queryable inside Rune itself.

## Decision

Create a dedicated `memory-bank/` workspace layout for durable project knowledge and expose it through two runtime tools:

- `memory_bank_list` — lists markdown documents under `memory-bank/`
- `memory_bank_get` — reads bounded content from a markdown document under `memory-bank/`

The initial Memory Bank slice should focus on architectural knowledge, starting with ADRs. Existing docs under `docs/adr/` remain valid repo documentation, but Memory Bank becomes the runtime-facing knowledge surface that tools can expose safely and predictably.

## Consequences

### Positive

- Models can discover durable architecture docs without guessing file paths.
- Runtime knowledge access is safer than unconstrained file reads because paths stay scoped to `memory-bank/`.
- The Memory Bank layout gives future phases a stable place for design notes, playbooks, and project-specific knowledge.

### Negative

- ADR knowledge now exists in two places unless older docs are mirrored or migrated deliberately.
- Another tool surface adds maintenance overhead for registration, validation, and tests.

## Follow-up

- Add more seeded Memory Bank documents for subsystems that currently only live in repo docs.
- Consider linking or mirroring `docs/adr/` entries into `memory-bank/adr/` as the bank expands.
