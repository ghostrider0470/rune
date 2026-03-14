//! Directory scanner and hot-reloader for SKILL.md-based skills.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use tokio::fs;
use tracing::{debug, info, warn};

use crate::skill::{Skill, SkillRegistry, parse_skill_frontmatter};

/// Summary returned by a scan/reload pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkillScanSummary {
    pub discovered: usize,
    pub loaded: usize,
    pub removed: usize,
}

/// Scans a skills directory for `*/SKILL.md` files and populates a [`SkillRegistry`].
pub struct SkillLoader {
    /// Root directory to scan (e.g. `~/.rune/skills/`).
    skills_dir: PathBuf,
    /// Registry to populate.
    registry: Arc<SkillRegistry>,
}

impl SkillLoader {
    /// Create a new loader for the given directory and registry.
    pub fn new(skills_dir: impl Into<PathBuf>, registry: Arc<SkillRegistry>) -> Self {
        Self {
            skills_dir: skills_dir.into(),
            registry,
        }
    }

    /// Perform an initial scan of the skills directory.
    pub async fn scan(&self) -> usize {
        self.scan_summary().await.loaded
    }

    /// Reconcile the registry with on-disk `*/SKILL.md` files.
    ///
    /// Existing entries that no longer exist on disk are removed, changed skills are
    /// overwritten in-place, and newly discovered skills are added.
    pub async fn scan_summary(&self) -> SkillScanSummary {
        let start = Instant::now();

        if !self.skills_dir.exists() {
            debug!(dir = %self.skills_dir.display(), "skills directory does not exist, skipping scan");
            return SkillScanSummary {
                discovered: 0,
                loaded: 0,
                removed: self.remove_all_registry_entries().await,
            };
        }

        let mut discovered = 0;
        let mut loaded = 0;
        let mut seen_names = Vec::new();
        let mut entries = match fs::read_dir(&self.skills_dir).await {
            Ok(entries) => entries,
            Err(e) => {
                warn!(error = %e, dir = %self.skills_dir.display(), "failed to read skills directory");
                return SkillScanSummary {
                    discovered: 0,
                    loaded: 0,
                    removed: 0,
                };
            }
        };

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let skill_md = path.join("SKILL.md");
            if !skill_md.exists() {
                continue;
            }

            discovered += 1;
            match load_skill_from_path(&skill_md).await {
                Ok(mut skill) => {
                    if let Some(existing) = self.registry.get(&skill.name).await {
                        skill.enabled = existing.enabled;
                    }
                    seen_names.push(skill.name.clone());
                    self.registry.register(skill).await;
                    loaded += 1;
                }
                Err(e) => {
                    warn!(
                        path = %skill_md.display(),
                        error = %e,
                        "failed to load skill"
                    );
                }
            }
        }

        let removed = self.remove_missing_registry_entries(&seen_names).await;
        let elapsed = start.elapsed();
        info!(
            discovered,
            loaded,
            removed,
            elapsed_us = elapsed.as_micros(),
            dir = %self.skills_dir.display(),
            "skills scan complete"
        );

        SkillScanSummary {
            discovered,
            loaded,
            removed,
        }
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

    /// Get the skills directory path.
    pub fn skills_dir(&self) -> &Path {
        &self.skills_dir
    }

    async fn remove_missing_registry_entries(&self, seen_names: &[String]) -> usize {
        let current = self.registry.list().await;
        let mut removed = 0;
        for skill in current {
            if !seen_names.iter().any(|name| name == &skill.name) {
                if self.registry.remove(&skill.name).await.is_some() {
                    removed += 1;
                }
            }
        }
        removed
    }

    async fn remove_all_registry_entries(&self) -> usize {
        let current = self.registry.list().await;
        let mut removed = 0;
        for skill in current {
            if self.registry.remove(&skill.name).await.is_some() {
                removed += 1;
            }
        }
        removed
    }
}

/// Load a single skill from a SKILL.md file path.
async fn load_skill_from_path(path: &Path) -> Result<Skill, String> {
    let content = fs::read_to_string(path)
        .await
        .map_err(|e| format!("read error: {e}"))?;

    let frontmatter = parse_skill_frontmatter(&content)
        .ok_or_else(|| "no valid frontmatter found".to_string())?;

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
        .unwrap_or_else(|| format!("Skill: {name}"));

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

    Ok(Skill {
        name,
        description,
        parameters,
        binary_path,
        source_dir: source_dir.to_path_buf(),
        enabled: frontmatter.enabled.unwrap_or(true),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn scan_skills_dir() {
        let tmp = TempDir::new().unwrap();

        // Create a skill directory
        let skill_dir = tmp.path().join("my-skill");
        fs::create_dir_all(&skill_dir).await.unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            r#"---
name: my-skill
description: A test skill
---

# My Skill
"#,
        )
        .await
        .unwrap();

        // Create another without SKILL.md (should be skipped)
        let no_skill_dir = tmp.path().join("not-a-skill");
        fs::create_dir_all(&no_skill_dir).await.unwrap();

        let registry = Arc::new(SkillRegistry::new());
        let loader = SkillLoader::new(tmp.path(), registry.clone());

        let count = loader.scan().await;
        assert_eq!(count, 1);

        let skills = registry.list().await;
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "my-skill");
    }

    #[tokio::test]
    async fn scan_summary_removes_missing_skills_and_tracks_counts() {
        let tmp = TempDir::new().unwrap();
        let registry = Arc::new(SkillRegistry::new());
        let loader = SkillLoader::new(tmp.path(), registry.clone());

        let alpha_dir = tmp.path().join("alpha");
        fs::create_dir_all(&alpha_dir).await.unwrap();
        fs::write(
            alpha_dir.join("SKILL.md"),
            r#"---
name: alpha
description: Alpha skill
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
    async fn scan_summary_keeps_valid_skills_when_one_skill_is_invalid() {
        let tmp = TempDir::new().unwrap();
        let registry = Arc::new(SkillRegistry::new());
        let loader = SkillLoader::new(tmp.path(), registry.clone());

        let valid_dir = tmp.path().join("valid");
        fs::create_dir_all(&valid_dir).await.unwrap();
        fs::write(
            valid_dir.join("SKILL.md"),
            r#"---
name: valid
description: Valid skill
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
            invalid_dir.join("SKILL.md"),
            "name: invalid without frontmatter",
        )
        .await
        .unwrap();

        let summary = loader.scan_summary().await;
        assert_eq!(summary.discovered, 2);
        assert_eq!(summary.loaded, 1);
        assert_eq!(summary.removed, 0);

        let skill = registry.get("valid").await.expect("valid skill loaded");
        assert_eq!(skill.description, "Valid skill");
        assert_eq!(skill.binary_path, Some(valid_dir.join("run.sh")));
        assert_eq!(
            skill.parameters,
            serde_json::json!({"type":"object","properties":{"path":{"type":"string"}}})
        );
        assert!(registry.get("invalid").await.is_none());
    }

    #[tokio::test]
    async fn scan_preserves_runtime_enabled_state_on_reload() {
        let tmp = TempDir::new().unwrap();
        let registry = Arc::new(SkillRegistry::new());
        let loader = SkillLoader::new(tmp.path(), registry.clone());

        let skill_dir = tmp.path().join("sticky-skill");
        fs::create_dir_all(&skill_dir).await.unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            r#"---
name: sticky-skill
description: Sticky skill
enabled: true
---
"#,
        )
        .await
        .unwrap();

        let first = loader.scan_summary().await;
        assert_eq!(first.loaded, 1);
        assert!(registry.get("sticky-skill").await.unwrap().enabled);

        assert!(registry.disable("sticky-skill").await);
        assert!(!registry.get("sticky-skill").await.unwrap().enabled);

        let second = loader.scan_summary().await;
        assert_eq!(second.loaded, 1);
        assert!(!registry.get("sticky-skill").await.unwrap().enabled);
    }
}
