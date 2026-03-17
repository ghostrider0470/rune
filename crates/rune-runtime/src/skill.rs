//! Dynamic skill registry for hot-reloading SKILL.md-defined tools.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, info};

/// A single skill parsed from a SKILL.md file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Skill {
    /// Unique skill name (derived from directory name or frontmatter).
    pub name: String,
    /// Human-readable description for the model.
    pub description: String,
    /// JSON Schema for parameters (injected into tool definition).
    pub parameters: serde_json::Value,
    /// Optional path to a binary or script that implements the skill.
    pub binary_path: Option<PathBuf>,
    /// The directory containing the SKILL.md file.
    pub source_dir: PathBuf,
    /// Whether this skill is currently enabled.
    pub enabled: bool,
}

/// YAML frontmatter parsed from a SKILL.md file.
#[derive(Clone, Debug, Deserialize)]
pub struct SkillFrontmatter {
    pub name: Option<String>,
    pub description: Option<String>,
    pub binary: Option<String>,
    pub parameters: Option<serde_json::Value>,
    pub enabled: Option<bool>,
}

/// Thread-safe dynamic skill registry with add/remove/list/toggle.
#[derive(Clone)]
pub struct SkillRegistry {
    inner: Arc<RwLock<HashMap<String, Skill>>>,
}

impl SkillRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register or update a skill.
    pub async fn register(&self, skill: Skill) {
        let name = skill.name.clone();
        self.inner.write().await.insert(name.clone(), skill);
        debug!(skill = %name, "skill registered");
    }

    /// Remove a skill by name.
    pub async fn remove(&self, name: &str) -> Option<Skill> {
        let removed = self.inner.write().await.remove(name);
        if removed.is_some() {
            debug!(skill = %name, "skill removed");
        }
        removed
    }

    /// List all skills (enabled and disabled).
    pub async fn list(&self) -> Vec<Skill> {
        self.inner.read().await.values().cloned().collect()
    }

    /// List only enabled skills.
    pub async fn list_enabled(&self) -> Vec<Skill> {
        self.inner
            .read()
            .await
            .values()
            .filter(|s| s.enabled)
            .cloned()
            .collect()
    }

    /// Enable a skill by name. Returns true if found.
    pub async fn enable(&self, name: &str) -> bool {
        if let Some(skill) = self.inner.write().await.get_mut(name) {
            skill.enabled = true;
            info!(skill = %name, "skill enabled");
            true
        } else {
            false
        }
    }

    /// Disable a skill by name. Returns true if found.
    pub async fn disable(&self, name: &str) -> bool {
        if let Some(skill) = self.inner.write().await.get_mut(name) {
            skill.enabled = false;
            info!(skill = %name, "skill disabled");
            true
        } else {
            false
        }
    }

    /// Get a skill by name.
    pub async fn get(&self, name: &str) -> Option<Skill> {
        self.inner.read().await.get(name).cloned()
    }

    /// Number of registered skills.
    pub async fn len(&self) -> usize {
        self.inner.read().await.len()
    }

    /// Whether there are no registered skills.
    pub async fn is_empty(&self) -> bool {
        self.inner.read().await.is_empty()
    }

    /// Build a system prompt fragment for all enabled skills.
    pub async fn system_prompt_fragment(&self) -> Option<String> {
        let enabled = self.list_enabled().await;
        if enabled.is_empty() {
            return None;
        }

        let mut fragment = String::from("\n\n## Available Skills\n\n");
        for skill in &enabled {
            fragment.push_str(&format!("### {}\n", skill.name));
            fragment.push_str(&format!("{}\n", skill.description));
            if let Some(binary) = &skill.binary_path {
                fragment.push_str(&format!("Binary: {}\n", binary.display()));
            }
            fragment.push('\n');
        }
        Some(fragment)
    }
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse YAML frontmatter from SKILL.md content.
///
/// Expects the file to start with `---\n` and end the frontmatter with `---\n`.
pub fn parse_skill_frontmatter(content: &str) -> Option<SkillFrontmatter> {
    let content = content.trim();
    if !content.starts_with("---") {
        return None;
    }

    let after_first = &content[3..];
    let end_pos = after_first.find("\n---")?;
    let yaml_block = &after_first[..end_pos].trim();

    serde_json::from_value(parse_yaml_to_json(yaml_block)?).ok()
}

/// Minimal YAML-subset parser for skill frontmatter.
/// Handles simple key: value pairs (strings, bools, and JSON objects for parameters).
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
name: my-skill
description: A test skill
enabled: true
---

# My Skill

Body content here.
"#;
        let fm = parse_skill_frontmatter(content).unwrap();
        assert_eq!(fm.name.as_deref(), Some("my-skill"));
        assert_eq!(fm.description.as_deref(), Some("A test skill"));
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
        let fm = parse_skill_frontmatter(content).unwrap();
        assert_eq!(fm.name.as_deref(), Some("code-runner"));
        assert_eq!(fm.binary.as_deref(), Some("./run.sh"));
        assert!(fm.parameters.is_some());
    }

    #[test]
    fn parse_frontmatter_accepts_quoted_values_and_comments() {
        let content = r#"---
# comment line
name: "quoted-skill"
description: "Quoted description"
enabled: false
---
"#;
        let fm = parse_skill_frontmatter(content).unwrap();
        assert_eq!(fm.name.as_deref(), Some("quoted-skill"));
        assert_eq!(fm.description.as_deref(), Some("Quoted description"));
        assert_eq!(fm.enabled, Some(false));
    }

    #[test]
    fn parse_frontmatter_rejects_missing_closing_delimiter() {
        let content = "---\nname: broken\ndescription: nope\n";
        assert!(parse_skill_frontmatter(content).is_none());
    }

    #[test]
    fn system_prompt_fragment_only_includes_enabled_skills() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let reg = SkillRegistry::new();
            reg.register(Skill {
                name: "enabled-skill".into(),
                description: "Visible to prompt".into(),
                parameters: serde_json::json!({}),
                binary_path: Some(PathBuf::from("/tmp/run-enabled")),
                source_dir: PathBuf::from("/tmp/enabled-skill"),
                enabled: true,
            })
            .await;
            reg.register(Skill {
                name: "disabled-skill".into(),
                description: "Should stay hidden".into(),
                parameters: serde_json::json!({}),
                binary_path: None,
                source_dir: PathBuf::from("/tmp/disabled-skill"),
                enabled: false,
            })
            .await;

            let fragment = reg.system_prompt_fragment().await.unwrap();
            assert!(fragment.contains("## Available Skills"));
            assert!(fragment.contains("enabled-skill"));
            assert!(fragment.contains("Visible to prompt"));
            assert!(fragment.contains("/tmp/run-enabled"));
            assert!(!fragment.contains("disabled-skill"));
            assert!(!fragment.contains("Should stay hidden"));
        });
    }

    #[tokio::test]
    async fn registry_crud() {
        let reg = SkillRegistry::new();

        let skill = Skill {
            name: "test".into(),
            description: "Test skill".into(),
            parameters: serde_json::json!({}),
            binary_path: None,
            source_dir: PathBuf::from("/tmp"),
            enabled: true,
        };

        reg.register(skill).await;
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
