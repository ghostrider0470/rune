//! Unified plugin scanner for Rune-native and Claude Code plugins.
//!
//! Iterates a list of scan directories in order; for each subdirectory:
//! - If `.claude-plugin/plugin.json` exists → parse as a Claude Code plugin and
//!   register its skills, agents, hooks, and commands.
//! - If `PLUGIN.md` exists → count as a native plugin (existing [`PluginLoader`]
//!   handles those; we just tally them here for the summary).
//!
//! First-directory-wins semantics: once a plugin name is seen it is skipped in
//! subsequent directories (override behavior).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tracing::{debug, info, warn};

use crate::agent_registry::{AgentRegistry, AgentTemplate};
use crate::claude_plugin;
use crate::command_registry::{Command, CommandRegistry};
use crate::hooks::{HookEvent, HookHandler, HookRegistry};
use crate::plugin::PluginRegistry;
use crate::skill::{Skill, SkillRegistry};
use rune_config::{PluginOverride, PluginsConfig};

// ---------------------------------------------------------------------------
// Summary
// ---------------------------------------------------------------------------

/// Summary produced by a [`PluginScanner::scan`] call.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnifiedScanSummary {
    /// Number of native (`PLUGIN.md`) plugin directories encountered.
    pub native_plugins: usize,
    /// Number of Claude Code (`.claude-plugin/plugin.json`) plugin directories loaded.
    pub claude_plugins: usize,
    /// Total skills registered from Claude Code plugins.
    pub skills_registered: usize,
    /// Total agent templates registered from Claude Code plugins.
    pub agents_registered: usize,
    /// Total hook handlers registered from Claude Code plugins.
    pub hooks_registered: usize,
    /// Total commands registered from Claude Code plugins.
    pub commands_registered: usize,
    /// Total MCP server entries discovered (informational; not yet launched).
    pub mcp_servers_found: usize,
}

// ---------------------------------------------------------------------------
// ClaudeHookHandler
// ---------------------------------------------------------------------------

/// A [`HookHandler`] that dispatches to a shell command defined in a Claude
/// Code plugin's `hooks/hooks.json`.
struct ClaudeHookHandler {
    /// The plugin that owns this hook (for logging).
    plugin: String,
    /// Optional tool-name glob pattern. `None` means match all tools.
    matcher_tool_name: Option<String>,
    /// The shell command to run (for "command" type hooks).
    command: Option<String>,
    /// Hook action type (e.g. "command"). Non-command types are no-ops for now.
    action_type: String,
    /// Session kinds this handler applies to. None = all kinds.
    session_kinds: Option<Vec<String>>,
}

#[async_trait::async_trait]
impl HookHandler for ClaudeHookHandler {
    async fn handle(
        &self,
        _event: &HookEvent,
        context: &mut serde_json::Value,
    ) -> Result<(), String> {
        // Apply tool_name matcher if present.
        if let Some(ref pattern) = self.matcher_tool_name {
            let tool_name = context
                .get("tool_name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !glob_match(pattern, tool_name) {
                return Ok(());
            }
        }

        match self.action_type.as_str() {
            "command" => {
                if let Some(ref cmd) = self.command {
                    let output = tokio::process::Command::new("sh")
                        .arg("-c")
                        .arg(cmd)
                        .output()
                        .await
                        .map_err(|e| format!("hook command spawn failed: {e}"))?;

                    if !output.status.success() {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        warn!(
                            plugin = %self.plugin,
                            cmd = %cmd,
                            exit = ?output.status.code(),
                            stderr = %stderr,
                            "hook command exited with non-zero status"
                        );
                    } else {
                        debug!(plugin = %self.plugin, cmd = %cmd, "hook command succeeded");
                    }
                    Ok(())
                } else {
                    Err("hook action_type is 'command' but no command string provided".to_string())
                }
            }
            other => {
                // Future enhancement: handle "prompt" type hooks, etc.
                debug!(plugin = %self.plugin, action_type = %other, "hook type not yet implemented, skipping");
                Ok(())
            }
        }
    }

    fn plugin_name(&self) -> &str {
        &self.plugin
    }

    fn session_kinds_filter(&self) -> Option<&[String]> {
        self.session_kinds.as_deref()
    }
}

// ---------------------------------------------------------------------------
// PluginScanner
// ---------------------------------------------------------------------------

/// Unified scanner: iterates scan directories and registers Claude Code and
/// native plugins into the appropriate registries.
pub struct PluginScanner {
    scan_dirs: Vec<PathBuf>,
    overrides: std::collections::HashMap<String, PluginOverride>,
    /// Held so the registry Arc stays alive; not read directly.
    #[allow(dead_code)]
    plugin_registry: Arc<PluginRegistry>,
    skill_registry: Arc<SkillRegistry>,
    agent_registry: Arc<AgentRegistry>,
    command_registry: Arc<CommandRegistry>,
    hook_registry: Arc<HookRegistry>,
    /// MCP servers discovered during the last scan.
    discovered_mcp_servers: Arc<tokio::sync::RwLock<Vec<claude_plugin::ClaudeMcpServer>>>,
}

impl PluginScanner {
    /// Create a new scanner.
    pub fn new(
        plugins_config: &PluginsConfig,
        plugin_registry: Arc<PluginRegistry>,
        skill_registry: Arc<SkillRegistry>,
        agent_registry: Arc<AgentRegistry>,
        command_registry: Arc<CommandRegistry>,
        hook_registry: Arc<HookRegistry>,
    ) -> Self {
        Self {
            scan_dirs: plugins_config.scan_dirs.iter().map(PathBuf::from).collect(),
            overrides: plugins_config.overrides.clone(),
            plugin_registry,
            skill_registry,
            agent_registry,
            command_registry,
            hook_registry,
            discovered_mcp_servers: Arc::new(tokio::sync::RwLock::new(Vec::new())),
        }
    }

    /// Return MCP servers discovered during the last scan.
    pub async fn discovered_mcp_servers(&self) -> Vec<claude_plugin::ClaudeMcpServer> {
        self.discovered_mcp_servers.read().await.clone()
    }

    /// Scan all configured directories and register discovered plugins.
    ///
    /// First-directory-wins: a plugin whose name is already seen from an
    /// earlier directory is skipped.
    pub async fn scan(&self) -> UnifiedScanSummary {
        let mut summary = UnifiedScanSummary::default();
        let mut seen_names: HashSet<String> = HashSet::new();
        let mut mcp_servers: Vec<claude_plugin::ClaudeMcpServer> = Vec::new();

        for dir in &self.scan_dirs {
            let expanded = expand_tilde(dir);

            if !expanded.exists() {
                debug!(dir = %expanded.display(), "scan directory does not exist, skipping");
                continue;
            }

            if let Err(e) = tokio::fs::read_dir(&expanded).await {
                warn!(dir = %expanded.display(), error = %e, "failed to read scan directory");
                continue;
            }

            let mut dirs_to_scan = vec![expanded];
            // Recurse up to 3 levels to handle Claude cache structure:
            // cache/claude-plugins-official/<name>/<version>/
            for _depth in 0..3 {
                let mut next_dirs = Vec::new();
                for scan_dir in &dirs_to_scan {
                    let mut entries = match tokio::fs::read_dir(scan_dir).await {
                        Ok(e) => e,
                        Err(_) => continue,
                    };
                    while let Ok(Some(entry)) = entries.next_entry().await {
                        let path = entry.path();
                        if !path.is_dir() {
                            continue;
                        }

                        if claude_plugin::is_claude_plugin_dir(&path) {
                            self.handle_claude_plugin(
                                &path,
                                &mut seen_names,
                                &mut summary,
                                &mut mcp_servers,
                            )
                            .await;
                        } else if path.join("PLUGIN.md").exists() {
                            let dir_name = path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("")
                                .to_string();
                            if seen_names.insert(dir_name) {
                                summary.native_plugins += 1;
                            }
                        } else {
                            // Not a plugin dir — recurse deeper (marketplace/version dirs)
                            next_dirs.push(path);
                        }
                    }
                }
                if next_dirs.is_empty() {
                    break;
                }
                dirs_to_scan = next_dirs;
            }
        }

        *self.discovered_mcp_servers.write().await = mcp_servers;

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

    /// Parse and register a Claude Code plugin directory.
    async fn handle_claude_plugin(
        &self,
        path: &Path,
        seen_names: &mut HashSet<String>,
        summary: &mut UnifiedScanSummary,
        mcp_servers: &mut Vec<claude_plugin::ClaudeMcpServer>,
    ) {
        let parsed = match claude_plugin::parse_claude_plugin(path).await {
            Ok(p) => p,
            Err(e) => {
                warn!(dir = %path.display(), error = %e, "failed to parse Claude plugin");
                return;
            }
        };

        let plugin_name = parsed.name.clone();

        // First-directory-wins.
        if !seen_names.insert(plugin_name.clone()) {
            debug!(plugin = %plugin_name, "plugin already registered from an earlier directory, skipping");
            return;
        }

        let plugin_override = self
            .overrides
            .get(&plugin_name)
            .cloned()
            .unwrap_or_default();
        if !plugin_override.enabled {
            debug!(plugin = %plugin_name, "plugin disabled by config override, skipping");
            return;
        }

        summary.claude_plugins += 1;

        // --- Register skills ---
        for cs in &parsed.skills {
            let skill = Skill {
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
                namespace: None,
                version: None,
                author: None,
                kind: Default::default(),
                requires: vec![],
                tags: vec![],
                match_rules: None,
                triggers: vec![],
            };
            self.skill_registry.register(skill).await;
            summary.skills_registered += 1;
        }

        // --- Register agents ---
        for ca in &parsed.agents {
            let template = AgentTemplate {
                name: ca.name.clone(),
                description: ca.description.clone(),
                when_to_use: ca.when_to_use.clone(),
                system_prompt: ca.system_prompt.clone(),
                model: ca.model.clone(),
                allowed_tools: ca.allowed_tools.clone(),
            };
            self.agent_registry.register(template).await;
            summary.agents_registered += 1;
        }

        // --- Register hooks ---
        for ch in &parsed.hooks {
            let Some(event) = HookEvent::from_str(&ch.event) else {
                warn!(
                    plugin = %plugin_name,
                    event = %ch.event,
                    "unknown hook event in Claude plugin, skipping"
                );
                continue;
            };

            let handler = ClaudeHookHandler {
                plugin: plugin_name.clone(),
                matcher_tool_name: ch.matcher.as_ref().and_then(|m| m.tool_name.clone()),
                command: ch.hook.command.clone(),
                action_type: ch.hook.action_type.clone(),
                session_kinds: plugin_override.session_kinds.clone(),
            };

            self.hook_registry.register(event, Box::new(handler)).await;
            summary.hooks_registered += 1;
        }

        // --- Register commands ---
        for cc in &parsed.commands {
            let cmd = Command {
                name: cc.name.clone(),
                description: cc.description.clone(),
                prompt_body: cc.prompt_body.clone(),
                plugin_name: plugin_name.clone(),
            };
            self.command_registry.register(cmd).await;
            summary.commands_registered += 1;
        }

        // --- MCP servers ---
        summary.mcp_servers_found += parsed.mcp_servers.len();
        mcp_servers.extend(parsed.mcp_servers.iter().cloned());

        debug!(
            plugin = %plugin_name,
            skills = parsed.skills.len(),
            agents = parsed.agents.len(),
            hooks = parsed.hooks.len(),
            commands = parsed.commands.len(),
            mcp = parsed.mcp_servers.len(),
            "Claude plugin registered"
        );
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Expand a leading `~` to the user's home directory.
///
/// Returns the path unchanged if it does not start with `~` or the home
/// directory cannot be determined.
fn expand_tilde(path: &Path) -> PathBuf {
    let s = match path.to_str() {
        Some(s) => s,
        None => return path.to_path_buf(),
    };

    if s == "~" {
        return dirs::home_dir().unwrap_or_else(|| path.to_path_buf());
    }

    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }

    path.to_path_buf()
}

/// Match a glob pattern against a string.
///
/// Supported wildcards:
/// - `*` matches any sequence of characters (within a single path component)
/// - `?` matches any single character
///
/// This is intentionally minimal — only the subset needed for tool-name
/// matching is implemented.
fn glob_match(pattern: &str, text: &str) -> bool {
    glob_match_inner(pattern.as_bytes(), text.as_bytes())
}

fn glob_match_inner(pattern: &[u8], text: &[u8]) -> bool {
    match (pattern.first(), text.first()) {
        (None, None) => true,
        (Some(&b'*'), _) => {
            // Try matching zero characters, then one, then more…
            glob_match_inner(&pattern[1..], text)
                || (!text.is_empty() && glob_match_inner(pattern, &text[1..]))
        }
        (Some(&b'?'), Some(_)) => glob_match_inner(&pattern[1..], &text[1..]),
        (Some(p), Some(t)) if p == t => glob_match_inner(&pattern[1..], &text[1..]),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_name_glob_matching() {
        // Exact match
        assert!(glob_match("Bash", "Bash"));
        assert!(!glob_match("Bash", "bash"));

        // Star wildcard
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*", ""));
        assert!(glob_match("Ba*", "Bash"));
        assert!(glob_match("Ba*", "Batch"));
        assert!(!glob_match("Ba*", "Read"));

        // Prefix + suffix wildcards
        assert!(glob_match("*Tool*", "SomeTool"));
        assert!(glob_match("*Tool*", "MyToolExecutor"));
        assert!(!glob_match("*Tool*", "Bash"));

        // Question mark
        assert!(glob_match("B?sh", "Bash"));
        assert!(glob_match("B?sh", "Bush"));
        assert!(!glob_match("B?sh", "Bsh"));

        // Trailing wildcard
        assert!(glob_match("Read*", "ReadFile"));
        assert!(glob_match("Read*", "Read"));
    }

    #[test]
    fn tilde_expansion() {
        let home = dirs::home_dir();

        if let Some(ref home) = home {
            let p = PathBuf::from("~");
            assert_eq!(expand_tilde(&p), *home);

            let p = PathBuf::from("~/plugins");
            assert_eq!(expand_tilde(&p), home.join("plugins"));
        }

        // No tilde — path returned unchanged.
        let p = PathBuf::from("/absolute/path");
        assert_eq!(expand_tilde(&p), PathBuf::from("/absolute/path"));

        let p = PathBuf::from("relative/path");
        assert_eq!(expand_tilde(&p), PathBuf::from("relative/path"));
    }

    #[tokio::test]
    async fn scan_empty_dir_returns_zero_summary() {
        let tmp = tempfile::TempDir::new().unwrap();

        let scanner = PluginScanner::new(
            &PluginsConfig {
                scan_dirs: vec![tmp.path().display().to_string()],
                ..Default::default()
            },
            Arc::new(PluginRegistry::new()),
            Arc::new(SkillRegistry::new()),
            Arc::new(AgentRegistry::new()),
            Arc::new(CommandRegistry::new()),
            Arc::new(HookRegistry::new()),
        );

        let summary = scanner.scan().await;
        assert_eq!(summary.native_plugins, 0);
        assert_eq!(summary.claude_plugins, 0);
        assert_eq!(summary.skills_registered, 0);
    }

    #[tokio::test]
    async fn scan_registers_claude_plugin() {
        let tmp = tempfile::TempDir::new().unwrap();
        let plugin_dir = tmp.path().join("my-plugin");

        // .claude-plugin/plugin.json
        tokio::fs::create_dir_all(plugin_dir.join(".claude-plugin"))
            .await
            .unwrap();
        tokio::fs::write(
            plugin_dir.join(".claude-plugin/plugin.json"),
            r#"{"name":"my-plugin","description":"Test","version":"1.0.0"}"#,
        )
        .await
        .unwrap();

        // skills/greet/greet.md
        let skill_dir = plugin_dir.join("skills/greet");
        tokio::fs::create_dir_all(&skill_dir).await.unwrap();
        tokio::fs::write(
            skill_dir.join("greet.md"),
            "---\nname: greet\ndescription: Greet the user\nuser-invocable: true\n---\n\nSay hello.\n",
        )
        .await
        .unwrap();

        let skill_registry = Arc::new(SkillRegistry::new());
        let scanner = PluginScanner::new(
            &PluginsConfig {
                scan_dirs: vec![tmp.path().display().to_string()],
                ..Default::default()
            },
            Arc::new(PluginRegistry::new()),
            skill_registry.clone(),
            Arc::new(AgentRegistry::new()),
            Arc::new(CommandRegistry::new()),
            Arc::new(HookRegistry::new()),
        );

        let summary = scanner.scan().await;
        assert_eq!(summary.claude_plugins, 1);
        assert_eq!(summary.skills_registered, 1);

        let skill = skill_registry.get("my-plugin:greet").await;
        assert!(skill.is_some());
        let skill = skill.unwrap();
        assert_eq!(skill.description, "Greet the user");
        assert!(skill.prompt_body.as_deref().unwrap().contains("Say hello"));
        assert!(skill.user_invocable);
    }

    #[tokio::test]
    async fn disabled_plugin_override_skips_registration() {
        let tmp = tempfile::TempDir::new().unwrap();
        let plugin_dir = tmp.path().join("my-plugin");
        tokio::fs::create_dir_all(plugin_dir.join(".claude-plugin"))
            .await
            .unwrap();
        tokio::fs::write(
            plugin_dir.join(".claude-plugin/plugin.json"),
            r#"{"name":"my-plugin","description":"Test","version":"1.0.0"}"#,
        )
        .await
        .unwrap();
        let skill_dir = plugin_dir.join("skills/greet");
        tokio::fs::create_dir_all(&skill_dir).await.unwrap();
        tokio::fs::write(
            skill_dir.join("greet.md"),
            "---
name: greet
description: Greet the user
---

Say hello.
",
        )
        .await
        .unwrap();

        let skill_registry = Arc::new(SkillRegistry::new());
        let mut overrides = std::collections::HashMap::new();
        overrides.insert(
            "my-plugin".to_string(),
            PluginOverride {
                enabled: false,
                ..Default::default()
            },
        );
        let scanner = PluginScanner::new(
            &PluginsConfig {
                scan_dirs: vec![tmp.path().display().to_string()],
                overrides,
                ..Default::default()
            },
            Arc::new(PluginRegistry::new()),
            skill_registry.clone(),
            Arc::new(AgentRegistry::new()),
            Arc::new(CommandRegistry::new()),
            Arc::new(HookRegistry::new()),
        );

        let summary = scanner.scan().await;
        assert_eq!(summary.claude_plugins, 0);
        assert!(skill_registry.get("my-plugin:greet").await.is_none());
    }

    #[tokio::test]
    async fn first_directory_wins() {
        let tmp = tempfile::TempDir::new().unwrap();

        // Two scan directories both containing a plugin named "my-plugin".
        let dir_a = tmp.path().join("a");
        let dir_b = tmp.path().join("b");
        tokio::fs::create_dir_all(&dir_a).await.unwrap();
        tokio::fs::create_dir_all(&dir_b).await.unwrap();

        for (dir, desc) in [(&dir_a, "from-a"), (&dir_b, "from-b")] {
            let plugin_dir = dir.join("my-plugin");
            tokio::fs::create_dir_all(plugin_dir.join(".claude-plugin"))
                .await
                .unwrap();
            tokio::fs::write(
                plugin_dir.join(".claude-plugin/plugin.json"),
                format!(r#"{{"name":"my-plugin","description":"{desc}","version":"1.0.0"}}"#),
            )
            .await
            .unwrap();
        }

        let skill_registry = Arc::new(SkillRegistry::new());
        let scanner = PluginScanner::new(
            &PluginsConfig {
                scan_dirs: vec![dir_a.display().to_string(), dir_b.display().to_string()],
                ..Default::default()
            },
            Arc::new(PluginRegistry::new()),
            skill_registry.clone(),
            Arc::new(AgentRegistry::new()),
            Arc::new(CommandRegistry::new()),
            Arc::new(HookRegistry::new()),
        );

        let summary = scanner.scan().await;
        // Only one plugin should be loaded (the one from dir_a wins).
        assert_eq!(summary.claude_plugins, 1);
    }
}
