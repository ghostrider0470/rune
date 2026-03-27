//! LanceDB implementation of [`MemoryFactRepo`].

use arrow_array::Array;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use lancedb::query::{ExecutableQuery, QueryBase};
use uuid::Uuid;

use crate::error::StoreError;
use crate::models::{MemoryFact, MemoryFactEdge};
use crate::repos::MemoryFactRepo;

use super::{
    LanceStore, collect_batches, embedding_col, f64_value, fact_batch, facts_schema, i32_col,
    parse_embedding_str, str_col, upsert_batch,
};

#[async_trait]
impl MemoryFactRepo for LanceStore {
    async fn recall(
        &self,
        embedding_str: &str,
        threshold: f64,
        limit: i64,
    ) -> Result<Vec<MemoryFact>, StoreError> {
        let table = self.open_facts_table().await?;
        let embedding = parse_embedding_str(embedding_str)?;
        let stream = table
            .vector_search(embedding.as_slice())
            .map_err(|e| StoreError::Database(format!("lancedb nearest_to: {e}")))?
            .distance_type(lancedb::DistanceType::Cosine)
            .limit(limit as usize)
            .execute()
            .await
            .map_err(|e| StoreError::Database(format!("lancedb recall: {e}")))?;
        let batches = collect_batches(stream).await?;

        let mut results = Vec::new();
        for batch in &batches {
            let fact_ids = str_col(batch, "fact_id");
            let facts = str_col(batch, "fact");
            let categories = str_col(batch, "category");
            let session_ids = str_col(batch, "source_session_id");
            let created = str_col(batch, "created_at");
            let updated = str_col(batch, "updated_at");
            let access = i32_col(batch, "access_count");
            for i in 0..batch.num_rows() {
                let similarity = 1.0 - f64_value(batch, "_distance", i);
                if similarity <= threshold {
                    continue;
                }
                results.push(MemoryFact {
                    id: Uuid::parse_str(fact_ids.value(i))
                        .map_err(|e| StoreError::Serialization(e.to_string()))?,
                    fact: facts.value(i).to_string(),
                    category: categories.value(i).to_string(),
                    source_session_id: if session_ids.is_null(i) {
                        None
                    } else {
                        Some(
                            Uuid::parse_str(session_ids.value(i))
                                .map_err(|e| StoreError::Serialization(e.to_string()))?,
                        )
                    },
                    created_at: DateTime::parse_from_rfc3339(created.value(i))
                        .map_err(|e| StoreError::Serialization(e.to_string()))?
                        .with_timezone(&Utc),
                    updated_at: DateTime::parse_from_rfc3339(updated.value(i))
                        .map_err(|e| StoreError::Serialization(e.to_string()))?
                        .with_timezone(&Utc),
                    access_count: access.value(i),
                });
            }
        }
        Ok(results)
    }

    async fn increment_access(&self, ids: &[Uuid]) -> Result<(), StoreError> {
        let table = self.open_facts_table().await?;
        for id in ids {
            let filter = format!("fact_id = '{}'", id);
            let stream = table
                .query()
                .only_if(filter.clone())
                .limit(1)
                .execute()
                .await
                .map_err(|e| StoreError::Database(format!("lancedb read: {e}")))?;
            let batches = collect_batches(stream).await?;
            if let Some(batch) = batches.first() {
                if batch.num_rows() > 0 {
                    let current = i32_col(batch, "access_count").value(0);
                    table
                        .update()
                        .only_if(filter)
                        .column("access_count", format!("{}", current + 1))
                        .execute()
                        .await
                        .map_err(|e| StoreError::Database(format!("lancedb update: {e}")))?;
                }
            }
        }
        Ok(())
    }

    async fn dedup_check(
        &self,
        embedding_str: &str,
        threshold: f64,
    ) -> Result<Option<(Uuid, String, f64)>, StoreError> {
        let table = self.open_facts_table().await?;
        let embedding = parse_embedding_str(embedding_str)?;
        let stream = table
            .vector_search(embedding.as_slice())
            .map_err(|e| StoreError::Database(format!("lancedb nearest_to: {e}")))?
            .distance_type(lancedb::DistanceType::Cosine)
            .limit(1)
            .execute()
            .await
            .map_err(|e| StoreError::Database(format!("lancedb dedup: {e}")))?;
        let batches = collect_batches(stream).await?;

        if let Some(batch) = batches.first() {
            if batch.num_rows() > 0 {
                let similarity = 1.0 - f64_value(batch, "_distance", 0);
                if similarity > threshold {
                    let fact_id = Uuid::parse_str(str_col(batch, "fact_id").value(0))
                        .map_err(|e| StoreError::Serialization(e.to_string()))?;
                    let fact = str_col(batch, "fact").value(0).to_string();
                    return Ok(Some((fact_id, fact, similarity)));
                }
            }
        }
        Ok(None)
    }

    async fn insert(
        &self,
        id: Uuid,
        fact: &str,
        category: &str,
        embedding_str: &str,
        source_session_id: Option<Uuid>,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let table = self.open_facts_table().await?;
        let embedding = parse_embedding_str(embedding_str)?;
        let schema = facts_schema(self.embedding_dims);
        let batch = fact_batch(
            &schema,
            &id.to_string(),
            &id.to_string(),
            fact,
            category,
            &embedding,
            source_session_id.as_ref().map(|u| u.to_string()).as_deref(),
            &now.to_rfc3339(),
            &now.to_rfc3339(),
            0,
            self.embedding_dims,
        )?;
        upsert_batch(&table, &schema, batch).await
    }

    async fn update(
        &self,
        id: Uuid,
        fact: &str,
        category: &str,
        embedding_str: &str,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let table = self.open_facts_table().await?;
        let embedding = parse_embedding_str(embedding_str)?;

        // Read existing doc to preserve source_session_id and access_count.
        let filter = format!("fact_id = '{}'", id);
        let stream = table
            .query()
            .only_if(filter)
            .limit(1)
            .execute()
            .await
            .map_err(|e| StoreError::Database(format!("lancedb read: {e}")))?;
        let batches = collect_batches(stream).await?;
        let (source_session_id, access_count, created_at) =
            if let Some(batch) = batches.first().filter(|b| b.num_rows() > 0) {
                let ssid = if str_col(batch, "source_session_id").is_null(0) {
                    None
                } else {
                    Some(str_col(batch, "source_session_id").value(0).to_string())
                };
                (
                    ssid,
                    i32_col(batch, "access_count").value(0),
                    str_col(batch, "created_at").value(0).to_string(),
                )
            } else {
                (None, 0, now.to_rfc3339())
            };

        let schema = facts_schema(self.embedding_dims);
        let batch = fact_batch(
            &schema,
            &id.to_string(),
            &id.to_string(),
            fact,
            category,
            &embedding,
            source_session_id.as_deref(),
            &created_at,
            &now.to_rfc3339(),
            access_count,
            self.embedding_dims,
        )?;
        upsert_batch(&table, &schema, batch).await
    }

    async fn delete(&self, id: &str) -> Result<(), StoreError> {
        let table = self.open_facts_table().await?;
        let _ = table.delete(&format!("fact_id = '{}'", id)).await;
        Ok(())
    }

    async fn list_all(&self) -> Result<Vec<MemoryFact>, StoreError> {
        let table = self.open_facts_table().await?;
        let stream = table
            .query()
            .select(lancedb::query::Select::Columns(vec![
                "fact_id".into(),
                "fact".into(),
                "category".into(),
                "source_session_id".into(),
                "created_at".into(),
                "updated_at".into(),
                "access_count".into(),
            ]))
            .execute()
            .await
            .map_err(|e| StoreError::Database(format!("lancedb list_all: {e}")))?;
        let batches = collect_batches(stream).await?;

        let mut results = Vec::new();
        for batch in &batches {
            let fact_ids = str_col(batch, "fact_id");
            let facts = str_col(batch, "fact");
            let categories = str_col(batch, "category");
            let session_ids = str_col(batch, "source_session_id");
            let created = str_col(batch, "created_at");
            let updated = str_col(batch, "updated_at");
            let access = i32_col(batch, "access_count");

            for i in 0..batch.num_rows() {
                results.push(MemoryFact {
                    id: Uuid::parse_str(fact_ids.value(i))
                        .map_err(|e| StoreError::Serialization(e.to_string()))?,
                    fact: facts.value(i).to_string(),
                    category: categories.value(i).to_string(),
                    source_session_id: if session_ids.is_null(i) {
                        None
                    } else {
                        Some(
                            Uuid::parse_str(session_ids.value(i))
                                .map_err(|e| StoreError::Serialization(e.to_string()))?,
                        )
                    },
                    created_at: DateTime::parse_from_rfc3339(created.value(i))
                        .map_err(|e| StoreError::Serialization(e.to_string()))?
                        .with_timezone(&Utc),
                    updated_at: DateTime::parse_from_rfc3339(updated.value(i))
                        .map_err(|e| StoreError::Serialization(e.to_string()))?
                        .with_timezone(&Utc),
                    access_count: access.value(i),
                });
            }
        }
        Ok(results)
    }

    async fn graph_edges(
        &self,
        threshold: f64,
        neighbors_k: i64,
    ) -> Result<Vec<MemoryFactEdge>, StoreError> {
        let table = self.open_facts_table().await?;
        let stream = table
            .query()
            .select(lancedb::query::Select::Columns(vec![
                "fact_id".into(),
                "embedding".into(),
            ]))
            .execute()
            .await
            .map_err(|e| StoreError::Database(format!("lancedb graph: {e}")))?;
        let batches = collect_batches(stream).await?;

        let mut facts: Vec<(Uuid, Vec<f32>)> = Vec::new();
        for batch in &batches {
            let ids = str_col(batch, "fact_id");
            let embeddings = embedding_col(batch, "embedding");
            for i in 0..batch.num_rows() {
                let fid = Uuid::parse_str(ids.value(i))
                    .map_err(|e| StoreError::Serialization(e.to_string()))?;
                facts.push((fid, embeddings[i].clone()));
            }
        }

        let mut edges = Vec::new();
        for (i, (a_id, a_emb)) in facts.iter().enumerate() {
            let mut neighbors: Vec<(Uuid, f64)> = Vec::new();
            for (b_id, b_emb) in facts.iter().skip(i + 1) {
                let sim = cosine_similarity(a_emb, b_emb);
                if sim > threshold {
                    neighbors.push((*b_id, sim));
                }
            }
            neighbors.sort_by(|x, y| y.1.partial_cmp(&x.1).unwrap_or(std::cmp::Ordering::Equal));
            for (target_id, sim) in neighbors.into_iter().take(neighbors_k as usize) {
                edges.push(MemoryFactEdge {
                    source: *a_id,
                    target: target_id,
                    similarity: sim,
                });
            }
        }
        Ok(edges)
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    let (mut dot, mut na, mut nb) = (0.0f64, 0.0f64, 0.0f64);
    for (x, y) in a.iter().zip(b.iter()) {
        let (x, y) = (*x as f64, *y as f64);
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    let d = na.sqrt() * nb.sqrt();
    if d == 0.0 { 0.0 } else { dot / d }
}
