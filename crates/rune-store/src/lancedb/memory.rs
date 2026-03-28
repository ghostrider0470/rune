//! LanceDB implementation of [`MemoryEmbeddingRepo`].

use async_trait::async_trait;
use chrono::Utc;
use lancedb::query::{ExecutableQuery, QueryBase};

use crate::error::StoreError;
use crate::models::{KeywordSearchRow, VectorSearchRow};
use crate::repos::MemoryEmbeddingRepo;

use super::{
    LanceStore, collect_batches, embedding_batch, embeddings_schema, f64_value, str_col,
    upsert_batch,
};

/// Document ID for a memory embedding chunk.
fn escape_lance(value: &str) -> String {
    value.replace('\'', "''")
}

fn project_filter(project_id: Option<&str>) -> String {
    match project_id {
        Some(project_id) => format!("project_id = '{}'", escape_lance(project_id)),
        None => "project_id IS NULL".to_string(),
    }
}

fn chunk_id(project_id: Option<&str>, file_path: &str, chunk_index: i32) -> String {
    format!("{}:{}:{}", project_id.unwrap_or(""), file_path, chunk_index)
}

#[async_trait]
impl MemoryEmbeddingRepo for LanceStore {
    async fn upsert_chunk(
        &self,
        project_id: Option<&str>,
        file_path: &str,
        chunk_index: i32,
        chunk_text: &str,
        embedding: &[f32],
    ) -> Result<(), StoreError> {
        let table = self.open_embeddings_table().await?;
        let schema = embeddings_schema(self.embedding_dims);
        let batch = embedding_batch(
            &schema,
            &chunk_id(project_id, file_path, chunk_index),
            project_id,
            file_path,
            chunk_index,
            chunk_text,
            embedding,
            &Utc::now().to_rfc3339(),
            self.embedding_dims,
        )?;
        upsert_batch(&table, &schema, batch).await
    }

    async fn delete_by_file(
        &self,
        project_id: Option<&str>,
        file_path: &str,
    ) -> Result<usize, StoreError> {
        let table = self.open_embeddings_table().await?;
        let count_before = table
            .count_rows(Some(format!(
                "{} AND file_path = '{}'",
                project_filter(project_id),
                escape_lance(file_path)
            )))
            .await
            .map_err(|e| StoreError::Database(format!("lancedb count: {e}")))?;
        table
            .delete(&format!(
                "{} AND file_path = '{}'",
                project_filter(project_id),
                escape_lance(file_path)
            ))
            .await
            .map_err(|e| StoreError::Database(format!("lancedb delete: {e}")))?;
        Ok(count_before)
    }

    async fn keyword_search(
        &self,
        project_id: Option<&str>,
        query: &str,
        limit: i64,
    ) -> Result<Vec<KeywordSearchRow>, StoreError> {
        let table = self.open_embeddings_table().await?;
        let escaped = query.replace('\'', "''").to_lowercase();
        let filter = format!("lower(chunk_text) LIKE '%{}%'", escaped.replace('%', "\\%"));
        let stream = table
            .query()
            .only_if(project_filter(project_id))
            .only_if(filter)
            .limit(limit as usize)
            .execute()
            .await
            .map_err(|e| StoreError::Database(format!("lancedb keyword search: {e}")))?;
        let batches = collect_batches(stream).await?;

        let mut results = Vec::new();
        for batch in &batches {
            let paths = str_col(batch, "file_path");
            let texts = str_col(batch, "chunk_text");
            for i in 0..batch.num_rows() {
                results.push(KeywordSearchRow {
                    project_id: project_id.map(str::to_string),
                    file_path: paths.value(i).to_string(),
                    chunk_text: texts.value(i).to_string(),
                    score: 1.0,
                });
            }
        }
        Ok(results)
    }

    async fn vector_search(
        &self,
        project_id: Option<&str>,
        embedding: &[f32],
        limit: i64,
    ) -> Result<Vec<VectorSearchRow>, StoreError> {
        let table = self.open_embeddings_table().await?;
        let stream = table
            .vector_search(embedding)
            .map_err(|e| StoreError::Database(format!("lancedb nearest_to: {e}")))?
            .distance_type(lancedb::DistanceType::Cosine)
            .only_if(project_filter(project_id))
            .limit(limit as usize)
            .execute()
            .await
            .map_err(|e| StoreError::Database(format!("lancedb vector search: {e}")))?;
        let batches = collect_batches(stream).await?;

        let mut results = Vec::new();
        for batch in &batches {
            let paths = str_col(batch, "file_path");
            let texts = str_col(batch, "chunk_text");
            for i in 0..batch.num_rows() {
                results.push(VectorSearchRow {
                    project_id: project_id.map(str::to_string),
                    file_path: paths.value(i).to_string(),
                    chunk_text: texts.value(i).to_string(),
                    score: 1.0 - f64_value(batch, "_distance", i),
                });
            }
        }
        Ok(results)
    }

    async fn count(&self, project_id: Option<&str>) -> Result<i64, StoreError> {
        let table = self.open_embeddings_table().await?;
        let n = table
            .count_rows(Some(project_filter(project_id)))
            .await
            .map_err(|e| StoreError::Database(format!("lancedb count: {e}")))?;
        Ok(n as i64)
    }

    async fn list_indexed_files(
        &self,
        project_id: Option<&str>,
    ) -> Result<Vec<String>, StoreError> {
        let table = self.open_embeddings_table().await?;
        let stream = table
            .query()
            .select(lancedb::query::Select::Columns(vec![
                "file_path".to_string(),
            ]))
            .only_if(project_filter(project_id))
            .execute()
            .await
            .map_err(|e| StoreError::Database(format!("lancedb list files: {e}")))?;
        let batches = collect_batches(stream).await?;

        let mut paths = std::collections::HashSet::new();
        for batch in &batches {
            let col = str_col(batch, "file_path");
            for i in 0..batch.num_rows() {
                paths.insert(col.value(i).to_string());
            }
        }
        Ok(paths.into_iter().collect())
    }

    async fn delete_chunk(
        &self,
        project_id: Option<&str>,
        file_path: &str,
        chunk_index: i32,
    ) -> Result<bool, StoreError> {
        let table = self.open_embeddings_table().await?;
        let id = chunk_id(project_id, file_path, chunk_index);
        let existed = table
            .count_rows(Some(format!("id = '{}'", id)))
            .await
            .map_err(|e| StoreError::Database(format!("lancedb count: {e}")))?
            > 0;
        if existed {
            table
                .delete(&format!("id = '{}'", id))
                .await
                .map_err(|e| StoreError::Database(format!("lancedb delete: {e}")))?;
        }
        Ok(existed)
    }

    async fn delete_all(&self, project_id: Option<&str>) -> Result<usize, StoreError> {
        let table = self.open_embeddings_table().await?;
        let count = table
            .count_rows(Some(project_filter(project_id)))
            .await
            .map_err(|e| StoreError::Database(format!("lancedb count: {e}")))?;
        if count > 0 {
            table
                .delete(&project_filter(project_id))
                .await
                .map_err(|e| StoreError::Database(format!("lancedb delete_all: {e}")))?;
        }
        Ok(count)
    }
}
