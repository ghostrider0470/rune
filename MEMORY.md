# MEMORY.md — Rune Long-Term Memory

## Identity
- I am Rune, a Rust-based AI runtime built by Hamza Abdagic (Horizon Tech d.o.o.)
- Repository: ghostrider0470/rune on GitHub
- Partner: Horizon AI (OpenClaw instance) — we coordinate via .comms/ mailboxes

## Current State (2026-03-27)
- Main at 2b34d14 (after PRs #308, #309, #310 merged by Horizon AI)
- 47 open issues, ~72 closed
- Gateway: PG backend, ~1032 sessions
- Spells foundation shipped (#299/#300 via PR #308)
- Shared memory REST API shipped (#294 via PR #309)
- MCP memory server shipped (#296 via PR #310)

## Shipped Work
- Diesel→tokio-postgres migration (phases 1-3)
- Cosmos DB backend skeleton (factory wired, repos implemented)
- Spells system (renamed from skills, SPELL.md manifest with namespace/version/kind)
- Shared vector memory (pgvector + Mem0, REST API + MCP server)
- Webchat with session resume
- ACP dispatch (Claude Code + Codex)
- Auto-update CLI

## Active Work
- Security audit spell (#301) — branch: agent/rune/spell-security-audit
- rune-store integration tests (tokio-postgres)

## Key Issues
- #295: Shared Knowledge Graph epic
- #307: ClawHub Skill Parity epic (#301-#306)
- #297: Auto-capture hooks (P2)
- #13: Memory epic (~85% done)
- #51: Multi-database (Cosmos skeleton, not fully wired)

## Hard Rules (from Hamza, 2026-03-27)
- ALL changes through PRs. No direct commits to main. EVER.
- Every PR references an issue.
- Update issues as execution trail.
- Branch convention: agent/<role>/<scope>
- Squash merge only.
- No data leaks externally. All secrets stay local.

## Lessons Learned
- Direct commits to main are NOT acceptable — Hamza explicitly corrected this behavior
- Inbox handler cron only acks messages — need to actually reason on substance
- Coordinate with Horizon AI before starting work to avoid lane conflicts
