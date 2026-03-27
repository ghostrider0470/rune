//! Dynamic spell registry for hot-reloading SPELL.md-defined tools.
//!
//! Renamed from the original "skill" system (#299). The old `Skill` /
//! `SkillRegistry` / `SkillFrontmatter` type aliases are re-exported from
//! `crate::skill` for backward compatibility.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, info};

// ── Spell kind ──────────────────────────────────────────────────────

/// The kind of spell (issue #300).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpellKind {
    /// A conversational / advisory spell (default).
    #[default]
    Assistant,
    /// A spell that exposes a callable tool.
    Tool,
    /// A multi-step workflow orchestration spell.
    Workflow,
    /// A passive spell triggered by external events.
    Sensor,
}

// ── Spell ───────────────────────────────────────────────────────────

/// A single spell parsed from a SPELL.md (or legacy SKILL.md) file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Spell {
    /// Unique spell name (derived from directory name or frontmatter).
    pub name: String,
    /// Human-readable description for the model.
    pub description: String,
    /// JSON Schema for parameters (injected into tool definition).
    pub parameters: serde_json::Value,
    /// Optional path to a binary or script that implements the spell.
    pub binary_path: Option<PathBuf>,
    /// The directory containing the SPELL.md file.
    pub source_dir: PathBuf,
    /// Whether this spell is currently enabled.
    pub enabled: bool,
    /// Optional prompt body for Claude Code-style agentic spells.
    #[serde(default)]
    pub prompt_body: Option<String>,
    /// Optional model override for this spell.
    #[serde(default)]
    pub model: Option<String>,
    /// Optional list of tools this spell is allowed to use.
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
    /// Whether this spell can be directly invoked by the user.
    #[serde(default)]
    pub user_invocable: bool,

    // ── #300 SPELL.md manifest fields ───────────────────────────────

    /// Dotted namespace, e.g. `"horizon.security-audit"`.
    #[serde(default)]
    pub namespace: Option<String>,
    /// Semantic version string, e.g. `"0.1.0"`.
    #[serde(default)]
    pub version: Option<String>,
    /// Optional spell author / org.
    #[serde(default)]
    pub author: Option<String>,
    /// Spell kind (assistant / tool / workflow / sensor).
    #[serde(default)]
    pub kind: SpellKind,
    /// Capabilities the spell requires, e.g. `["network", "filesystem"]`.
    #[serde(default)]
    pub requires: Vec<String>,
    /// Free-form tags for search/filtering.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Optional context-matching rules for auto-activation.
    #[serde(default)]
    pub match_rules: Option<serde_json::Value>,
    /// Trigger patterns that activate this spell.
    #[serde(default)]
    pub triggers: Vec<String>,
}

// ── Frontmatter ─────────────────────────────────────────────────────

/// YAML frontmatter parsed from a SPELL.md file.
///
/// Backward-compatible: the old `name`, `description`, `binary`,
/// `parameters`, `enabled` fields are still accepted alongside the new
/// `namespace`, `version`, `kind`, `requires`, `tags`, `triggers` fields
/// from issue #300.
#[derive(Clone, Debug, Deserialize)]
pub struct SpellFrontmatter {
    // Legacy / core fields
    pub name: Option<String>,
    pub description: Option<String>,
    pub binary: Option<String>,
    pub parameters: Option<serde_json::Value>,
    pub enabled: Option<bool>,

    // #300 new fields
    pub namespace: Option<String>,
    pub version: Option<String>,
    pub kind: Option<SpellKind>,
    pub author: Option<String>,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub match_rules: Option<serde_json::Value>,
    #[serde(default)]
    pub triggers: Vec<String>,
}

// ── Registry ────────────────────────────────────────────────────────

/// Thread-safe dynamic spell registry with add/remove/list/toggle.
#[derive(Clone)]
pub struct SpellRegistry {
    inner: Arc<RwLock<HashMap<String, Spell>>>,
}

impl SpellRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register or update a spell.
    pub async fn register(&self, spell: Spell) {
        let name = spell.name.clone();
        self.inner.write().await.insert(name.clone(), spell);
        debug!(spell = %name, "spell registered");
    }

    /// Remove a spell by name.
    pub async fn remove(&self, name: &str) -> Option<Spell> {
        let removed = self.inner.write().await.remove(name);
        if removed.is_some() {
            debug!(spell = %name, "spell removed");
        }
        removed
    }

    /// List all spells (enabled and disabled).
    pub async fn list(&self) -> Vec<Spell> {
        self.inner.read().await.values().cloned().collect()
    }

    /// List only enabled spells.
    pub async fn list_enabled(&self) -> Vec<Spell> {
        self.inner
            .read()
            .await
            .values()
            .filter(|s| s.enabled)
            .cloned()
            .collect()
    }

    /// Enable a spell by name. Returns true if found.
    pub async fn enable(&self, name: &str) -> bool {
        if let Some(spell) = self.inner.write().await.get_mut(name) {
            spell.enabled = true;
            info!(spell = %name, "spell enabled");
            true
        } else {
            false
        }
    }

    /// Disable a spell by name. Returns true if found.
    pub async fn disable(&self, name: &str) -> bool {
        if let Some(spell) = self.inner.write().await.get_mut(name) {
            spell.enabled = false;
            info!(spell = %name, "spell disabled");
            true
        } else {
            false
        }
    }

    /// Get a spell by name.
    pub async fn get(&self, name: &str) -> Option<Spell> {
        self.inner.read().await.get(name).cloned()
    }

    /// Clear all registered spells.
    pub async fn clear(&self) {
        self.inner.write().await.clear();
    }

    /// Number of registered spells.
    pub async fn len(&self) -> usize {
        self.inner.read().await.len()
    }

    /// Whether there are no registered spells.
    pub async fn is_empty(&self) -> bool {
        self.inner.read().await.is_empty()
    }

    /// Build a system prompt fragment for all enabled spells.
    pub async fn system_prompt_fragment(&self) -> Option<String> {
        let enabled = self.list_enabled().await;
        if enabled.is_empty() {
            return None;
        }

        let mut fragment = String::from("\n\n## Available Spells\n\n");
        for spell in &enabled {
            fragment.push_str(&format!("### {}\n", spell.name));
            fragment.push_str(&format!("{}\n", spell.description));
            if let Some(binary) = &spell.binary_path {
                fragment.push_str(&format!("Binary: {}\n", binary.display()));
            }
            fragment.push('\n');
        }
        Some(fragment)
    }
}

impl Default for SpellRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ── Parsing helpers ─────────────────────────────────────────────────

/// Parse YAML frontmatter from SPELL.md / SKILL.md content.
///
/// Expects the file to start with `---\n` and end the frontmatter with `---\n`.
pub fn parse_spell_frontmatter(content: &str) -> Option<SpellFrontmatter> {
    let content = content.trim();
    if !content.starts_with("---") {
        return None;
    }

    let after_first = &content[3..];
    let end_pos = after_first.find("\n---")?;
    let yaml_block = &after_first[..end_pos].trim();

    serde_json::from_value(parse_yaml_to_json(yaml_block)?).ok()
}

/// Minimal YAML-subset parser for spell frontmatter.
/// Handles simple key: value pairs (strings, bools, lists, and JSON objects for parameters).
fn parse_yaml_to_json(yaml: &str) -> Option<serde_json::Value> {
    let mut map = serde_json::Map::new();

    for line in yaml.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let (key, value) = line.split_once(':')?;
        let key = key.trim().to_string();
        let value = value.trim();

        if value.is_empty() {
            map.insert(key, serde_json::Value::Null);
        } else if value == "true" || value == "false" {
            map.insert(
                key,
                serde_json::Value::Bool(value.parse::<bool>().unwrap_or(false)),
            );
        } else if value.starts_with('{') || value.starts_with('[') {
            if let Ok(parsed) = serde_json::from_str(value) {
                map.insert(key, parsed);
            } else {
                map.insert(key, serde_json::Value::String(value.to_string()));
            }
        } else {
            // Strip surrounding quotes if present
            let value = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .unwrap_or(value);
            map.insert(key, serde_json::Value::String(value.to_string()));
        }
    }

    Some(serde_json::Value::Object(map))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_frontmatter_basic() {
        let content = r#"---
name: my-spell
description: A test spell
enabled: true
---

# My Spell

Body content here.
"#;
        let fm = parse_spell_frontmatter(content).unwrap();
        assert_eq!(fm.name.as_deref(), Some("my-spell"));
        assert_eq!(fm.description.as_deref(), Some("A test spell"));
        assert_eq!(fm.enabled, Some(true));
    }

    #[test]
    fn parse_frontmatter_with_binary() {
        let content = r#"---
name: code-runner
description: Runs code
binary: ./run.sh
parameters: {"type": "object", "properties": {"code": {"type": "string"}}}
---
"#;
        let fm = parse_spell_frontmatter(content).unwrap();
        assert_eq!(fm.name.as_deref(), Some("code-runner"));
        assert_eq!(fm.binary.as_deref(), Some("./run.sh"));
        assert!(fm.parameters.is_some());
    }

    #[test]
    fn parse_frontmatter_accepts_quoted_values_and_comments() {
        let content = r#"---
# comment line
name: "quoted-spell"
description: "Quoted description"
enabled: false
---
"#;
        let fm = parse_spell_frontmatter(content).unwrap();
        assert_eq!(fm.name.as_deref(), Some("quoted-spell"));
        assert_eq!(fm.description.as_deref(), Some("Quoted description"));
        assert_eq!(fm.enabled, Some(false));
    }

    #[test]
    fn parse_frontmatter_rejects_missing_closing_delimiter() {
        let content = "---\nname: broken\ndescription: nope\n";
        assert!(parse_spell_frontmatter(content).is_none());
    }

    #[test]
    fn parse_frontmatter_with_spell_manifest_fields() {
        let content = r#"---
name: security-audit
namespace: horizon.security-audit
version: 0.1.0
kind: tool
description: Audits security posture
requires: ["network", "filesystem"]
tags: ["security", "audit"]
triggers: ["on:schedule:daily"]
---
"#;
        let fm = parse_spell_frontmatter(content).unwrap();
        assert_eq!(fm.namespace.as_deref(), Some("horizon.security-audit"));
        assert_eq!(fm.version.as_deref(), Some("0.1.0"));
        assert_eq!(fm.kind, Some(SpellKind::Tool));
        assert_eq!(fm.requires, vec!["network", "filesystem"]);
        assert_eq!(fm.tags, vec!["security", "audit"]);
        assert_eq!(fm.triggers, vec!["on:schedule:daily"]);
    }

    #[test]
    fn system_prompt_fragment_only_includes_enabled_spells() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let reg = SpellRegistry::new();
            reg.register(Spell {
                name: "enabled-spell".into(),
                description: "Visible to prompt".into(),
                parameters: serde_json::json!({}),
                binary_path: Some(PathBuf::from("/tmp/run-enabled")),
                source_dir: PathBuf::from("/tmp/enabled-spell"),
                enabled: true,
                prompt_body: None,
                model: None,
                allowed_tools: None,
                user_invocable: false,
                namespace: None,
                version: None,
                kind: SpellKind::default(),
                requires: vec![],
                tags: vec![],
                triggers: vec![],
            })
            .await;
            reg.register(Spell {
                name: "disabled-spell".into(),
                description: "Should stay hidden".into(),
                parameters: serde_json::json!({}),
                binary_path: None,
                source_dir: PathBuf::from("/tmp/disabled-spell"),
                enabled: false,
                prompt_body: None,
                model: None,
                allowed_tools: None,
                user_invocable: false,
                namespace: None,
                version: None,
                kind: SpellKind::default(),
                requires: vec![],
                tags: vec![],
                triggers: vec![],
            })
            .await;

            let fragment = reg.system_prompt_fragment().await.unwrap();
            assert!(fragment.contains("## Available Spells"));
            assert!(fragment.contains("enabled-spell"));
            assert!(fragment.contains("Visible to prompt"));
            assert!(fragment.contains("/tmp/run-enabled"));
            assert!(!fragment.contains("disabled-spell"));
            assert!(!fragment.contains("Should stay hidden"));
        });
    }

    #[tokio::test]
    async fn registry_crud() {
        let reg = SpellRegistry::new();

        let spell = Spell {
            name: "test".into(),
            description: "Test spell".into(),
            parameters: serde_json::json!({}),
            binary_path: None,
            source_dir: PathBuf::from("/tmp"),
            enabled: true,
            prompt_body: None,
            model: None,
            allowed_tools: None,
            user_invocable: false,
            namespace: None,
            version: None,
            kind: SpellKind::default(),
            requires: vec![],
            tags: vec![],
            triggers: vec![],
        };

        reg.register(spell).await;
        assert_eq!(reg.len().await, 1);
        assert_eq!(reg.list_enabled().await.len(), 1);

        reg.disable("test").await;
        assert_eq!(reg.list_enabled().await.len(), 0);

        reg.enable("test").await;
        assert_eq!(reg.list_enabled().await.len(), 1);

        reg.remove("test").await;
        assert_eq!(reg.len().await, 0);
    }
}
