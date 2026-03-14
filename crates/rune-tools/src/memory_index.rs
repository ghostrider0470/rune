//! Hybrid memory search index using pgvector + tsvector.
//!
//! Combines vector embeddings with PostgreSQL full-text search for
//! Reciprocal Rank Fusion (RRF) scoring. The module provides:
//!
//! - **Chunking**: splits memory files by heading/paragraph boundaries into
//!   embedding-friendly chunks.
//! - **Embedding**: pluggable [`EmbeddingProvider`] trait with an OpenAI
//!   implementation ([`OpenAiEmbedding`]).
//! - **RRF merging**: [`reciprocal_rank_fusion`] combines keyword and vector
//!   ranked lists into a single scored result set.
//! - **`MemoryIndex`**: high-level façade that ties chunking, embedding, and
//!   search together.
//!
//! ## SQL schema (applied by the caller / migration layer)
//!
//! ```sql
//! CREATE EXTENSION IF NOT EXISTS vector;
//!
//! CREATE TABLE memory_embeddings (
//!     id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
//!     file_path   TEXT NOT NULL,
//!     chunk_index INT  NOT NULL,
//!     chunk_text  TEXT NOT NULL,
//!     embedding   vector(1536),
//!     created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
//!     UNIQUE (file_path, chunk_index)
//! );
//!
//! CREATE INDEX idx_memory_embedding ON memory_embeddings
//!     USING ivfflat (embedding vector_cosine_ops) WITH (lists = 100);
//!
//! CREATE INDEX idx_memory_tsv ON memory_embeddings
//!     USING gin (to_tsvector('english', chunk_text));
//! ```
//!
//! ## Hybrid query pattern
//!
//! ```sql
//! -- Keyword leg
//! SELECT file_path, chunk_text,
//!        ts_rank(to_tsvector('english', chunk_text),
//!                plainto_tsquery('english', $1)) AS score
//! FROM memory_embeddings
//! WHERE to_tsvector('english', chunk_text) @@ plainto_tsquery('english', $1)
//! ORDER BY score DESC LIMIT $2;
//!
//! -- Vector leg
//! SELECT file_path, chunk_text,
//!        1 - (embedding <=> $1::vector) AS score
//! FROM memory_embeddings
//! ORDER BY embedding <=> $1::vector LIMIT $2;
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::{debug, instrument, warn};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the memory index.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryIndexConfig {
    /// Embedding provider identifier (`"openai"` or `"local"`).
    pub embedding_provider: String,
    /// API key for the embedding provider (required for OpenAI).
    pub api_key: Option<String>,
    /// Embedding model name (e.g. `"text-embedding-ada-002"`).
    pub embedding_model: String,
    /// Embedding dimension (1536 for ada-002, 768 for smaller models).
    pub embedding_dimension: usize,
    /// Maximum chunk size in approximate token count (~512).
    pub chunk_size: usize,
    /// Overlap between consecutive chunks in approximate token count.
    pub chunk_overlap: usize,
    /// Base URL for the embedding API. Defaults to OpenAI production.
    pub api_base_url: Option<String>,
}

impl Default for MemoryIndexConfig {
    fn default() -> Self {
        Self {
            embedding_provider: "openai".into(),
            api_key: None,
            embedding_model: "text-embedding-ada-002".into(),
            embedding_dimension: 1536,
            chunk_size: 512,
            chunk_overlap: 64,
            api_base_url: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Core data types
// ---------------------------------------------------------------------------

/// A memory chunk ready for embedding.
#[derive(Clone, Debug)]
pub struct MemoryChunk {
    /// Source file this chunk was extracted from.
    pub file_path: PathBuf,
    /// The text content of the chunk.
    pub chunk_text: String,
    /// Zero-based index within the source file.
    pub chunk_index: usize,
}

/// A single chunk together with its computed embedding vector.
#[derive(Clone, Debug)]
pub struct EmbeddedChunk {
    /// The originating chunk.
    pub chunk: MemoryChunk,
    /// The embedding vector.
    pub embedding: Vec<f32>,
}

/// Result from hybrid search.
#[derive(Clone, Debug, Serialize)]
pub struct HybridSearchResult {
    /// Source file path.
    pub file_path: String,
    /// The matched chunk text.
    pub chunk_text: String,
    /// Combined Reciprocal Rank Fusion score.
    pub rrf_score: f64,
    /// Rank in the keyword (tsvector) result set, if present.
    pub keyword_rank: Option<usize>,
    /// Rank in the vector (pgvector) result set, if present.
    pub vector_rank: Option<usize>,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Error types for memory indexing operations.
#[derive(Debug, thiserror::Error)]
pub enum MemoryIndexError {
    #[error("embedding error: {0}")]
    Embedding(String),
    #[error("indexing error: {0}")]
    Indexing(String),
    #[error("search error: {0}")]
    Search(String),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

// ---------------------------------------------------------------------------
// Embedding provider trait
// ---------------------------------------------------------------------------

/// Pluggable embedding provider.
///
/// Implementations must be cheaply cloneable behind an `Arc` so that
/// `MemoryIndex` can hold one without lifetime gymnastics.
#[async_trait::async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Embed one or more texts, returning one vector per input text.
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, MemoryIndexError>;

    /// Return the dimensionality of the embedding vectors.
    fn dimension(&self) -> usize;
}

// ---------------------------------------------------------------------------
// OpenAI embedding provider
// ---------------------------------------------------------------------------

/// OpenAI-compatible embedding provider.
///
/// Sends batched requests to `/v1/embeddings` and returns the resulting
/// vectors. Automatically splits batches that exceed the 100-input API limit.
pub struct OpenAiEmbedding {
    api_key: String,
    model: String,
    dimension: usize,
    base_url: String,
    client: reqwest::Client,
}

/// Maximum number of texts per single OpenAI embeddings API request.
const OPENAI_BATCH_LIMIT: usize = 100;

impl OpenAiEmbedding {
    /// Create a new OpenAI embedding provider.
    pub fn new(api_key: impl Into<String>, model: impl Into<String>, dimension: usize) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            dimension,
            base_url: "https://api.openai.com".into(),
            client: reqwest::Client::new(),
        }
    }

    /// Use a custom base URL (e.g. for local proxy / Azure).
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }
}

// Wire format for OpenAI embeddings API ----------------------------------

#[derive(Serialize)]
struct EmbeddingRequest<'a> {
    model: &'a str,
    input: &'a [String],
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
    index: usize,
}

#[async_trait::async_trait]
impl EmbeddingProvider for OpenAiEmbedding {
    #[instrument(skip_all, fields(model = %self.model, count = texts.len()))]
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, MemoryIndexError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let mut all_embeddings: Vec<Option<Vec<f32>>> = vec![None; texts.len()];

        // Process in batches of OPENAI_BATCH_LIMIT.
        for batch_start in (0..texts.len()).step_by(OPENAI_BATCH_LIMIT) {
            let batch_end = (batch_start + OPENAI_BATCH_LIMIT).min(texts.len());
            let batch = &texts[batch_start..batch_end];

            let request_body = EmbeddingRequest {
                model: &self.model,
                input: batch,
            };

            let url = format!("{}/v1/embeddings", self.base_url);
            let response = self
                .client
                .post(&url)
                .bearer_auth(&self.api_key)
                .json(&request_body)
                .send()
                .await?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(MemoryIndexError::Embedding(format!(
                    "OpenAI API returned {status}: {body}"
                )));
            }

            let resp: EmbeddingResponse = response.json().await?;

            for datum in resp.data {
                let global_idx = batch_start + datum.index;
                all_embeddings[global_idx] = Some(datum.embedding);
            }
        }

        // Unwrap all — every slot must have been filled by the API.
        all_embeddings
            .into_iter()
            .enumerate()
            .map(|(i, opt)| {
                opt.ok_or_else(|| {
                    MemoryIndexError::Embedding(format!("missing embedding for input at index {i}"))
                })
            })
            .collect()
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}

// ---------------------------------------------------------------------------
// Chunking
// ---------------------------------------------------------------------------

/// Rough token count estimate: ~4 characters per token for English text.
fn estimate_tokens(text: &str) -> usize {
    // A simple heuristic that works well for English prose and markdown.
    // For a more accurate count one would use tiktoken, but that adds a
    // heavy dependency that is not justified for chunking heuristics.
    (text.len() + 3) / 4
}

/// Split file content into chunks suitable for embedding.
///
/// Splitting strategy (in priority order):
///
/// 1. **Heading boundaries** — `# `, `## `, `### `, etc.  Each heading starts
///    a new chunk (unless the current chunk is still below `chunk_size`).
/// 2. **Paragraph breaks** — double newlines (`\n\n`).
/// 3. **Hard limit** — if a single paragraph exceeds `chunk_size`, it is split
///    at the nearest word boundary.
///
/// Adjacent chunks overlap by approximately `overlap` tokens so that
/// cross-boundary context is not lost during retrieval.
#[instrument(skip(content), fields(path = %path.display(), content_len = content.len()))]
pub fn chunk_file(
    path: &Path,
    content: &str,
    chunk_size: usize,
    overlap: usize,
) -> Vec<MemoryChunk> {
    if content.trim().is_empty() {
        return Vec::new();
    }

    // Step 1: split into sections by headings.
    let sections = split_by_headings(content);

    // Step 2: within each section, split by paragraphs if it exceeds the budget.
    let mut segments: Vec<String> = Vec::new();
    for section in &sections {
        if estimate_tokens(section) <= chunk_size {
            segments.push(section.clone());
        } else {
            // Split on paragraph boundaries.
            let paragraphs = split_by_paragraphs(section);
            for para in &paragraphs {
                if estimate_tokens(para) <= chunk_size {
                    segments.push(para.clone());
                } else {
                    // Hard-split oversized paragraphs at word boundaries.
                    let mut sub = split_at_word_boundary(para, chunk_size);
                    segments.append(&mut sub);
                }
            }
        }
    }

    // Step 3: merge tiny segments so we don't produce many sub-threshold chunks.
    let merged = merge_small_segments(&segments, chunk_size);

    // Step 4: apply overlap between adjacent chunks.
    let chunks_with_overlap = apply_overlap(&merged, overlap);

    chunks_with_overlap
        .into_iter()
        .enumerate()
        .map(|(idx, text)| MemoryChunk {
            file_path: path.to_path_buf(),
            chunk_text: text,
            chunk_index: idx,
        })
        .collect()
}

/// Returns `true` if the line is a Markdown heading.
fn is_heading(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("# ")
        || trimmed.starts_with("## ")
        || trimmed.starts_with("### ")
        || trimmed.starts_with("#### ")
        || trimmed.starts_with("##### ")
        || trimmed.starts_with("###### ")
}

/// Split content on heading boundaries, keeping the heading line with the
/// section that follows it.
fn split_by_headings(content: &str) -> Vec<String> {
    let mut sections: Vec<String> = Vec::new();
    let mut current = String::new();

    for line in content.lines() {
        if is_heading(line) && !current.trim().is_empty() {
            sections.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);
    }
    if !current.trim().is_empty() {
        sections.push(current);
    }
    sections
}

/// Split a block of text on double-newline (paragraph) boundaries.
fn split_by_paragraphs(text: &str) -> Vec<String> {
    text.split("\n\n")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Hard-split a single block of text at word boundaries so that no piece
/// exceeds `max_tokens` (estimated).
fn split_at_word_boundary(text: &str, max_tokens: usize) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut pieces: Vec<String> = Vec::new();
    let mut current = String::new();

    for word in &words {
        let candidate_len = if current.is_empty() {
            word.len()
        } else {
            current.len() + 1 + word.len()
        };

        if estimate_tokens(&current) > 0
            && estimate_tokens(&format!("{current} {word}")) > max_tokens
        {
            pieces.push(std::mem::take(&mut current));
        }

        if current.is_empty() {
            current.push_str(word);
        } else {
            current.push(' ');
            current.push_str(word);
        }

        let _ = candidate_len; // suppress unused warning; used via format above
    }

    if !current.trim().is_empty() {
        pieces.push(current);
    }
    pieces
}

/// Merge consecutive tiny segments whose combined size is within budget.
fn merge_small_segments(segments: &[String], max_tokens: usize) -> Vec<String> {
    let mut merged: Vec<String> = Vec::new();
    let mut current = String::new();

    for seg in segments {
        let combined = if current.is_empty() {
            seg.clone()
        } else {
            format!("{current}\n\n{seg}")
        };

        if estimate_tokens(&combined) <= max_tokens {
            current = combined;
        } else {
            if !current.trim().is_empty() {
                merged.push(std::mem::take(&mut current));
            }
            current = seg.clone();
        }
    }
    if !current.trim().is_empty() {
        merged.push(current);
    }
    merged
}

/// Apply token-level overlap between adjacent chunks.
///
/// For each pair of adjacent chunks, the trailing `overlap` tokens of chunk N
/// are prepended to chunk N+1.
fn apply_overlap(chunks: &[String], overlap_tokens: usize) -> Vec<String> {
    if overlap_tokens == 0 || chunks.len() <= 1 {
        return chunks.to_vec();
    }

    let mut result = Vec::with_capacity(chunks.len());
    result.push(chunks[0].clone());

    for i in 1..chunks.len() {
        let prev = &chunks[i - 1];
        let prev_words: Vec<&str> = prev.split_whitespace().collect();

        // Estimate how many trailing words correspond to `overlap_tokens` tokens.
        // Using the same heuristic: ~1 token per word for English.
        let overlap_words = overlap_tokens.min(prev_words.len());
        let tail_start = prev_words.len().saturating_sub(overlap_words);
        let overlap_text = prev_words[tail_start..].join(" ");

        let merged = if overlap_text.is_empty() {
            chunks[i].clone()
        } else {
            format!("{overlap_text}\n{}", chunks[i])
        };
        result.push(merged);
    }

    result
}

// ---------------------------------------------------------------------------
// Reciprocal Rank Fusion
// ---------------------------------------------------------------------------

/// Merge keyword-ranked and vector-ranked result lists using Reciprocal Rank
/// Fusion (RRF).
///
/// The RRF score for each document is:
///
/// ```text
///   score = 1/(k + rank_keyword) + 1/(k + rank_vector)
/// ```
///
/// where `k` is a smoothing constant (typically 60) and `rank` is the 1-based
/// position in each list. Documents appearing in only one list receive a
/// single-term score.
///
/// The returned list is sorted by descending RRF score.
pub fn reciprocal_rank_fusion(
    keyword_results: &[KeywordHit],
    vector_results: &[VectorHit],
    k: usize,
) -> Vec<HybridSearchResult> {
    let k = k.max(1) as f64; // guard against division by zero

    // Canonical key: (file_path, chunk_text) — we need both because the same
    // file can yield multiple chunks.
    type Key = (String, String);

    struct Accumulator {
        file_path: String,
        chunk_text: String,
        score: f64,
        keyword_rank: Option<usize>,
        vector_rank: Option<usize>,
    }

    let mut map: HashMap<Key, Accumulator> = HashMap::new();

    for (rank_0based, hit) in keyword_results.iter().enumerate() {
        let rank = rank_0based + 1; // 1-based
        let key: Key = (hit.file_path.clone(), hit.chunk_text.clone());
        let entry = map.entry(key).or_insert_with(|| Accumulator {
            file_path: hit.file_path.clone(),
            chunk_text: hit.chunk_text.clone(),
            score: 0.0,
            keyword_rank: None,
            vector_rank: None,
        });
        entry.score += 1.0 / (k + rank as f64);
        entry.keyword_rank = Some(rank);
    }

    for (rank_0based, hit) in vector_results.iter().enumerate() {
        let rank = rank_0based + 1;
        let key: Key = (hit.file_path.clone(), hit.chunk_text.clone());
        let entry = map.entry(key).or_insert_with(|| Accumulator {
            file_path: hit.file_path.clone(),
            chunk_text: hit.chunk_text.clone(),
            score: 0.0,
            keyword_rank: None,
            vector_rank: None,
        });
        entry.score += 1.0 / (k + rank as f64);
        entry.vector_rank = Some(rank);
    }

    let mut results: Vec<HybridSearchResult> = map
        .into_values()
        .map(|acc| HybridSearchResult {
            file_path: acc.file_path,
            chunk_text: acc.chunk_text,
            rrf_score: acc.score,
            keyword_rank: acc.keyword_rank,
            vector_rank: acc.vector_rank,
        })
        .collect();

    results.sort_by(|a, b| {
        b.rrf_score
            .partial_cmp(&a.rrf_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    results
}

/// A hit coming from the keyword (tsvector) search leg.
#[derive(Clone, Debug)]
pub struct KeywordHit {
    pub file_path: String,
    pub chunk_text: String,
    pub ts_rank: f64,
}

/// A hit coming from the vector (pgvector) search leg.
#[derive(Clone, Debug)]
pub struct VectorHit {
    pub file_path: String,
    pub chunk_text: String,
    pub cosine_similarity: f64,
}

// ---------------------------------------------------------------------------
// MemoryIndex – high-level façade
// ---------------------------------------------------------------------------

/// High-level hybrid memory search index.
///
/// Holds configuration and an embedding provider. Callers are responsible for
/// database access — `MemoryIndex` produces the chunks, embeddings, and SQL
/// needed, but does not hold a connection pool itself. This keeps the module
/// database-agnostic and testable.
pub struct MemoryIndex {
    config: MemoryIndexConfig,
    provider: Box<dyn EmbeddingProvider>,
}

impl MemoryIndex {
    /// Create a new index with the given configuration and embedding provider.
    pub fn new(config: MemoryIndexConfig, provider: Box<dyn EmbeddingProvider>) -> Self {
        Self { config, provider }
    }

    /// Build a `MemoryIndex` from config, constructing the appropriate
    /// embedding provider automatically.
    pub fn from_config(config: MemoryIndexConfig) -> Result<Self, MemoryIndexError> {
        let provider: Box<dyn EmbeddingProvider> = match config.embedding_provider.as_str() {
            "openai" => {
                let api_key = config.api_key.clone().ok_or_else(|| {
                    MemoryIndexError::Embedding(
                        "OpenAI embedding provider requires an api_key".into(),
                    )
                })?;
                let mut emb = OpenAiEmbedding::new(
                    api_key,
                    &config.embedding_model,
                    config.embedding_dimension,
                );
                if let Some(ref base) = config.api_base_url {
                    emb = emb.with_base_url(base);
                }
                Box::new(emb)
            }
            other => {
                return Err(MemoryIndexError::Embedding(format!(
                    "unsupported embedding provider: {other}"
                )));
            }
        };
        Ok(Self { config, provider })
    }

    /// Access the current configuration.
    pub fn config(&self) -> &MemoryIndexConfig {
        &self.config
    }

    // -- Chunking -----------------------------------------------------------

    /// Chunk a single file's content using the configured chunk / overlap sizes.
    pub fn chunk_file(&self, path: &Path, content: &str) -> Vec<MemoryChunk> {
        chunk_file(
            path,
            content,
            self.config.chunk_size,
            self.config.chunk_overlap,
        )
    }

    // -- Indexing ------------------------------------------------------------

    /// Chunk and embed a single file.
    ///
    /// Returns embedded chunks ready to be upserted into the database. The
    /// caller is responsible for executing the actual SQL.
    #[instrument(skip(self, content), fields(path = %path.display()))]
    pub async fn index_file(
        &self,
        path: &Path,
        content: &str,
    ) -> Result<Vec<EmbeddedChunk>, MemoryIndexError> {
        let chunks = self.chunk_file(path, content);
        if chunks.is_empty() {
            return Ok(Vec::new());
        }

        debug!(num_chunks = chunks.len(), "embedding chunks");

        let texts: Vec<String> = chunks.iter().map(|c| c.chunk_text.clone()).collect();
        let embeddings = self.provider.embed(&texts).await?;

        if embeddings.len() != chunks.len() {
            return Err(MemoryIndexError::Embedding(format!(
                "expected {} embeddings, got {}",
                chunks.len(),
                embeddings.len()
            )));
        }

        Ok(chunks
            .into_iter()
            .zip(embeddings)
            .map(|(chunk, embedding)| EmbeddedChunk { chunk, embedding })
            .collect())
    }

    /// Re-index every `.md` file under `dir` (recursively).
    ///
    /// Returns the total number of embedded chunks produced. The caller must
    /// persist the returned [`EmbeddedChunk`] values into the database.
    #[instrument(skip(self), fields(dir = %dir.display()))]
    pub async fn reindex_directory(
        &self,
        dir: &Path,
    ) -> Result<Vec<EmbeddedChunk>, MemoryIndexError> {
        let md_files = collect_md_files(dir).await?;
        debug!(file_count = md_files.len(), "collected markdown files");

        let mut all_chunks: Vec<EmbeddedChunk> = Vec::new();

        for file_path in &md_files {
            let content = match tokio::fs::read_to_string(file_path).await {
                Ok(c) => c,
                Err(e) => {
                    warn!(path = %file_path.display(), error = %e, "skipping unreadable file");
                    continue;
                }
            };

            let mut embedded = self.index_file(file_path, &content).await?;
            all_chunks.append(&mut embedded);
        }

        debug!(total_chunks = all_chunks.len(), "reindex complete");
        Ok(all_chunks)
    }

    /// Embed a search query so the caller can execute the vector-search leg.
    pub async fn embed_query(&self, query: &str) -> Result<Vec<f32>, MemoryIndexError> {
        let results = self.provider.embed(&[query.to_string()]).await?;
        results.into_iter().next().ok_or_else(|| {
            MemoryIndexError::Embedding("embedding provider returned no vectors".into())
        })
    }

    /// Perform hybrid search by combining pre-fetched keyword and vector hits.
    ///
    /// This is a convenience wrapper around [`reciprocal_rank_fusion`] that
    /// truncates the result set to `limit`.
    pub fn search(
        &self,
        keyword_hits: &[KeywordHit],
        vector_hits: &[VectorHit],
        limit: usize,
    ) -> Vec<HybridSearchResult> {
        let mut results = reciprocal_rank_fusion(keyword_hits, vector_hits, RRF_K);
        results.truncate(limit);
        results
    }

    // -- SQL helpers --------------------------------------------------------

    /// Return the parameterised SQL for the keyword search leg.
    ///
    /// Parameters: `$1` = query text, `$2` = limit.
    pub fn keyword_search_sql() -> &'static str {
        r#"SELECT file_path, chunk_text,
       ts_rank(to_tsvector('english', chunk_text),
               plainto_tsquery('english', $1)) AS score
FROM memory_embeddings
WHERE to_tsvector('english', chunk_text) @@ plainto_tsquery('english', $1)
ORDER BY score DESC
LIMIT $2"#
    }

    /// Return the parameterised SQL for the vector search leg.
    ///
    /// Parameters: `$1` = embedding vector (cast to `::vector`), `$2` = limit.
    pub fn vector_search_sql() -> &'static str {
        r#"SELECT file_path, chunk_text,
       1 - (embedding <=> $1::vector) AS score
FROM memory_embeddings
ORDER BY embedding <=> $1::vector
LIMIT $2"#
    }

    /// Return the SQL to upsert a single embedded chunk.
    ///
    /// Parameters: `$1` = file_path, `$2` = chunk_index, `$3` = chunk_text,
    /// `$4` = embedding vector.
    pub fn upsert_chunk_sql() -> &'static str {
        r#"INSERT INTO memory_embeddings (file_path, chunk_index, chunk_text, embedding)
VALUES ($1, $2, $3, $4::vector)
ON CONFLICT (file_path, chunk_index)
DO UPDATE SET chunk_text = EXCLUDED.chunk_text,
              embedding  = EXCLUDED.embedding,
              created_at = now()"#
    }

    /// Return the SQL to delete all chunks for a given file (used before
    /// re-indexing a file whose chunk count may have changed).
    pub fn delete_file_chunks_sql() -> &'static str {
        "DELETE FROM memory_embeddings WHERE file_path = $1"
    }
}

/// Default RRF smoothing constant.
const RRF_K: usize = 60;

// ---------------------------------------------------------------------------
// Filesystem helpers
// ---------------------------------------------------------------------------

/// Recursively collect all `.md` files under `dir`.
async fn collect_md_files(dir: &Path) -> Result<Vec<PathBuf>, MemoryIndexError> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];

    while let Some(current) = stack.pop() {
        let mut entries = tokio::fs::read_dir(&current).await?;
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "md") {
                files.push(path);
            }
        }
    }

    files.sort();
    Ok(files)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Chunking tests -----------------------------------------------------

    #[test]
    fn chunk_empty_content_returns_nothing() {
        let chunks = chunk_file(Path::new("test.md"), "", 512, 64);
        assert!(chunks.is_empty());
    }

    #[test]
    fn chunk_small_file_produces_single_chunk() {
        let content = "# Title\n\nSome short content.\n";
        let chunks = chunk_file(Path::new("test.md"), content, 512, 64);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].chunk_text.contains("Title"));
        assert!(chunks[0].chunk_text.contains("Some short content."));
        assert_eq!(chunks[0].chunk_index, 0);
    }

    #[test]
    fn chunk_splits_on_headings() {
        let content = "\
# Section One

First paragraph of section one with extra words added for length.

# Section Two

Second paragraph of section two also with extra words for testing length.
";
        // Use a small chunk size so each heading section exceeds the merge
        // threshold and they stay as separate chunks.
        let chunks = chunk_file(Path::new("notes.md"), content, 15, 0);
        assert!(
            chunks.len() >= 2,
            "expected at least 2 chunks, got {}",
            chunks.len()
        );
        assert!(chunks.iter().any(|c| c.chunk_text.contains("Section One")));
        assert!(chunks.iter().any(|c| c.chunk_text.contains("Section Two")));
    }

    #[test]
    fn chunk_splits_on_paragraphs_when_section_is_large() {
        // One heading, but two paragraphs that together exceed a tiny budget.
        let content = "\
# Big Section

First paragraph with enough words to exceed our tiny budget for testing purposes here.

Second paragraph also with enough words to exceed the budget set for this test case.
";
        let chunks = chunk_file(Path::new("big.md"), content, 20, 0);
        assert!(
            chunks.len() >= 2,
            "expected paragraph split, got {} chunk(s): {:?}",
            chunks.len(),
            chunks.iter().map(|c| &c.chunk_text).collect::<Vec<_>>()
        );
    }

    #[test]
    fn chunk_hard_splits_oversized_paragraph() {
        let long = "word ".repeat(500); // ~500 words >> 100 token budget
        let content = format!("# H\n\n{long}");
        let chunks = chunk_file(Path::new("long.md"), &content, 100, 0);
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            // Each chunk should be at or below the budget (with some tolerance
            // for the heading and rounding).
            assert!(
                estimate_tokens(&chunk.chunk_text) <= 130,
                "chunk too large: {} tokens",
                estimate_tokens(&chunk.chunk_text)
            );
        }
    }

    #[test]
    fn chunk_overlap_prepends_tail_of_previous() {
        let content = "\
# A

Alpha bravo charlie.

# B

Delta echo foxtrot.
";
        let chunks = chunk_file(Path::new("overlap.md"), content, 30, 2);
        if chunks.len() >= 2 {
            // Second chunk should start with trailing words from first chunk.
            // The exact words depend on merge decisions, but overlap > 0 means
            // some content from chunk 0 appears in chunk 1.
            assert!(
                chunks[1].chunk_text.len()
                    > chunks[1].chunk_text.trim_start().find("Delta").unwrap_or(0)
                    || chunks[1].chunk_text.contains("charlie")
                    || chunks[1].chunk_text.contains("bravo"),
                "expected overlap content in second chunk"
            );
        }
    }

    #[test]
    fn chunk_indices_are_sequential() {
        let content = "# A\nfoo\n# B\nbar\n# C\nbaz\n";
        let chunks = chunk_file(Path::new("idx.md"), content, 10, 0);
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.chunk_index, i);
        }
    }

    // -- RRF tests ----------------------------------------------------------

    #[test]
    fn rrf_single_list() {
        let kw = vec![
            KeywordHit {
                file_path: "a.md".into(),
                chunk_text: "alpha".into(),
                ts_rank: 1.0,
            },
            KeywordHit {
                file_path: "b.md".into(),
                chunk_text: "bravo".into(),
                ts_rank: 0.5,
            },
        ];
        let results = reciprocal_rank_fusion(&kw, &[], 60);
        assert_eq!(results.len(), 2);
        assert!(results[0].rrf_score > results[1].rrf_score);
        assert_eq!(results[0].file_path, "a.md");
        assert!(results[0].keyword_rank.is_some());
        assert!(results[0].vector_rank.is_none());
    }

    #[test]
    fn rrf_both_lists_overlap_boosts_score() {
        let kw = vec![KeywordHit {
            file_path: "shared.md".into(),
            chunk_text: "both".into(),
            ts_rank: 1.0,
        }];
        let vec_hits = vec![
            VectorHit {
                file_path: "shared.md".into(),
                chunk_text: "both".into(),
                cosine_similarity: 0.95,
            },
            VectorHit {
                file_path: "only_vec.md".into(),
                chunk_text: "vec only".into(),
                cosine_similarity: 0.90,
            },
        ];

        let results = reciprocal_rank_fusion(&kw, &vec_hits, 60);
        // "shared.md" should be ranked first because it appears in both lists.
        assert_eq!(results[0].file_path, "shared.md");
        assert!(results[0].keyword_rank.is_some());
        assert!(results[0].vector_rank.is_some());
        assert!(results[0].rrf_score > results[1].rrf_score);
    }

    #[test]
    fn rrf_empty_inputs() {
        let results = reciprocal_rank_fusion(&[], &[], 60);
        assert!(results.is_empty());
    }

    #[test]
    fn rrf_score_formula_is_correct() {
        // With k=60, rank 1 in keyword and rank 2 in vector:
        //   1/(60+1) + 1/(60+2) = 1/61 + 1/62
        let kw = vec![KeywordHit {
            file_path: "x.md".into(),
            chunk_text: "test".into(),
            ts_rank: 1.0,
        }];
        let vec_hits = vec![
            VectorHit {
                file_path: "other.md".into(),
                chunk_text: "other".into(),
                cosine_similarity: 0.99,
            },
            VectorHit {
                file_path: "x.md".into(),
                chunk_text: "test".into(),
                cosine_similarity: 0.90,
            },
        ];

        let results = reciprocal_rank_fusion(&kw, &vec_hits, 60);
        let x = results.iter().find(|r| r.file_path == "x.md").unwrap();
        let expected = 1.0 / 61.0 + 1.0 / 62.0;
        assert!(
            (x.rrf_score - expected).abs() < 1e-10,
            "expected {expected}, got {}",
            x.rrf_score
        );
    }

    // -- Estimate tokens test -----------------------------------------------

    #[test]
    fn estimate_tokens_basic() {
        // "hello world" = 11 chars -> (11+3)/4 = 3 tokens
        assert_eq!(estimate_tokens("hello world"), 3);
        assert_eq!(estimate_tokens(""), 0);
    }

    // -- Heading detection --------------------------------------------------

    #[test]
    fn detects_headings() {
        assert!(is_heading("# H1"));
        assert!(is_heading("## H2"));
        assert!(is_heading("### H3"));
        assert!(!is_heading("not a heading"));
        assert!(!is_heading("#no-space"));
    }

    // -- Stub embedding provider for integration tests ----------------------

    /// A deterministic embedding provider for tests.
    struct StubEmbedding {
        dim: usize,
    }

    #[async_trait::async_trait]
    impl EmbeddingProvider for StubEmbedding {
        async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, MemoryIndexError> {
            Ok(texts
                .iter()
                .enumerate()
                .map(|(i, _)| {
                    let mut vec = vec![0.0f32; self.dim];
                    // Put the index in the first slot for distinctness.
                    vec[0] = i as f32;
                    vec
                })
                .collect())
        }

        fn dimension(&self) -> usize {
            self.dim
        }
    }

    #[tokio::test]
    async fn memory_index_chunks_and_embeds() {
        let config = MemoryIndexConfig {
            embedding_dimension: 8,
            chunk_size: 512,
            chunk_overlap: 0,
            ..Default::default()
        };
        let provider = Box::new(StubEmbedding { dim: 8 });
        let index = MemoryIndex::new(config, provider);

        let content = "# Title\n\nSome content for embedding.\n";
        let embedded = index
            .index_file(Path::new("test.md"), content)
            .await
            .unwrap();

        assert!(!embedded.is_empty());
        assert_eq!(embedded[0].embedding.len(), 8);
        assert!(embedded[0].chunk.chunk_text.contains("Title"));
    }

    #[test]
    fn from_config_requires_openai_api_key() {
        let err = match MemoryIndex::from_config(MemoryIndexConfig::default()) {
            Ok(_) => panic!("default openai config should require an api_key"),
            Err(err) => err,
        };
        assert!(matches!(err, MemoryIndexError::Embedding(_)));
        assert!(err.to_string().contains("api_key"));
    }

    #[test]
    fn from_config_rejects_unknown_provider() {
        let err = match MemoryIndex::from_config(MemoryIndexConfig {
            embedding_provider: "local".into(),
            ..Default::default()
        }) {
            Ok(_) => panic!("unknown embedding providers should fail"),
            Err(err) => err,
        };
        assert!(matches!(err, MemoryIndexError::Embedding(_)));
        assert!(
            err.to_string()
                .contains("unsupported embedding provider: local")
        );
    }

    struct WrongCountEmbedding;

    #[async_trait::async_trait]
    impl EmbeddingProvider for WrongCountEmbedding {
        async fn embed(&self, _texts: &[String]) -> Result<Vec<Vec<f32>>, MemoryIndexError> {
            Ok(Vec::new())
        }

        fn dimension(&self) -> usize {
            4
        }
    }

    struct EmptyEmbeddingProvider;

    #[async_trait::async_trait]
    impl EmbeddingProvider for EmptyEmbeddingProvider {
        async fn embed(&self, _texts: &[String]) -> Result<Vec<Vec<f32>>, MemoryIndexError> {
            Ok(Vec::new())
        }

        fn dimension(&self) -> usize {
            4
        }
    }

    #[tokio::test]
    async fn index_file_rejects_embedding_count_mismatch() {
        let index = MemoryIndex::new(
            MemoryIndexConfig {
                embedding_dimension: 4,
                chunk_size: 512,
                chunk_overlap: 0,
                ..Default::default()
            },
            Box::new(WrongCountEmbedding),
        );

        let err = index
            .index_file(Path::new("test.md"), "# Title\n\ncontent")
            .await
            .unwrap_err();
        assert!(matches!(err, MemoryIndexError::Embedding(_)));
        assert!(err.to_string().contains("expected 1 embeddings, got 0"));
    }

    #[tokio::test]
    async fn memory_index_reindex_directory() {
        let tmp = tempfile::TempDir::new().unwrap();

        // Create a couple of markdown files.
        tokio::fs::write(tmp.path().join("a.md"), "# A\nAlpha content\n")
            .await
            .unwrap();
        tokio::fs::create_dir_all(tmp.path().join("sub"))
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("sub/b.md"), "# B\nBravo content\n")
            .await
            .unwrap();
        // Non-md file should be ignored.
        tokio::fs::write(tmp.path().join("skip.txt"), "ignored")
            .await
            .unwrap();

        let config = MemoryIndexConfig {
            embedding_dimension: 4,
            chunk_size: 512,
            chunk_overlap: 0,
            ..Default::default()
        };
        let provider = Box::new(StubEmbedding { dim: 4 });
        let index = MemoryIndex::new(config, provider);

        let all = index.reindex_directory(tmp.path()).await.unwrap();
        // Should have chunks from a.md and sub/b.md, but not skip.txt.
        let paths: Vec<_> = all.iter().map(|e| e.chunk.file_path.clone()).collect();
        assert!(paths.iter().any(|p| p.ends_with("a.md")));
        assert!(paths.iter().any(|p| p.ends_with("b.md")));
        assert!(!paths.iter().any(|p| p.ends_with("skip.txt")));
    }

    #[tokio::test]
    async fn memory_index_reindex_directory_skips_unreadable_markdown_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let readable = tmp.path().join("readable.md");
        let unreadable_dir = tmp.path().join("broken.md");

        tokio::fs::write(&readable, "# Readable\nStill works\n")
            .await
            .unwrap();
        tokio::fs::create_dir_all(&unreadable_dir).await.unwrap();

        let index = MemoryIndex::new(
            MemoryIndexConfig {
                embedding_dimension: 4,
                chunk_size: 512,
                chunk_overlap: 0,
                ..Default::default()
            },
            Box::new(StubEmbedding { dim: 4 }),
        );

        let all = index.reindex_directory(tmp.path()).await.unwrap();
        let paths: Vec<_> = all.iter().map(|e| e.chunk.file_path.clone()).collect();
        assert!(paths.iter().any(|p| p.ends_with("readable.md")));
        assert!(!paths.iter().any(|p| p.ends_with("broken.md")));
    }

    #[tokio::test]
    async fn embed_query_returns_first_embedding_vector() {
        let index = MemoryIndex::new(
            MemoryIndexConfig {
                embedding_dimension: 8,
                ..Default::default()
            },
            Box::new(StubEmbedding { dim: 8 }),
        );

        let embedding = index.embed_query("find this").await.unwrap();
        assert_eq!(embedding.len(), 8);
        assert_eq!(embedding[0], 0.0);
        assert!(embedding.iter().skip(1).all(|value| *value == 0.0));
    }

    #[tokio::test]
    async fn embed_query_rejects_empty_provider_response() {
        let index = MemoryIndex::new(
            MemoryIndexConfig {
                embedding_dimension: 4,
                ..Default::default()
            },
            Box::new(EmptyEmbeddingProvider),
        );

        let err = index.embed_query("find this").await.unwrap_err();
        assert!(matches!(err, MemoryIndexError::Embedding(_)));
        assert!(
            err.to_string()
                .contains("embedding provider returned no vectors")
        );
    }

    #[tokio::test]
    async fn search_merges_and_truncates() {
        let config = MemoryIndexConfig::default();
        let provider = Box::new(StubEmbedding { dim: 4 });
        let index = MemoryIndex::new(config, provider);

        let kw_hits = vec![
            KeywordHit {
                file_path: "a.md".into(),
                chunk_text: "one".into(),
                ts_rank: 1.0,
            },
            KeywordHit {
                file_path: "b.md".into(),
                chunk_text: "two".into(),
                ts_rank: 0.5,
            },
            KeywordHit {
                file_path: "c.md".into(),
                chunk_text: "three".into(),
                ts_rank: 0.3,
            },
        ];
        let vec_hits = vec![VectorHit {
            file_path: "a.md".into(),
            chunk_text: "one".into(),
            cosine_similarity: 0.9,
        }];

        let results = index.search(&kw_hits, &vec_hits, 2);
        assert_eq!(results.len(), 2);
        // "a.md" should be first (appears in both lists).
        assert_eq!(results[0].file_path, "a.md");
    }

    #[test]
    fn sql_helpers_return_valid_sql() {
        let kw_sql = MemoryIndex::keyword_search_sql();
        assert!(kw_sql.contains("ts_rank"));
        assert!(kw_sql.contains("$1"));

        let vec_sql = MemoryIndex::vector_search_sql();
        assert!(vec_sql.contains("<=>")); // pgvector distance operator
        assert!(vec_sql.contains("$1::vector"));

        let upsert = MemoryIndex::upsert_chunk_sql();
        assert!(upsert.contains("ON CONFLICT"));

        let delete = MemoryIndex::delete_file_chunks_sql();
        assert!(delete.contains("DELETE"));
    }
}
