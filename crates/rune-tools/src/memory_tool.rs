//! Implementation of memory tools: `memory_search`, `memory_get`, and `memory_write`.
//!
//! Memory in Rune is file-oriented: MEMORY.md and memory/*.md files in the workspace.
//! Search prefers the persisted hybrid backend when one is configured, falling back
//! to local keyword scanning when persistence or embeddings are unavailable.

use std::path::{Component, Path, PathBuf};

use chrono::Utc;
use std::sync::Arc;

use async_trait::async_trait;
use rune_store::{MemoryEmbeddingRepo, models::KeywordSearchRow, models::VectorSearchRow};
use tracing::{instrument, warn};

use crate::definition::{ToolCall, ToolResult};
use crate::error::ToolError;
use crate::executor::ToolExecutor;
use crate::memory_index::MemoryIndex;
use crate::memory_ranking::{self, MemoryHit};

/// Configuration for memory search ranking.
#[derive(Clone, Debug)]
pub struct MemorySearchConfig {
    /// Enable temporal decay on search results (default: true).
    pub temporal_decay_enabled: bool,
    /// Half-life in days for temporal decay (default: 30).
    pub half_life_days: f64,
    /// Enable MMR diversity re-ranking (default: true).
    pub mmr_enabled: bool,
    /// Lambda for MMR: 1.0 = pure relevance, 0.0 = pure diversity (default: 0.7).
    pub mmr_lambda: f64,
    /// Enable query expansion (default: true).
    pub query_expansion_enabled: bool,
    /// Boost matches found in evergreen files like MEMORY.md so durable facts
    /// outrank equally-matching daily notes (default: true).
    pub evergreen_file_boost_enabled: bool,
    /// Multiplicative boost applied to evergreen files like MEMORY.md.
    pub evergreen_file_boost: f64,
}

impl Default for MemorySearchConfig {
    fn default() -> Self {
        Self {
            temporal_decay_enabled: true,
            half_life_days: memory_ranking::DEFAULT_HALF_LIFE_DAYS,
            mmr_enabled: true,
            mmr_lambda: memory_ranking::DEFAULT_MMR_LAMBDA,
            query_expansion_enabled: true,
            evergreen_file_boost_enabled: true,
            evergreen_file_boost: 1.25,
        }
    }
}

/// Executor for memory tools operating on workspace memory files.
pub struct MemoryToolExecutor {
    workspace_root: PathBuf,
    hybrid_search: Option<Arc<dyn HybridMemorySearchBackend>>,
    search_config: MemorySearchConfig,
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

pub struct PersistedKeywordMemorySearch {
    repo: Arc<dyn MemoryEmbeddingRepo>,
}

impl PersistedKeywordMemorySearch {
    pub fn new(repo: Arc<dyn MemoryEmbeddingRepo>) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl HybridMemorySearchBackend for PersistedKeywordMemorySearch {
    async fn search(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<HybridMemorySearchHit>, ToolError> {
        let limit = i64::try_from(max_results)
            .map_err(|_| ToolError::InvalidArgument("maxResults is too large".into()))?;

        let keyword_hits = self
            .repo
            .keyword_search(None, query, limit)
            .await
            .map_err(|e| {
                ToolError::ExecutionFailed(format!("persisted memory keyword search failed: {e}"))
            })?;

        Ok(keyword_hits
            .into_iter()
            .map(|row| HybridMemorySearchHit {
                file_path: row.file_path,
                chunk_text: row.chunk_text,
            })
            .collect())
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

        let keyword_hits = self
            .repo
            .keyword_search(None, query, limit)
            .await
            .map_err(|e| {
                ToolError::ExecutionFailed(format!("persisted memory keyword search failed: {e}"))
            })?;

        let query_embedding = self.index.embed_query(query).await.map_err(|e| {
            ToolError::ExecutionFailed(format!("persisted memory query embedding failed: {e}"))
        })?;

        let vector_hits = self
            .repo
            .vector_search(None, &query_embedding, limit)
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
            search_config: MemorySearchConfig::default(),
        }
    }

    /// Create a memory tool executor with custom search ranking config.
    pub fn with_config(
        workspace_root: impl Into<PathBuf>,
        search_config: MemorySearchConfig,
    ) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            hybrid_search: None,
            search_config,
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
            search_config: MemorySearchConfig::default(),
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
            let mut stack = vec![memory_dir];
            while let Some(dir) = stack.pop() {
                if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
                    while let Ok(Some(entry)) = entries.next_entry().await {
                        let path = entry.path();
                        if path.is_dir() {
                            stack.push(path);
                        } else if path.extension().is_some_and(|e| e == "md") {
                            files.push(path);
                        }
                    }
                }
            }
        }

        files.sort();
        files
    }

    fn format_results(query: &str, results: &[MemoryHit]) -> String {
        if results.is_empty() {
            format!("No results found for query: {query}")
        } else {
            results
                .iter()
                .map(|hit| format!("Source: {}\n{}", hit.file_path, hit.chunk_text.trim()))
                .collect::<Vec<_>>()
                .join("\n---\n")
        }
    }

    fn apply_evergreen_file_boost(&self, hits: &mut [MemoryHit]) {
        if !self.search_config.evergreen_file_boost_enabled {
            return;
        }

        for hit in hits.iter_mut() {
            let path = hit.file_path.split('#').next().unwrap_or(&hit.file_path);
            let filename = Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(path);

            if filename.eq_ignore_ascii_case("MEMORY.md") {
                hit.score *= self.search_config.evergreen_file_boost;
            }
        }
    }

    /// Apply the ranking pipeline (evergreen boost + temporal decay + MMR) to search results.
    fn apply_ranking(&self, mut hits: Vec<MemoryHit>, max_results: usize) -> Vec<MemoryHit> {
        self.apply_evergreen_file_boost(&mut hits);
        if self.search_config.temporal_decay_enabled {
            memory_ranking::apply_temporal_decay(&mut hits, self.search_config.half_life_days);
            // Re-sort after decay so MMR sees the adjusted scores
            hits.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        if self.search_config.mmr_enabled {
            memory_ranking::mmr_rerank(hits, self.search_config.mmr_lambda, max_results)
        } else {
            hits.truncate(max_results);
            hits
        }
    }

    /// Optionally expand the query before searching.
    fn maybe_expand_query<'a>(&self, query: &'a str) -> std::borrow::Cow<'a, str> {
        if self.search_config.query_expansion_enabled {
            let expanded = memory_ranking::expand_query(query);
            if expanded != query.to_lowercase() || expanded != query {
                tracing::debug!(original = query, expanded = %expanded, "query expanded");
            }
            std::borrow::Cow::Owned(expanded)
        } else {
            std::borrow::Cow::Borrowed(query)
        }
    }

    async fn local_keyword_search(&self, query: &str, max_results: usize) -> Vec<MemoryHit> {
        let query_lower = query.to_lowercase();
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();
        if query_words.is_empty() {
            return Vec::new();
        }

        let files = self.memory_files().await;
        let mut results: Vec<MemoryHit> = Vec::new();

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

                    results.push(MemoryHit {
                        file_path: format!("{}#{}", rel_path, i + 1),
                        chunk_text: snippet,
                        score,
                    });
                }
            }
        }

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        // Return more than max_results so ranking pipeline can select diverse results
        results.truncate(max_results * 3);
        results
    }

    async fn memory_bank_files(&self) -> Result<Vec<PathBuf>, ToolError> {
        let root = self.workspace_root.join("memory-bank");
        if !root.exists() {
            return Ok(Vec::new());
        }

        let mut pending = vec![root];
        let mut files = Vec::new();
        while let Some(dir) = pending.pop() {
            let mut entries = tokio::fs::read_dir(&dir).await.map_err(|e| {
                ToolError::ExecutionFailed(format!("failed to read {}: {e}", dir.display()))
            })?;
            while let Some(entry) = entries.next_entry().await.map_err(|e| {
                ToolError::ExecutionFailed(format!("failed to read {}: {e}", dir.display()))
            })? {
                let path = entry.path();
                let ty = entry.file_type().await.map_err(|e| {
                    ToolError::ExecutionFailed(format!("failed to stat {}: {e}", path.display()))
                })?;
                if ty.is_dir() {
                    pending.push(path);
                } else if path.extension().is_some_and(|ext| ext == "md") {
                    files.push(path);
                }
            }
        }

        files.sort();
        Ok(files)
    }

    fn validate_memory_bank_path(&self, path_str: &str) -> Result<PathBuf, ToolError> {
        let rel = Path::new(path_str);
        if rel.is_absolute()
            || rel.components().any(|c| {
                matches!(
                    c,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(ToolError::InvalidArgument(
                "memory_bank path traversal is not allowed".into(),
            ));
        }

        let full = self.workspace_root.join("memory-bank").join(rel);
        if let Ok(canonical) = full.canonicalize() {
            if let Ok(root) = self.workspace_root.join("memory-bank").canonicalize() {
                if !canonical.starts_with(&root) {
                    return Err(ToolError::InvalidArgument(
                        "resolved path escapes memory-bank boundary".into(),
                    ));
                }
            }
        }
        Ok(full)
    }

    #[instrument(skip(self, call), fields(tool = "memory_bank_list"))]
    async fn memory_bank_list(&self, call: &ToolCall) -> Result<ToolResult, ToolError> {
        let scope = call.arguments.get("path").and_then(|v| v.as_str());
        let mut files = self.memory_bank_files().await?;

        if let Some(scope) = scope {
            let scope = scope.trim_matches('/');
            if scope.contains("..") {
                return Err(ToolError::InvalidArgument(
                    "memory_bank path traversal is not allowed".into(),
                ));
            }
            files.retain(|path| {
                path.strip_prefix(self.workspace_root.join("memory-bank"))
                    .ok()
                    .is_some_and(|rel| rel.starts_with(scope))
            });
        }

        let output = if files.is_empty() {
            "No Memory Bank documents found.".to_string()
        } else {
            files
                .into_iter()
                .filter_map(|path| {
                    path.strip_prefix(&self.workspace_root)
                        .ok()
                        .map(|rel| rel.display().to_string())
                })
                .collect::<Vec<_>>()
                .join(
                    "
",
                )
        };

        Ok(ToolResult {
            tool_call_id: call.tool_call_id.clone(),
            output,
            is_error: false,
            tool_execution_id: None,
        })
    }

    #[instrument(skip(self, call), fields(tool = "memory_bank_get"))]
    async fn memory_bank_get(&self, call: &ToolCall) -> Result<ToolResult, ToolError> {
        let path_str = call
            .arguments
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgument("missing required parameter: path".into()))?;
        let full_path = self.validate_memory_bank_path(path_str)?;

        if full_path.extension().is_none_or(|ext| ext != "md") {
            return Err(ToolError::InvalidArgument(
                "memory_bank_get only reads markdown files under memory-bank/".into(),
            ));
        }

        let content = tokio::fs::read_to_string(&full_path).await.map_err(|e| {
            ToolError::ExecutionFailed(format!("failed to read memory-bank/{path_str}: {e}"))
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
        let file_lines: Vec<&str> = content.lines().collect();
        let start = from.saturating_sub(1).min(file_lines.len());
        let end = match line_count {
            Some(count) => (start + count).min(file_lines.len()),
            None => file_lines.len(),
        };
        Ok(ToolResult {
            tool_call_id: call.tool_call_id.clone(),
            output: file_lines[start..end].join(
                "
",
            ),
            is_error: false,
            tool_execution_id: None,
        })
    }

    /// Search memory, preferring persisted hybrid search when configured.
    ///
    /// Pipeline: query expansion → search → temporal decay → MMR re-ranking → format.
    #[instrument(skip(self, call), fields(tool = "memory_search"))]
    async fn memory_search(&self, call: &ToolCall) -> Result<ToolResult, ToolError> {
        let raw_query = call
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

        // Step 1: Query expansion
        let query = self.maybe_expand_query(raw_query);

        // Step 2: Search (hybrid or local keyword)
        let hits = if let Some(backend) = &self.hybrid_search {
            match backend.search(&query, max_results * 3).await {
                Ok(results) => results
                    .into_iter()
                    .enumerate()
                    .map(|(i, hit)| MemoryHit {
                        file_path: hit.file_path,
                        chunk_text: hit.chunk_text,
                        // Assign score based on position (backend results are ranked)
                        score: 1.0 / (1.0 + i as f64),
                    })
                    .collect(),
                Err(err) => {
                    warn!(error = %err, "persisted hybrid memory search failed; falling back to local keyword scan");
                    self.local_keyword_search(&query, max_results).await
                }
            }
        } else {
            self.local_keyword_search(&query, max_results).await
        };

        // Step 3: Temporal decay + MMR re-ranking
        let ranked = self.apply_ranking(hits, max_results);

        // Step 4: Format
        let output = Self::format_results(raw_query, &ranked);

        Ok(ToolResult {
            tool_call_id: call.tool_call_id.clone(),
            output,
            is_error: false,
            tool_execution_id: None,
        })
    }

    /// Append a note to today's daily memory file using append-only semantics.
    #[instrument(skip(self, call), fields(tool = "memory_write"))]
    async fn memory_write(&self, call: &ToolCall) -> Result<ToolResult, ToolError> {
        let text = call
            .arguments
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgument("missing required parameter: text".into()))?;

        if text.trim().is_empty() {
            return Err(ToolError::InvalidArgument(
                "memory_write requires non-empty text".into(),
            ));
        }

        let today = Utc::now().format("%Y-%m-%d").to_string();
        let relative_path = format!("memory/{today}.md");
        let full_path = self.workspace_root.join(&relative_path);

        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                ToolError::ExecutionFailed(format!(
                    "failed to create memory directory for {relative_path}: {e}"
                ))
            })?;
        }

        let mut note = String::new();
        if full_path.exists() {
            note.push('\n');
        }
        note.push_str(text.trim_end());
        note.push('\n');

        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&full_path)
            .await
            .map_err(|e| {
                ToolError::ExecutionFailed(format!("failed to open {relative_path} for append: {e}"))
            })?;

        file.write_all(note.as_bytes()).await.map_err(|e| {
            ToolError::ExecutionFailed(format!("failed to append to {relative_path}: {e}"))
        })?;
        file.flush().await.map_err(|e| {
            ToolError::ExecutionFailed(format!("failed to flush {relative_path}: {e}"))
        })?;

        Ok(ToolResult {
            tool_call_id: call.tool_call_id.clone(),
            output: format!("Appended note to {relative_path}"),
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
            tool_call_id: call.tool_call_id.clone(),
            output,
            is_error: false,
            tool_execution_id: None,
        })
    }
}

pub fn memory_bank_list_tool_definition() -> crate::ToolDefinition {
    crate::ToolDefinition {
        name: "memory_bank_list".into(),
        description: "List discoverable Memory Bank documents in the workspace, including architecture decision records and knowledge indexes.".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Optional workspace-relative Memory Bank subdirectory to scope listing, e.g. 'adr'"
                }
            }
        }),
        category: rune_core::ToolCategory::FileRead,
        requires_approval: false,
    }
}

pub fn memory_bank_get_tool_definition() -> crate::ToolDefinition {
    crate::ToolDefinition {
        name: "memory_bank_get".into(),
        description: "Read a Memory Bank markdown document from the workspace, including architecture decision records.".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Workspace-relative path under memory-bank/ to read" },
                "from": { "type": "integer", "description": "1-indexed line number to start from" },
                "lines": { "type": "integer", "description": "Maximum number of lines to return" }
            },
            "required": ["path"]
        }),
        category: rune_core::ToolCategory::FileRead,
        requires_approval: false,
    }
}

pub fn memory_write_tool_definition() -> crate::ToolDefinition {
    crate::ToolDefinition {
        name: "memory_write".into(),
        description: "Append a note to today's daily memory file under memory/YYYY-MM-DD.md.".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "Note text to append to today's daily memory file"
                }
            },
            "required": ["text"]
        }),
        category: rune_core::ToolCategory::FileWrite,
        requires_approval: false,
    }
}

#[async_trait]
impl ToolExecutor for MemoryToolExecutor {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        match call.tool_name.as_str() {
            "memory_search" => self.memory_search(&call).await,
            "memory_get" => self.memory_get(&call).await,
            "memory_bank_list" => self.memory_bank_list(&call).await,
            "memory_bank_get" => self.memory_bank_get(&call).await,
            "memory_write" => self.memory_write(&call).await,
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
            _project_id: Option<&str>,
            _file_path: &str,
            _chunk_index: i32,
            _chunk_text: &str,
            _embedding: &[f32],
        ) -> Result<(), StoreError> {
            unreachable!()
        }

        async fn delete_by_file(
            &self,
            _project_id: Option<&str>,
            _file_path: &str,
        ) -> Result<usize, StoreError> {
            unreachable!()
        }

        async fn keyword_search(
            &self,
            _project_id: Option<&str>,
            _query: &str,
            _limit: i64,
        ) -> Result<Vec<KeywordSearchRow>, StoreError> {
            Ok(self.keyword_hits.clone())
        }

        async fn vector_search(
            &self,
            _project_id: Option<&str>,
            _embedding: &[f32],
            _limit: i64,
        ) -> Result<Vec<VectorSearchRow>, StoreError> {
            Ok(self.vector_hits.clone())
        }

        async fn count(&self, _project_id: Option<&str>) -> Result<i64, StoreError> {
            Ok((self.keyword_hits.len().max(self.vector_hits.len())) as i64)
        }

        async fn list_indexed_files(
            &self,
            _project_id: Option<&str>,
        ) -> Result<Vec<String>, StoreError> {
            Ok(Vec::new())
        }

        async fn delete_chunk(
            &self,
            _project_id: Option<&str>,
            _file_path: &str,
            _chunk_index: i32,
        ) -> Result<bool, StoreError> {
            Ok(false)
        }

        async fn delete_all(&self, _project_id: Option<&str>) -> Result<usize, StoreError> {
            Ok(0)
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
                    project_id: None,
                    file_path: "MEMORY.md".into(),
                    chunk_text: "Prefers dark mode and compact UI".into(),
                    score: 0.9,
                }],
                vector_hits: vec![VectorSearchRow {
                    project_id: None,
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
    async fn evergreen_memory_file_outranks_equally_matching_daily_note() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::write(
            tmp.path().join("MEMORY.md"),
            "- prefers rust and dark mode
",
        )
        .await
        .unwrap();
        tokio::fs::create_dir_all(tmp.path().join("memory"))
            .await
            .unwrap();
        tokio::fs::write(
            tmp.path().join("memory/2026-03-13.md"),
            "- prefers rust and dark mode
",
        )
        .await
        .unwrap();

        let exec = MemoryToolExecutor::new(tmp.path());
        let call = make_call(
            "memory_search",
            serde_json::json!({"query": "prefers rust dark mode", "maxResults": 1}),
        );
        let result = exec.execute(call).await.unwrap();

        assert!(
            result.output.contains("Source: MEMORY.md#1"),
            "{0}",
            result.output
        );
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
    async fn memory_bank_list_returns_seeded_documents() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::create_dir_all(tmp.path().join("memory-bank/adr"))
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("memory-bank/README.md"), "# Memory Bank")
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("memory-bank/adr/ADR-0001.md"), "# ADR")
            .await
            .unwrap();

        let exec = MemoryToolExecutor::new(tmp.path());
        let call = make_call("memory_bank_list", serde_json::json!({}));
        let result = exec.execute(call).await.unwrap();

        assert!(result.output.contains("memory-bank/README.md"));
        assert!(result.output.contains("memory-bank/adr/ADR-0001.md"));
    }

    #[tokio::test]
    async fn memory_bank_get_reads_seeded_document() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::create_dir_all(tmp.path().join("memory-bank/adr"))
            .await
            .unwrap();
        tokio::fs::write(
            tmp.path().join("memory-bank/adr/ADR-0001.md"),
            "# ADR\n\nDecision line\nSecond line",
        )
        .await
        .unwrap();

        let exec = MemoryToolExecutor::new(tmp.path());
        let call = make_call(
            "memory_bank_get",
            serde_json::json!({"path": "adr/ADR-0001.md", "from": 3, "lines": 1}),
        );
        let result = exec.execute(call).await.unwrap();

        assert_eq!(result.output, "Decision line");
    }

    #[tokio::test]
    async fn memory_bank_get_rejects_path_traversal() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::create_dir_all(tmp.path().join("memory-bank/adr"))
            .await
            .unwrap();

        let exec = MemoryToolExecutor::new(tmp.path());
        let call = make_call(
            "memory_bank_get",
            serde_json::json!({"path": "../docs/adr/ADR-0001.md"}),
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

    #[tokio::test]
    async fn memory_write_appends_to_todays_daily_note() {
        let tmp = TempDir::new().unwrap();
        let exec = MemoryToolExecutor::new(tmp.path());
        let today = Utc::now().format("%Y-%m-%d").to_string();
        let relative = format!("memory/{today}.md");

        let first = make_call("memory_write", serde_json::json!({"text": "first note"}));
        let second = make_call("memory_write", serde_json::json!({"text": "second note"}));

        exec.execute(first).await.unwrap();
        exec.execute(second).await.unwrap();

        let content = tokio::fs::read_to_string(tmp.path().join(relative)).await.unwrap();
        assert_eq!(content, "first note\n\nsecond note\n");
    }

    #[tokio::test]
    async fn memory_write_rejects_blank_text() {
        let tmp = TempDir::new().unwrap();
        let exec = MemoryToolExecutor::new(tmp.path());
        let call = make_call("memory_write", serde_json::json!({"text": "   "}));

        let err = exec.execute(call).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgument(_)));
    }

}
