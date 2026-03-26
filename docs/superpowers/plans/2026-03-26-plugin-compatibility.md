# Claude Code Plugin Compatibility — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable Rune to load and execute Claude Code plugins natively — skills, agents, hooks, commands, and MCP servers — alongside Rune's existing PLUGIN.md format.

**Architecture:** An adapter layer (`claude_plugin.rs`) reads Claude Code's `plugin.json` + component directories and translates them into Rune's existing registry types (`Skill`, `HookHandler`, etc.). A unified `PluginScanner` detects format per directory and delegates to the appropriate loader. New registries (`AgentRegistry`, `CommandRegistry`) handle agent templates and slash commands. Config in `[plugins]` section controls scan dirs, overrides, and per-plugin session kind filtering.

**Tech Stack:** Rust, tokio, serde_json

---

### Task 1: Add Plugins Config Section

**Files:**
- Modify: `crates/rune-config/src/lib.rs`

- [ ] **Step 1: Add PluginsConfig and PluginOverride structs**

In `crates/rune-config/src/lib.rs`, add after the `RuntimeConfig` struct:

```rust
/// Plugin system configuration.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginsConfig {
    /// Directories to scan for plugins, in priority order.
    /// Default: ["~/.rune/plugins", "~/.claude/plugins/cache"]
    #[serde(default = "default_plugin_scan_dirs")]
    pub scan_dirs: Vec<String>,
    /// Scan interval in seconds for hot-reload. Default: 300.
    #[serde(default = "default_plugin_scan_interval")]
    pub scan_interval_secs: u64,
    /// Per-plugin overrides keyed by plugin name.
    #[serde(default)]
    pub overrides: HashMap<String, PluginOverride>,
}

/// Per-plugin configuration override.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginOverride {
    /// Whether the plugin is enabled. Default: true.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Session kinds this plugin's hooks apply to.
    /// Default: all (direct, channel, scheduled, subagent).
    #[serde(default)]
    pub session_kinds: Option<Vec<String>>,
    /// MCP server lifecycle: "shared" or "per_session". Default: "shared".
    #[serde(default)]
    pub mcp_lifecycle: Option<String>,
}

fn default_plugin_scan_dirs() -> Vec<String> {
    vec![
        "~/.rune/plugins".to_string(),
        "~/.claude/plugins/cache".to_string(),
    ]
}

fn default_plugin_scan_interval() -> u64 { 300 }
```

Then add to `AppConfig` (find the struct, add the field):

```rust
    #[serde(default)]
    pub plugins: PluginsConfig,
```

- [ ] **Step 2: Build**

Run: `cargo build -p rune-config`
Expected: success

- [ ] **Step 3: Commit**

```bash
git add crates/rune-config/src/lib.rs
git commit -m "feat(config): add [plugins] config section with scan dirs and overrides"
```

---

### Task 2: Claude Code Plugin Format Parser

**Files:**
- Create: `crates/rune-runtime/src/claude_plugin.rs`

- [ ] **Step 1: Create the Claude Code format parser**

Create `crates/rune-runtime/src/claude_plugin.rs`:

```rust
//! Parser for the Claude Code plugin format.
//!
//! Reads `.claude-plugin/plugin.json`, `skills/`, `agents/`, `hooks/hooks.json`,
//! `commands/`, and `.mcp.json` from a plugin directory and produces Rune-native
//! types ready for registration.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

// ---------------------------------------------------------------------------
// plugin.json
// ---------------------------------------------------------------------------

/// Parsed from `.claude-plugin/plugin.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClaudePluginManifest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub version: String,
}

// ---------------------------------------------------------------------------
// Skills
// ---------------------------------------------------------------------------

/// A skill parsed from a Claude Code `skills/<name>/<name>.md` file.
#[derive(Clone, Debug)]
pub struct ClaudeSkill {
    pub name: String,
    pub description: String,
    pub prompt_body: String,
    pub model: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
    pub user_invocable: bool,
    pub source_dir: PathBuf,
}

/// YAML frontmatter for a Claude Code skill file.
#[derive(Clone, Debug, Deserialize)]
struct SkillFrontmatter {
    name: Option<String>,
    description: Option<String>,
    model: Option<String>,
    #[serde(rename = "allowed-tools")]
    allowed_tools: Option<Vec<String>>,
    #[serde(rename = "user-invocable", default)]
    user_invocable: Option<bool>,
}

// ---------------------------------------------------------------------------
// Agents
// ---------------------------------------------------------------------------

/// An agent parsed from a Claude Code `agents/<name>.md` file.
#[derive(Clone, Debug)]
pub struct ClaudeAgent {
    pub name: String,
    pub description: String,
    pub when_to_use: String,
    pub system_prompt: String,
    pub model: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize)]
struct AgentFrontmatter {
    name: Option<String>,
    description: Option<String>,
    #[serde(rename = "when-to-use")]
    when_to_use: Option<String>,
    model: Option<String>,
    #[serde(rename = "allowed-tools")]
    allowed_tools: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// Hooks
// ---------------------------------------------------------------------------

/// A hook entry from `hooks/hooks.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClaudeHook {
    pub event: String,
    #[serde(default)]
    pub matcher: Option<HookMatcher>,
    pub hook: HookAction,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HookMatcher {
    pub tool_name: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HookAction {
    #[serde(rename = "type")]
    pub action_type: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// A command parsed from `commands/<name>.md`.
#[derive(Clone, Debug)]
pub struct ClaudeCommand {
    pub name: String,
    pub description: String,
    pub prompt_body: String,
}

#[derive(Clone, Debug, Deserialize)]
struct CommandFrontmatter {
    name: Option<String>,
    description: Option<String>,
}

// ---------------------------------------------------------------------------
// MCP Servers
// ---------------------------------------------------------------------------

/// MCP server declaration from `.mcp.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClaudeMcpServer {
    pub name: String,
    #[serde(rename = "type", default)]
    pub transport: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Full parsed plugin
// ---------------------------------------------------------------------------

/// Everything loaded from a Claude Code plugin directory.
#[derive(Clone, Debug, Default)]
pub struct ParsedClaudePlugin {
    pub name: String,
    pub version: String,
    pub description: String,
    pub source_dir: PathBuf,
    pub skills: Vec<ClaudeSkill>,
    pub agents: Vec<ClaudeAgent>,
    pub hooks: Vec<ClaudeHook>,
    pub commands: Vec<ClaudeCommand>,
    pub mcp_servers: Vec<ClaudeMcpServer>,
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Parse a Claude Code plugin directory into a `ParsedClaudePlugin`.
pub async fn parse_claude_plugin(dir: &Path) -> Result<ParsedClaudePlugin, String> {
    let manifest_path = dir.join(".claude-plugin/plugin.json");
    let manifest_content = tokio::fs::read_to_string(&manifest_path)
        .await
        .map_err(|e| format!("failed to read plugin.json: {e}"))?;
    let manifest: ClaudePluginManifest = serde_json::from_str(&manifest_content)
        .map_err(|e| format!("failed to parse plugin.json: {e}"))?;

    let plugin_name = manifest.name.clone();
    let mut plugin = ParsedClaudePlugin {
        name: manifest.name,
        version: manifest.version,
        description: manifest.description,
        source_dir: dir.to_path_buf(),
        ..Default::default()
    };

    // Skills
    let skills_dir = dir.join("skills");
    if skills_dir.is_dir() {
        plugin.skills = parse_skills_dir(&skills_dir, &plugin_name).await;
    }

    // Agents
    let agents_dir = dir.join("agents");
    if agents_dir.is_dir() {
        plugin.agents = parse_agents_dir(&agents_dir, &plugin_name).await;
    }

    // Hooks
    let hooks_file = dir.join("hooks/hooks.json");
    if hooks_file.is_file() {
        plugin.hooks = parse_hooks_file(&hooks_file).await;
    }

    // Commands
    let commands_dir = dir.join("commands");
    if commands_dir.is_dir() {
        plugin.commands = parse_commands_dir(&commands_dir, &plugin_name).await;
    }

    // MCP servers
    let mcp_file = dir.join(".mcp.json");
    if mcp_file.is_file() {
        plugin.mcp_servers = parse_mcp_file(&mcp_file).await;
    }

    debug!(
        plugin = %plugin.name,
        skills = plugin.skills.len(),
        agents = plugin.agents.len(),
        hooks = plugin.hooks.len(),
        commands = plugin.commands.len(),
        mcp_servers = plugin.mcp_servers.len(),
        "parsed claude code plugin"
    );

    Ok(plugin)
}

/// Detect whether a directory contains a Claude Code plugin.
pub fn is_claude_plugin_dir(dir: &Path) -> bool {
    dir.join(".claude-plugin/plugin.json").is_file()
}

// ---------------------------------------------------------------------------
// Component parsers
// ---------------------------------------------------------------------------

async fn parse_skills_dir(dir: &Path, plugin_name: &str) -> Vec<ClaudeSkill> {
    let mut skills = Vec::new();
    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(e) => e,
        Err(_) => return skills,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let skill_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown");
        // Look for <name>.md or any .md file in the skill directory
        let md_path = path.join(format!("{skill_name}.md"));
        let md_path = if md_path.is_file() {
            md_path
        } else {
            // Fall back to first .md file
            match find_first_md(&path).await {
                Some(p) => p,
                None => continue,
            }
        };

        match parse_skill_file(&md_path, plugin_name, skill_name).await {
            Ok(skill) => skills.push(skill),
            Err(e) => warn!(skill = skill_name, error = %e, "failed to parse skill"),
        }
    }
    skills
}

async fn parse_skill_file(path: &Path, plugin_name: &str, dir_name: &str) -> Result<ClaudeSkill, String> {
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| format!("read error: {e}"))?;

    let (frontmatter, body) = split_frontmatter(&content);
    let fm: SkillFrontmatter = frontmatter
        .and_then(|yaml| serde_json::from_value(parse_yaml_value(yaml)).ok())
        .unwrap_or(SkillFrontmatter {
            name: None, description: None, model: None,
            allowed_tools: None, user_invocable: None,
        });

    let name = format!("{}:{}", plugin_name, fm.name.as_deref().unwrap_or(dir_name));

    Ok(ClaudeSkill {
        name,
        description: fm.description.unwrap_or_else(|| format!("Skill from {plugin_name}")),
        prompt_body: body.to_string(),
        model: fm.model,
        allowed_tools: fm.allowed_tools,
        user_invocable: fm.user_invocable.unwrap_or(false),
        source_dir: path.parent().unwrap_or(Path::new(".")).to_path_buf(),
    })
}

async fn parse_agents_dir(dir: &Path, plugin_name: &str) -> Vec<ClaudeAgent> {
    let mut agents = Vec::new();
    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(e) => e,
        Err(_) => return agents,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let file_stem = path.file_stem().and_then(|n| n.to_str()).unwrap_or("unknown");
        match parse_agent_file(&path, plugin_name, file_stem).await {
            Ok(agent) => agents.push(agent),
            Err(e) => warn!(agent = file_stem, error = %e, "failed to parse agent"),
        }
    }
    agents
}

async fn parse_agent_file(path: &Path, plugin_name: &str, file_stem: &str) -> Result<ClaudeAgent, String> {
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| format!("read error: {e}"))?;

    let (frontmatter, body) = split_frontmatter(&content);
    let fm: AgentFrontmatter = frontmatter
        .and_then(|yaml| serde_json::from_value(parse_yaml_value(yaml)).ok())
        .unwrap_or(AgentFrontmatter {
            name: None, description: None, when_to_use: None,
            model: None, allowed_tools: None,
        });

    let name = format!("{}:{}", plugin_name, fm.name.as_deref().unwrap_or(file_stem));

    Ok(ClaudeAgent {
        name,
        description: fm.description.unwrap_or_else(|| format!("Agent from {plugin_name}")),
        when_to_use: fm.when_to_use.unwrap_or_default(),
        system_prompt: body.to_string(),
        model: fm.model,
        allowed_tools: fm.allowed_tools,
    })
}

async fn parse_hooks_file(path: &Path) -> Vec<ClaudeHook> {
    let content = match tokio::fs::read_to_string(path).await {
        Ok(c) => c,
        Err(e) => {
            warn!(path = %path.display(), error = %e, "failed to read hooks.json");
            return Vec::new();
        }
    };
    serde_json::from_str(&content).unwrap_or_else(|e| {
        warn!(error = %e, "failed to parse hooks.json");
        Vec::new()
    })
}

async fn parse_commands_dir(dir: &Path, plugin_name: &str) -> Vec<ClaudeCommand> {
    let mut commands = Vec::new();
    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(e) => e,
        Err(_) => return commands,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let file_stem = path.file_stem().and_then(|n| n.to_str()).unwrap_or("unknown");
        match parse_command_file(&path, plugin_name, file_stem).await {
            Ok(cmd) => commands.push(cmd),
            Err(e) => warn!(command = file_stem, error = %e, "failed to parse command"),
        }
    }
    commands
}

async fn parse_command_file(path: &Path, plugin_name: &str, file_stem: &str) -> Result<ClaudeCommand, String> {
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| format!("read error: {e}"))?;

    let (frontmatter, body) = split_frontmatter(&content);
    let fm: CommandFrontmatter = frontmatter
        .and_then(|yaml| serde_json::from_value(parse_yaml_value(yaml)).ok())
        .unwrap_or(CommandFrontmatter { name: None, description: None });

    let name = format!("{}:{}", plugin_name, fm.name.as_deref().unwrap_or(file_stem));

    Ok(ClaudeCommand {
        name,
        description: fm.description.unwrap_or_else(|| format!("Command from {plugin_name}")),
        prompt_body: body.to_string(),
    })
}

async fn parse_mcp_file(path: &Path) -> Vec<ClaudeMcpServer> {
    let content = match tokio::fs::read_to_string(path).await {
        Ok(c) => c,
        Err(e) => {
            warn!(path = %path.display(), error = %e, "failed to read .mcp.json");
            return Vec::new();
        }
    };

    #[derive(Deserialize)]
    struct McpFile {
        #[serde(rename = "mcpServers", default)]
        mcp_servers: std::collections::HashMap<String, McpServerEntry>,
    }

    #[derive(Deserialize)]
    struct McpServerEntry {
        #[serde(rename = "type", default)]
        transport: String,
        #[serde(default)]
        command: Option<String>,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: std::collections::HashMap<String, String>,
    }

    let mcp: McpFile = match serde_json::from_str(&content) {
        Ok(m) => m,
        Err(e) => {
            warn!(error = %e, "failed to parse .mcp.json");
            return Vec::new();
        }
    };

    mcp.mcp_servers
        .into_iter()
        .map(|(name, entry)| ClaudeMcpServer {
            name,
            transport: entry.transport,
            command: entry.command,
            args: entry.args,
            env: entry.env,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

fn split_frontmatter(content: &str) -> (Option<&str>, &str) {
    let trimmed = content.trim();
    if !trimmed.starts_with("---") {
        return (None, content);
    }
    let after_first = &trimmed[3..];
    match after_first.find("\n---") {
        Some(end) => {
            let yaml = after_first[..end].trim();
            let body = after_first[end + 4..].trim();
            (Some(yaml), body)
        }
        None => (None, content),
    }
}

fn parse_yaml_value(yaml: &str) -> serde_json::Value {
    // Reuse the minimal YAML parser pattern from skill.rs and plugin.rs
    let mut map = serde_json::Map::new();
    for line in yaml.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else { continue };
        let key = key.trim().to_string();
        let value = value.trim();
        if value.is_empty() {
            map.insert(key, serde_json::Value::Null);
        } else if value == "true" || value == "false" {
            map.insert(key, serde_json::Value::Bool(value == "true"));
        } else if value.starts_with('{') || value.starts_with('[') {
            if let Ok(parsed) = serde_json::from_str(value) {
                map.insert(key, parsed);
            } else {
                map.insert(key, serde_json::Value::String(value.to_string()));
            }
        } else {
            let value = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')).unwrap_or(value);
            map.insert(key, serde_json::Value::String(value.to_string()));
        }
    }
    serde_json::Value::Object(map)
}

async fn find_first_md(dir: &Path) -> Option<PathBuf> {
    let mut entries = tokio::fs::read_dir(dir).await.ok()?;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("md") && path.is_file() {
            return Some(path);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn split_frontmatter_works() {
        let content = "---\nname: test\n---\n\n# Body";
        let (fm, body) = split_frontmatter(content);
        assert_eq!(fm, Some("name: test"));
        assert_eq!(body, "# Body");
    }

    #[test]
    fn split_frontmatter_no_frontmatter() {
        let content = "# Just body";
        let (fm, body) = split_frontmatter(content);
        assert!(fm.is_none());
        assert_eq!(body, "# Just body");
    }

    #[tokio::test]
    async fn parse_claude_plugin_directory() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();

        // Create plugin.json
        tokio::fs::create_dir_all(dir.join(".claude-plugin")).await.unwrap();
        tokio::fs::write(
            dir.join(".claude-plugin/plugin.json"),
            r#"{"name":"test-plugin","description":"A test","version":"1.0.0"}"#,
        ).await.unwrap();

        // Create a skill
        tokio::fs::create_dir_all(dir.join("skills/greet")).await.unwrap();
        tokio::fs::write(
            dir.join("skills/greet/greet.md"),
            "---\nname: greet\ndescription: Greet the user\nuser-invocable: true\n---\n\nSay hello warmly.",
        ).await.unwrap();

        // Create an agent
        tokio::fs::create_dir_all(dir.join("agents")).await.unwrap();
        tokio::fs::write(
            dir.join("agents/reviewer.md"),
            "---\nname: reviewer\ndescription: Code reviewer\nwhen-to-use: When reviewing code\n---\n\nReview code carefully.",
        ).await.unwrap();

        // Create hooks
        tokio::fs::create_dir_all(dir.join("hooks")).await.unwrap();
        tokio::fs::write(
            dir.join("hooks/hooks.json"),
            r#"[{"event":"PreToolUse","hook":{"type":"command","command":"echo ok"}}]"#,
        ).await.unwrap();

        // Create a command
        tokio::fs::create_dir_all(dir.join("commands")).await.unwrap();
        tokio::fs::write(
            dir.join("commands/deploy.md"),
            "---\nname: deploy\ndescription: Deploy to prod\n---\n\nDeploy the application.",
        ).await.unwrap();

        let plugin = parse_claude_plugin(dir).await.unwrap();
        assert_eq!(plugin.name, "test-plugin");
        assert_eq!(plugin.skills.len(), 1);
        assert_eq!(plugin.skills[0].name, "test-plugin:greet");
        assert!(plugin.skills[0].user_invocable);
        assert_eq!(plugin.agents.len(), 1);
        assert_eq!(plugin.agents[0].name, "test-plugin:reviewer");
        assert_eq!(plugin.hooks.len(), 1);
        assert_eq!(plugin.commands.len(), 1);
        assert_eq!(plugin.commands[0].name, "test-plugin:deploy");
    }

    #[test]
    fn is_claude_plugin_dir_detection() {
        let tmp = TempDir::new().unwrap();
        assert!(!is_claude_plugin_dir(tmp.path()));

        std::fs::create_dir_all(tmp.path().join(".claude-plugin")).unwrap();
        std::fs::write(
            tmp.path().join(".claude-plugin/plugin.json"),
            r#"{"name":"x"}"#,
        ).unwrap();
        assert!(is_claude_plugin_dir(tmp.path()));
    }
}
```

- [ ] **Step 2: Add module to lib.rs**

In `crates/rune-runtime/src/lib.rs`, add:

```rust
pub mod claude_plugin;
```

- [ ] **Step 3: Build and test**

Run: `cargo test -p rune-runtime claude_plugin -- --nocapture`
Expected: all tests pass

- [ ] **Step 4: Commit**

```bash
git add crates/rune-runtime/src/claude_plugin.rs crates/rune-runtime/src/lib.rs
git commit -m "feat(runtime): add Claude Code plugin format parser"
```

---

### Task 3: Extend Skill Type for Claude Code Fields

**Files:**
- Modify: `crates/rune-runtime/src/skill.rs`

- [ ] **Step 1: Add Claude Code fields to Skill struct**

In `crates/rune-runtime/src/skill.rs`, add fields to the `Skill` struct:

```rust
pub struct Skill {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    pub binary_path: Option<PathBuf>,
    pub source_dir: PathBuf,
    pub enabled: bool,
    // Claude Code compatibility fields
    /// Full prompt body (markdown content below frontmatter).
    #[serde(default)]
    pub prompt_body: Option<String>,
    /// Model override for this skill.
    #[serde(default)]
    pub model: Option<String>,
    /// Restrict tool access when this skill is active.
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
    /// Whether this skill can be invoked as a slash command.
    #[serde(default)]
    pub user_invocable: bool,
}
```

- [ ] **Step 2: Fix all Skill construction sites**

Search for `Skill {` and add the new fields with defaults: `prompt_body: None, model: None, allowed_tools: None, user_invocable: false`.

Files to update: `crates/rune-runtime/src/skill_loader.rs` (load_skill_from_path), `crates/rune-runtime/src/skill.rs` (tests).

- [ ] **Step 3: Build and test**

Run: `cargo test -p rune-runtime skill -- --nocapture`
Expected: all pass

- [ ] **Step 4: Commit**

```bash
git add crates/rune-runtime/src/skill.rs crates/rune-runtime/src/skill_loader.rs
git commit -m "feat(runtime): extend Skill type with Claude Code fields"
```

---

### Task 4: Extend HookEvent Enum

**Files:**
- Modify: `crates/rune-runtime/src/hooks.rs`

- [ ] **Step 1: Add new HookEvent variants**

In `crates/rune-runtime/src/hooks.rs`, add to the `HookEvent` enum:

```rust
pub enum HookEvent {
    PreToolCall,
    PostToolCall,
    PreTurn,
    PostTurn,
    SessionCreated,
    SessionCompleted,
    // Claude Code compatibility events
    Stop,
    SubagentStop,
    UserPromptSubmit,
    PreCompact,
    Notification,
}
```

Update `as_str()`:

```rust
    HookEvent::Stop => "stop",
    HookEvent::SubagentStop => "subagent_stop",
    HookEvent::UserPromptSubmit => "user_prompt_submit",
    HookEvent::PreCompact => "pre_compact",
    HookEvent::Notification => "notification",
```

Update `from_str()`:

```rust
    "stop" | "Stop" => Some(HookEvent::Stop),
    "subagent_stop" | "SubagentStop" => Some(HookEvent::SubagentStop),
    "user_prompt_submit" | "UserPromptSubmit" => Some(HookEvent::UserPromptSubmit),
    "pre_compact" | "PreCompact" => Some(HookEvent::PreCompact),
    "notification" | "Notification" => Some(HookEvent::Notification),
```

Update `all()` to include the new variants.

- [ ] **Step 2: Build and test**

Run: `cargo test -p rune-runtime hooks -- --nocapture`
Expected: all pass (the roundtrip test will cover new variants since `all()` includes them)

- [ ] **Step 3: Commit**

```bash
git add crates/rune-runtime/src/hooks.rs
git commit -m "feat(runtime): add Claude Code hook events (Stop, PreCompact, etc.)"
```

---

### Task 5: Agent Registry

**Files:**
- Create: `crates/rune-runtime/src/agent_registry.rs`

- [ ] **Step 1: Create AgentRegistry**

Create `crates/rune-runtime/src/agent_registry.rs`:

```rust
//! Registry for subagent templates loaded from plugins.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::debug;

/// A subagent template that can be instantiated for a session.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentTemplate {
    /// Namespaced name, e.g. "superpowers:code-reviewer".
    pub name: String,
    /// Short description for listing.
    pub description: String,
    /// When the model should use this agent.
    pub when_to_use: String,
    /// System prompt injected into the subagent session.
    pub system_prompt: String,
    /// Optional model override.
    pub model: Option<String>,
    /// Restrict tool access for this agent.
    pub allowed_tools: Option<Vec<String>>,
}

/// Thread-safe registry for agent templates.
#[derive(Clone)]
pub struct AgentRegistry {
    inner: Arc<RwLock<HashMap<String, AgentTemplate>>>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register(&self, template: AgentTemplate) {
        let name = template.name.clone();
        self.inner.write().await.insert(name.clone(), template);
        debug!(agent = %name, "agent template registered");
    }

    pub async fn remove(&self, name: &str) -> Option<AgentTemplate> {
        self.inner.write().await.remove(name)
    }

    pub async fn get(&self, name: &str) -> Option<AgentTemplate> {
        self.inner.read().await.get(name).cloned()
    }

    pub async fn list(&self) -> Vec<AgentTemplate> {
        self.inner.read().await.values().cloned().collect()
    }

    pub async fn clear(&self) {
        self.inner.write().await.clear();
    }

    pub async fn len(&self) -> usize {
        self.inner.read().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.inner.read().await.is_empty()
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn agent_registry_crud() {
        let reg = AgentRegistry::new();

        reg.register(AgentTemplate {
            name: "test:reviewer".into(),
            description: "Reviews code".into(),
            when_to_use: "When reviewing code".into(),
            system_prompt: "Review carefully".into(),
            model: None,
            allowed_tools: None,
        }).await;

        assert_eq!(reg.len().await, 1);
        let t = reg.get("test:reviewer").await.unwrap();
        assert_eq!(t.description, "Reviews code");

        reg.remove("test:reviewer").await;
        assert!(reg.is_empty().await);
    }
}
```

- [ ] **Step 2: Add module and re-export**

In `crates/rune-runtime/src/lib.rs`, add:

```rust
pub mod agent_registry;
pub use agent_registry::AgentRegistry;
```

- [ ] **Step 3: Build and test**

Run: `cargo test -p rune-runtime agent_registry -- --nocapture`
Expected: pass

- [ ] **Step 4: Commit**

```bash
git add crates/rune-runtime/src/agent_registry.rs crates/rune-runtime/src/lib.rs
git commit -m "feat(runtime): add AgentRegistry for subagent templates"
```

---

### Task 6: Command Registry

**Files:**
- Create: `crates/rune-runtime/src/command_registry.rs`

- [ ] **Step 1: Create CommandRegistry**

Create `crates/rune-runtime/src/command_registry.rs`:

```rust
//! Registry for slash commands loaded from plugins.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::debug;

/// A slash command from a plugin.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Command {
    /// Namespaced name, e.g. "superpowers:commit".
    pub name: String,
    /// Short description.
    pub description: String,
    /// The prompt template (markdown body).
    pub prompt_body: String,
    /// Plugin this command came from.
    pub plugin_name: String,
}

impl Command {
    /// The short name (without plugin prefix) for aliasing.
    pub fn short_name(&self) -> &str {
        self.name.split_once(':').map(|(_, s)| s).unwrap_or(&self.name)
    }

    /// Expand the prompt template with arguments.
    pub fn expand(&self, args: &str) -> String {
        // Replace $ARGUMENTS placeholder with the provided args
        if self.prompt_body.contains("$ARGUMENTS") {
            self.prompt_body.replace("$ARGUMENTS", args)
        } else if args.is_empty() {
            self.prompt_body.clone()
        } else {
            format!("{}\n\nARGUMENTS: {}", self.prompt_body, args)
        }
    }
}

/// Thread-safe registry for commands.
#[derive(Clone)]
pub struct CommandRegistry {
    inner: Arc<RwLock<HashMap<String, Command>>>,
    /// Short name → full name aliases for conflict-free commands.
    aliases: Arc<RwLock<HashMap<String, String>>>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            aliases: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register(&self, cmd: Command) {
        let name = cmd.name.clone();
        let short = cmd.short_name().to_string();

        // Register alias if no conflict
        let mut aliases = self.aliases.write().await;
        if !aliases.contains_key(&short) {
            aliases.insert(short.clone(), name.clone());
        }
        drop(aliases);

        self.inner.write().await.insert(name.clone(), cmd);
        debug!(command = %name, "command registered");
    }

    /// Look up by full name or short alias.
    pub async fn get(&self, name: &str) -> Option<Command> {
        let inner = self.inner.read().await;
        if let Some(cmd) = inner.get(name) {
            return Some(cmd.clone());
        }
        // Try alias
        let aliases = self.aliases.read().await;
        if let Some(full) = aliases.get(name) {
            return inner.get(full).cloned();
        }
        None
    }

    pub async fn list(&self) -> Vec<Command> {
        self.inner.read().await.values().cloned().collect()
    }

    pub async fn clear(&self) {
        self.inner.write().await.clear();
        self.aliases.write().await.clear();
    }

    pub async fn len(&self) -> usize {
        self.inner.read().await.len()
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn command_registry_crud_and_alias() {
        let reg = CommandRegistry::new();

        reg.register(Command {
            name: "superpowers:commit".into(),
            description: "Create a commit".into(),
            prompt_body: "Commit changes. $ARGUMENTS".into(),
            plugin_name: "superpowers".into(),
        }).await;

        // Full name lookup
        assert!(reg.get("superpowers:commit").await.is_some());
        // Short alias lookup
        assert!(reg.get("commit").await.is_some());

        assert_eq!(reg.len().await, 1);
    }

    #[test]
    fn command_expand_with_arguments() {
        let cmd = Command {
            name: "test:deploy".into(),
            description: "Deploy".into(),
            prompt_body: "Deploy to $ARGUMENTS environment.".into(),
            plugin_name: "test".into(),
        };
        assert_eq!(cmd.expand("production"), "Deploy to production environment.");
    }

    #[test]
    fn command_expand_without_placeholder() {
        let cmd = Command {
            name: "test:status".into(),
            description: "Status".into(),
            prompt_body: "Show system status.".into(),
            plugin_name: "test".into(),
        };
        assert_eq!(cmd.expand(""), "Show system status.");
        assert_eq!(cmd.expand("verbose"), "Show system status.\n\nARGUMENTS: verbose");
    }
}
```

- [ ] **Step 2: Add module and re-export**

In `crates/rune-runtime/src/lib.rs`, add:

```rust
pub mod command_registry;
pub use command_registry::CommandRegistry;
```

- [ ] **Step 3: Build and test**

Run: `cargo test -p rune-runtime command_registry -- --nocapture`
Expected: pass

- [ ] **Step 4: Commit**

```bash
git add crates/rune-runtime/src/command_registry.rs crates/rune-runtime/src/lib.rs
git commit -m "feat(runtime): add CommandRegistry for slash commands"
```

---

### Task 7: Unified Plugin Scanner

**Files:**
- Create: `crates/rune-runtime/src/plugin_scanner.rs`

- [ ] **Step 1: Create the unified scanner**

Create `crates/rune-runtime/src/plugin_scanner.rs`:

```rust
//! Unified plugin scanner that discovers both Rune-native (PLUGIN.md) and
//! Claude Code (.claude-plugin/plugin.json) plugins from configured directories.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tracing::{debug, info, warn};

use crate::agent_registry::{AgentRegistry, AgentTemplate};
use crate::claude_plugin::{self, ParsedClaudePlugin};
use crate::command_registry::{Command, CommandRegistry};
use crate::hooks::{HookEvent, HookRegistry};
use crate::plugin::{PluginLoader, PluginRegistry};
use crate::skill::{Skill, SkillRegistry};

/// Result of a unified scan pass.
#[derive(Debug, Clone)]
pub struct UnifiedScanSummary {
    pub native_plugins: usize,
    pub claude_plugins: usize,
    pub skills_registered: usize,
    pub agents_registered: usize,
    pub hooks_registered: usize,
    pub commands_registered: usize,
    pub mcp_servers_found: usize,
}

/// Scans multiple directories for both Rune-native and Claude Code plugins.
pub struct PluginScanner {
    scan_dirs: Vec<PathBuf>,
    plugin_registry: Arc<PluginRegistry>,
    skill_registry: Arc<SkillRegistry>,
    agent_registry: Arc<AgentRegistry>,
    command_registry: Arc<CommandRegistry>,
    hook_registry: Arc<HookRegistry>,
}

impl PluginScanner {
    pub fn new(
        scan_dirs: Vec<PathBuf>,
        plugin_registry: Arc<PluginRegistry>,
        skill_registry: Arc<SkillRegistry>,
        agent_registry: Arc<AgentRegistry>,
        command_registry: Arc<CommandRegistry>,
        hook_registry: Arc<HookRegistry>,
    ) -> Self {
        Self {
            scan_dirs,
            plugin_registry,
            skill_registry,
            agent_registry,
            command_registry,
            hook_registry,
        }
    }

    /// Perform a full scan of all configured directories.
    pub async fn scan(&self) -> UnifiedScanSummary {
        let mut summary = UnifiedScanSummary {
            native_plugins: 0,
            claude_plugins: 0,
            skills_registered: 0,
            agents_registered: 0,
            hooks_registered: 0,
            commands_registered: 0,
            mcp_servers_found: 0,
        };

        let mut seen_plugin_names: HashSet<String> = HashSet::new();

        for scan_dir in &self.scan_dirs {
            let expanded = expand_tilde(scan_dir);
            if !expanded.is_dir() {
                debug!(dir = %expanded.display(), "scan directory does not exist, skipping");
                continue;
            }

            let mut entries = match tokio::fs::read_dir(&expanded).await {
                Ok(e) => e,
                Err(e) => {
                    warn!(dir = %expanded.display(), error = %e, "failed to read scan directory");
                    continue;
                }
            };

            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }

                // Determine format
                if claude_plugin::is_claude_plugin_dir(&path) {
                    match claude_plugin::parse_claude_plugin(&path).await {
                        Ok(plugin) => {
                            if seen_plugin_names.contains(&plugin.name) {
                                debug!(plugin = %plugin.name, "skipping duplicate plugin (override exists)");
                                continue;
                            }
                            seen_plugin_names.insert(plugin.name.clone());
                            summary.claude_plugins += 1;
                            self.register_claude_plugin(&plugin, &mut summary).await;
                        }
                        Err(e) => {
                            warn!(dir = %path.display(), error = %e, "failed to parse claude plugin");
                        }
                    }
                } else if path.join("PLUGIN.md").is_file() {
                    // Rune-native — delegate to existing PluginLoader for this dir
                    let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown");
                    if seen_plugin_names.contains(dir_name) {
                        continue;
                    }
                    seen_plugin_names.insert(dir_name.to_string());
                    summary.native_plugins += 1;
                    // The existing PluginLoader handles PLUGIN.md scanning
                    // We just count it here; actual loading is done by PluginLoader
                }
            }
        }

        info!(
            native = summary.native_plugins,
            claude = summary.claude_plugins,
            skills = summary.skills_registered,
            agents = summary.agents_registered,
            hooks = summary.hooks_registered,
            commands = summary.commands_registered,
            mcp = summary.mcp_servers_found,
            "unified plugin scan complete"
        );

        summary
    }

    async fn register_claude_plugin(&self, plugin: &ParsedClaudePlugin, summary: &mut UnifiedScanSummary) {
        // Register skills
        for cs in &plugin.skills {
            self.skill_registry.register(Skill {
                name: cs.name.clone(),
                description: cs.description.clone(),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
                binary_path: None,
                source_dir: cs.source_dir.clone(),
                enabled: true,
                prompt_body: Some(cs.prompt_body.clone()),
                model: cs.model.clone(),
                allowed_tools: cs.allowed_tools.clone(),
                user_invocable: cs.user_invocable,
            }).await;
            summary.skills_registered += 1;
        }

        // Register agents
        for ca in &plugin.agents {
            self.agent_registry.register(AgentTemplate {
                name: ca.name.clone(),
                description: ca.description.clone(),
                when_to_use: ca.when_to_use.clone(),
                system_prompt: ca.system_prompt.clone(),
                model: ca.model.clone(),
                allowed_tools: ca.allowed_tools.clone(),
            }).await;
            summary.agents_registered += 1;
        }

        // Register hooks
        for ch in &plugin.hooks {
            if let Some(event) = HookEvent::from_str(&ch.event) {
                let handler = ClaudeHookHandler {
                    plugin_name: plugin.name.clone(),
                    hook: ch.clone(),
                    source_dir: plugin.source_dir.clone(),
                };
                self.hook_registry.register(event, Box::new(handler)).await;
                summary.hooks_registered += 1;
            } else {
                warn!(event = %ch.event, plugin = %plugin.name, "unknown hook event");
            }
        }

        // Register commands
        for cc in &plugin.commands {
            self.command_registry.register(Command {
                name: cc.name.clone(),
                description: cc.description.clone(),
                prompt_body: cc.prompt_body.clone(),
                plugin_name: plugin.name.clone(),
            }).await;
            summary.commands_registered += 1;
        }

        // MCP servers (just count for now; actual MCP wiring is a follow-up)
        summary.mcp_servers_found += plugin.mcp_servers.len();
    }
}

/// Hook handler that executes Claude Code hook actions (shell commands).
struct ClaudeHookHandler {
    plugin_name: String,
    hook: claude_plugin::ClaudeHook,
    source_dir: PathBuf,
}

#[async_trait::async_trait]
impl crate::hooks::HookHandler for ClaudeHookHandler {
    async fn handle(
        &self,
        _event: &HookEvent,
        context: &mut serde_json::Value,
    ) -> Result<(), String> {
        // Check matcher
        if let Some(ref matcher) = self.hook.matcher {
            if let Some(ref tool_name_pattern) = matcher.tool_name {
                let ctx_tool = context.get("tool_name").and_then(|v| v.as_str()).unwrap_or("");
                if !tool_name_matches(ctx_tool, tool_name_pattern) {
                    return Ok(()); // matcher doesn't match, skip
                }
            }
        }

        match self.hook.hook.action_type.as_str() {
            "command" => {
                if let Some(ref cmd) = self.hook.hook.command {
                    let context_json = serde_json::to_string(context).unwrap_or_default();
                    let output = tokio::process::Command::new("sh")
                        .arg("-c")
                        .arg(cmd)
                        .current_dir(&self.source_dir)
                        .env("HOOK_CONTEXT", &context_json)
                        .output()
                        .await
                        .map_err(|e| format!("hook command failed: {e}"))?;

                    if !output.status.success() {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        return Err(format!("hook command exited {}: {stderr}", output.status));
                    }
                }
                Ok(())
            }
            _ => Ok(()), // Prompt-based hooks are a future enhancement
        }
    }

    fn plugin_name(&self) -> &str {
        &self.plugin_name
    }
}

fn tool_name_matches(actual: &str, pattern: &str) -> bool {
    if pattern.contains('*') {
        // Simple glob: "Bash*" matches "Bash", "BashTool", etc.
        let prefix = pattern.trim_end_matches('*');
        actual.starts_with(prefix)
    } else {
        actual == pattern
    }
}

fn expand_tilde(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if s.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(&s[2..]);
        }
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_name_glob_matching() {
        assert!(tool_name_matches("Bash", "Bash"));
        assert!(tool_name_matches("Bash", "Bash*"));
        assert!(tool_name_matches("BashTool", "Bash*"));
        assert!(!tool_name_matches("Read", "Bash*"));
    }

    #[test]
    fn tilde_expansion() {
        let p = expand_tilde(Path::new("~/.rune/plugins"));
        assert!(!p.to_string_lossy().starts_with("~/"));
    }
}
```

- [ ] **Step 2: Add `dirs` dependency if not present**

Check `crates/rune-runtime/Cargo.toml` for `dirs` crate. If missing:

```bash
cd crates/rune-runtime && cargo add dirs
```

- [ ] **Step 3: Add module and re-export**

In `crates/rune-runtime/src/lib.rs`, add:

```rust
pub mod plugin_scanner;
pub use plugin_scanner::PluginScanner;
```

- [ ] **Step 4: Build and test**

Run: `cargo test -p rune-runtime plugin_scanner -- --nocapture`
Expected: pass

- [ ] **Step 5: Commit**

```bash
git add crates/rune-runtime/src/plugin_scanner.rs crates/rune-runtime/src/lib.rs crates/rune-runtime/Cargo.toml
git commit -m "feat(runtime): add unified PluginScanner for Rune + Claude Code plugins"
```

---

### Task 8: Wire Scanner into Gateway Startup

**Files:**
- Modify: `apps/gateway/src/main.rs`

- [ ] **Step 1: Initialize registries and run initial scan at startup**

In `apps/gateway/src/main.rs`, after the existing `SkillLoader` and `PluginLoader` scan calls, add:

```rust
    // Unified plugin scan (Claude Code + Rune native)
    let agent_registry = Arc::new(rune_runtime::AgentRegistry::new());
    let command_registry = Arc::new(rune_runtime::CommandRegistry::new());

    let scan_dirs: Vec<std::path::PathBuf> = config.plugins.scan_dirs
        .iter()
        .map(|s| std::path::PathBuf::from(s))
        .collect();

    let plugin_scanner = Arc::new(rune_runtime::PluginScanner::new(
        scan_dirs,
        plugin_registry.clone(),
        skill_registry.clone(),
        agent_registry.clone(),
        command_registry.clone(),
        hook_registry.clone(),
    ));

    let scan_summary = plugin_scanner.scan().await;
    info!(
        native = scan_summary.native_plugins,
        claude = scan_summary.claude_plugins,
        skills = scan_summary.skills_registered,
        agents = scan_summary.agents_registered,
        commands = scan_summary.commands_registered,
        "unified plugin scan complete"
    );
```

- [ ] **Step 2: Build**

Run: `cargo build --release --bin rune-gateway`
Expected: success

- [ ] **Step 3: Commit**

```bash
git add apps/gateway/src/main.rs
git commit -m "feat(gateway): wire PluginScanner into startup"
```

---

### Task 9: Periodic Plugin Re-scan in Supervisor

**Files:**
- Modify: `crates/rune-gateway/src/supervisor.rs`

- [ ] **Step 1: Add PluginScanner to SupervisorDeps and periodic re-scan**

Add to `SupervisorDeps`:

```rust
    /// Optional unified plugin scanner for periodic re-scan.
    pub plugin_scanner: Option<Arc<rune_runtime::PluginScanner>>,
    /// Plugin scan interval in ticks (each tick = 10s).
    pub plugin_scan_interval_ticks: u64,
```

In `supervisor_loop`, add after the stale session cleanup block:

```rust
        // --- Plugin re-scan (configurable interval) ---
        if let Some(ref scanner) = deps.plugin_scanner {
            if deps.plugin_scan_interval_ticks > 0 && tick_count % deps.plugin_scan_interval_ticks == 0 {
                let summary = scanner.scan().await;
                if summary.claude_plugins > 0 || summary.native_plugins > 0 {
                    debug!(
                        native = summary.native_plugins,
                        claude = summary.claude_plugins,
                        "plugin re-scan complete"
                    );
                }
            }
        }
```

- [ ] **Step 2: Wire in main.rs**

When constructing `SupervisorDeps` in `apps/gateway/src/main.rs`, add:

```rust
        plugin_scanner: Some(plugin_scanner.clone()),
        plugin_scan_interval_ticks: config.plugins.scan_interval_secs / 10,
```

- [ ] **Step 3: Build**

Run: `cargo build --release --bin rune-gateway`
Expected: success

- [ ] **Step 4: Commit**

```bash
git add crates/rune-gateway/src/supervisor.rs apps/gateway/src/main.rs
git commit -m "feat(supervisor): periodic plugin re-scan for hot-reload"
```

---

### Task 10: Build, Test, Deploy

- [ ] **Step 1: Full build**

```bash
cargo build --release --bin rune-gateway
```

- [ ] **Step 2: Run all tests**

```bash
cargo test -p rune-runtime && cargo test -p rune-config && cargo test -p rune-gateway
```

- [ ] **Step 3: Restart and verify plugin discovery**

```bash
systemctl --user restart rune-gateway && sleep 3
journalctl --user -u rune-gateway --since "30s ago" --no-pager -o cat | grep -i plugin
```

Expected: log lines showing "unified plugin scan complete" with counts of discovered Claude Code plugins from `~/.claude/plugins/cache/`.

- [ ] **Step 4: Push**

```bash
git push origin main
```
