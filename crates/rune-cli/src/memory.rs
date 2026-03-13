//! Local file-oriented memory inspection helpers for the operator CLI.
//!
//! This intentionally mirrors Rune/OpenClaw's current workspace memory model:
//! `MEMORY.md` plus daily files under `memory/*.md`.

use std::path::{Path, PathBuf};

use crate::output::{
    MemoryGetResponse, MemorySearchHit, MemorySearchResponse, MemoryStatusResponse,
};
use anyhow::{Context, Result, bail};

fn workspace_root_from_config() -> PathBuf {
    let config = rune_config::AppConfig::load(None::<&std::path::Path>).unwrap_or_default();
    if let Some(default_agent) = config.agents.default_agent()
        && let Some(workspace) = config.agents.effective_workspace(default_agent)
    {
        return PathBuf::from(workspace);
    }

    if let Some(parent) = config.paths.memory_dir.parent() {
        return parent.to_path_buf();
    }

    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

async fn memory_files(workspace_root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    let memory_md = workspace_root.join("MEMORY.md");
    if memory_md.exists() {
        files.push(memory_md);
    }

    let memory_dir = workspace_root.join("memory");
    if memory_dir.is_dir() {
        let mut entries = tokio::fs::read_dir(&memory_dir)
            .await
            .with_context(|| format!("failed to read {}", memory_dir.display()))?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "md") {
                files.push(path);
            }
        }
    }

    files.sort();
    Ok(files)
}

fn ensure_memory_path(path: &str) -> Result<()> {
    let path = Path::new(path);
    let is_memory_md = path == Path::new("MEMORY.md");
    let is_daily_note = path.parent().is_some_and(|p| p == Path::new("memory"))
        && path.extension().is_some_and(|ext| ext == "md");

    if is_memory_md || is_daily_note {
        Ok(())
    } else {
        bail!("memory get only reads MEMORY.md or memory/*.md files");
    }
}

/// Inspect the configured workspace memory layout.
pub async fn status() -> Result<MemoryStatusResponse> {
    let config = rune_config::AppConfig::load(None::<&std::path::Path>).unwrap_or_default();
    let workspace_root = workspace_root_from_config();
    let files = memory_files(&workspace_root).await?;
    let latest_daily_file = files
        .iter()
        .filter_map(|path| {
            let rel = path.strip_prefix(&workspace_root).ok()?;
            if rel.parent().is_some_and(|p| p == Path::new("memory")) {
                Some(rel.display().to_string())
            } else {
                None
            }
        })
        .max();

    Ok(MemoryStatusResponse {
        workspace_root: workspace_root.display().to_string(),
        memory_dir: workspace_root.join("memory").display().to_string(),
        semantic_search_enabled: config.memory.semantic_search_enabled,
        long_term_exists: workspace_root.join("MEMORY.md").exists(),
        daily_file_count: files
            .iter()
            .filter(|path| {
                path.strip_prefix(&workspace_root)
                    .ok()
                    .and_then(|rel| rel.parent())
                    .is_some_and(|p| p == Path::new("memory"))
            })
            .count(),
        latest_daily_file,
    })
}

/// Search across `MEMORY.md` and `memory/*.md` using simple keyword matching.
pub async fn search(query: &str, max_results: usize) -> Result<MemorySearchResponse> {
    let workspace_root = workspace_root_from_config();
    let files = memory_files(&workspace_root).await?;
    let query_lower = query.to_lowercase();
    let query_words: Vec<&str> = query_lower.split_whitespace().collect();

    let mut hits = Vec::new();
    for file_path in files {
        let content = match tokio::fs::read_to_string(&file_path).await {
            Ok(content) => content,
            Err(_) => continue,
        };

        let rel_path = file_path
            .strip_prefix(&workspace_root)
            .unwrap_or(&file_path)
            .display()
            .to_string();
        let lines: Vec<&str> = content.lines().collect();

        for (index, line) in lines.iter().enumerate() {
            let line_lower = line.to_lowercase();
            let word_hits = query_words
                .iter()
                .filter(|word| line_lower.contains(**word))
                .count();
            if word_hits == 0 {
                continue;
            }

            let score = word_hits as f64 / query_words.len().max(1) as f64;
            let start = index.saturating_sub(1);
            let end = (index + 2).min(lines.len());
            let snippet = lines[start..end].join("\n");
            hits.push(MemorySearchHit {
                path: rel_path.clone(),
                line: index + 1,
                score,
                snippet,
            });
        }
    }

    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.line.cmp(&b.line))
    });
    hits.truncate(max_results);

    Ok(MemorySearchResponse {
        query: query.to_string(),
        total: hits.len(),
        hits,
    })
}

/// Read a bounded snippet from a workspace memory file.
pub async fn get(path: &str, from: usize, lines: Option<usize>) -> Result<MemoryGetResponse> {
    ensure_memory_path(path)?;

    let workspace_root = workspace_root_from_config();
    let full_path = workspace_root.join(path);
    let content = tokio::fs::read_to_string(&full_path)
        .await
        .with_context(|| format!("failed to read {}", full_path.display()))?;

    let file_lines: Vec<&str> = content.lines().collect();
    let start = from.saturating_sub(1).min(file_lines.len());
    let end = match lines {
        Some(count) => (start + count).min(file_lines.len()),
        None => file_lines.len(),
    };
    let snippet = file_lines[start..end].join("\n");

    Ok(MemoryGetResponse {
        path: path.to_string(),
        from,
        lines: end.saturating_sub(start),
        content: snippet,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::LazyLock;
    use tempfile::TempDir;
    use tokio::sync::Mutex;

    static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn with_workspace_env(tmp: &TempDir) {
        unsafe {
            std::env::set_var("RUNE_AGENTS__LIST", "[]");
            std::env::set_var(
                "RUNE_PATHS__MEMORY_DIR",
                tmp.path().join("memory").display().to_string(),
            );
        }
    }

    #[tokio::test]
    async fn status_reports_files() {
        let _guard = ENV_LOCK.lock().await;
        let tmp = TempDir::new().unwrap();
        tokio::fs::create_dir_all(tmp.path().join("memory"))
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("MEMORY.md"), "# Memory")
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("memory/2026-03-13.md"), "# Daily")
            .await
            .unwrap();
        with_workspace_env(&tmp);

        let status = status().await.unwrap();
        assert!(status.long_term_exists);
        assert_eq!(status.daily_file_count, 1);
    }

    #[tokio::test]
    async fn search_finds_matches() {
        let _guard = ENV_LOCK.lock().await;
        let tmp = TempDir::new().unwrap();
        tokio::fs::create_dir_all(tmp.path().join("memory"))
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("MEMORY.md"), "- Prefers dark mode")
            .await
            .unwrap();
        with_workspace_env(&tmp);

        let result = search("dark mode", 5).await.unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.hits[0].path, "MEMORY.md");
    }

    #[tokio::test]
    async fn get_rejects_non_memory_paths() {
        let err = get("secrets/api-key.txt", 1, Some(10)).await.unwrap_err();
        assert!(err.to_string().contains("memory get only reads"));
    }
}
