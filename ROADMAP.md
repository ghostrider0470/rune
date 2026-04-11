# Rune Roadmap — Active Execution Plan

Last updated: 2026-04-11

## Status: awaiting new product direction

All previously tracked execution lanes are shipped, including memory-bank follow-up fixes landed through #996. `gh issue list --state open --limit 20` currently returns no open issues, so there is no active implementation lane to resume.

## Execution Order

_No active roadmap lane._

### Current truth
- Closed recently: #994, #995, #996
- No open GitHub issues in `gh issue list --state open --limit 20`
- No unmerged active branch is required for roadmap continuation

### Next required action
Hamza needs to define the next product lane or issue set before autonomous ship-code execution can resume.

Until that exists, heartbeat sessions should return `HEARTBEAT_OK` unless one of these happens first:
1. a new GitHub issue is opened
2. Hamza gives a direct implementation task
3. OpenClaw sends a priority/directive that creates a new lane

## Execution Rules

1. One active PR at a time, merge immediately when green
2. Every PR references its issue number
3. Update issue with progress comments as work ships
4. Branch convention: `agent/rune/<issue-slug>`
5. Close issue when all acceptance criteria met
6. If blocked, comment on issue and move to the next unblocked item

## Done

- #996 — Clarify memory bank shipped-lane truth ✅
- #995 — Clarify empty scoped memory bank listings ✅
- #994 — Honor `.rune/knowledge` scope for bank listing ✅
- #993 — Remove stale gateway route stub labels ✅
- #992 — Align CLI readiness blockers with gateway truth ✅
- #762 — Deterministic plugin/hook engine ✅
- #754 — Anti-thrash and bounded self-repair loops ✅
- #409 — Smart Context Management ✅
- #408 — Universal Backend Interchangeability ✅
- #412 — Ollama provider support ✅
- #68 — Media & Document Understanding ✅
- #65 — Beautiful operator UI ✅
- #58 — Spells ✅
