# ARCHITECTURE

Generated 2026-04-07.

## Purpose
- Structured project knowledge for Rune's runtime, gateway, UI, and orchestration layers.

## Runtime Shape
- `crates/rune-runtime`: session loop, executor, orchestration, context, heartbeat, skills/spells/plugins.
- `crates/rune-gateway`: HTTP/WebSocket control plane, RPC, channel/service routes, admin-facing APIs.
- `crates/rune-tools`: built-in tool registry and executors for file/exec/git/web/process/session/subagent/comms flows.
- `ui/`: TanStack Router admin UI for chat, usage, config, agents, skills, and operations surfaces.

## Integration Notes
- Workspace identity and operating rules come from `AGENTS.md`, `SOUL.md`, `USER.md`, `TOOLS.md`, and roadmap/canonical planning docs.
- Prompt context already uses tiered assembly; the memory bank should remain concise enough to fit as high-signal project guidance.
- `.rune/knowledge/` is the canonical location for durable project knowledge files introduced by Phase 25.
