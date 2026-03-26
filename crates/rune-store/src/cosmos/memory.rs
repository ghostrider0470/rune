//! Cosmos DB implementation of [`MemoryEmbeddingRepo`](crate::repos::MemoryEmbeddingRepo).

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::cosmos::{collect_query, pk, CosmosStore};
use crate::error::StoreError;
use crate::models::{KeywordSearchRow, VectorSearchRow};
use crate::repos::MemoryEmbeddingRepo;
use azure_data_cosmos::PartitionKey;

/// Cosmos document representation for a memory embedding chunk.
#[derive(Debug, Serialize, Deserialize)]
struct MemoryEmbeddingDoc {
    id: String,
    pk: String,
    #[serde(rename = "type")]
    doc_type: String,
    file_path: String,
    chunk_index: i32,
    chunk_text: String,
    embedding: Vec<f32>,
    created_at: chrono::DateTime<chrono::Utc>,
}

/// Helper to build the partition key for memory embedding docs.
fn mem_pk(file_path: &str) -> String {
    format!("mem:{}", file_path)
}

/// Helper to build the document id for a memory embedding chunk.
fn mem_id(file_path: &str, chunk_index: i32) -> String {
    format!("{}:{}", file_path, chunk_index)
}

/// Deserialize helper for DISTINCT file_path query results.
#[derive(Debug, Deserialize)]
struct FilePathResult {
    file_path: String,
}

/// Deserialize helper for vector search results.
#[derive(Debug, Deserialize)]
struct VectorSearchResult {
    file_path: String,
    chunk_text: String,
    score: f64,
}

/// Deserialize helper for keyword search results.
#[derive(Debug, Deserialize)]
struct KeywordSearchResult {
    file_path: String,
    chunk_text: String,
}

#[async_trait]
impl MemoryEmbeddingRepo for CosmosStore {
    async fn upsert_chunk(
        &self,
        file_path: &str,
        chunk_index: i32,
        chunk_text: &str,
        embedding: &[f32],
    ) -> Result<(), StoreError> {
        let doc = MemoryEmbeddingDoc {
            id: mem_id(file_path, chunk_index),
            pk: mem_pk(file_path),
            doc_type: "memory_embedding".to_string(),
            file_path: file_path.to_string(),
            chunk_index,
            chunk_text: chunk_text.to_string(),
            embedding: embedding.to_vec(),
            created_at: Utc::now(),
        };
        let pk_val = doc.pk.clone();
        self.container()
            .upsert_item(pk(&pk_val), &doc, None)
            .await?;
        Ok(())
    }

    async fn delete_by_file(&self, file_path: &str) -> Result<usize, StoreError> {
        let pk_val = mem_pk(file_path);
        let query = "SELECT c.id FROM c WHERE c.type = 'memory_embedding'";
        let stream = self
            .container()
            .query_items::<serde_json::Value>(query, pk(&pk_val), None)
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let ids: Vec<serde_json::Value> = collect_query(stream).await?;
        let mut count = 0usize;
        for val in &ids {
            if let Some(doc_id) = val.get("id").and_then(|v| v.as_str()) {
                self.container()
                    .delete_item(pk(&pk_val), doc_id, None)
                    .await?;
                count += 1;
            }
        }
        Ok(count)
    }

    async fn keyword_search(
        &self,
        query: &str,
        limit: i64,
    ) -> Result<Vec<KeywordSearchRow>, StoreError> {
        let escaped = query.replace('\'', "''").to_lowercase();
        let sql = format!(
            "SELECT c.file_path, c.chunk_text FROM c \
             WHERE c.type = 'memory_embedding' \
             AND CONTAINS(LOWER(c.chunk_text), '{}') \
             OFFSET 0 LIMIT {}",
            escaped, limit
        );
        let stream = self
            .container()
            .query_items::<serde_json::Value>(&sql, PartitionKey::EMPTY, None)
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let results: Vec<KeywordSearchResult> = collect_query(stream).await?;
        Ok(results
            .into_iter()
            .map(|r| KeywordSearchRow {
                file_path: r.file_path,
                chunk_text: r.chunk_text,
                score: 1.0,
            })
            .collect())
    }

    async fn vector_search(
        &self,
        embedding: &[f32],
        limit: i64,
    ) -> Result<Vec<VectorSearchRow>, StoreError> {
        // Build the vector as a JSON array literal inline.
        let vec_str: String = format!(
            "[{}]",
            embedding
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        let sql = format!(
            "SELECT TOP {} c.file_path, c.chunk_text, \
             VectorDistance(c.embedding, {}) AS score \
             FROM c WHERE c.type = 'memory_embedding' \
             ORDER BY VectorDistance(c.embedding, {})",
            limit, vec_str, vec_str
        );
        let stream = self
            .container()
            .query_items::<serde_json::Value>(&sql, PartitionKey::EMPTY, None)
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let results: Vec<VectorSearchResult> = collect_query(stream).await?;
        Ok(results
            .into_iter()
            .map(|r| VectorSearchRow {
                file_path: r.file_path,
                chunk_text: r.chunk_text,
                score: r.score,
            })
            .collect())
    }

    async fn count(&self) -> Result<i64, StoreError> {
        let query = "SELECT VALUE COUNT(1) FROM c WHERE c.type = 'memory_embedding'";
        let stream = self
            .container()
            .query_items::<serde_json::Value>(query, PartitionKey::EMPTY, None)
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let results: Vec<serde_json::Value> = collect_query(stream).await?;
        // VALUE COUNT returns a single scalar.
        if let Some(val) = results.into_iter().next() {
            Ok(val.as_i64().unwrap_or(0))
        } else {
            Ok(0)
        }
    }

    async fn list_indexed_files(&self) -> Result<Vec<String>, StoreError> {
        let query =
            "SELECT DISTINCT c.file_path FROM c WHERE c.type = 'memory_embedding'";
        let stream = self
            .container()
            .query_items::<serde_json::Value>(query, PartitionKey::EMPTY, None)
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let results: Vec<FilePathResult> = collect_query(stream).await?;
        Ok(results.into_iter().map(|r| r.file_path).collect())
    }

    async fn delete_chunk(&self, file_path: &str, chunk_index: i32) -> Result<bool, StoreError> {
        let pk_val = mem_pk(file_path);
        let item_id = mem_id(file_path, chunk_index);
        match self
            .container()
            .delete_item(pk(&pk_val), &item_id, None)
            .await
        {
            Ok(_) => Ok(true),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("NotFound") || msg.contains("404") {
                    Ok(false)
                } else {
                    Err(StoreError::Database(msg))
                }
            }
        }
    }

    async fn delete_all(&self) -> Result<usize, StoreError> {
        let query =
            "SELECT c.id, c.pk FROM c WHERE c.type = 'memory_embedding'";
        let stream = self
            .container()
            .query_items::<serde_json::Value>(query, PartitionKey::EMPTY, None)
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let items: Vec<serde_json::Value> = collect_query(stream).await?;
        let mut count = 0usize;
        for val in &items {
            if let (Some(doc_id), Some(doc_pk)) = (
                val.get("id").and_then(|v| v.as_str()),
                val.get("pk").and_then(|v| v.as_str()),
            ) {
                self.container()
                    .delete_item(pk(doc_pk), doc_id, None)
                    .await?;
                count += 1;
            }
        }
        Ok(count)
    }
}
