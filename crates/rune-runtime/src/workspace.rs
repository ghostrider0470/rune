//! Workspace context loader for session startup.
//!
//! Loads configurable workspace files (AGENTS.md, SOUL.md, USER.md, TOOLS.md, etc.)
//! and formats them for system prompt injection. Implements the OpenClaw convention
//! of reading workspace context on session startup.

use std::path::PathBuf;

use rune_core::SessionKind;
use tracing::{debug, warn};

/// Loaded workspace context ready for prompt injection.
#[derive(Clone, Debug, Default)]
pub struct WorkspaceContext {
    /// Loaded files as (filename, content) pairs.
    pub files: Vec<(String, String)>,
}

impl WorkspaceContext {
    /// Format for system prompt injection.
    pub fn format_for_prompt(&self) -> String {
        if self.files.is_empty() {
            return String::new();
        }

        let mut out = String::from(
            "# Project Context\n\nThe following project context files have been loaded:\n",
        );

        for (name, content) in &self.files {
            out.push_str(&format!("\n## {name}\n{content}\n"));
        }

        out
    }

    /// Check if a specific file was loaded.
    pub fn has_file(&self, name: &str) -> bool {
        self.files.iter().any(|(n, _)| n == name)
    }
}

/// Loads workspace context files from the workspace root.
pub struct WorkspaceLoader {
    workspace_root: PathBuf,
    /// Files to load, in order. Filenames relative to workspace root.
    files_to_load: Vec<String>,
}

impl WorkspaceLoader {
    /// Create with default OpenClaw-compatible file list for a session kind.
    pub fn new(workspace_root: impl Into<PathBuf>, session_kind: SessionKind) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            files_to_load: default_files_for_session(session_kind),
        }
    }

    /// Create with a custom file list.
    pub fn with_files(workspace_root: impl Into<PathBuf>, files: Vec<String>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            files_to_load: files,
        }
    }

    /// Load all configured workspace files.
    pub async fn load(&self) -> WorkspaceContext {
        let mut ctx = WorkspaceContext::default();

        for filename in &self.files_to_load {
            let path = self.workspace_root.join(filename);
            match tokio::fs::read_to_string(&path).await {
                Ok(content) if !content.trim().is_empty() => {
                    debug!(file = %filename, "loaded workspace file");
                    ctx.files.push((filename.clone(), content));
                }
                Ok(_) => {
                    debug!(file = %filename, "workspace file empty, skipping");
                }
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        debug!(file = %filename, "workspace file not found, skipping");
                    } else {
                        warn!(file = %filename, error = %e, "failed to read workspace file");
                    }
                }
            }
        }

        ctx
    }
}

fn default_files_for_session(session_kind: SessionKind) -> Vec<String> {
    let mut files = vec![
        "AGENTS.md".into(),
        "SOUL.md".into(),
        "USER.md".into(),
        "TOOLS.md".into(),
        "IDENTITY.md".into(),
        "ROADMAP.md".into(),
    ];

    // Direct (main) sessions get MEMORY.md for long-term continuity
    if matches!(session_kind, SessionKind::Direct) {
        files.push("MEMORY.md".into());
        files.push("memory/lessons.md".into());
        files.push("agent-orchestration-runbook.md".into());
    }

    // Scheduled sessions get MEMORY.md + heartbeat prompt
    if matches!(session_kind, SessionKind::Scheduled) {
        files.push("MEMORY.md".into());
        files.push("HEARTBEAT.md".into());
    }

    // All session types get today's and yesterday's daily notes
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    files.push(format!("memory/{today}.md"));
    if let Some(yesterday) = chrono::Local::now()
        .date_naive()
        .pred_opt()
    {
        files.push(format!("memory/{}.md", yesterday.format("%Y-%m-%d")));
    }

    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn setup() -> TempDir {
        let tmp = TempDir::new().unwrap();
        tokio::fs::write(tmp.path().join("AGENTS.md"), "# Agents\nBe helpful.")
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("SOUL.md"), "# Soul\nBe genuine.")
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("USER.md"), "# User\nName: Hamza")
            .await
            .unwrap();
        tmp
    }

    #[tokio::test]
    async fn loads_existing_files() {
        let tmp = setup().await;
        let loader = WorkspaceLoader::new(tmp.path(), SessionKind::Direct);
        let ctx = loader.load().await;

        assert!(ctx.has_file("AGENTS.md"));
        assert!(ctx.has_file("SOUL.md"));
        assert!(ctx.has_file("USER.md"));
        assert!(!ctx.has_file("TOOLS.md")); // not created
    }

    #[tokio::test]
    async fn skips_missing_files_gracefully() {
        let tmp = TempDir::new().unwrap();
        let loader = WorkspaceLoader::new(tmp.path(), SessionKind::Direct);
        let ctx = loader.load().await;

        assert!(ctx.files.is_empty());
    }

    #[tokio::test]
    async fn format_includes_file_content() {
        let tmp = setup().await;
        let loader = WorkspaceLoader::new(tmp.path(), SessionKind::Direct);
        let ctx = loader.load().await;

        let formatted = ctx.format_for_prompt();
        assert!(formatted.contains("AGENTS.md"));
        assert!(formatted.contains("Be helpful"));
        assert!(formatted.contains("SOUL.md"));
    }

    #[tokio::test]
    async fn custom_files() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::write(tmp.path().join("CUSTOM.md"), "custom content")
            .await
            .unwrap();

        let loader = WorkspaceLoader::with_files(tmp.path(), vec!["CUSTOM.md".into()]);
        let ctx = loader.load().await;

        assert!(ctx.has_file("CUSTOM.md"));
        assert_eq!(ctx.files.len(), 1);
    }

    #[tokio::test]
    async fn empty_context_formats_empty() {
        let ctx = WorkspaceContext::default();
        assert!(ctx.format_for_prompt().is_empty());
    }

    #[tokio::test]
    async fn heartbeat_file_only_loaded_for_scheduled_sessions() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::write(tmp.path().join("HEARTBEAT.md"), "check inbox")
            .await
            .unwrap();

        let direct = WorkspaceLoader::new(tmp.path(), SessionKind::Direct)
            .load()
            .await;
        let scheduled = WorkspaceLoader::new(tmp.path(), SessionKind::Scheduled)
            .load()
            .await;

        assert!(!direct.has_file("HEARTBEAT.md"));
        assert!(scheduled.has_file("HEARTBEAT.md"));
    }
}
