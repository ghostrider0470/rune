# Plugin Runtime — Design Spec

**Date:** 2026-03-26
**Goal:** Make loaded Claude Code plugins actually execute — command routing, agent template lookup, MCP server startup, hook session filtering, plugin lifecycle management, and a `/plugins` API.

## Decisions

- **Command routing:** `/command` prefix from all surfaces + model can invoke via tool call
- **Agent lookup:** Exact match first, then wildcard across plugins, fall back to default
- **MCP lifecycle:** Shared servers start at boot, per-session servers start lazily
- **Hook filtering:** Session-kind filtering from config.toml `[plugins.overrides]`
- **Scope:** Full plugin runtime with lifecycle management and API

## 1. Command Routing

Intercept messages starting with `/` in the session loop before they reach the model. Look up in `CommandRegistry` (full name or alias). If found, expand the template with arguments and send as the user message.

**Built-in meta-commands:**
- `/commands` — list all registered commands with descriptions
- `/plugins` — list loaded plugins with component counts

**Surfaces:** Telegram, webchat, CLI, WS RPC — all enter through session loop.

## 2. Agent Template Lookup

When a subagent session is created (via `sessions_spawn` tool or internal spawn), check `AgentRegistry`:

1. Exact match on `subagent_type` (e.g. `"superpowers:code-reviewer"`)
2. Wildcard — strip prefix, search `*:code-reviewer` across all plugins
3. No match — existing behavior (generic subagent)

When matched: apply template's `system_prompt`, `allowed_tools`, and `model` override.

Agent descriptions are injected into the system prompt fragment alongside skills, using `when_to_use` field.

## 3. MCP Server Startup

After plugin scan at boot, start all MCP servers with `shared` lifecycle (default). Register their tools in the tool registry.

**Shared servers:** Started once, tools available to all sessions. Restarted on crash.
**Per-session servers:** Started when a session first uses a tool from that server. Stopped when session ends. Configured via `[plugins.overrides.<name>].mcp_lifecycle = "per_session"`.

Uses the existing `rune-mcp` bridge for server management and tool registration.

## 4. Hook Session-Kind Filtering

Hooks already fire at `PreToolCall` and `PostToolCall` in the executor. Add session-kind filtering:

Before calling a hook handler, check if the current session's kind is in the plugin's `session_kinds` list from config. If not, skip the handler.

This requires passing `session_kind` into the hook context.

## 5. Plugin Lifecycle Management

**PluginManager** — a new coordinator that owns all registries and the scanner:
- `start(name)` / `stop(name)` — enable/disable a plugin at runtime
- `reload()` — re-scan directories, add new plugins, remove deleted ones
- `status()` — list all plugins with their state (enabled/disabled, component counts)

State is in-memory only — persisted via config.toml overrides.

## 6. Plugins API

HTTP routes for plugin management:

- `GET /plugins` — list all loaded plugins with component counts
- `GET /plugins/:name` — detail view (skills, agents, hooks, commands, MCP servers)
- `POST /plugins/:name/enable` — enable a plugin
- `POST /plugins/:name/disable` — disable a plugin
- `POST /plugins/reload` — trigger re-scan

WS RPC equivalents for the dashboard.

## Files Changed

| File | Changes |
|------|---------|
| `crates/rune-runtime/src/session_loop.rs` | Command interception before model call |
| `crates/rune-runtime/src/executor.rs` | Agent template lookup on subagent spawn, hook session-kind filtering, agent descriptions in system prompt |
| `crates/rune-runtime/src/plugin_manager.rs` | New — PluginManager coordinator |
| `crates/rune-gateway/src/routes.rs` | Plugins API routes |
| `crates/rune-gateway/src/ws_rpc.rs` | WS RPC plugin commands |
| `crates/rune-gateway/src/server.rs` | Wire PluginManager, start shared MCP servers |
| `apps/gateway/src/main.rs` | Initialize PluginManager, pass to routes and supervisor |

## Success Criteria

1. `/commit -m "fix"` from Telegram expands the commit command template and executes
2. `/commands` lists all registered plugin commands
3. Subagent spawned with `subagent_type: "code-reviewer"` gets the plugin's system prompt
4. Shared MCP server tools appear in the tool registry at startup
5. Hooks don't fire for cron sessions when configured with `session_kinds = ["direct", "channel"]`
6. `GET /plugins` returns all loaded plugins with counts
7. `POST /plugins/:name/disable` stops a plugin's hooks/skills from firing
