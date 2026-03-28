//! Directory scanner and hot-reloader for SPELL.md-based spells.
//!
//! Supports both flat layout (`spells/<name>/SPELL.md`) and recursive
//! namespace layout (`spells/<ns>/<name>/SPELL.md`) per issue #300.
//! Also provides backward compatibility with legacy `SKILL.md` files (#299).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use tokio::fs;
use tracing::{debug, info, warn};

use crate::spell::{Spell, SpellRegistry, parse_spell_frontmatter};

/// Summary returned by a scan/reload pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpellScanSummary {
    pub discovered: usize,
    pub loaded: usize,
    pub removed: usize,
}

/// Scans a spells directory for `SPELL.md` files (recursively) and populates a [`SpellRegistry`].
///
/// Supports two layouts:
/// - **Flat**: `spells/<name>/SPELL.md`
/// - **Namespaced**: `spells/<ns-part>/<name>/SPELL.md` (arbitrary depth)
///
/// Also accepts legacy `SKILL.md` filenames for backward compatibility.
pub struct SpellLoader {
    /// Root directory to scan (e.g. `~/.rune/spells/`).
    spells_dir: PathBuf,
    /// Registry to populate.
    registry: Arc<SpellRegistry>,
}

impl SpellLoader {
    /// Create a new loader for the given directory and registry.
    pub fn new(spells_dir: impl Into<PathBuf>, registry: Arc<SpellRegistry>) -> Self {
        Self {
            spells_dir: spells_dir.into(),
            registry,
        }
    }

    /// Perform an initial scan of the spells directory.
    pub async fn scan(&self) -> usize {
        self.scan_summary().await.loaded
    }

    /// Reconcile the registry with on-disk spell files.
    ///
    /// Existing entries that no longer exist on disk are removed, changed spells are
    /// overwritten in-place, and newly discovered spells are added.
    pub async fn scan_summary(&self) -> SpellScanSummary {
        let start = Instant::now();

        if !self.spells_dir.exists() {
            debug!(dir = %self.spells_dir.display(), "spells directory does not exist, skipping scan");
            return SpellScanSummary {
                discovered: 0,
                loaded: 0,
                removed: self.remove_all_registry_entries().await,
            };
        }

        let mut discovered = 0;
        let mut loaded = 0;
        let mut seen_names = Vec::new();

        self.scan_dir_recursive(&self.spells_dir.clone(), &mut discovered, &mut loaded, &mut seen_names).await;

        let removed = self.remove_missing_registry_entries(&seen_names).await;
        let elapsed = start.elapsed();
        info!(
            discovered,
            loaded,
            removed,
            elapsed_us = elapsed.as_micros(),
            dir = %self.spells_dir.display(),
            "spells scan complete"
        );

        SpellScanSummary {
            discovered,
            loaded,
            removed,
        }
    }

    /// Recursively scan directories for SPELL.md / SKILL.md files.
    fn scan_dir_recursive<'a>(
        &'a self,
        dir: &'a Path,
        discovered: &'a mut usize,
        loaded: &'a mut usize,
        seen_names: &'a mut Vec<String>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            let mut entries = match fs::read_dir(dir).await {
                Ok(entries) => entries,
                Err(e) => {
                    warn!(error = %e, dir = %dir.display(), "failed to read spells directory");
                    return;
                }
            };

            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }

                // Check for SPELL.md first, then legacy SKILL.md
                let spell_md = path.join("SPELL.md");
                let skill_md = path.join("SKILL.md");
                let manifest_path = if spell_md.exists() {
                    spell_md
                } else if skill_md.exists() {
                    warn!(
                        path = %skill_md.display(),
                        "found legacy SKILL.md — rename to SPELL.md (SKILL.md support will be removed in a future release)"
                    );
                    skill_md
                } else {
                    // No manifest here — recurse deeper for namespace dirs
                    self.scan_dir_recursive(&path, discovered, loaded, seen_names).await;
                    continue;
                };

                *discovered += 1;

                // Derive namespace from path relative to spells root
                let rel = path.strip_prefix(&self.spells_dir).unwrap_or(&path);
                let dir_namespace = path_to_namespace(rel);

                match load_spell_from_path(&manifest_path, dir_namespace.as_deref()).await {
                    Ok(mut spell) => {
                        // Validate namespace matches directory structure
                        if let Some(ref ns) = spell.namespace {
                            let expected = dir_namespace.as_deref().unwrap_or("");
                            if ns != expected && !expected.is_empty() {
                                warn!(
                                    spell = %spell.name,
                                    declared_namespace = %ns,
                                    directory_namespace = %expected,
                                    "spell namespace does not match directory structure"
                                );
                            }
                        }

                        if let Some(existing) = self.registry.get(&spell.name).await {
                            spell.enabled = existing.enabled;
                        }

                        let ns_display = spell.namespace.as_deref().unwrap_or("-");
                        info!(
                            spell = %spell.name,
                            namespace = %ns_display,
                            kind = ?spell.kind,
                            "loaded spell"
                        );

                        seen_names.push(spell.name.clone());
                        self.registry.register(spell).await;
                        *loaded += 1;
                    }
                    Err(e) => {
                        warn!(
                            path = %manifest_path.display(),
                            error = %e,
                            "failed to load spell"
                        );
                    }
                }
            }
        })
    }

    /// Start a background file watcher that re-scans on changes.
    ///
    /// Uses a simple polling approach (checks every `interval` seconds).
    /// A production implementation would use `notify` crate for inotify/FSEvents.
    pub fn start_watcher(self: Arc<Self>, interval_secs: u64) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut poll_interval =
                tokio::time::interval(std::time::Duration::from_secs(interval_secs));

            loop {
                poll_interval.tick().await;
                self.scan().await;
            }
        })
    }

    /// Get the spells directory path.
    pub fn spells_dir(&self) -> &Path {
        &self.spells_dir
    }

    /// Backward-compat alias.
    pub fn skills_dir(&self) -> &Path {
        &self.spells_dir
    }

    async fn remove_missing_registry_entries(&self, seen_names: &[String]) -> usize {
        let current = self.registry.list().await;
        let mut removed = 0;
        for spell in current {
            if !seen_names.iter().any(|name| name == &spell.name)
                && self.registry.remove(&spell.name).await.is_some()
            {
                removed += 1;
            }
        }
        removed
    }

    async fn remove_all_registry_entries(&self) -> usize {
        let current = self.registry.list().await;
        let mut removed = 0;
        for spell in current {
            if self.registry.remove(&spell.name).await.is_some() {
                removed += 1;
            }
        }
        removed
    }
}

/// Convert a relative path like `horizon/security-audit` to a dotted namespace
/// like `"horizon.security-audit"`. Returns `None` for single-component paths
/// (the leaf directory is the spell name, not a namespace part).
fn path_to_namespace(rel: &Path) -> Option<String> {
    let components: Vec<&str> = rel
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();

    if components.len() <= 1 {
        // Flat layout: the single component is the spell name itself
        return None;
    }

    // Namespace is the parent path only; the leaf directory is the spell name, not part of the namespace
    Some(components[..components.len() - 1].join("."))
}

/// Load a single spell from a SPELL.md (or SKILL.md) file path.
async fn load_spell_from_path(path: &Path, dir_namespace: Option<&str>) -> Result<Spell, String> {
    let content = fs::read_to_string(path)
        .await
        .map_err(|e| format!("read error: {e}"))?;

    let frontmatter = parse_spell_frontmatter(&content)
        .ok_or_else(|| "no valid frontmatter found".to_string())?;

    let prompt_body = split_markdown_frontmatter(&content)
        .map(|(_, body)| body.trim().to_string())
        .filter(|body| !body.is_empty());

    let source_dir = path
        .parent()
        .ok_or_else(|| "no parent directory".to_string())?;

    // Derive name from frontmatter or directory name
    let dir_name = source_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    let name = frontmatter.name.unwrap_or_else(|| dir_name.to_string());

    let description = frontmatter
        .description
        .unwrap_or_else(|| format!("Spell: {name}"));

    if frontmatter.version.as_deref().is_none_or(str::is_empty) {
        return Err("missing required field: version".to_string());
    }

    let binary_path = frontmatter.binary.map(|b| {
        let bp = PathBuf::from(&b);
        if bp.is_relative() {
            source_dir.join(bp)
        } else {
            bp
        }
    });

    let parameters = frontmatter
        .parameters
        .unwrap_or_else(|| serde_json::json!({"type": "object", "properties": {}}));

    // Use frontmatter namespace or fall back to directory-derived namespace
    let namespace = frontmatter.namespace.or_else(|| dir_namespace.map(String::from));

    Ok(Spell {
        name,
        description,
        parameters,
        binary_path,
        source_dir: source_dir.to_path_buf(),
        enabled: frontmatter.enabled.unwrap_or(true),
        prompt_body,
        model: frontmatter.model,
        allowed_tools: frontmatter.allowed_tools,
        user_invocable: frontmatter.user_invocable.unwrap_or(false),
        namespace,
        version: frontmatter.version,
        author: frontmatter.author,
        kind: frontmatter.kind.unwrap_or_default(),
        requires: frontmatter.requires,
        tags: frontmatter.tags,
        match_rules: frontmatter.match_rules,
        triggers: frontmatter.triggers,
    })
}


fn split_markdown_frontmatter(content: &str) -> Option<(&str, &str)> {
    let trimmed = content.trim();
    if !trimmed.starts_with("---") {
        return None;
    }

    let after_first = &trimmed[3..];
    let end_pos = after_first.find("\n---")?;
    let yaml_block = after_first[..end_pos].trim();
    let body_start = end_pos + "\n---".len();
    let body = after_first[body_start..].trim_start_matches(['\r', '\n']);
    Some((yaml_block, body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spell::SpellKind;
    use tempfile::TempDir;

    #[tokio::test]
    async fn scan_spells_dir() {
        let tmp = TempDir::new().unwrap();

        // Create a spell directory with SPELL.md
        let spell_dir = tmp.path().join("my-spell");
        fs::create_dir_all(&spell_dir).await.unwrap();
        fs::write(
            spell_dir.join("SPELL.md"),
            r#"---
name: my-spell
version: 0.1.0
description: A test spell
---

# My Spell
"#,
        )
        .await
        .unwrap();

        // Create another without SPELL.md (should be skipped)
        let no_spell_dir = tmp.path().join("not-a-spell");
        fs::create_dir_all(&no_spell_dir).await.unwrap();

        let registry = Arc::new(SpellRegistry::new());
        let loader = SpellLoader::new(tmp.path(), registry.clone());

        let count = loader.scan().await;
        assert_eq!(count, 1);

        let spells = registry.list().await;
        assert_eq!(spells.len(), 1);
        assert_eq!(spells[0].name, "my-spell");
    }

    #[tokio::test]
    async fn scan_legacy_skill_md() {
        let tmp = TempDir::new().unwrap();

        let spell_dir = tmp.path().join("legacy");
        fs::create_dir_all(&spell_dir).await.unwrap();
        fs::write(
            spell_dir.join("SKILL.md"),
            r#"---
name: legacy-skill
version: 0.1.0
description: Uses old SKILL.md filename
---
"#,
        )
        .await
        .unwrap();

        let registry = Arc::new(SpellRegistry::new());
        let loader = SpellLoader::new(tmp.path(), registry.clone());

        let count = loader.scan().await;
        assert_eq!(count, 1);

        let spell = registry.get("legacy-skill").await.unwrap();
        assert_eq!(spell.description, "Uses old SKILL.md filename");
    }

    #[tokio::test]
    async fn scan_recursive_namespace() {
        let tmp = TempDir::new().unwrap();

        // Create namespaced spell: horizon/security-audit/SPELL.md
        let ns_dir = tmp.path().join("horizon").join("security-audit");
        fs::create_dir_all(&ns_dir).await.unwrap();
        fs::write(
            ns_dir.join("SPELL.md"),
            r#"---
name: security-audit
namespace: horizon.security-audit
description: Audits security posture
kind: tool
version: 0.1.0
---
"#,
        )
        .await
        .unwrap();

        let registry = Arc::new(SpellRegistry::new());
        let loader = SpellLoader::new(tmp.path(), registry.clone());

        let count = loader.scan().await;
        assert_eq!(count, 1);

        let spell = registry.get("security-audit").await.unwrap();
        assert_eq!(spell.namespace.as_deref(), Some("horizon.security-audit"));
        assert_eq!(spell.kind, SpellKind::Tool);
        assert_eq!(spell.version.as_deref(), Some("0.1.0"));
    }

    #[tokio::test]
    async fn scan_summary_removes_missing_spells_and_tracks_counts() {
        let tmp = TempDir::new().unwrap();
        let registry = Arc::new(SpellRegistry::new());
        let loader = SpellLoader::new(tmp.path(), registry.clone());

        let alpha_dir = tmp.path().join("alpha");
        fs::create_dir_all(&alpha_dir).await.unwrap();
        fs::write(
            alpha_dir.join("SPELL.md"),
            r#"---
name: alpha
version: 0.1.0
description: Alpha spell
---
"#,
        )
        .await
        .unwrap();

        let first = loader.scan_summary().await;
        assert_eq!(first.discovered, 1);
        assert_eq!(first.loaded, 1);
        assert_eq!(first.removed, 0);
        assert!(registry.get("alpha").await.is_some());

        fs::remove_dir_all(&alpha_dir).await.unwrap();

        let second = loader.scan_summary().await;
        assert_eq!(second.discovered, 0);
        assert_eq!(second.loaded, 0);
        assert_eq!(second.removed, 1);
        assert!(registry.get("alpha").await.is_none());
    }

    #[tokio::test]
    async fn scan_summary_keeps_valid_spells_when_one_is_invalid() {
        let tmp = TempDir::new().unwrap();
        let registry = Arc::new(SpellRegistry::new());
        let loader = SpellLoader::new(tmp.path(), registry.clone());

        let valid_dir = tmp.path().join("valid");
        fs::create_dir_all(&valid_dir).await.unwrap();
        fs::write(
            valid_dir.join("SPELL.md"),
            r#"---
name: valid
version: 0.1.0
description: Valid spell
binary: ./run.sh
parameters: {"type":"object","properties":{"path":{"type":"string"}}}
enabled: true
---
"#,
        )
        .await
        .unwrap();

        let invalid_dir = tmp.path().join("invalid");
        fs::create_dir_all(&invalid_dir).await.unwrap();
        fs::write(
            invalid_dir.join("SPELL.md"),
            "name: invalid without frontmatter",
        )
        .await
        .unwrap();

        let summary = loader.scan_summary().await;
        assert_eq!(summary.discovered, 2);
        assert_eq!(summary.loaded, 1);
        assert_eq!(summary.removed, 0);

        let spell = registry.get("valid").await.expect("valid spell loaded");
        assert_eq!(spell.description, "Valid spell");
        assert_eq!(spell.binary_path, Some(valid_dir.join("run.sh")));
        assert!(registry.get("invalid").await.is_none());
    }

    #[tokio::test]
    async fn scan_loads_prompt_body_and_tool_metadata() {
        let tmp = TempDir::new().unwrap();
        let registry = Arc::new(SpellRegistry::new());
        let loader = SpellLoader::new(tmp.path(), registry.clone());

        let spell_dir = tmp.path().join("shell-runner");
        fs::create_dir_all(&spell_dir).await.unwrap();
        fs::write(
            spell_dir.join("SPELL.md"),
            r#"---
name: shell-runner
version: 0.2.0
description: Runs shell commands
model: gpt-4.1-mini
allowed-tools: ["exec", "read"]
user-invocable: true
---

# Shell Runner

Run shell commands carefully.
"#,
        )
        .await
        .unwrap();

        let summary = loader.scan_summary().await;
        assert_eq!(summary.loaded, 1);

        let spell = registry.get("shell-runner").await.expect("spell loaded");
        assert_eq!(spell.model.as_deref(), Some("gpt-4.1-mini"));
        assert_eq!(spell.allowed_tools, Some(vec!["exec".into(), "read".into()]));
        assert!(spell.user_invocable);
        assert!(spell
            .prompt_body
            .as_deref()
            .unwrap_or_default()
            .contains("Run shell commands carefully."));
    }

    #[tokio::test]
    async fn scan_preserves_runtime_enabled_state_on_reload() {
        let tmp = TempDir::new().unwrap();
        let registry = Arc::new(SpellRegistry::new());
        let loader = SpellLoader::new(tmp.path(), registry.clone());

        let spell_dir = tmp.path().join("sticky-spell");
        fs::create_dir_all(&spell_dir).await.unwrap();
        fs::write(
            spell_dir.join("SPELL.md"),
            r#"---
name: sticky-spell
version: 0.1.0
description: Sticky spell
enabled: true
---
"#,
        )
        .await
        .unwrap();

        let first = loader.scan_summary().await;
        assert_eq!(first.loaded, 1);
        assert!(registry.get("sticky-spell").await.unwrap().enabled);

        assert!(registry.disable("sticky-spell").await);
        assert!(!registry.get("sticky-spell").await.unwrap().enabled);

        let second = loader.scan_summary().await;
        assert_eq!(second.loaded, 1);
        assert!(!registry.get("sticky-spell").await.unwrap().enabled);
    }


    #[tokio::test]
    async fn scan_rejects_spell_missing_required_version() {
        let tmp = TempDir::new().unwrap();

        let spell_dir = tmp.path().join("missing-version");
        fs::create_dir_all(&spell_dir).await.unwrap();
        fs::write(
            spell_dir.join("SPELL.md"),
            r#"---
name: missing-version
description: Missing version should fail
---
"#,
        )
        .await
        .unwrap();

        let registry = Arc::new(SpellRegistry::new());
        let loader = SpellLoader::new(tmp.path(), registry.clone());

        let summary = loader.scan_summary().await;
        assert_eq!(summary.discovered, 1);
        assert_eq!(summary.loaded, 0);
        assert_eq!(summary.removed, 0);
        assert!(registry.get("missing-version").await.is_none());
    }

    #[test]
    fn path_to_namespace_flat() {
        let ns = path_to_namespace(Path::new("my-spell"));
        assert_eq!(ns, None);
    }

    #[test]
    fn path_to_namespace_nested() {
        let ns = path_to_namespace(Path::new("horizon/security-audit"));
        assert_eq!(ns.as_deref(), Some("horizon"));
    }

    #[test]
    fn path_to_namespace_deep() {
        let ns = path_to_namespace(Path::new("org/team/my-spell"));
        assert_eq!(ns.as_deref(), Some("org.team"));
    }
}
