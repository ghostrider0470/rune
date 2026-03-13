//! Memory loading and privacy boundaries for context injection.
//!
//! Implements OpenClaw-compatible memory conventions:
//! - MEMORY.md: long-term curated memory (main session only)
//! - memory/*.md: daily notes (always available)
//! - Privacy boundary: MEMORY.md must NOT be loaded in shared/group contexts

use std::path::PathBuf;

use chrono::{NaiveDate, Utc};
use rune_core::SessionKind;
use tracing::{debug, warn};

/// Memory context that can be injected into prompts.
#[derive(Clone, Debug, Default)]
pub struct MemoryContext {
    /// Long-term memory content (from MEMORY.md).
    pub long_term: Option<String>,
    /// Today's daily notes.
    pub today: Option<String>,
    /// Yesterday's daily notes.
    pub yesterday: Option<String>,
}

impl MemoryContext {
    /// Format memory context for injection into the system prompt.
    pub fn format_for_prompt(&self) -> String {
        let mut parts = Vec::new();

        if let Some(lt) = &self.long_term {
            if !lt.trim().is_empty() {
                parts.push(format!("## Long-term Memory\n{lt}"));
            }
        }

        if let Some(today) = &self.today {
            if !today.trim().is_empty() {
                parts.push(format!("## Today's Notes\n{today}"));
            }
        }

        if let Some(yesterday) = &self.yesterday {
            if !yesterday.trim().is_empty() {
                parts.push(format!("## Yesterday's Notes\n{yesterday}"));
            }
        }

        if parts.is_empty() {
            String::new()
        } else {
            format!("# Memory Context\n\n{}", parts.join("\n\n"))
        }
    }
}

/// Loads memory files from the workspace with privacy boundaries.
pub struct MemoryLoader {
    workspace_root: PathBuf,
}

impl MemoryLoader {
    /// Create a new memory loader for the given workspace.
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
        }
    }

    /// Load memory context with privacy boundaries.
    ///
    /// - `SessionKind::Direct`: loads MEMORY.md + daily notes (full access)
    /// - `SessionKind::Channel`, `Subagent`, `Scheduled`: daily notes only (MEMORY.md excluded)
    pub async fn load(&self, session_kind: SessionKind) -> MemoryContext {
        let mut ctx = MemoryContext::default();

        // Privacy boundary: MEMORY.md only for direct (main) sessions
        if session_kind == SessionKind::Direct {
            ctx.long_term = self.read_file("MEMORY.md").await;
            if ctx.long_term.is_some() {
                debug!("loaded MEMORY.md for direct session");
            }
        } else {
            debug!(
                kind = ?session_kind,
                "skipping MEMORY.md load (privacy boundary)"
            );
        }

        // Daily notes: always available
        let today = Utc::now().date_naive();
        let yesterday = today.pred_opt().unwrap_or(today);

        ctx.today = self.read_daily(today).await;
        ctx.yesterday = self.read_daily(yesterday).await;

        ctx
    }

    async fn read_file(&self, relative: &str) -> Option<String> {
        let path = self.workspace_root.join(relative);
        match tokio::fs::read_to_string(&path).await {
            Ok(content) if !content.trim().is_empty() => Some(content),
            Ok(_) => None,
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    warn!(path = %path.display(), error = %e, "failed to read memory file");
                }
                None
            }
        }
    }

    async fn read_daily(&self, date: NaiveDate) -> Option<String> {
        let filename = format!("memory/{}.md", date.format("%Y-%m-%d"));
        self.read_file(&filename).await
    }
}

/// Check if a session kind has access to long-term memory.
pub fn has_memory_access(kind: SessionKind) -> bool {
    matches!(kind, SessionKind::Direct)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn setup_workspace() -> TempDir {
        let tmp = TempDir::new().unwrap();

        tokio::fs::write(
            tmp.path().join("MEMORY.md"),
            "# Memory\n\n- Prefers dark mode\n- Works at Horizon Tech\n",
        )
        .await
        .unwrap();

        tokio::fs::create_dir_all(tmp.path().join("memory"))
            .await
            .unwrap();

        let today = Utc::now().date_naive();
        tokio::fs::write(
            tmp.path()
                .join(format!("memory/{}.md", today.format("%Y-%m-%d"))),
            "# Today\n\n- Working on Rune\n",
        )
        .await
        .unwrap();

        tmp
    }

    #[tokio::test]
    async fn direct_session_loads_all_memory() {
        let tmp = setup_workspace().await;
        let loader = MemoryLoader::new(tmp.path());

        let ctx = loader.load(SessionKind::Direct).await;
        assert!(ctx.long_term.is_some());
        assert!(ctx.long_term.unwrap().contains("dark mode"));
        assert!(ctx.today.is_some());
    }

    #[tokio::test]
    async fn channel_session_excludes_long_term() {
        let tmp = setup_workspace().await;
        let loader = MemoryLoader::new(tmp.path());

        let ctx = loader.load(SessionKind::Channel).await;
        assert!(
            ctx.long_term.is_none(),
            "MEMORY.md should be excluded for channel sessions"
        );
        assert!(ctx.today.is_some());
    }

    #[tokio::test]
    async fn subagent_session_excludes_long_term() {
        let tmp = setup_workspace().await;
        let loader = MemoryLoader::new(tmp.path());

        let ctx = loader.load(SessionKind::Subagent).await;
        assert!(ctx.long_term.is_none());
    }

    #[tokio::test]
    async fn format_includes_all_sections() {
        let ctx = MemoryContext {
            long_term: Some("long term content".into()),
            today: Some("today content".into()),
            yesterday: Some("yesterday content".into()),
        };

        let formatted = ctx.format_for_prompt();
        assert!(formatted.contains("Long-term Memory"));
        assert!(formatted.contains("Today's Notes"));
        assert!(formatted.contains("Yesterday's Notes"));
    }

    #[tokio::test]
    async fn format_empty_returns_empty() {
        let ctx = MemoryContext::default();
        assert!(ctx.format_for_prompt().is_empty());
    }

    #[tokio::test]
    async fn missing_workspace_returns_empty_context() {
        let loader = MemoryLoader::new("/nonexistent/path");
        let ctx = loader.load(SessionKind::Direct).await;
        assert!(ctx.long_term.is_none());
        assert!(ctx.today.is_none());
    }

    #[test]
    fn memory_access_boundary() {
        assert!(has_memory_access(SessionKind::Direct));
        assert!(!has_memory_access(SessionKind::Channel));
        assert!(!has_memory_access(SessionKind::Subagent));
        assert!(!has_memory_access(SessionKind::Scheduled));
    }
}
