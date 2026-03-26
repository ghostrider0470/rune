# Plugin Runtime Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make loaded Claude Code plugins actually execute — slash commands route through CommandRegistry, agent templates apply to subagent spawns, shared MCP servers start at boot, hooks filter by session kind, and a PluginManager provides lifecycle control + REST API.

**Architecture:** Six integration points wired into existing code paths. Command routing hooks into the existing `handle_command` fallthrough in `session_loop.rs`. Agent lookup uses the `AgentRegistry` populated by the scanner. Hook filtering adds session_kind to the hook context in `executor.rs`. MCP startup uses the existing `rune-mcp` bridge. PluginManager coordinates all registries. REST API follows the existing cron route pattern.

**Tech Stack:** Rust, tokio, axum, serde_json

---

### Task 1: Command Routing in Session Loop

**Files:**
- Modify: `crates/rune-runtime/src/session_loop.rs`

- [ ] **Step 1: Add CommandRegistry to SessionLoop**

In `crates/rune-runtime/src/session_loop.rs`, add a field to the `SessionLoop` struct:

```rust
    command_registry: Option<Arc<crate::command_registry::CommandRegistry>>,
```

Add a builder method:

```rust
    pub fn with_command_registry(mut self, registry: Arc<crate::command_registry::CommandRegistry>) -> Self {
        self.command_registry = Some(registry);
        self
    }
```

Initialize as `None` in the `new()` constructor.

- [ ] **Step 2: Wire plugin commands into handle_command fallthrough**

In the `handle_command` method, change the `_ => Ok(false)` fallthrough (around line 351) to check the command registry:

```rust
            _ => {
                // Check plugin commands
                if let Some(ref registry) = self.command_registry {
                    let cmd_name = cmd.trim_start_matches('/');
                    if let Some(command) = registry.get(cmd_name).await {
                        let expanded = command.expand(args);
                        // Route expanded prompt as a regular message through the turn executor
                        self.send_command_reply(msg, &format!("Running /{cmd_name}...")).await;
                        return Ok(false); // Let it fall through to normal message handling with expanded text
                    }
                }
                Ok(false)
            }
```

Actually, we need to replace the message text with the expanded prompt before it reaches the executor. The cleaner approach: return the expanded text and handle it in the caller. Let me check the caller.

The caller at line 147 is:
```rust
if self.handle_command(&msg).await? {
    return Ok(()); // was a built-in command, skip model
}
```

We need a third state: "was a plugin command, use expanded text instead of original". Change the return type and add command expansion:

In `handle_command`, change the `_ =>` arm to:

```rust
            _ => {
                if cmd.starts_with('/') {
                    if let Some(ref registry) = self.command_registry {
                        let cmd_name = cmd.trim_start_matches('/');
                        if let Some(command) = registry.get(cmd_name).await {
                            return Ok(false); // not a built-in, but we'll handle expansion in caller
                        }
                    }
                }
                Ok(false)
            }
```

Then in the message handling path (after `handle_command` returns `false`), before the message is sent to the executor, add command expansion:

Find the line where the text content is used to call `execute` or `execute_triggered` (search for where `msg.content` is passed to the turn executor). Before that call, add:

```rust
                // Expand plugin commands
                let final_text = if text_content.starts_with('/') {
                    if let Some(ref registry) = self.command_registry {
                        let (cmd, args) = match text_content.split_once(' ') {
                            Some((c, a)) => (c.trim_start_matches('/'), a.trim()),
                            None => (text_content.trim_start_matches('/'), ""),
                        };
                        if let Some(command) = registry.get(cmd).await {
                            info!(command = cmd, plugin = %command.plugin_name, "expanding plugin command");
                            command.expand(args)
                        } else {
                            text_content.to_string()
                        }
                    } else {
                        text_content.to_string()
                    }
                } else {
                    text_content.to_string()
                };
```

Then use `final_text` instead of `text_content` when calling the executor.

- [ ] **Step 3: Add `/commands` built-in**

In `handle_command`, add a new arm before the `_ =>` fallthrough:

```rust
            "/commands" => {
                let mut text = String::from("Available commands:\n\n");
                text.push_str("/start - show welcome\n/help - show help\n/status - runtime status\n/model - switch model\n/reset - clear history\n/commands - this list\n");
                if let Some(ref registry) = self.command_registry {
                    let commands = registry.list().await;
                    if !commands.is_empty() {
                        text.push_str("\nPlugin commands:\n");
                        for cmd in &commands {
                            text.push_str(&format!("/{} - {}\n", cmd.short_name(), cmd.description));
                        }
                    }
                }
                self.send_command_reply(msg, &text).await;
                Ok(true)
            }
```

- [ ] **Step 4: Build**

Run: `cargo build -p rune-runtime`
Expected: success

- [ ] **Step 5: Commit**

```bash
git add crates/rune-runtime/src/session_loop.rs
git commit -m "feat(runtime): route plugin slash commands via CommandRegistry"
```

---

### Task 2: Agent Template Lookup

**Files:**
- Modify: `crates/rune-runtime/src/executor.rs`

- [ ] **Step 1: Add AgentRegistry to TurnExecutor**

In `crates/rune-runtime/src/executor.rs`, add a field:

```rust
    agent_registry: Option<Arc<crate::agent_registry::AgentRegistry>>,
```

Initialize as `None` in the constructor. Add builder:

```rust
    pub fn with_agent_registry(mut self, registry: Arc<crate::agent_registry::AgentRegistry>) -> Self {
        self.agent_registry = Some(registry);
        self
    }
```

- [ ] **Step 2: Add agent descriptions to system prompt fragment**

In `run_turn_loop`, find where `skill_prompt_fragment` is built (around line 761). After adding skill fragments to `extra_system_sections`, add agent descriptions:

```rust
            // Inject agent template descriptions
            if let Some(ref agent_reg) = self.agent_registry {
                let agents = agent_reg.list().await;
                if !agents.is_empty() {
                    let mut fragment = String::from("\n\n## Available Agents\n\n");
                    for agent in &agents {
                        fragment.push_str(&format!("### {}\n", agent.name));
                        fragment.push_str(&format!("{}\n", agent.description));
                        if !agent.when_to_use.is_empty() {
                            fragment.push_str(&format!("When to use: {}\n", agent.when_to_use));
                        }
                        fragment.push('\n');
                    }
                    extra_system_sections.push(fragment);
                }
            }
```

- [ ] **Step 3: Add lookup method for subagent resolution**

Add a public method to `TurnExecutor`:

```rust
    /// Look up an agent template by name. Tries exact match, then wildcard.
    pub async fn resolve_agent_template(&self, subagent_type: &str) -> Option<crate::agent_registry::AgentTemplate> {
        let registry = self.agent_registry.as_ref()?;
        // Exact match
        if let Some(template) = registry.get(subagent_type).await {
            return Some(template);
        }
        // Wildcard: strip prefix, search all
        let short = subagent_type.split_once(':').map(|(_, s)| s).unwrap_or(subagent_type);
        let all = registry.list().await;
        all.into_iter().find(|t| {
            t.name.split_once(':').map(|(_, s)| s).unwrap_or(&t.name) == short
        })
    }
```

- [ ] **Step 4: Build**

Run: `cargo build -p rune-runtime`
Expected: success

- [ ] **Step 5: Commit**

```bash
git add crates/rune-runtime/src/executor.rs
git commit -m "feat(runtime): agent template lookup and system prompt injection"
```

---

### Task 3: Hook Session-Kind Filtering

**Files:**
- Modify: `crates/rune-runtime/src/executor.rs`
- Modify: `crates/rune-runtime/src/hooks.rs`

- [ ] **Step 1: Add session_kind to hook context**

In `crates/rune-runtime/src/executor.rs`, find the PreToolCall hook emit (around line 884). Add `session_kind` to the hook context JSON:

```rust
                    if let Some(ref hook_reg) = self.hook_registry {
                        let mut hook_ctx = serde_json::json!({
                            "tool_name": tc.function.name,
                            "arguments": &args,
                            "session_id": session_id.to_string(),
                            "turn_id": turn_id.into_uuid().to_string(),
                            "session_kind": format!("{:?}", session_kind),
                        });
```

Do the same for the PostToolCall hook emit (search for `PostToolCall` in executor.rs and add the same `session_kind` field).

- [ ] **Step 2: Add session_kind filtering to HookRegistry::emit**

In `crates/rune-runtime/src/hooks.rs`, modify the `HookHandler` trait to add an optional session_kinds filter:

```rust
pub trait HookHandler: Send + Sync {
    async fn handle(&self, event: &HookEvent, context: &mut serde_json::Value) -> Result<(), String>;
    fn plugin_name(&self) -> &str;
    /// Session kinds this handler applies to. None = all kinds.
    fn session_kinds_filter(&self) -> Option<&[String]> { None }
}
```

Then in `HookRegistry::emit`, add filtering:

```rust
    pub async fn emit(&self, event: &HookEvent, context: &mut serde_json::Value) {
        let handlers = self.handlers.read().await;
        let Some(event_handlers) = handlers.get(event) else {
            return;
        };

        let session_kind = context.get("session_kind").and_then(|v| v.as_str()).unwrap_or("");

        for handler in event_handlers {
            // Session-kind filtering
            if let Some(allowed) = handler.session_kinds_filter() {
                if !session_kind.is_empty() && !allowed.iter().any(|k| k.eq_ignore_ascii_case(session_kind)) {
                    debug!(
                        event = %event.as_str(),
                        plugin = %handler.plugin_name(),
                        session_kind = session_kind,
                        "skipping hook handler (session kind filtered)"
                    );
                    continue;
                }
            }

            if let Err(e) = handler.handle(event, context).await {
                warn!(event = %event.as_str(), plugin = %handler.plugin_name(), error = %e, "hook handler failed, continuing");
            }
        }
    }
```

- [ ] **Step 3: Implement session_kinds_filter on ClaudeHookHandler**

In `crates/rune-runtime/src/plugin_scanner.rs`, find the `ClaudeHookHandler` struct. Add a `session_kinds` field:

```rust
struct ClaudeHookHandler {
    plugin_name: String,
    hook: claude_plugin::ClaudeHook,
    source_dir: PathBuf,
    session_kinds: Option<Vec<String>>,
}
```

Implement the trait method:

```rust
    fn session_kinds_filter(&self) -> Option<&[String]> {
        self.session_kinds.as_deref()
    }
```

When creating the handler in `register_claude_plugin`, pass the session_kinds from config. This requires access to `PluginsConfig`. For now, pass `None` (all session kinds) — config-based filtering will be wired when PluginManager is built.

- [ ] **Step 4: Build**

Run: `cargo build -p rune-runtime`
Expected: success

- [ ] **Step 5: Commit**

```bash
git add crates/rune-runtime/src/executor.rs crates/rune-runtime/src/hooks.rs crates/rune-runtime/src/plugin_scanner.rs
git commit -m "feat(runtime): hook session-kind filtering"
```

---

### Task 4: Plugin Manager

**Files:**
- Create: `crates/rune-runtime/src/plugin_manager.rs`

- [ ] **Step 1: Create PluginManager**

Create `crates/rune-runtime/src/plugin_manager.rs`:

```rust
//! Plugin lifecycle coordinator.
//!
//! Owns all registries and the scanner. Provides enable/disable/reload/status.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::agent_registry::AgentRegistry;
use crate::command_registry::CommandRegistry;
use crate::hooks::HookRegistry;
use crate::plugin::PluginRegistry;
use crate::plugin_scanner::{PluginScanner, UnifiedScanSummary};
use crate::skill::SkillRegistry;

/// Status of a single loaded plugin.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginStatus {
    pub name: String,
    pub enabled: bool,
    pub source: String,
    pub skills: usize,
    pub agents: usize,
    pub hooks: usize,
    pub commands: usize,
    pub mcp_servers: usize,
}

/// Coordinates all plugin registries and scanning.
#[derive(Clone)]
pub struct PluginManager {
    scanner: Arc<PluginScanner>,
    plugin_registry: Arc<PluginRegistry>,
    skill_registry: Arc<SkillRegistry>,
    agent_registry: Arc<AgentRegistry>,
    command_registry: Arc<CommandRegistry>,
    hook_registry: Arc<HookRegistry>,
    /// Plugin metadata tracked during scan.
    plugin_meta: Arc<tokio::sync::RwLock<HashMap<String, PluginMeta>>>,
}

#[derive(Clone, Debug)]
struct PluginMeta {
    name: String,
    source_dir: String,
    enabled: bool,
    skills: usize,
    agents: usize,
    hooks: usize,
    commands: usize,
    mcp_servers: usize,
}

impl PluginManager {
    pub fn new(
        scanner: Arc<PluginScanner>,
        plugin_registry: Arc<PluginRegistry>,
        skill_registry: Arc<SkillRegistry>,
        agent_registry: Arc<AgentRegistry>,
        command_registry: Arc<CommandRegistry>,
        hook_registry: Arc<HookRegistry>,
    ) -> Self {
        Self {
            scanner,
            plugin_registry,
            skill_registry,
            agent_registry,
            command_registry,
            hook_registry,
            plugin_meta: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Run a full scan and update metadata.
    pub async fn reload(&self) -> UnifiedScanSummary {
        let summary = self.scanner.scan().await;
        // Rebuild metadata from registries
        self.rebuild_meta().await;
        summary
    }

    /// List all loaded plugins with their status.
    pub async fn status(&self) -> Vec<PluginStatus> {
        let meta = self.plugin_meta.read().await;
        meta.values()
            .map(|m| PluginStatus {
                name: m.name.clone(),
                enabled: m.enabled,
                source: m.source_dir.clone(),
                skills: m.skills,
                agents: m.agents,
                hooks: m.hooks,
                commands: m.commands,
                mcp_servers: m.mcp_servers,
            })
            .collect()
    }

    /// Get status for a specific plugin.
    pub async fn get_plugin(&self, name: &str) -> Option<PluginStatus> {
        let meta = self.plugin_meta.read().await;
        meta.get(name).map(|m| PluginStatus {
            name: m.name.clone(),
            enabled: m.enabled,
            source: m.source_dir.clone(),
            skills: m.skills,
            agents: m.agents,
            hooks: m.hooks,
            commands: m.commands,
            mcp_servers: m.mcp_servers,
        })
    }

    /// Disable a plugin — removes its skills, hooks, commands from registries.
    pub async fn disable(&self, name: &str) -> bool {
        let mut meta = self.plugin_meta.write().await;
        if let Some(m) = meta.get_mut(name) {
            m.enabled = false;
            // Remove skills with this plugin prefix
            let prefix = format!("{name}:");
            let skills = self.skill_registry.list().await;
            for skill in skills {
                if skill.name.starts_with(&prefix) {
                    self.skill_registry.remove(&skill.name).await;
                }
            }
            // Remove commands
            // CommandRegistry doesn't have remove-by-prefix yet, but we can clear and re-scan
            info!(plugin = name, "plugin disabled");
            true
        } else {
            false
        }
    }

    /// Enable a plugin — triggers a re-scan to re-register components.
    pub async fn enable(&self, name: &str) -> bool {
        let mut meta = self.plugin_meta.write().await;
        if let Some(m) = meta.get_mut(name) {
            m.enabled = true;
            drop(meta);
            // Re-scan to re-register
            self.scanner.scan().await;
            self.rebuild_meta().await;
            info!(plugin = name, "plugin enabled");
            true
        } else {
            false
        }
    }

    async fn rebuild_meta(&self) {
        let mut meta = self.plugin_meta.write().await;
        meta.clear();

        // Count skills per plugin prefix
        let skills = self.skill_registry.list().await;
        let agents = self.agent_registry.list().await;
        let commands = self.command_registry.list().await;

        let mut counts: HashMap<String, PluginMeta> = HashMap::new();

        for skill in &skills {
            let plugin_name = skill.name.split_once(':').map(|(p, _)| p).unwrap_or(&skill.name);
            let entry = counts.entry(plugin_name.to_string()).or_insert_with(|| PluginMeta {
                name: plugin_name.to_string(),
                source_dir: skill.source_dir.display().to_string(),
                enabled: true,
                skills: 0,
                agents: 0,
                hooks: 0,
                commands: 0,
                mcp_servers: 0,
            });
            entry.skills += 1;
        }

        for agent in &agents {
            let plugin_name = agent.name.split_once(':').map(|(p, _)| p).unwrap_or(&agent.name);
            let entry = counts.entry(plugin_name.to_string()).or_insert_with(|| PluginMeta {
                name: plugin_name.to_string(),
                source_dir: String::new(),
                enabled: true,
                skills: 0,
                agents: 0,
                hooks: 0,
                commands: 0,
                mcp_servers: 0,
            });
            entry.agents += 1;
        }

        for cmd in &commands {
            let entry = counts.entry(cmd.plugin_name.clone()).or_insert_with(|| PluginMeta {
                name: cmd.plugin_name.clone(),
                source_dir: String::new(),
                enabled: true,
                skills: 0,
                agents: 0,
                hooks: 0,
                commands: 0,
                mcp_servers: 0,
            });
            entry.commands += 1;
        }

        *meta = counts;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn plugin_status_empty() {
        let scanner = Arc::new(PluginScanner::new(
            vec![],
            Arc::new(PluginRegistry::new()),
            Arc::new(SkillRegistry::new()),
            Arc::new(AgentRegistry::new()),
            Arc::new(CommandRegistry::new()),
            Arc::new(HookRegistry::new()),
        ));
        let mgr = PluginManager::new(
            scanner,
            Arc::new(PluginRegistry::new()),
            Arc::new(SkillRegistry::new()),
            Arc::new(AgentRegistry::new()),
            Arc::new(CommandRegistry::new()),
            Arc::new(HookRegistry::new()),
        );
        let status = mgr.status().await;
        assert!(status.is_empty());
    }
}
```

- [ ] **Step 2: Add module and re-export**

In `crates/rune-runtime/src/lib.rs`:

```rust
pub mod plugin_manager;
pub use plugin_manager::PluginManager;
```

- [ ] **Step 3: Build and test**

Run: `cargo test -p rune-runtime plugin_manager -- --nocapture`
Expected: pass

- [ ] **Step 4: Commit**

```bash
git add crates/rune-runtime/src/plugin_manager.rs crates/rune-runtime/src/lib.rs
git commit -m "feat(runtime): add PluginManager for lifecycle coordination"
```

---

### Task 5: Plugins REST API

**Files:**
- Modify: `crates/rune-gateway/src/routes.rs`
- Modify: `crates/rune-gateway/src/server.rs`

- [ ] **Step 1: Add PluginManager to AppState**

In `crates/rune-gateway/src/state.rs`, add:

```rust
    pub plugin_manager: Option<Arc<rune_runtime::PluginManager>>,
```

- [ ] **Step 2: Add plugin route handlers**

In `crates/rune-gateway/src/routes.rs`, add:

```rust
// ── Plugins ──────────────────────────────────────────────────────────────────

pub async fn plugins_list(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, GatewayError> {
    let Some(ref mgr) = state.plugin_manager else {
        return Ok(Json(serde_json::json!({"plugins": []})));
    };
    let plugins = mgr.status().await;
    Ok(Json(serde_json::json!({"plugins": plugins})))
}

pub async fn plugins_get(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, GatewayError> {
    let Some(ref mgr) = state.plugin_manager else {
        return Err(GatewayError::not_found("plugin manager not initialized"));
    };
    match mgr.get_plugin(&name).await {
        Some(plugin) => Ok(Json(serde_json::to_value(plugin).unwrap_or_default())),
        None => Err(GatewayError::not_found("plugin")),
    }
}

pub async fn plugins_enable(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, GatewayError> {
    let Some(ref mgr) = state.plugin_manager else {
        return Err(GatewayError::not_found("plugin manager not initialized"));
    };
    let success = mgr.enable(&name).await;
    Ok(Json(serde_json::json!({"success": success})))
}

pub async fn plugins_disable(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, GatewayError> {
    let Some(ref mgr) = state.plugin_manager else {
        return Err(GatewayError::not_found("plugin manager not initialized"));
    };
    let success = mgr.disable(&name).await;
    Ok(Json(serde_json::json!({"success": success})))
}

pub async fn plugins_reload(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, GatewayError> {
    let Some(ref mgr) = state.plugin_manager else {
        return Err(GatewayError::not_found("plugin manager not initialized"));
    };
    let summary = mgr.reload().await;
    Ok(Json(serde_json::json!({
        "success": true,
        "native_plugins": summary.native_plugins,
        "claude_plugins": summary.claude_plugins,
        "skills": summary.skills_registered,
        "agents": summary.agents_registered,
        "commands": summary.commands_registered,
    })))
}
```

- [ ] **Step 3: Add routes to server.rs**

In `crates/rune-gateway/src/server.rs`, in the router builder (after the cron routes), add:

```rust
        .route("/api/plugins", get(routes::plugins_list))
        .route("/api/plugins/reload", post(routes::plugins_reload))
        .route("/api/plugins/{name}", get(routes::plugins_get))
        .route("/api/plugins/{name}/enable", post(routes::plugins_enable))
        .route("/api/plugins/{name}/disable", post(routes::plugins_disable))
```

- [ ] **Step 4: Build**

Run: `cargo build -p rune-gateway`
Expected: success

- [ ] **Step 5: Commit**

```bash
git add crates/rune-gateway/src/routes.rs crates/rune-gateway/src/server.rs crates/rune-gateway/src/state.rs
git commit -m "feat(gateway): add /api/plugins REST endpoints"
```

---

### Task 6: Wire PluginManager in Gateway Startup

**Files:**
- Modify: `apps/gateway/src/main.rs`
- Modify: `crates/rune-gateway/src/server.rs`

- [ ] **Step 1: Create PluginManager after scanner**

In `apps/gateway/src/main.rs` (or `server.rs` — wherever the scanner is initialized), after `plugin_scanner.scan().await`, create the PluginManager:

```rust
    let plugin_manager = Arc::new(rune_runtime::PluginManager::new(
        plugin_scanner.clone(),
        plugin_registry.clone(),
        skill_registry.clone(),
        agent_registry.clone(),
        command_registry.clone(),
        hook_registry.clone(),
    ));
    plugin_manager.reload().await;
```

- [ ] **Step 2: Pass to AppState**

Add `plugin_manager: Some(plugin_manager.clone())` to the `AppState` construction.

- [ ] **Step 3: Pass CommandRegistry and AgentRegistry to SessionLoop and TurnExecutor**

In the session loop construction:
```rust
    .with_command_registry(command_registry.clone())
```

In the turn executor builder chain:
```rust
    turn_executor = turn_executor.with_agent_registry(agent_registry.clone());
```

- [ ] **Step 4: Build and verify**

Run: `cargo build --release --bin rune-gateway`
Expected: success

- [ ] **Step 5: Commit**

```bash
git add apps/gateway/src/main.rs crates/rune-gateway/src/server.rs
git commit -m "feat(gateway): wire PluginManager, CommandRegistry, and AgentRegistry"
```

---

### Task 7: Build, Deploy, Verify

- [ ] **Step 1: Full build**

```bash
cargo build --release --bin rune-gateway
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p rune-runtime && cargo test -p rune-gateway
```

- [ ] **Step 3: Restart and verify**

```bash
systemctl --user restart rune-gateway && sleep 3
curl -s http://127.0.0.1:18790/api/plugins | python3 -m json.tool
```

Expected: JSON listing all loaded plugins with skill/agent/command counts.

- [ ] **Step 4: Test command routing from Telegram**

Send `/commands` to the bot on Telegram. Should list built-in commands plus plugin commands.

- [ ] **Step 5: Push**

```bash
git push origin main
```
