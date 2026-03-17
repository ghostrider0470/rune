//! Implementation of memory tools: `memory_search` and `memory_get`.
//!
//! Memory in Rune is file-oriented: MEMORY.md and memory/*.md files in the workspace.
//! Search prefers the persisted hybrid backend when one is configured, falling back
//! to local keyword scanning when persistence or embeddings are unavailable.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use rune_store::{MemoryEmbeddingRepo, models::KeywordSearchRow, models::VectorSearchRow};
use tracing::{instrument, warn};

use crate::definition::{ToolCall, ToolResult};
use crate::error::ToolError;
use crate::executor::ToolExecutor;
use crate::memory_index::MemoryIndex;

/// Executor for memory tools operating on workspace memory files.
pub struct MemoryToolExecutor {
    workspace_root: PathBuf,
    hybrid_search: Option<Arc<dyn HybridMemorySearchBackend>>,
}

#[async_trait]
pub trait HybridMemorySearchBackend: Send + Sync {
    async fn search(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<HybridMemorySearchHit>, ToolError>;
}

#[derive(Clone, Debug)]
pub struct HybridMemorySearchHit {
    pub file_path: String,
    pub chunk_text: String,
}

pub struct PersistedHybridMemorySearch {
    repo: Arc<dyn MemoryEmbeddingRepo>,
    index: MemoryIndex,
}

impl PersistedHybridMemorySearch {
    pub fn new(repo: Arc<dyn MemoryEmbeddingRepo>, index: MemoryIndex) -> Self {
        Self { repo, index }
    }
}

#[async_trait]
impl HybridMemorySearchBackend for PersistedHybridMemorySearch {
    async fn search(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<HybridMemorySearchHit>, ToolError> {
        let limit = i64::try_from(max_results)
            .map_err(|_| ToolError::InvalidArgument("maxResults is too large".into()))?;

        let keyword_hits = self.repo.keyword_search(query, limit).await.map_err(|e| {
            ToolError::ExecutionFailed(format!("persisted memory keyword search failed: {e}"))
        })?;

        let query_embedding = self.index.embed_query(query).await.map_err(|e| {
            ToolError::ExecutionFailed(format!("persisted memory query embedding failed: {e}"))
        })?;

        let vector_hits = self
            .repo
            .vector_search(&query_embedding, limit)
            .await
            .map_err(|e| {
                ToolError::ExecutionFailed(format!("persisted memory vector search failed: {e}"))
            })?;

        Ok(self
            .index
            .search(
                &keyword_rows_to_hits(&keyword_hits),
                &vector_rows_to_hits(&vector_hits),
                max_results,
            )
            .into_iter()
            .map(|hit| HybridMemorySearchHit {
                file_path: hit.file_path,
                chunk_text: hit.chunk_text,
            })
            .collect())
    }
}

fn keyword_rows_to_hits(rows: &[KeywordSearchRow]) -> Vec<crate::memory_index::KeywordHit> {
    rows.iter()
        .map(|row| crate::memory_index::KeywordHit {
            file_path: row.file_path.clone(),
            chunk_text: row.chunk_text.clone(),
            ts_rank: row.score,
        })
        .collect()
}

fn vector_rows_to_hits(rows: &[VectorSearchRow]) -> Vec<crate::memory_index::VectorHit> {
    rows.iter()
        .map(|row| crate::memory_index::VectorHit {
            file_path: row.file_path.clone(),
            chunk_text: row.chunk_text.clone(),
            cosine_similarity: row.score,
        })
        .collect()
}

impl MemoryToolExecutor {
    /// Create a new memory tool executor rooted at the given workspace path.
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            hybrid_search: None,
        }
    }

    /// Test/diagnostic hook for checking whether a persisted hybrid backend is attached.
    pub fn hybrid_search_backend(&self) -> Option<&Arc<dyn HybridMemorySearchBackend>> {
        self.hybrid_search.as_ref()
    }

    /// Create a memory tool executor that prefers the persisted hybrid search backend.
    pub fn with_hybrid_search(
        workspace_root: impl Into<PathBuf>,
        hybrid_search: Arc<dyn HybridMemorySearchBackend>,
    ) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            hybrid_search: Some(hybrid_search),
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

    fn format_hybrid_results(&self, query: &str, results: Vec<HybridMemorySearchHit>) -> String {
        if results.is_empty() {
            format!("No results found for query: {query}")
        } else {
            results
                .into_iter()
                .map(|hit| format!("Source: {}\n{}", hit.file_path, hit.chunk_text.trim()))
                .collect::<Vec<_>>()
                .join("\n---\n")
        }
    }

    async fn local_keyword_search(&self, query: &str, max_results: usize) -> String {
        let query_lower = query.to_lowercase();
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();
        if query_words.is_empty() {
            return format!("No results found for query: {query}");
        }

        let files = self.memory_files().await;
        let mut results: Vec<(String, usize, f64)> = Vec::new();

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

            let lines: Vec<&str> = content.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                let line_lower = line.to_lowercase();
                let word_hits = query_words
                    .iter()
                    .filter(|w| line_lower.contains(*w))
                    .count();

                if word_hits > 0 {
                    let score = word_hits as f64 / query_words.len() as f64;
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

        results.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(max_results);

        if results.is_empty() {
            format!("No results found for query: {query}")
        } else {
            results
                .iter()
                .map(|(snippet, _, _)| snippet.as_str())
                .collect::<Vec<_>>()
                .join("\n---\n")
        }
    }

    /// Search memory, preferring persisted hybrid search when configured.
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

        let output = if let Some(backend) = &self.hybrid_search {
            match backend.search(query, max_results).await {
                Ok(results) => self.format_hybrid_results(query, results),
                Err(err) => {
                    warn!(error = %err, "persisted hybrid memory search failed; falling back to local keyword scan");
                    self.local_keyword_search(query, max_results).await
                }
            }
        } else {
            self.local_keyword_search(query, max_results).await
        };

        Ok(ToolResult {
            tool_call_id: call.tool_call_id,
            output,
            is_error: false,
            tool_execution_id: None,
        })
    }

    /// Read a specific snippet from a memory file.
    #[instrument(skip(self, call), fields(tool = "memory_get"))]
    async fn memory_get(&self, call: &ToolCall) -> Result<ToolResult, ToolError> {
        let path_str = call
            .arguments
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgument("missing required parameter: path".into()))?;

        // Reject any path with parent directory traversal
        if Path::new(path_str)
            .components()
            .any(|c| matches!(c, Component::ParentDir))
        {
            return Err(ToolError::InvalidArgument(
                "path traversal is not allowed in memory_get".into(),
            ));
        }

        // Only allow MEMORY.md and memory/*.md
        let path = Path::new(path_str);
        let is_memory_md = path_str == "MEMORY.md";
        let is_memory_dir = path.parent().is_some_and(|p| p == Path::new("memory"))
            && path.extension().is_some_and(|e| e == "md");

        if !is_memory_md && !is_memory_dir {
            return Err(ToolError::InvalidArgument(
                "memory_get only reads MEMORY.md or memory/*.md files".into(),
            ));
        }

        let full_path = self.workspace_root.join(path);

        // Defense-in-depth: ensure resolved path is within workspace
        if let Ok(canonical) = full_path.canonicalize() {
            if let Ok(ws_canonical) = self.workspace_root.canonicalize() {
                if !canonical.starts_with(&ws_canonical) {
                    return Err(ToolError::InvalidArgument(
                        "resolved path escapes workspace boundary".into(),
                    ));
                }
            }
        }

        let content = tokio::fs::read_to_string(&full_path)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to read {path_str}: {e}")))?;

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
            tool_execution_id: None,
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
    use rune_store::StoreError;
    use rune_store::repos::MemoryEmbeddingRepo;
    use tempfile::TempDir;

    enum StubHybridSearchResult {
        Ok(Vec<HybridMemorySearchHit>),
        Err(&'static str),
    }

    struct StubHybridSearchBackend {
        result: StubHybridSearchResult,
    }

    #[async_trait]
    impl HybridMemorySearchBackend for StubHybridSearchBackend {
        async fn search(
            &self,
            _query: &str,
            _max_results: usize,
        ) -> Result<Vec<HybridMemorySearchHit>, ToolError> {
            match &self.result {
                StubHybridSearchResult::Ok(hits) => Ok(hits.clone()),
                StubHybridSearchResult::Err(message) => {
                    Err(ToolError::ExecutionFailed((*message).into()))
                }
            }
        }
    }

    #[derive(Clone)]
    struct StubMemoryEmbeddingRepo {
        keyword_hits: Vec<KeywordSearchRow>,
        vector_hits: Vec<VectorSearchRow>,
    }

    #[async_trait]
    impl MemoryEmbeddingRepo for StubMemoryEmbeddingRepo {
        async fn upsert_chunk(
            &self,
            _file_path: &str,
            _chunk_index: i32,
            _chunk_text: &str,
            _embedding: &[f32],
        ) -> Result<(), StoreError> {
            unreachable!()
        }

        async fn delete_by_file(&self, _file_path: &str) -> Result<usize, StoreError> {
            unreachable!()
        }

        async fn keyword_search(
            &self,
            _query: &str,
            _limit: i64,
        ) -> Result<Vec<KeywordSearchRow>, StoreError> {
            Ok(self.keyword_hits.clone())
        }

        async fn vector_search(
            &self,
            _embedding: &[f32],
            _limit: i64,
        ) -> Result<Vec<VectorSearchRow>, StoreError> {
            Ok(self.vector_hits.clone())
        }

        async fn count(&self) -> Result<i64, StoreError> {
            Ok((self.keyword_hits.len().max(self.vector_hits.len())) as i64)
        }

        async fn list_indexed_files(&self) -> Result<Vec<String>, StoreError> {
            Ok(Vec::new())
        }
    }

    struct StubEmbeddingProvider {
        embedding: Vec<f32>,
    }

    #[async_trait]
    impl crate::memory_index::EmbeddingProvider for StubEmbeddingProvider {
        async fn embed(
            &self,
            texts: &[String],
        ) -> Result<Vec<Vec<f32>>, crate::memory_index::MemoryIndexError> {
            Ok(texts.iter().map(|_| self.embedding.clone()).collect())
        }

        fn dimension(&self) -> usize {
            self.embedding.len()
        }
    }

    fn make_call(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: name.into(),
            arguments: args,
        }
    }

    async fn setup_workspace() -> TempDir {
        let tmp = TempDir::new().unwrap();

        tokio::fs::write(
            tmp.path().join("MEMORY.md"),
            "# Memory\n\n## Preferences\n- Prefers dark mode\n- Timezone: Europe/Sarajevo\n",
        )
        .await
        .unwrap();

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

    fn stub_memory_index() -> MemoryIndex {
        MemoryIndex::new(
            crate::memory_index::MemoryIndexConfig {
                embedding_dimension: 4,
                chunk_size: 512,
                chunk_overlap: 0,
                ..Default::default()
            },
            Box::new(StubEmbeddingProvider {
                embedding: vec![0.1, 0.2, 0.3, 0.4],
            }),
        )
    }

    #[tokio::test]
    async fn search_finds_matching_lines() {
        let tmp = setup_workspace().await;
        let exec = MemoryToolExecutor::new(tmp.path());

        let call = make_call("memory_search", serde_json::json!({"query": "dark mode"}));
        let result = exec.execute(call).await.unwrap();
        assert!(!result.is_error);
        assert!(result.output.contains("dark mode"));
        assert!(result.output.contains("MEMORY.md"));
    }

    #[tokio::test]
    async fn search_uses_hybrid_backend_when_configured() {
        let tmp = setup_workspace().await;
        let exec = MemoryToolExecutor::with_hybrid_search(
            tmp.path(),
            Arc::new(StubHybridSearchBackend {
                result: StubHybridSearchResult::Ok(vec![HybridMemorySearchHit {
                    file_path: "memory/2026-03-13.md".into(),
                    chunk_text: "Persisted hybrid hit for build pipeline".into(),
                }]),
            }),
        );

        let call = make_call(
            "memory_search",
            serde_json::json!({"query": "build pipeline"}),
        );
        let result = exec.execute(call).await.unwrap();

        assert!(
            result
                .output
                .contains("Persisted hybrid hit for build pipeline")
        );
        assert!(result.output.contains("memory/2026-03-13.md"));
    }

    #[tokio::test]
    async fn search_falls_back_to_local_scan_when_hybrid_backend_fails() {
        let tmp = setup_workspace().await;
        let exec = MemoryToolExecutor::with_hybrid_search(
            tmp.path(),
            Arc::new(StubHybridSearchBackend {
                result: StubHybridSearchResult::Err("backend unavailable"),
            }),
        );

        let call = make_call("memory_search", serde_json::json!({"query": "dark mode"}));
        let result = exec.execute(call).await.unwrap();

        assert!(result.output.contains("dark mode"));
        assert!(result.output.contains("MEMORY.md"));
    }

    #[tokio::test]
    async fn persisted_hybrid_search_merges_keyword_and_vector_hits() {
        let backend = PersistedHybridMemorySearch::new(
            Arc::new(StubMemoryEmbeddingRepo {
                keyword_hits: vec![KeywordSearchRow {
                    file_path: "MEMORY.md".into(),
                    chunk_text: "Prefers dark mode and compact UI".into(),
                    score: 0.9,
                }],
                vector_hits: vec![VectorSearchRow {
                    file_path: "MEMORY.md".into(),
                    chunk_text: "Prefers dark mode and compact UI".into(),
                    score: 0.8,
                }],
            }),
            stub_memory_index(),
        );

        let results = backend.search("dark mode", 5).await.unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_path, "MEMORY.md");
        assert!(results[0].chunk_text.contains("dark mode"));
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

        let call = make_call("memory_get", serde_json::json!({"path": "MEMORY.md"}));
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

        let call = make_call("memory_get", serde_json::json!({"path": "secrets/keys.md"}));
        let err = exec.execute(call).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }

    #[tokio::test]
    async fn get_rejects_path_traversal() {
        let tmp = setup_workspace().await;
        let exec = MemoryToolExecutor::new(tmp.path());

        let call = make_call(
            "memory_get",
            serde_json::json!({"path": "memory/../../etc/passwd"}),
        );
        let err = exec.execute(call).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }

    #[tokio::test]
    async fn get_rejects_dotdot_in_memory_dir() {
        let tmp = setup_workspace().await;
        let exec = MemoryToolExecutor::new(tmp.path());

        let call = make_call(
            "memory_get",
            serde_json::json!({"path": "memory/../secrets.md"}),
        );
        let err = exec.execute(call).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }
}
