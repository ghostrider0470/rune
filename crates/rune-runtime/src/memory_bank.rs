use std::path::PathBuf;

use chrono::Utc;
use tracing::warn;

/// Default relative path for the generated memory bank knowledge base.
pub const MEMORY_BANK_DIR: &str = ".rune/knowledge";
const ARCHITECTURE_FILE: &str = "ARCHITECTURE.md";
const DECISIONS_FILE: &str = "DECISIONS.md";
const CONVENTIONS_FILE: &str = "CONVENTIONS.md";
const DEPENDENCIES_FILE: &str = "DEPENDENCIES.md";

/// Workspace-backed memory bank context ready for prompt injection.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryBankContext {
    pub architecture: Option<String>,
    pub decisions: Option<String>,
    pub conventions: Option<String>,
    pub dependencies: Option<String>,
}

impl MemoryBankContext {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.architecture.is_none()
            && self.decisions.is_none()
            && self.conventions.is_none()
            && self.dependencies.is_none()
    }

    #[must_use]
    pub fn format_for_prompt(&self) -> String {
        let mut sections = Vec::new();

        if let Some(content) = &self.architecture {
            if !content.trim().is_empty() {
                sections.push(format!("## Architecture\n{content}"));
            }
        }
        if let Some(content) = &self.decisions {
            if !content.trim().is_empty() {
                sections.push(format!("## Decisions\n{content}"));
            }
        }
        if let Some(content) = &self.conventions {
            if !content.trim().is_empty() {
                sections.push(format!("## Conventions\n{content}"));
            }
        }
        if let Some(content) = &self.dependencies {
            if !content.trim().is_empty() {
                sections.push(format!("## Dependencies\n{content}"));
            }
        }

        if sections.is_empty() {
            String::new()
        } else {
            format!("# Memory Bank\n\n{}", sections.join("\n\n"))
        }
    }
}

/// Bootstraps and loads a workspace-local memory bank under `.rune/knowledge/`.
pub struct MemoryBankLoader {
    workspace_root: PathBuf,
}

impl MemoryBankLoader {
    #[must_use]
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
        }
    }

    #[must_use]
    pub fn knowledge_root(&self) -> PathBuf {
        self.workspace_root.join(MEMORY_BANK_DIR)
    }

    pub async fn ensure_seeded(&self) -> std::io::Result<()> {
        let root = self.knowledge_root();
        tokio::fs::create_dir_all(&root).await?;

        let now = Utc::now().date_naive();
        self.ensure_file(
            root.join(ARCHITECTURE_FILE),
            default_architecture_template(now),
        )
        .await?;
        self.ensure_file(root.join(DECISIONS_FILE), default_decisions_template(now))
            .await?;
        self.ensure_file(
            root.join(CONVENTIONS_FILE),
            default_conventions_template(now),
        )
        .await?;
        self.ensure_file(
            root.join(DEPENDENCIES_FILE),
            default_dependencies_template(now),
        )
        .await?;

        Ok(())
    }

    pub async fn load(&self) -> MemoryBankContext {
        if let Err(error) = self.ensure_seeded().await {
            warn!(
                path = %self.knowledge_root().display(),
                %error,
                "failed to seed memory bank"
            );
        }

        let root = self.knowledge_root();
        MemoryBankContext {
            architecture: read_optional(root.join(ARCHITECTURE_FILE)).await,
            decisions: read_optional(root.join(DECISIONS_FILE)).await,
            conventions: read_optional(root.join(CONVENTIONS_FILE)).await,
            dependencies: read_optional(root.join(DEPENDENCIES_FILE)).await,
        }
    }

    async fn ensure_file(&self, path: PathBuf, template: String) -> std::io::Result<()> {
        if tokio::fs::try_exists(&path).await? {
            return Ok(());
        }
        tokio::fs::write(path, template).await
    }
}

async fn read_optional(path: PathBuf) -> Option<String> {
    match tokio::fs::read_to_string(&path).await {
        Ok(content) if !content.trim().is_empty() => Some(content),
        Ok(_) => None,
        Err(error) => {
            if error.kind() != std::io::ErrorKind::NotFound {
                warn!(path = %path.display(), %error, "failed to read memory bank file");
            }
            None
        }
    }
}

fn default_architecture_template(date: chrono::NaiveDate) -> String {
    format!(
        "# ARCHITECTURE\n\nGenerated {date}.\n\n## Purpose\n- Describe the system's major runtime pieces and how they fit together.\n\n## Runtime Shape\n- Fill in the top-level crates/binaries that matter for this workspace.\n- Note critical execution flows, boundaries, and ownership.\n\n## Integration Notes\n- Record channel, storage, model, and UI integration points that future agents must preserve.\n"
    )
}

fn default_decisions_template(date: chrono::NaiveDate) -> String {
    format!(
        "# DECISIONS\n\nGenerated {date}.\n\n## ADR Log\n\n### {date} — Memory bank scaffold seeded\n- Context: Phase 25 requires a structured project knowledge base under `.rune/knowledge/`.\n- Decision: Seed canonical markdown files on first load instead of waiting for manual creation.\n- Consequences: Agents get a stable location for architectural notes immediately, and the repo can evolve richer automation on top of these files later.\n"
    )
}

fn default_conventions_template(date: chrono::NaiveDate) -> String {
    format!(
        "# CONVENTIONS\n\nGenerated {date}.\n\n## Working Rules\n- Document coding, review, testing, and release conventions that repeatedly matter in this repo.\n- Keep entries short, specific, and operational.\n\n## Prompt/Agent Conventions\n- Capture persistent agent workflow rules that should survive across sessions.\n"
    )
}

fn default_dependencies_template(date: chrono::NaiveDate) -> String {
    format!(
        "# DEPENDENCIES\n\nGenerated {date}.\n\n## Critical Dependencies\n- List key crates/services and why they exist.\n- Note any operational or security caveats future agents should know before changing them.\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    #[tokio::test]
    async fn ensure_seeded_creates_expected_files() {
        let tmp = TempDir::new().unwrap();
        let loader = MemoryBankLoader::new(tmp.path());

        loader.ensure_seeded().await.unwrap();

        for name in [
            ARCHITECTURE_FILE,
            DECISIONS_FILE,
            CONVENTIONS_FILE,
            DEPENDENCIES_FILE,
        ] {
            let path = loader.knowledge_root().join(name);
            assert!(tokio::fs::try_exists(path).await.unwrap());
        }
    }

    #[tokio::test]
    async fn load_returns_seeded_context() {
        let tmp = TempDir::new().unwrap();
        let loader = MemoryBankLoader::new(tmp.path());

        let context = loader.load().await;

        assert!(context.architecture.unwrap().contains("# ARCHITECTURE"));
        assert!(context.decisions.unwrap().contains("ADR Log"));
        assert!(context.conventions.unwrap().contains("# CONVENTIONS"));
        assert!(context.dependencies.unwrap().contains("# DEPENDENCIES"));
    }

    #[tokio::test]
    async fn ensure_seeded_does_not_overwrite_existing_files() {
        let tmp = TempDir::new().unwrap();
        let loader = MemoryBankLoader::new(tmp.path());
        let root = loader.knowledge_root();
        tokio::fs::create_dir_all(&root).await.unwrap();
        let architecture = root.join(ARCHITECTURE_FILE);
        tokio::fs::write(&architecture, "custom architecture").await.unwrap();

        loader.ensure_seeded().await.unwrap();

        let persisted = tokio::fs::read_to_string(architecture).await.unwrap();
        assert_eq!(persisted, "custom architecture");
    }

    #[test]
    fn format_for_prompt_includes_all_sections() {
        let context = MemoryBankContext {
            architecture: Some("arch".into()),
            decisions: Some("decisions".into()),
            conventions: Some("conventions".into()),
            dependencies: Some("dependencies".into()),
        };

        let prompt = context.format_for_prompt();
        assert!(prompt.contains("# Memory Bank"));
        assert!(prompt.contains("## Architecture\narch"));
        assert!(prompt.contains("## Decisions\ndecisions"));
        assert!(prompt.contains("## Conventions\nconventions"));
        assert!(prompt.contains("## Dependencies\ndependencies"));
    }

    #[test]
    fn empty_context_formats_to_empty_string() {
        assert!(MemoryBankContext::default().format_for_prompt().is_empty());
    }

    #[test]
    fn knowledge_root_uses_workspace_relative_directory() {
        let loader = MemoryBankLoader::new(Path::new("/tmp/workspace"));
        assert!(loader
            .knowledge_root()
            .ends_with(Path::new(".rune/knowledge")));
    }
}
