# DECISIONS

Generated 2026-04-07.

## ADR Log

### 2026-04-07 — Seed memory bank files under `.rune/knowledge/`
- Context: Phase 25 required a structured knowledge base, but no canonical files or loader existed.
- Decision: Bootstrap `ARCHITECTURE.md`, `DECISIONS.md`, `CONVENTIONS.md`, and `DEPENDENCIES.md` automatically on first load and inject them into direct-session context.
- Consequences: Agents now have a stable, repo-local knowledge surface that can be incrementally enriched without blocking on later automation.
