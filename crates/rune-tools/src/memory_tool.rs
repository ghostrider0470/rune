//! Implementation of memory tools: `memory_search` and `memory_get`.
//!
//! Memory in Rune is file-oriented: MEMORY.md and memory/*.md files in the workspace.
//! Search uses simple substring/keyword matching as a baseline; semantic search
//! can be layered on top later via an embedding provider.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tracing::instrument;

use crate::definition::{ToolCall, ToolResult};
use crate::error::ToolError;
use crate::executor::ToolExecutor;

/// Executor for memory tools operating on workspace memory files.
pub struct MemoryToolExecutor {
    workspace_root: PathBuf,
}

impl MemoryToolExecutor {
    /// Create a new memory tool executor rooted at the given workspace path.
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
        }
    }

    /// Collect all memory files: MEMORY.md + memory/*.md
    async fn memory_files(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();

        let memory_md = self.workspace_root.join("MEMORY.md");
        if memory_md.exists() {
            files.push(memory_md);
        }

        let memory_dir = self.workspace_root.join("memory");
        if memory_dir.is_dir() {
            if let Ok(mut entries) = tokio::fs::read_dir(&memory_dir).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "md") {
                        files.push(path);
                    }
                }
            }
        }

        files.sort();
        files
    }

    /// Simple keyword search across memory files.
    /// Returns matching snippets with file path and line numbers.
    #[instrument(skip(self, call), fields(tool = "memory_search"))]
    async fn memory_search(&self, call: &ToolCall) -> Result<ToolResult, ToolError> {
        let query = call
            .arguments
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolError::InvalidArgument("missing required parameter: query".into())
            })?;

        let max_results = call
            .arguments
            .get("maxResults")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as usize;

        let query_lower = query.to_lowercase();
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();

        let files = self.memory_files().await;
        let mut results: Vec<(String, usize, f64)> = Vec::new(); // (snippet, line, score)

        for file_path in &files {
            let content = match tokio::fs::read_to_string(file_path).await {
                Ok(c) => c,
                Err(_) => continue,
            };

            let rel_path = file_path
                .strip_prefix(&self.workspace_root)
                .unwrap_or(file_path)
                .display()
                .to_string();

            for (i, line) in content.lines().enumerate() {
                let line_lower = line.to_lowercase();
                let word_hits = query_words
                    .iter()
                    .filter(|w| line_lower.contains(*w))
                    .count();

                if word_hits > 0 {
                    let score = word_hits as f64 / query_words.len() as f64;

                    // Grab context: line ± 1
                    let lines: Vec<&str> = content.lines().collect();
                    let start = i.saturating_sub(1);
                    let end = (i + 2).min(lines.len());
                    let snippet = lines[start..end].join("\n");

                    results.push((
                        format!("Source: {}#{}\n{}", rel_path, i + 1, snippet),
                        i + 1,
                        score,
                    ));
                }
            }
        }

        // Sort by score descending, take top N
        results.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(max_results);

        let output = if results.is_empty() {
            format!("No results found for query: {query}")
        } else {
            results
                .iter()
                .map(|(snippet, _, _)| snippet.as_str())
                .collect::<Vec<_>>()
                .join("\n---\n")
        };

        Ok(ToolResult {
            tool_call_id: call.tool_call_id,
            output,
            is_error: false,
        })
    }

    /// Read a specific snippet from a memory file.
    #[instrument(skip(self, call), fields(tool = "memory_get"))]
    async fn memory_get(&self, call: &ToolCall) -> Result<ToolResult, ToolError> {
        let path_str = call
            .arguments
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolError::InvalidArgument("missing required parameter: path".into())
            })?;

        // Only allow MEMORY.md and memory/*.md
        let path = Path::new(path_str);
        let is_memory_md = path_str == "MEMORY.md";
        let is_memory_dir = path
            .parent()
            .is_some_and(|p| p == Path::new("memory"))
            && path.extension().is_some_and(|e| e == "md");

        if !is_memory_md && !is_memory_dir {
            return Err(ToolError::InvalidArgument(
                "memory_get only reads MEMORY.md or memory/*.md files".into(),
            ));
        }

        let full_path = self.workspace_root.join(path);
        let content = tokio::fs::read_to_string(&full_path).await.map_err(|e| {
            ToolError::ExecutionFailed(format!("failed to read {path_str}: {e}"))
        })?;

        let from = call
            .arguments
            .get("from")
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as usize;
        let line_count = call
            .arguments
            .get("lines")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);

        let lines: Vec<&str> = content.lines().collect();
        let start = from.saturating_sub(1).min(lines.len());
        let end = match line_count {
            Some(n) => (start + n).min(lines.len()),
            None => lines.len(),
        };

        let output = lines[start..end].join("\n");

        Ok(ToolResult {
            tool_call_id: call.tool_call_id,
            output,
            is_error: false,
        })
    }
}

#[async_trait]
impl ToolExecutor for MemoryToolExecutor {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        match call.tool_name.as_str() {
            "memory_search" => self.memory_search(&call).await,
            "memory_get" => self.memory_get(&call).await,
            other => Err(ToolError::NotFound(other.into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune_core::ToolCallId;
    use tempfile::TempDir;

    fn make_call(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: name.into(),
            arguments: args,
        }
    }

    async fn setup_workspace() -> TempDir {
        let tmp = TempDir::new().unwrap();

        // Create MEMORY.md
        tokio::fs::write(
            tmp.path().join("MEMORY.md"),
            "# Memory\n\n## Preferences\n- Prefers dark mode\n- Timezone: Europe/Sarajevo\n",
        )
        .await
        .unwrap();

        // Create memory dir with a daily file
        tokio::fs::create_dir_all(tmp.path().join("memory"))
            .await
            .unwrap();
        tokio::fs::write(
            tmp.path().join("memory/2026-03-13.md"),
            "# 2026-03-13\n\n- Fixed build pipeline\n- Deployed v0.1.0\n- Reviewed PR #42\n",
        )
        .await
        .unwrap();

        tmp
    }

    #[tokio::test]
    async fn search_finds_matching_lines() {
        let tmp = setup_workspace().await;
        let exec = MemoryToolExecutor::new(tmp.path());

        let call = make_call(
            "memory_search",
            serde_json::json!({"query": "dark mode"}),
        );
        let result = exec.execute(call).await.unwrap();
        assert!(!result.is_error);
        assert!(result.output.contains("dark mode"));
        assert!(result.output.contains("MEMORY.md"));
    }

    #[tokio::test]
    async fn search_returns_no_results_for_unmatched_query() {
        let tmp = setup_workspace().await;
        let exec = MemoryToolExecutor::new(tmp.path());

        let call = make_call(
            "memory_search",
            serde_json::json!({"query": "xyznonexistent"}),
        );
        let result = exec.execute(call).await.unwrap();
        assert!(result.output.contains("No results"));
    }

    #[tokio::test]
    async fn get_reads_memory_file() {
        let tmp = setup_workspace().await;
        let exec = MemoryToolExecutor::new(tmp.path());

        let call = make_call(
            "memory_get",
            serde_json::json!({"path": "MEMORY.md"}),
        );
        let result = exec.execute(call).await.unwrap();
        assert!(result.output.contains("Preferences"));
    }

    #[tokio::test]
    async fn get_with_from_and_lines() {
        let tmp = setup_workspace().await;
        let exec = MemoryToolExecutor::new(tmp.path());

        let call = make_call(
            "memory_get",
            serde_json::json!({"path": "memory/2026-03-13.md", "from": 3, "lines": 2}),
        );
        let result = exec.execute(call).await.unwrap();
        assert!(result.output.contains("Fixed build pipeline"));
    }

    #[tokio::test]
    async fn get_rejects_non_memory_path() {
        let tmp = setup_workspace().await;
        let exec = MemoryToolExecutor::new(tmp.path());

        let call = make_call(
            "memory_get",
            serde_json::json!({"path": "secrets/keys.md"}),
        );
        let err = exec.execute(call).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }
}
