//! Parser for the Claude Code plugin format.
//!
//! Reads `.claude-plugin/plugin.json`, `skills/`, `agents/`, `hooks/hooks.json`,
//! `commands/`, and `.mcp.json` from a plugin directory.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Parsed from `.claude-plugin/plugin.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClaudePluginManifest {
    pub name: String,
    pub description: String,
    pub version: String,
}

/// A skill parsed from `skills/<name>/<name>.md` (or the first `.md` found).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClaudeSkill {
    /// Namespaced name: `"plugin_name:skill_name"`.
    pub name: String,
    pub description: String,
    /// The full markdown body after the frontmatter block.
    pub prompt_body: String,
    pub model: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
    pub user_invocable: bool,
    /// The directory that contained the skill's `.md` file.
    pub source_dir: PathBuf,
}

/// An agent parsed from `agents/<name>.md`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClaudeAgent {
    /// Namespaced name: `"plugin_name:agent_name"`.
    pub name: String,
    pub description: String,
    pub when_to_use: String,
    pub system_prompt: String,
    pub model: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
}

/// A single hook entry from `hooks/hooks.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClaudeHook {
    pub event: String,
    pub matcher: Option<HookMatcher>,
    pub hook: HookAction,
    pub description: Option<String>,
}

/// Optional matcher for a hook (e.g. filter by tool name).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HookMatcher {
    pub tool_name: Option<String>,
}

/// The action a hook performs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HookAction {
    /// One of `"command"` or `"prompt"`.
    #[serde(rename = "type")]
    pub action_type: String,
    pub command: Option<String>,
    pub prompt: Option<String>,
}

/// A slash-command parsed from `commands/<name>.md`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClaudeCommand {
    /// Namespaced name: `"plugin_name:command_name"`.
    pub name: String,
    pub description: String,
    pub prompt_body: String,
}

/// An MCP server entry from `.mcp.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClaudeMcpServer {
    /// The key from the `mcpServers` map.
    pub name: String,
    pub transport: String,
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    pub url: Option<String>,
}

/// The fully-parsed representation of a Claude Code plugin directory.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParsedClaudePlugin {
    pub name: String,
    pub version: String,
    pub description: String,
    /// The root directory of the plugin (contains `.claude-plugin/`).
    pub source_dir: PathBuf,
    pub skills: Vec<ClaudeSkill>,
    pub agents: Vec<ClaudeAgent>,
    pub hooks: Vec<ClaudeHook>,
    pub commands: Vec<ClaudeCommand>,
    pub mcp_servers: Vec<ClaudeMcpServer>,
}

// ---------------------------------------------------------------------------
// Frontmatter deserialization targets
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SkillFrontmatter {
    name: Option<String>,
    description: Option<String>,
    model: Option<String>,
    #[serde(rename = "allowed-tools")]
    allowed_tools: Option<Vec<String>>,
    #[serde(rename = "user-invocable")]
    user_invocable: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct AgentFrontmatter {
    name: Option<String>,
    description: Option<String>,
    #[serde(rename = "when-to-use")]
    when_to_use: Option<String>,
    model: Option<String>,
    #[serde(rename = "allowed-tools")]
    allowed_tools: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct CommandFrontmatter {
    name: Option<String>,
    description: Option<String>,
}

// ---------------------------------------------------------------------------
// Raw `.mcp.json` deserialization
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct McpJsonFile {
    #[serde(rename = "mcpServers")]
    mcp_servers: HashMap<String, McpServerEntry>,
}

#[derive(Debug, Deserialize)]
struct McpServerEntry {
    /// Transport type: "stdio" or "http". Claude Code uses "type" as the key.
    #[serde(alias = "type", default = "default_transport")]
    transport: String,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    #[serde(default)]
    url: Option<String>,
}

fn default_transport() -> String {
    "stdio".to_string()
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Returns `true` if `dir` is a Claude Code plugin directory.
///
/// Detection heuristic: `.claude-plugin/plugin.json` must exist inside `dir`.
pub fn is_claude_plugin_dir(dir: &Path) -> bool {
    dir.join(".claude-plugin").join("plugin.json").exists()
}

/// Parse a Claude Code plugin directory into a [`ParsedClaudePlugin`].
///
/// # Errors
/// Returns a descriptive string on I/O or JSON parse failures for the
/// mandatory `plugin.json`. Missing optional sub-directories are silently
/// skipped (warnings are emitted via `tracing`).
pub async fn parse_claude_plugin(dir: &Path) -> Result<ParsedClaudePlugin, String> {
    let manifest_path = dir.join(".claude-plugin").join("plugin.json");

    let manifest_bytes = tokio::fs::read_to_string(&manifest_path)
        .await
        .map_err(|e| {
            format!(
                "failed to read plugin.json at {}: {e}",
                manifest_path.display()
            )
        })?;

    let manifest: ClaudePluginManifest =
        serde_json::from_str(&manifest_bytes).map_err(|e| format!("invalid plugin.json: {e}"))?;

    let plugin_name = manifest.name.clone();

    debug!(plugin = %plugin_name, dir = %dir.display(), "parsing Claude plugin");

    let skills = parse_skills_dir(&dir.join("skills"), &plugin_name).await;
    let agents = parse_agents_dir(&dir.join("agents"), &plugin_name).await;
    let hooks = parse_hooks_file(&dir.join("hooks").join("hooks.json")).await;
    let commands = parse_commands_dir(&dir.join("commands"), &plugin_name).await;
    let mcp_servers = parse_mcp_file(&dir.join(".mcp.json")).await;

    Ok(ParsedClaudePlugin {
        name: manifest.name,
        version: manifest.version,
        description: manifest.description,
        source_dir: dir.to_path_buf(),
        skills,
        agents,
        hooks,
        commands,
        mcp_servers,
    })
}

// ---------------------------------------------------------------------------
// Component parsers
// ---------------------------------------------------------------------------

/// Scan `<dir>/skills/<name>/` sub-directories for skill markdown files.
///
/// Looks for `<name>.md` first; falls back to the first `.md` file found.
async fn parse_skills_dir(dir: &Path, plugin_name: &str) -> Vec<ClaudeSkill> {
    let mut skills = Vec::new();

    if !dir.exists() {
        return skills;
    }

    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(e) => e,
        Err(e) => {
            warn!(dir = %dir.display(), error = %e, "failed to read skills directory");
            return skills;
        }
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let dir_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Prefer `<name>.md`; fall back to first `.md` file.
        let md_path = path.join(format!("{dir_name}.md"));
        let md_path = if md_path.exists() {
            md_path
        } else {
            match find_first_md(&path).await {
                Some(p) => p,
                None => {
                    debug!(dir = %path.display(), "no .md file found in skill directory, skipping");
                    continue;
                }
            }
        };

        match load_skill(&md_path, &dir_name, plugin_name, &path).await {
            Ok(skill) => {
                debug!(name = %skill.name, "loaded Claude skill");
                skills.push(skill);
            }
            Err(e) => {
                warn!(path = %md_path.display(), error = %e, "failed to load skill");
            }
        }
    }

    skills
}

/// Scan `<dir>/agents/*.md` for agent definitions.
async fn parse_agents_dir(dir: &Path, plugin_name: &str) -> Vec<ClaudeAgent> {
    let mut agents = Vec::new();

    if !dir.exists() {
        return agents;
    }

    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(e) => e,
        Err(e) => {
            warn!(dir = %dir.display(), error = %e, "failed to read agents directory");
            return agents;
        }
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };

        match load_agent(&path, &stem, plugin_name).await {
            Ok(agent) => {
                debug!(name = %agent.name, "loaded Claude agent");
                agents.push(agent);
            }
            Err(e) => {
                warn!(path = %path.display(), error = %e, "failed to load agent");
            }
        }
    }

    agents
}

/// Read `hooks/hooks.json` as a JSON array of hook entries.
async fn parse_hooks_file(path: &Path) -> Vec<ClaudeHook> {
    if !path.exists() {
        return Vec::new();
    }

    let content = match tokio::fs::read_to_string(path).await {
        Ok(c) => c,
        Err(e) => {
            warn!(path = %path.display(), error = %e, "failed to read hooks.json");
            return Vec::new();
        }
    };

    match serde_json::from_str::<Vec<ClaudeHook>>(&content) {
        Ok(hooks) => hooks,
        Err(e) => {
            warn!(path = %path.display(), error = %e, "failed to parse hooks.json");
            Vec::new()
        }
    }
}

/// Scan `<dir>/commands/*.md` for slash-command definitions.
async fn parse_commands_dir(dir: &Path, plugin_name: &str) -> Vec<ClaudeCommand> {
    let mut commands = Vec::new();

    if !dir.exists() {
        return commands;
    }

    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(e) => e,
        Err(e) => {
            warn!(dir = %dir.display(), error = %e, "failed to read commands directory");
            return commands;
        }
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };

        match load_command(&path, &stem, plugin_name).await {
            Ok(cmd) => {
                debug!(name = %cmd.name, "loaded Claude command");
                commands.push(cmd);
            }
            Err(e) => {
                warn!(path = %path.display(), error = %e, "failed to load command");
            }
        }
    }

    commands
}

/// Read `.mcp.json` — supports both formats:
/// - `{ "mcpServers": { "name": { ... } } }` (plugin.json style)
/// - `{ "name": { "command": ..., "args": [...] } }` (Claude Code .mcp.json style)
async fn parse_mcp_file(path: &Path) -> Vec<ClaudeMcpServer> {
    if !path.exists() {
        return Vec::new();
    }

    let content = match tokio::fs::read_to_string(path).await {
        Ok(c) => c,
        Err(e) => {
            warn!(path = %path.display(), error = %e, "failed to read .mcp.json");
            return Vec::new();
        }
    };

    // Try the { "mcpServers": { ... } } format first
    if let Ok(parsed) = serde_json::from_str::<McpJsonFile>(&content) {
        return parsed
            .mcp_servers
            .into_iter()
            .map(|(name, entry)| ClaudeMcpServer {
                name,
                transport: entry.transport,
                command: entry.command,
                args: entry.args,
                env: entry.env,
                url: entry.url,
            })
            .collect();
    }

    // Fall back to flat format: { "server_name": { "command": ..., "args": [...] } }
    if let Ok(flat) = serde_json::from_str::<HashMap<String, McpServerEntry>>(&content) {
        return flat
            .into_iter()
            .map(|(name, entry)| ClaudeMcpServer {
                name,
                transport: entry.transport,
                command: entry.command,
                args: entry.args,
                env: entry.env,
                url: entry.url,
            })
            .collect();
    }

    warn!(path = %path.display(), "failed to parse .mcp.json in any known format");
    Vec::new()
}

// ---------------------------------------------------------------------------
// Individual file loaders
// ---------------------------------------------------------------------------

async fn load_skill(
    md_path: &Path,
    dir_name: &str,
    plugin_name: &str,
    source_dir: &Path,
) -> Result<ClaudeSkill, String> {
    let content = tokio::fs::read_to_string(md_path)
        .await
        .map_err(|e| format!("read error: {e}"))?;

    let (frontmatter_str, body) = split_frontmatter(&content);

    let fm: SkillFrontmatter = if let Some(yaml) = frontmatter_str {
        let val = parse_yaml_value(yaml);
        serde_json::from_value(val).unwrap_or(SkillFrontmatter {
            name: None,
            description: None,
            model: None,
            allowed_tools: None,
            user_invocable: None,
        })
    } else {
        SkillFrontmatter {
            name: None,
            description: None,
            model: None,
            allowed_tools: None,
            user_invocable: None,
        }
    };

    let raw_name = fm.name.unwrap_or_else(|| dir_name.to_string());
    let namespaced_name = format!("{plugin_name}:{raw_name}");
    let description = fm
        .description
        .unwrap_or_else(|| format!("Skill: {raw_name}"));

    Ok(ClaudeSkill {
        name: namespaced_name,
        description,
        prompt_body: body.trim().to_string(),
        model: fm.model,
        allowed_tools: fm.allowed_tools,
        user_invocable: fm.user_invocable.unwrap_or(false),
        source_dir: source_dir.to_path_buf(),
    })
}

async fn load_agent(md_path: &Path, stem: &str, plugin_name: &str) -> Result<ClaudeAgent, String> {
    let content = tokio::fs::read_to_string(md_path)
        .await
        .map_err(|e| format!("read error: {e}"))?;

    let (frontmatter_str, body) = split_frontmatter(&content);

    let fm: AgentFrontmatter = if let Some(yaml) = frontmatter_str {
        let val = parse_yaml_value(yaml);
        serde_json::from_value(val).unwrap_or(AgentFrontmatter {
            name: None,
            description: None,
            when_to_use: None,
            model: None,
            allowed_tools: None,
        })
    } else {
        AgentFrontmatter {
            name: None,
            description: None,
            when_to_use: None,
            model: None,
            allowed_tools: None,
        }
    };

    let raw_name = fm.name.unwrap_or_else(|| stem.to_string());
    let namespaced_name = format!("{plugin_name}:{raw_name}");
    let description = fm
        .description
        .unwrap_or_else(|| format!("Agent: {raw_name}"));
    let when_to_use = fm.when_to_use.unwrap_or_default();

    Ok(ClaudeAgent {
        name: namespaced_name,
        description,
        when_to_use,
        system_prompt: body.trim().to_string(),
        model: fm.model,
        allowed_tools: fm.allowed_tools,
    })
}

async fn load_command(
    md_path: &Path,
    stem: &str,
    plugin_name: &str,
) -> Result<ClaudeCommand, String> {
    let content = tokio::fs::read_to_string(md_path)
        .await
        .map_err(|e| format!("read error: {e}"))?;

    let (frontmatter_str, body) = split_frontmatter(&content);

    let fm: CommandFrontmatter = if let Some(yaml) = frontmatter_str {
        let val = parse_yaml_value(yaml);
        serde_json::from_value(val).unwrap_or(CommandFrontmatter {
            name: None,
            description: None,
        })
    } else {
        CommandFrontmatter {
            name: None,
            description: None,
        }
    };

    let raw_name = fm.name.unwrap_or_else(|| stem.to_string());
    let namespaced_name = format!("{plugin_name}:{raw_name}");
    let description = fm
        .description
        .unwrap_or_else(|| format!("Command: {raw_name}"));

    Ok(ClaudeCommand {
        name: namespaced_name,
        description,
        prompt_body: body.trim().to_string(),
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Find the first `.md` file in `dir` (non-recursive).
async fn find_first_md(dir: &Path) -> Option<PathBuf> {
    let mut entries = tokio::fs::read_dir(dir).await.ok()?;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            return Some(path);
        }
    }
    None
}

/// Split YAML frontmatter from markdown content.
///
/// Returns `(Some(yaml_str), body)` when the content begins with `---\n`
/// and contains a closing `\n---` delimiter.  Otherwise returns
/// `(None, full_content)`.
pub fn split_frontmatter(content: &str) -> (Option<&str>, &str) {
    let trimmed = content.trim_start_matches('\n');

    if !trimmed.starts_with("---") {
        return (None, content);
    }

    let after_open = &trimmed[3..];

    // The opening `---` must be followed by a newline (not `---something`).
    let after_open = match after_open.strip_prefix('\n') {
        Some(s) => s,
        None => return (None, content),
    };

    let close_pos = match after_open.find("\n---") {
        Some(p) => p,
        None => return (None, content),
    };
    let yaml = &after_open[..close_pos];

    // Advance past the closing `---` (and optional newline after it).
    let rest = &after_open[close_pos + 4..]; // skip "\n---"
    let body = rest.strip_prefix('\n').unwrap_or(rest);

    (Some(yaml), body)
}

/// Minimal YAML-subset parser that handles:
/// - `key: value` string pairs (with optional surrounding quotes)
/// - `key: true` / `key: false` booleans
/// - `key: [...]` / `key: {...}` JSON-embedded values
/// - YAML block lists (`- item` under a key)
/// - `#` comment lines
///
/// Returns a `serde_json::Value::Object`.
pub fn parse_yaml_value(yaml: &str) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    let mut current_list_key: Option<String> = None;

    for line in yaml.lines() {
        let line_raw = line;
        let line = line.trim();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Detect list item under current key (`  - value` or `- value`).
        if line.starts_with("- ") {
            if let Some(ref key) = current_list_key {
                let item = line[2..].trim().to_string();
                // Strip surrounding quotes.
                let item = item
                    .strip_prefix('"')
                    .and_then(|v| v.strip_suffix('"'))
                    .map(|s| s.to_string())
                    .unwrap_or(item);

                let arr = map
                    .entry(key.clone())
                    .or_insert_with(|| serde_json::Value::Array(Vec::new()));
                if let serde_json::Value::Array(v) = arr {
                    v.push(serde_json::Value::String(item));
                }
            }
            continue;
        }

        // If the line is not indented (leading whitespace check) AND has a
        // colon, it is a new key—reset the list accumulator.
        let is_key_line = !line_raw.starts_with(' ') && !line_raw.starts_with('\t');

        if !is_key_line {
            // Indented non-list line: skip (complex YAML we don't support).
            continue;
        }

        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().to_string();
        let value = value.trim();

        // Reset list accumulator whenever we see a new top-level key.
        current_list_key = None;

        if value.is_empty() {
            // Could be the start of a block list — defer insertion.
            current_list_key = Some(key.clone());
            map.insert(key, serde_json::Value::Array(Vec::new()));
        } else if value == "true" || value == "false" {
            map.insert(
                key,
                serde_json::Value::Bool(value.parse::<bool>().unwrap_or(false)),
            );
        } else if value.starts_with('[') || value.starts_with('{') {
            // Inline JSON array or object.
            if let Ok(parsed) = serde_json::from_str(value) {
                map.insert(key, parsed);
            } else {
                map.insert(key, serde_json::Value::String(value.to_string()));
            }
        } else {
            let value = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .unwrap_or(value);
            map.insert(key, serde_json::Value::String(value.to_string()));
        }
    }

    serde_json::Value::Object(map)
}

// ---------------------------------------------------------------------------
// Standalone MCP discovery (for use before full plugin scan)
// ---------------------------------------------------------------------------

/// Scan directories for Claude Code plugins and return all MCP server
/// declarations, without registering skills/agents/hooks/commands.
///
/// This is used at gateway startup to discover plugin MCP servers before
/// the full plugin scan runs, so they can be connected alongside
/// config-declared MCP servers.
pub async fn discover_plugin_mcp_servers(scan_dirs: &[PathBuf]) -> Vec<ClaudeMcpServer> {
    let mut servers = Vec::new();

    for scan_dir in scan_dirs {
        if !scan_dir.is_dir() {
            continue;
        }
        // Recurse up to 3 levels (cache/marketplace/name/version/)
        let mut dirs = vec![scan_dir.clone()];
        for _depth in 0..3 {
            let mut next = Vec::new();
            for dir in &dirs {
                let mut entries = match tokio::fs::read_dir(dir).await {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let path = entry.path();
                    if !path.is_dir() {
                        continue;
                    }
                    let mcp_path = path.join(".mcp.json");
                    if mcp_path.is_file() {
                        let mcp = parse_mcp_file(&mcp_path).await;
                        if !mcp.is_empty() {
                            debug!(
                                dir = %path.display(),
                                count = mcp.len(),
                                "discovered plugin MCP servers"
                            );
                            servers.extend(mcp);
                        }
                    } else if !is_claude_plugin_dir(&path) && !path.join("PLUGIN.md").exists() {
                        next.push(path);
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            dirs = next;
        }
    }

    servers
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // --- Frontmatter helpers ---

    #[test]
    fn split_frontmatter_works() {
        let content = "---\nname: my-skill\ndescription: A test\n---\n\n# Body\n\nSome text.\n";
        let (fm, body) = split_frontmatter(content);
        assert!(fm.is_some());
        let yaml = fm.unwrap();
        assert!(yaml.contains("name: my-skill"));
        assert!(yaml.contains("description: A test"));
        assert!(body.contains("# Body"));
        assert!(!body.contains("---"));
    }

    #[test]
    fn split_frontmatter_no_frontmatter() {
        let content = "# Just a heading\n\nNo frontmatter here.\n";
        let (fm, body) = split_frontmatter(content);
        assert!(fm.is_none());
        assert_eq!(body, content);
    }

    #[test]
    fn split_frontmatter_missing_close_delimiter() {
        let content = "---\nname: broken\n";
        let (fm, body) = split_frontmatter(content);
        assert!(fm.is_none());
        assert_eq!(body, content);
    }

    #[test]
    fn parse_yaml_value_simple() {
        let yaml = "name: my-skill\ndescription: A test skill\nenabled: true\n";
        let val = parse_yaml_value(yaml);
        assert_eq!(val["name"], serde_json::Value::String("my-skill".into()));
        assert_eq!(
            val["description"],
            serde_json::Value::String("A test skill".into())
        );
        assert_eq!(val["enabled"], serde_json::Value::Bool(true));
    }

    #[test]
    fn parse_yaml_value_block_list() {
        let yaml = "name: test\nallowed-tools:\n- Bash\n- Read\n- Write\n";
        let val = parse_yaml_value(yaml);
        let tools = val["allowed-tools"].as_array().unwrap();
        assert_eq!(tools.len(), 3);
        assert_eq!(tools[0], serde_json::Value::String("Bash".into()));
        assert_eq!(tools[2], serde_json::Value::String("Write".into()));
    }

    #[test]
    fn parse_yaml_value_inline_json_array() {
        // inline JSON array on the same line
        let yaml = "allowed-tools: [\"Bash\",\"Read\"]\n";
        let val = parse_yaml_value(yaml);
        assert_eq!(val["allowed-tools"].as_array().unwrap().len(), 2);
    }

    // --- Integration: full plugin directory parse ---

    #[tokio::test]
    async fn parse_claude_plugin_directory() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        // .claude-plugin/plugin.json
        tokio::fs::create_dir_all(root.join(".claude-plugin"))
            .await
            .unwrap();
        tokio::fs::write(
            root.join(".claude-plugin/plugin.json"),
            r#"{"name":"test-plugin","description":"A test plugin","version":"1.0.0"}"#,
        )
        .await
        .unwrap();

        // skills/my-skill/my-skill.md
        let skill_dir = root.join("skills/my-skill");
        tokio::fs::create_dir_all(&skill_dir).await.unwrap();
        tokio::fs::write(
            skill_dir.join("my-skill.md"),
            "---\nname: my-skill\ndescription: Does things\nuser-invocable: true\n---\n\nDo stuff.\n",
        )
        .await
        .unwrap();

        // agents/my-agent.md
        tokio::fs::create_dir_all(root.join("agents"))
            .await
            .unwrap();
        tokio::fs::write(
            root.join("agents/my-agent.md"),
            "---\nname: my-agent\ndescription: An agent\nwhen-to-use: When needed\n---\n\nSystem prompt here.\n",
        )
        .await
        .unwrap();

        // hooks/hooks.json
        tokio::fs::create_dir_all(root.join("hooks")).await.unwrap();
        tokio::fs::write(
            root.join("hooks/hooks.json"),
            r#"[{"event":"pre_tool_call","hook":{"type":"command","command":"echo hi"},"description":"A hook"}]"#,
        )
        .await
        .unwrap();

        // commands/do-thing.md
        tokio::fs::create_dir_all(root.join("commands"))
            .await
            .unwrap();
        tokio::fs::write(
            root.join("commands/do-thing.md"),
            "---\nname: do-thing\ndescription: Does a thing\n---\n\nDo the thing.\n",
        )
        .await
        .unwrap();

        let plugin = parse_claude_plugin(root).await.unwrap();

        assert_eq!(plugin.name, "test-plugin");
        assert_eq!(plugin.version, "1.0.0");
        assert_eq!(plugin.skills.len(), 1, "expected 1 skill");
        assert_eq!(plugin.agents.len(), 1, "expected 1 agent");
        assert_eq!(plugin.hooks.len(), 1, "expected 1 hook");
        assert_eq!(plugin.commands.len(), 1, "expected 1 command");
        assert_eq!(plugin.mcp_servers.len(), 0, "expected 0 mcp servers");

        let skill = &plugin.skills[0];
        assert_eq!(skill.name, "test-plugin:my-skill");
        assert_eq!(skill.description, "Does things");
        assert!(skill.user_invocable);
        assert!(skill.prompt_body.contains("Do stuff"));

        let agent = &plugin.agents[0];
        assert_eq!(agent.name, "test-plugin:my-agent");
        assert_eq!(agent.when_to_use, "When needed");
        assert!(agent.system_prompt.contains("System prompt here"));

        let hook = &plugin.hooks[0];
        assert_eq!(hook.event, "pre_tool_call");
        assert_eq!(hook.hook.action_type, "command");
        assert_eq!(hook.hook.command.as_deref(), Some("echo hi"));

        let cmd = &plugin.commands[0];
        assert_eq!(cmd.name, "test-plugin:do-thing");
        assert!(cmd.prompt_body.contains("Do the thing"));
    }

    // --- Detection ---

    #[tokio::test]
    async fn is_claude_plugin_dir_detection() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        // Not yet a plugin dir.
        assert!(!is_claude_plugin_dir(root));

        // Create the sentinel file.
        tokio::fs::create_dir_all(root.join(".claude-plugin"))
            .await
            .unwrap();
        tokio::fs::write(
            root.join(".claude-plugin/plugin.json"),
            r#"{"name":"x","description":"x","version":"0.0.1"}"#,
        )
        .await
        .unwrap();

        assert!(is_claude_plugin_dir(root));
    }

    // --- MCP server parsing ---

    #[tokio::test]
    async fn parse_mcp_file_parses_servers() {
        let tmp = TempDir::new().unwrap();
        let mcp_path = tmp.path().join(".mcp.json");

        tokio::fs::write(
            &mcp_path,
            r#"{
  "mcpServers": {
    "context7": {
      "transport": "stdio",
      "command": "npx",
      "args": ["-y", "@upstash/context7-mcp@latest"],
      "env": {"NODE_ENV": "production"}
    }
  }
}"#,
        )
        .await
        .unwrap();

        let servers = parse_mcp_file(&mcp_path).await;
        assert_eq!(servers.len(), 1);
        let s = &servers[0];
        assert_eq!(s.name, "context7");
        assert_eq!(s.transport, "stdio");
        assert_eq!(s.command.as_deref(), Some("npx"));
        assert_eq!(s.args, vec!["-y", "@upstash/context7-mcp@latest"]);
        assert_eq!(
            s.env.get("NODE_ENV").map(|s| s.as_str()),
            Some("production")
        );
    }
}
