//! SQLite implementation of [`MemoryFactRepo`](crate::repos::MemoryFactRepo).
//! SQLite lacks vector search, so recall/dedup/graph return empty results.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::{parse_dt, parse_uuid, parse_uuid_opt, to_rfc3339};
use crate::error::StoreError;
use crate::models::{MemoryFact, MemoryFactEdge};
use crate::repos::MemoryFactRepo;

const FACT_COLS: &str =
    "id, fact, category, source_session_id, created_at, updated_at, access_count";

fn row_to_memory_fact(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryFact> {
    Ok(MemoryFact {
        id: parse_uuid(&row.get::<_, String>(0)?),
        fact: row.get(1)?,
        category: row.get(2)?,
        source_session_id: parse_uuid_opt(row.get(3)?),
        created_at: parse_dt(&row.get::<_, String>(4)?),
        updated_at: parse_dt(&row.get::<_, String>(5)?),
        access_count: row.get(6)?,
    })
}

#[derive(Clone)]
pub struct SqliteMemoryFactRepo {
    conn: Arc<tokio_rusqlite::Connection>,
}

impl SqliteMemoryFactRepo {
    pub fn new(conn: Arc<tokio_rusqlite::Connection>) -> Self {
        Self { conn }
    }
}

#[async_trait]
impl MemoryFactRepo for SqliteMemoryFactRepo {
    /// SQLite has no vector index — always returns an empty list.
    async fn recall(
        &self,
        _embedding_str: &str,
        _threshold: f64,
        _limit: i64,
    ) -> Result<Vec<MemoryFact>, StoreError> {
        Ok(Vec::new())
    }

    async fn increment_access(&self, ids: &[Uuid]) -> Result<(), StoreError> {
        if ids.is_empty() {
            return Ok(());
        }
        let ids: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
        self.conn
            .call(move |conn| {
                for id_s in &ids {
                    conn.execute(
                        "UPDATE rune_memory_facts \
                         SET access_count = access_count + 1 \
                         WHERE id = ?1",
                        rusqlite::params![id_s],
                    )?;
                }
                Ok(())
            })
            .await
            .map_err(StoreError::from)
    }

    /// SQLite has no vector index — always returns `None`.
    async fn dedup_check(
        &self,
        _embedding_str: &str,
        _threshold: f64,
    ) -> Result<Option<(Uuid, String, f64)>, StoreError> {
        Ok(None)
    }

    async fn insert(
        &self,
        id: Uuid,
        fact: &str,
        category: &str,
        _embedding_str: &str,
        source_session_id: Option<Uuid>,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let fact = fact.to_string();
        let category = category.to_string();
        let now_s = to_rfc3339(&now);
        let sid = source_session_id.map(|u| u.to_string());
        self.conn
            .call(move |conn| {
                conn.execute(
                    &format!(
                        "INSERT OR REPLACE INTO rune_memory_facts ({FACT_COLS}) \
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)"
                    ),
                    rusqlite::params![id.to_string(), fact, category, sid, now_s, now_s],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::from)
    }

    async fn update(
        &self,
        id: Uuid,
        fact: &str,
        category: &str,
        _embedding_str: &str,
        now: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let fact = fact.to_string();
        let category = category.to_string();
        let now_s = to_rfc3339(&now);
        let id_s = id.to_string();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "UPDATE rune_memory_facts \
                     SET fact = ?2, category = ?3, updated_at = ?4 \
                     WHERE id = ?1",
                    rusqlite::params![id_s, fact, category, now_s],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::from)
    }

    async fn delete(&self, id: &str) -> Result<(), StoreError> {
        let id_s = id.to_string();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM rune_memory_facts WHERE id = ?1",
                    rusqlite::params![id_s],
                )?;
                Ok(())
            })
            .await
            .map_err(StoreError::from)
    }

    async fn list_all(&self) -> Result<Vec<MemoryFact>, StoreError> {
        self.conn
            .call(move |conn| {
                conn.prepare(&format!(
                    "SELECT {FACT_COLS} FROM rune_memory_facts \
                     ORDER BY created_at DESC"
                ))?
                .query_map([], row_to_memory_fact)?
                .collect::<Result<Vec<_>, _>>()
            })
            .await
            .map_err(StoreError::from)
    }

    /// SQLite has no vector index — always returns an empty list.
    async fn graph_edges(
        &self,
        _threshold: f64,
        _neighbors_k: i64,
    ) -> Result<Vec<MemoryFactEdge>, StoreError> {
        Ok(Vec::new())
    }
}
