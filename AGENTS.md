# AGENTS.md — Rune Repository Rules

## Session Startup

1. Read `SOUL.md` — this is who you are
2. Read `MEMORY.md` — this is what you remember
3. Check open GitHub issues for current work

## Git Workflow — MANDATORY

**Every change goes through a PR. No exceptions.**

1. Create a branch: `agent/<role>/<scope>` (e.g., `agent/rune/spell-security-audit`)
2. Make atomic commits, each referencing an issue: `feat(module): description (#issue)`
3. Push the branch
4. Open a PR with clear title and body referencing the issue
5. Ensure `cargo check` and `cargo clippy` pass
6. Squash merge to main
   - Merge only after all enforced CI jobs for that PR are green (for Rune: `check-and-lint` and `test`)
7. Delete the branch after merge
8. Update/close the GitHub issue

### What NOT to do
- ❌ Commit directly to main
- ❌ Push to main
- ❌ Create commits without issue references
- ❌ Leave PRs open without description
- ❌ Ignore failing tests

### GitHub Issue Maintenance
- Update issues with progress comments as you work
- Close issues when the PR merges (use "Closes #N" in PR body)
- If blocked, comment on the issue explaining why
- Never leave stale issues without status updates

## Coordination with Horizon AI

You work alongside Horizon AI (OpenClaw). Coordinate via `.comms/` filesystem mailboxes:
- Check `.comms/rune/inbox/` for messages
- Send responses to `.comms/horizon-ai/inbox/`
- When both agents are working, claim lanes to avoid conflicts
- If Horizon AI ships a PR that affects your work, rebase before continuing

## Project Structure

```
crates/
  rune-core/       — shared types, traits
  rune-config/     — configuration management
  rune-store/      — storage backends (SQLite, PostgreSQL, Cosmos)
  rune-runtime/    — agent runtime, spells, memory
  rune-mcp/        — MCP client + server
  rune-gateway/    — HTTP gateway server
  rune-cli/        — CLI implementation
  rune-tools/      — tool execution
  rune-tts/        — text-to-speech
  rune-stt/        — speech-to-text
apps/
  cli/             — CLI binary
  gateway/         — gateway binary
```

## Build Commands

```bash
cargo check                    # type check
cargo clippy                   # lint
cargo test                     # all tests
cargo test -p rune-runtime     # single crate tests
cargo build --release -p rune-gateway-app  # release build
```

## Worktrees — MANDATORY

**Never edit files in the main worktree. Always use a git worktree.**

Multiple sessions run concurrently via tmux, and LSP plugins (rust-analyzer, typescript-lsp) revert uncommitted changes between tool calls. This has destroyed work repeatedly.

Before ANY file edit:
1. `git worktree add ../rune-work-<feature> -b <branch>`
2. Do all edits and commits in the worktree
3. Push from the worktree
4. Use `isolation: "worktree"` on Agent/subagent calls

No exceptions. Not even for one-liners.

## Red Lines

- Never commit secrets, credentials, API keys, or tokens
- Never push directly to main
- Never delete branches with unmerged work without asking
- Never edit files in the main worktree (use git worktrees)
- When in doubt, ask via comms
