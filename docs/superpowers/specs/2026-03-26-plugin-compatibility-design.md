# Claude Code Plugin Compatibility — Design Spec

**Date:** 2026-03-26
**Goal:** Enable Rune to load and execute Claude Code plugins natively — skills, agents, hooks, commands, and MCP servers — while preserving Rune's own PLUGIN.md-based Rust plugin format as first-class.

## Decisions

- **Plugin sources:** `~/.rune/plugins/` (overrides) then `~/.claude/plugins/cache/` (fallback)
- **Components:** All five — skills, agents, hooks, commands, MCP servers
- **Hook scope:** Configurable per-plugin in `config.toml` (which session kinds)
- **MCP lifecycle:** Shared long-lived by default, per-session opt-in
- **Updates:** Passive scan every 5 min + `rune plugin` CLI for managing overrides
- **Architecture:** Adapter layer — Claude Code format in, Rune-native types out. Rune's PLUGIN.md + binary format stays first-class.

## Plugin Discovery

### Scanner

A unified `PluginScanner` scans two directories in order:

1. `~/.rune/plugins/` — each subdirectory is a plugin. Format detected by:
   - `PLUGIN.md` present → Rune-native (existing `plugin.rs` loader)
   - `.claude-plugin/plugin.json` present → Claude Code format (new adapter)
2. `~/.claude/plugins/cache/` — Claude Code format only. Plugins already loaded by the same name from `~/.rune/plugins/` are skipped (override).

### Scan Triggers

- Gateway startup
- Every 5 minutes via supervisor loop (hot-reload, diffing against current registry)
- `rune plugin reload` CLI command

### Config

```toml
[plugins]
scan_dirs = ["~/.rune/plugins", "~/.claude/plugins/cache"]
scan_interval_secs = 300

[plugins.overrides.superpowers]
session_kinds = ["direct", "channel"]

[plugins.overrides.security-guidance]
enabled = false

[plugins.overrides.sourcegraph]
mcp_lifecycle = "per_session"
```

Fields per override:
- `enabled` — bool, default `true`. Set `false` to disable without deleting.
- `session_kinds` — list of session kinds hooks fire for. Default: all (`["direct", "channel", "scheduled", "subagent"]`).
- `mcp_lifecycle` — `"shared"` (default) or `"per_session"`.

## Component Adapters

### 1. Skills

**Source:** `skills/<name>/<name>.md` — markdown with YAML frontmatter.

**Frontmatter fields:** `name`, `description`, `model`, `allowed-tools`, `user-invocable`.

**Adaptation to `Skill` type:**
- `name` → prefixed with plugin name: `superpowers:brainstorming`
- `description` → used for model tool matching and listing
- Markdown body → stored as prompt template
- `user-invocable` → exposed as slash command on Telegram/webchat/CLI
- `allowed-tools` → restricts tool registry for the turn executing the skill
- `model` → override passed to turn executor
- Sub-files in the skill directory are available by relative path for `Read` tool access during skill execution

**Skill fields added to `Skill` struct:**
- `allowed_tools: Option<Vec<String>>`
- `user_invocable: bool`
- `model: Option<String>`
- `prompt_body: String` (the full markdown content below frontmatter)

**Trigger:** When a skill is invoked (slash command or model selection), its `prompt_body` is prepended to the user message. If `allowed_tools` is set, the tool registry is filtered for that turn.

### 2. Agents

**Source:** `agents/<name>.md` — markdown with YAML frontmatter.

**Frontmatter fields:** `name`, `description`, `model`, `allowed-tools`, `when-to-use`, `color`.

**Adaptation:** Each agent becomes a subagent template in a new `AgentRegistry`:

```rust
pub struct AgentTemplate {
    pub name: String,           // e.g. "superpowers:code-reviewer"
    pub description: String,
    pub when_to_use: String,    // trigger description for model
    pub system_prompt: String,  // markdown body
    pub model: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
}
```

**Integration:** When the model or a skill requests a subagent with `subagent_type: "code-reviewer"`, the session engine looks up the matching `AgentTemplate`, creates a subagent session with the template's system prompt, and applies tool/model overrides.

The `AgentRegistry` is queried during tool definition generation — agent descriptions are included so the model knows which agents are available and when to use them.

### 3. Hooks

**Source:** `hooks/hooks.json` — array of hook entries.

**Hook entry format:**
```json
{
  "event": "PreToolUse",
  "matcher": { "tool_name": "Bash" },
  "hook": { "type": "command", "command": "check-safety.sh" },
  "description": "Safety check"
}
```

**Events — extending Rune's `HookEvent` enum:**

| Claude Code Event | Rune HookEvent | Status |
|---|---|---|
| PreToolUse | `PreToolCall` | Exists |
| PostToolUse | `PostToolCall` | Exists |
| Stop | `Stop` | New |
| SubagentStop | `SubagentStop` | New |
| SessionStart | `SessionCreated` | Exists |
| SessionEnd | `SessionCompleted` | Exists |
| UserPromptSubmit | `UserPromptSubmit` | New |
| PreCompact | `PreCompact` | New |
| Notification | `Notification` | New |

**Hook types:**
- `"command"` — shell command. Receives context as JSON on stdin, env vars for tool name/session id. Exit code 0 = allow, non-zero = block (for Pre* hooks). Stdout is captured as hook output.
- `"prompt"` — prompt-based hook. The hook body is sent to the model with context. Model response determines action (allow/block/modify).

**Matcher:** Evaluated before calling the handler. Supports `tool_name` (exact or glob), `resource` patterns. Only fires if matcher matches.

**Session kind filtering:** Applied from `config.toml` `[plugins.overrides.<name>].session_kinds`. If the current session kind is not in the list, the hook is skipped.

**Adaptation:** Each hook entry becomes a `ClaudePluginHookHandler` implementing `HookHandler` trait, registered in `HookRegistry` under the appropriate `HookEvent`.

### 4. Commands

**Source:** `commands/<name>.md` — markdown with YAML frontmatter.

**Frontmatter fields:** `name`, `description`, `args` (argument definitions).

**Adaptation:** Commands become entries in a new `CommandRegistry`:

```rust
pub struct Command {
    pub name: String,           // e.g. "superpowers:commit"
    pub description: String,
    pub args: Vec<CommandArg>,
    pub prompt_body: String,    // markdown template
    pub plugin_name: String,
}

pub struct CommandArg {
    pub name: String,
    pub description: String,
    pub required: bool,
}
```

**Trigger:** Slash commands on Telegram (`/commit`), webchat, or CLI. The `prompt_body` is expanded with argument substitution (`$ARGUMENTS`, named args) and sent as the user message.

**Namespacing:** Full name is `plugin:command` (e.g. `superpowers:commit`). If no conflict, the short name (`/commit`) is aliased.

### 5. MCP Servers

**Source:** `.mcp.json` at the plugin root.

**Format:**
```json
{
  "mcpServers": {
    "server-name": {
      "type": "stdio",
      "command": "npx",
      "args": ["-y", "@package/mcp"],
      "env": { "API_KEY": "..." }
    }
  }
}
```

**Adaptation:** Each server entry is registered with `rune-mcp`'s existing MCP bridge.

**Lifecycle:**
- `"shared"` (default): Server started at gateway boot (or on first scan that discovers it). Runs continuously. Tools registered globally in the tool registry. All sessions can use them.
- `"per_session"`: Server started when a session begins that needs it, stopped when session ends. Configured via `[plugins.overrides.<name>].mcp_lifecycle = "per_session"`.

**Environment:** `env` values from `.mcp.json` are passed to the subprocess. `${CLAUDE_PLUGIN_ROOT}` is expanded to the plugin directory path (Claude Code convention).

## Architecture

```
config.toml [plugins]
       │
       ▼
PluginScanner (new)
  Scans ~/.rune/plugins + ~/.claude/plugins/cache
  Detects: PLUGIN.md → native loader
           plugin.json → Claude adapter
       │
       ├──→ SkillRegistry (existing, extended)
       ├──→ HookRegistry (existing, extended)
       ├──→ AgentRegistry (new)
       ├──→ CommandRegistry (new)
       └──→ MCP Bridge (existing rune-mcp)
```

## New Files

| File | Purpose |
|------|---------|
| `crates/rune-runtime/src/plugin_scanner.rs` | Unified scanner, format detection, delegates to loaders |
| `crates/rune-runtime/src/claude_plugin.rs` | Claude Code format parser — reads plugin.json, skills/, hooks/, agents/, commands/, .mcp.json |
| `crates/rune-runtime/src/agent_registry.rs` | Subagent template registry |
| `crates/rune-runtime/src/command_registry.rs` | Slash command registry |

## Modified Files

| File | Changes |
|------|---------|
| `crates/rune-runtime/src/hooks.rs` | Add HookEvent variants: Stop, SubagentStop, UserPromptSubmit, PreCompact, Notification |
| `crates/rune-runtime/src/skill.rs` | Add fields: `allowed_tools`, `user_invocable`, `model`, `prompt_body` |
| `crates/rune-config/src/lib.rs` | Add `[plugins]` config section with `PluginsConfig`, `PluginOverride` |
| `crates/rune-runtime/src/plugin.rs` | Existing native loader stays; scanner calls it for PLUGIN.md dirs |
| `crates/rune-runtime/src/executor.rs` | Hook dispatch at PreToolCall/PostToolCall points; skill allowed_tools filtering |
| `apps/gateway/src/main.rs` | Initialize PluginScanner, register periodic re-scan in supervisor |
| `crates/rune-gateway/src/supervisor.rs` | Periodic plugin re-scan (every N ticks) |

## Unchanged

- `crates/rune-mcp/` — MCP bridge already handles server lifecycle and tool registration
- `crates/rune-runtime/src/skill_loader.rs` — existing SKILL.md loader for Rune-native skills
- Rune's PLUGIN.md + binary subprocess format — remains first-class

## Out of Scope

- Plugin marketplace / install from URL (future)
- Plugin sandboxing / permission model (future)
- Plugin authoring tooling for Rune-native format (future)
- Claude Code plugin creation from within Rune (future)

## Success Criteria

1. `rune plugin list` shows all discovered plugins from both directories
2. Claude Code skills (e.g. superpowers:brainstorming) are invocable via Telegram slash commands
3. Claude Code hooks (e.g. security-guidance PreToolUse) fire during Rune turns
4. Claude Code MCP servers (e.g. context7) start at boot and their tools appear in the registry
5. Claude Code agents (e.g. code-reviewer) are available as subagent templates
6. Rune-native PLUGIN.md plugins still work unchanged
7. Per-plugin session_kinds filtering works via config.toml
8. Hot-reload detects new/removed plugins within 5 minutes
