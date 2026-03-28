//! Cosmos DB implementation of [`MemoryEmbeddingRepo`](crate::repos::MemoryEmbeddingRepo).

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::cosmos::{CosmosStore, collect_query, pk};
use crate::error::StoreError;
use crate::models::{KeywordSearchRow, VectorSearchRow};
use crate::repos::MemoryEmbeddingRepo;

/// Cosmos document representation for a memory embedding chunk.
#[derive(Debug, Serialize, Deserialize)]
struct MemoryEmbeddingDoc {
    id: String,
    pk: String,
    #[serde(rename = "type")]
    doc_type: String,
    project_id: Option<String>,
    file_path: String,
    chunk_index: i32,
    chunk_text: String,
    embedding: Vec<f32>,
    created_at: chrono::DateTime<chrono::Utc>,
}

fn mem_pk(project_id: Option<&str>, file_path: &str) -> String {
    format!("mem:{}:{}", project_id.unwrap_or("default"), file_path)
}

fn mem_id(project_id: Option<&str>, file_path: &str, chunk_index: i32) -> String {
    format!("{}:{}:{}", project_id.unwrap_or(""), file_path, chunk_index)
}

#[derive(Debug, Deserialize)]
struct FilePathResult {
    file_path: String,
}

#[derive(Debug, Deserialize)]
struct VectorSearchResult {
    project_id: Option<String>,
    file_path: String,
    chunk_text: String,
    score: f64,
}

#[derive(Debug, Deserialize)]
struct KeywordSearchResult {
    project_id: Option<String>,
    file_path: String,
    chunk_text: String,
}

#[async_trait]
impl MemoryEmbeddingRepo for CosmosStore {
    async fn upsert_chunk(
        &self,
        project_id: Option<&str>,
        file_path: &str,
        chunk_index: i32,
        chunk_text: &str,
        embedding: &[f32],
    ) -> Result<(), StoreError> {
        let doc = MemoryEmbeddingDoc {
            id: mem_id(project_id, file_path, chunk_index),
            pk: mem_pk(project_id, file_path),
            doc_type: "memory_embedding".to_string(),
            project_id: project_id.map(str::to_string),
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

    async fn delete_by_file(
        &self,
        project_id: Option<&str>,
        file_path: &str,
    ) -> Result<usize, StoreError> {
        let pk_val = mem_pk(project_id, file_path);
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
        project_id: Option<&str>,
        query: &str,
        limit: i64,
    ) -> Result<Vec<KeywordSearchRow>, StoreError> {
        let escaped = query.replace('\'', "''").to_lowercase();
        let project_clause = match project_id {
            Some(project_id) => format!(" AND c.project_id = '{}'", project_id.replace('\'', "''")),
            None => String::new(),
        };
        let sql = format!(
            "SELECT TOP {} c.project_id, c.file_path, c.chunk_text FROM c \
             WHERE c.type = 'memory_embedding'{} \
             AND CONTAINS(LOWER(c.chunk_text), '{}')",
            limit, project_clause, escaped
        );
        let results: Vec<KeywordSearchResult> = self.query_cross_partition(&sql).await?;
        Ok(results
            .into_iter()
            .map(|r| KeywordSearchRow {
                project_id: r.project_id,
                file_path: r.file_path,
                chunk_text: r.chunk_text,
                score: 1.0,
            })
            .collect())
    }

    async fn vector_search(
        &self,
        project_id: Option<&str>,
        embedding: &[f32],
        limit: i64,
    ) -> Result<Vec<VectorSearchRow>, StoreError> {
        let vec_str = format!(
            "[{}]",
            embedding
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        let project_clause = match project_id {
            Some(project_id) => format!(" AND c.project_id = '{}'", project_id.replace('\'', "''")),
            None => String::new(),
        };
        let sql = format!(
            "SELECT TOP {} c.project_id, c.file_path, c.chunk_text, \
             VectorDistance(c.embedding, {}) AS score \
             FROM c WHERE c.type = 'memory_embedding'{} \
             ORDER BY VectorDistance(c.embedding, {})",
            limit, vec_str, project_clause, vec_str
        );
        let results: Vec<VectorSearchResult> = self.query_cross_partition(&sql).await?;
        Ok(results
            .into_iter()
            .map(|r| VectorSearchRow {
                project_id: r.project_id,
                file_path: r.file_path,
                chunk_text: r.chunk_text,
                score: r.score,
            })
            .collect())
    }

    async fn count(&self, project_id: Option<&str>) -> Result<i64, StoreError> {
        let project_clause = match project_id {
            Some(project_id) => format!(" AND c.project_id = '{}'", project_id.replace('\'', "''")),
            None => String::new(),
        };
        let query = format!(
            "SELECT VALUE COUNT(1) FROM c WHERE c.type = 'memory_embedding'{}",
            project_clause
        );
        let results: Vec<serde_json::Value> = self.query_cross_partition(&query).await?;
        Ok(results
            .into_iter()
            .next()
            .and_then(|v| v.as_i64())
            .unwrap_or(0))
    }

    async fn list_indexed_files(
        &self,
        project_id: Option<&str>,
    ) -> Result<Vec<String>, StoreError> {
        let project_clause = match project_id {
            Some(project_id) => format!(" AND c.project_id = '{}'", project_id.replace('\'', "''")),
            None => String::new(),
        };
        let query = format!(
            "SELECT DISTINCT c.file_path FROM c WHERE c.type = 'memory_embedding'{}",
            project_clause
        );
        let results: Vec<FilePathResult> = self.query_cross_partition(&query).await?;
        Ok(results.into_iter().map(|r| r.file_path).collect())
    }

    async fn delete_chunk(
        &self,
        project_id: Option<&str>,
        file_path: &str,
        chunk_index: i32,
    ) -> Result<bool, StoreError> {
        let pk_val = mem_pk(project_id, file_path);
        let item_id = mem_id(project_id, file_path, chunk_index);
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

    async fn delete_all(&self, project_id: Option<&str>) -> Result<usize, StoreError> {
        let project_clause = match project_id {
            Some(project_id) => format!(" AND c.project_id = '{}'", project_id.replace('\'', "''")),
            None => String::new(),
        };
        let query = format!(
            "SELECT c.id, c.pk FROM c WHERE c.type = 'memory_embedding'{}",
            project_clause
        );
        let items: Vec<serde_json::Value> = self.query_cross_partition(&query).await?;
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
