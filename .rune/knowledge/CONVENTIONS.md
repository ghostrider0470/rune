# CONVENTIONS

Generated 2026-04-07.

## Working Rules
- Keep one active coding branch/PR per coherent batch.
- Validate with `cargo check` before shipping Rust changes.
- Treat roadmap/progress docs as historical unless superseded by canonical plan files and live GitHub issues.

## Prompt/Agent Conventions
- Load `.rune/knowledge/` files as project context for direct sessions.
- Keep memory bank entries operational, concise, and safe to inject into prompts.
