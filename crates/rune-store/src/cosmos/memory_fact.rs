//! Cosmos DB implementation of [`MemoryFactRepo`](crate::repos::MemoryFactRepo).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::cosmos::{pk, CosmosStore};
use crate::error::StoreError;
use crate::models::{MemoryFact, MemoryFactEdge};
use crate::repos::MemoryFactRepo;

#[derive(Debug, Serialize, Deserialize)]
struct MemoryFactDoc {
    id: String,
    pk: String,
    #[serde(rename = "type")]
    doc_type: String,
    fact_id: Uuid,
    fact: String,
    category: String,
    embedding: Vec<f32>,
    source_session_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    access_count: i32,
}

impl From<MemoryFactDoc> for MemoryFact {
    fn from(d: MemoryFactDoc) -> Self {
        Self { id: d.fact_id, fact: d.fact, category: d.category, source_session_id: d.source_session_id, created_at: d.created_at, updated_at: d.updated_at, access_count: d.access_count }
    }
}

#[derive(Debug, Deserialize)]
struct VectorHit { fact_id: Uuid, fact: String, category: String, source_session_id: Option<Uuid>, created_at: DateTime<Utc>, updated_at: DateTime<Utc>, access_count: i32, score: f64 }

#[derive(Debug, Deserialize)]
struct DedupHit { fact_id: Uuid, fact: String, score: f64 }

#[derive(Debug, Deserialize)]
struct EmbeddingOnly { fact_id: Uuid, embedding: Vec<f32> }

#[async_trait]
impl MemoryFactRepo for CosmosStore {
    async fn recall(&self, embedding_str: &str, threshold: f64, limit: i64) -> Result<Vec<MemoryFact>, StoreError> {
        let sql = format!("SELECT TOP {} c.fact_id, c.fact, c.category, c.source_session_id, c.created_at, c.updated_at, c.access_count, VectorDistance(c.embedding, {}) AS score FROM c WHERE c.type = 'memory_fact' ORDER BY VectorDistance(c.embedding, {})", limit, embedding_str, embedding_str);
        let hits: Vec<VectorHit> = self.query_cross_partition(&sql).await?;
        Ok(hits.into_iter().filter(|h| h.score > threshold).map(|h| MemoryFact { id: h.fact_id, fact: h.fact, category: h.category, source_session_id: h.source_session_id, created_at: h.created_at, updated_at: h.updated_at, access_count: h.access_count }).collect())
    }

    async fn increment_access(&self, ids: &[Uuid]) -> Result<(), StoreError> {
        for id in ids {
            let query = format!("SELECT * FROM c WHERE c.type = 'memory_fact' AND c.fact_id = '{}'", id);
            let docs: Vec<MemoryFactDoc> = self.query_cross_partition(&query).await?;
            if let Some(mut doc) = docs.into_iter().next() {
                doc.access_count += 1;
                self.container().upsert_item(pk(&doc.pk), &doc, None).await?;
            }
        }
        Ok(())
    }

    async fn dedup_check(&self, embedding_str: &str, threshold: f64) -> Result<Option<(Uuid, String, f64)>, StoreError> {
        let sql = format!("SELECT TOP 1 c.fact_id, c.fact, VectorDistance(c.embedding, {}) AS score FROM c WHERE c.type = 'memory_fact' ORDER BY VectorDistance(c.embedding, {})", embedding_str, embedding_str);
        let hits: Vec<DedupHit> = self.query_cross_partition(&sql).await?;
        Ok(hits.into_iter().next().filter(|h| h.score > threshold).map(|h| (h.fact_id, h.fact, h.score)))
    }

    async fn insert(&self, id: Uuid, fact: &str, category: &str, embedding_str: &str, source_session_id: Option<Uuid>, now: DateTime<Utc>) -> Result<(), StoreError> {
        let embedding = parse_embedding_str(embedding_str)?;
        let doc = MemoryFactDoc { id: id.to_string(), pk: format!("fact:{}", id), doc_type: "memory_fact".to_string(), fact_id: id, fact: fact.to_string(), category: category.to_string(), embedding, source_session_id, created_at: now, updated_at: now, access_count: 0 };
        self.container().upsert_item(pk(&doc.pk), &doc, None).await?;
        Ok(())
    }

    async fn update(&self, id: Uuid, fact: &str, category: &str, embedding_str: &str, now: DateTime<Utc>) -> Result<(), StoreError> {
        let query = format!("SELECT * FROM c WHERE c.type = 'memory_fact' AND c.fact_id = '{}'", id);
        let docs: Vec<MemoryFactDoc> = self.query_cross_partition(&query).await?;
        if let Some(mut doc) = docs.into_iter().next() {
            doc.fact = fact.to_string();
            doc.category = category.to_string();
            doc.embedding = parse_embedding_str(embedding_str)?;
            doc.updated_at = now;
            self.container().upsert_item(pk(&doc.pk), &doc, None).await?;
        }
        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<(), StoreError> {
        let pk_val = format!("fact:{}", id);
        match self.container().delete_item(pk(&pk_val), id, None).await {
            Ok(_) => Ok(()),
            Err(e) => { let msg = e.to_string(); if msg.contains("NotFound") || msg.contains("404") { Ok(()) } else { Err(StoreError::Database(msg)) } }
        }
    }

    async fn list_all(&self) -> Result<Vec<MemoryFact>, StoreError> {
        let sql = "SELECT * FROM c WHERE c.type = 'memory_fact' ORDER BY c.created_at DESC";
        let docs: Vec<MemoryFactDoc> = self.query_cross_partition(sql).await?;
        Ok(docs.into_iter().map(MemoryFact::from).collect())
    }

    async fn graph_edges(&self, threshold: f64, neighbors_k: i64) -> Result<Vec<MemoryFactEdge>, StoreError> {
        let sql = "SELECT c.fact_id, c.embedding FROM c WHERE c.type = 'memory_fact'";
        let docs: Vec<EmbeddingOnly> = self.query_cross_partition(sql).await?;
        let mut edges = Vec::new();
        for (i, a) in docs.iter().enumerate() {
            let mut neighbors: Vec<(Uuid, f64)> = Vec::new();
            for b in docs.iter().skip(i + 1) {
                let sim = cosine_similarity(&a.embedding, &b.embedding);
                if sim > threshold { neighbors.push((b.fact_id, sim)); }
            }
            neighbors.sort_by(|x, y| y.1.partial_cmp(&x.1).unwrap_or(std::cmp::Ordering::Equal));
            for (target_id, sim) in neighbors.into_iter().take(neighbors_k as usize) {
                edges.push(MemoryFactEdge { source: a.fact_id, target: target_id, similarity: sim });
            }
        }
        Ok(edges)
    }
}

fn parse_embedding_str(s: &str) -> Result<Vec<f32>, StoreError> {
    s.trim_start_matches('[').trim_end_matches(']').split(',')
        .map(|v| v.trim().parse::<f32>().map_err(|e| StoreError::Serialization(format!("bad embedding float: {e}"))))
        .collect()
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    let (mut dot, mut na, mut nb) = (0.0f64, 0.0f64, 0.0f64);
    for (x, y) in a.iter().zip(b.iter()) { let (x, y) = (*x as f64, *y as f64); dot += x * y; na += x * x; nb += y * y; }
    let d = na.sqrt() * nb.sqrt();
    if d == 0.0 { 0.0 } else { dot / d }
}
