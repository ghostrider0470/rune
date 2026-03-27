//! Mem0-style auto-capture/recall memory engine.
//!
//! Provides persistent cross-session memory for the agent by:
//! - **Recall**: Before each turn, embedding the user message and searching
//!   `rune_memories` for semantically similar past facts (pgvector cosine).
//! - **Capture**: After each turn, extracting durable facts from the
//!   conversation via a cheap LLM call and storing them with embeddings.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use rune_config::Mem0Config;
use rune_models::ModelProvider;
use serde::{Deserialize, Serialize};
use tokio_postgres::Client;
use tokio_postgres::types::Type;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// A single remembered fact.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Memory {
    pub id: Uuid,
    pub fact: String,
    pub category: String,
    pub source_session_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub access_count: i32,
}

/// A graph of memories: nodes + similarity edges for visualization.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryGraph {
    pub nodes: Vec<Memory>,
    pub edges: Vec<MemoryEdge>,
}

/// A similarity edge between two memories.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryEdge {
    pub source: Uuid,
    pub target: Uuid,
    pub similarity: f64,
}

/// Mem0 engine that manages the full recall/capture lifecycle.
///
/// Thread-safe and cheaply cloneable — hold one `Arc<Mem0Engine>` in the
/// `TurnExecutor` and share it across concurrent turns.
pub struct Mem0Engine {
    client: tokio::sync::Mutex<Client>,
    pg_url: String,
    http: reqwest::Client,
    config: Mem0Config,
    extraction_provider: Arc<dyn ModelProvider>,
}

// ── SQL constants ────────────────────────────────────────────────────

/// On Azure Cosmos DB for PostgreSQL (Citus), `CREATE EXTENSION` is blocked —
/// use the Citus helper function instead.  We try `create_extension()` first,
/// then fall back to `CREATE EXTENSION IF NOT EXISTS` for vanilla Postgres.
const ENSURE_EXTENSION_CITUS_SQL: &str = "SELECT create_extension('vector')";
const ENSURE_EXTENSION_SQL: &str = "CREATE EXTENSION IF NOT EXISTS vector";

fn ensure_table_sql(dims: usize) -> String {
    format!(
        r#"CREATE TABLE IF NOT EXISTS rune_memories (
    id UUID PRIMARY KEY,
    fact TEXT NOT NULL,
    category TEXT NOT NULL DEFAULT 'general',
    embedding vector({dims}),
    source_session_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    access_count INTEGER NOT NULL DEFAULT 0
)"#
    )
}

/// We use HNSW for the index — it works without needing a minimum row count
/// (unlike ivfflat which needs `lists` tuning).  Falls back gracefully if
/// the index already exists.
const ENSURE_INDEX_SQL: &str = r#"
CREATE INDEX IF NOT EXISTS idx_rune_memories_embedding
    ON rune_memories USING hnsw (embedding vector_cosine_ops)
"#;

const RECALL_SQL: &str = r#"
SELECT id, fact, category, source_session_id, created_at, updated_at, access_count
FROM rune_memories
WHERE 1 - (embedding <=> $1::vector) > $2
ORDER BY embedding <=> $1::vector
LIMIT $3
"#;

const INCREMENT_ACCESS_SQL: &str = r#"
UPDATE rune_memories SET access_count = access_count + 1 WHERE id = ANY($1)
"#;

const DEDUP_CHECK_SQL: &str = r#"
SELECT id, fact, 1 - (embedding <=> $1::vector) AS similarity
FROM rune_memories
WHERE 1 - (embedding <=> $1::vector) > $2
ORDER BY embedding <=> $1::vector
LIMIT 1
"#;

const INSERT_MEMORY_SQL: &str = r#"
INSERT INTO rune_memories (id, fact, category, embedding, source_session_id, created_at, updated_at)
VALUES ($1, $2, $3, $4::vector, $5, $6, $7)
"#;

const UPDATE_MEMORY_SQL: &str = r#"
UPDATE rune_memories
SET fact = $2, category = $3, embedding = $4::vector, updated_at = $5
WHERE id = $1
"#;

// ── Extraction prompt ────────────────────────────────────────────────

const EXTRACTION_SYSTEM_PROMPT: &str = r#"Extract factual memories from this conversation that would be useful to recall in future sessions. Return a JSON array of objects with "fact" and "category" fields.

Categories: preference, project, ops, decision, person, technical, workflow

Only extract genuinely useful, durable facts. Skip ephemeral details like timestamps, greetings, or questions about immediate tasks. Focus on preferences, decisions, project context, people mentioned, and technical choices.

Respond ONLY with the JSON array. If there is nothing worth remembering, respond with an empty array: []"#;

/// A fact extracted by the LLM.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct ExtractedFact {
    fact: String,
    category: String,
}

impl Mem0Engine {
    /// Connect to Postgres and ensure the schema exists.
    ///
    /// Returns `None` if the config is disabled or connection fails, so the
    /// caller can gracefully degrade.
    pub async fn try_connect(
        config: &Mem0Config,
        extraction_provider: Arc<dyn ModelProvider>,
    ) -> Option<Arc<Self>> {
        if !config.enabled {
            info!("mem0 disabled by configuration");
            return None;
        }

        let pg_url = match &config.postgres_url {
            Some(url) => url.clone(),
            None => {
                warn!("mem0 enabled but no postgres_url configured — skipping");
                return None;
            }
        };

        if config.embedding_endpoint.is_none() || config.embedding_api_key.is_none() {
            warn!("mem0 enabled but embedding endpoint/key not configured — skipping");
            return None;
        }

        let client = match connect_postgres(&pg_url).await {
            Ok(c) => c,
            Err(e) => {
                error!(error = %e, "mem0: failed to connect to Postgres — disabling");
                return None;
            }
        };

        // Ensure vector extension — try Citus helper first (required for
        // Azure Cosmos DB for PostgreSQL), then fall back to standard CREATE EXTENSION.
        let ext_ok = match client.batch_execute(ENSURE_EXTENSION_CITUS_SQL).await {
            Ok(()) => {
                debug!("mem0: vector extension enabled via Citus create_extension()");
                true
            }
            Err(_) => {
                // Not Citus — try standard SQL
                match client.batch_execute(ENSURE_EXTENSION_SQL).await {
                    Ok(()) => {
                        debug!("mem0: vector extension enabled via CREATE EXTENSION");
                        true
                    }
                    Err(e) => {
                        warn!(error = %e, "mem0: failed to create vector extension");
                        // Continue — extension may already exist from a prior run
                        true
                    }
                }
            }
        };

        if !ext_ok {
            error!("mem0: could not ensure vector extension — disabling");
            return None;
        }

        let table_sql = ensure_table_sql(config.embedding_dims);
        if let Err(e) = client.batch_execute(&table_sql).await {
            error!(error = %e, "mem0: failed to create rune_memories table — disabling");
            return None;
        }

        // Older pgvector releases (like Cosmos DB's 0.8.0) cap HNSW indexes at
        // 2000 dims, while pgvector >= 0.9.0 supports 3072-dim embeddings.
        // If index creation fails, queries still work via sequential scan.
        if let Err(e) = client.batch_execute(ENSURE_INDEX_SQL).await {
            info!(error = %e, "mem0: HNSW index not created (likely pgvector dimension limit/version mismatch) — brute-force cosine search will be used");
        }

        info!("mem0 engine connected and schema ensured");

        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("failed to build reqwest client");

        Some(Arc::new(Self {
            client: tokio::sync::Mutex::new(client),
            pg_url: pg_url.to_string(),
            http,
            config: config.clone(),
            extraction_provider,
        }))
    }

    // ── Connection management ─────────────────────────────────────────

    /// Ensure the PG client is alive. If the connection has dropped, attempt
    /// to reconnect transparently. This is called before every query.
    async fn ensure_connected(&self) -> Result<(), String> {
        let client = self.client.lock().await;
        // Simple health check — execute a trivial query
        match client.simple_query("SELECT 1").await {
            Ok(_) => return Ok(()),
            Err(e) => {
                warn!(error = %e, "mem0 PG connection dead, attempting reconnect");
            }
        }
        drop(client);

        // Reconnect
        match connect_postgres(&self.pg_url).await {
            Ok(new_client) => {
                let mut client = self.client.lock().await;
                *client = new_client;
                info!("mem0 PG connection restored");
                Ok(())
            }
            Err(e) => {
                error!(error = %e, "mem0 PG reconnect failed");
                Err(format!("reconnect failed: {e}"))
            }
        }
    }

    // ── Public API ───────────────────────────────────────────────────

    /// Recall memories semantically similar to the query.
    ///
    /// Returns an empty vec on any error (never blocks the turn).
    pub async fn recall(&self, query: &str) -> Vec<Memory> {
        if let Err(e) = self.ensure_connected().await {
            warn!(error = %e, "mem0 recall: connection check failed");
            return Vec::new();
        }
        let embedding = match self.embed(query).await {
            Ok(e) => e,
            Err(e) => {
                warn!(error = %e, "mem0 recall: embedding failed");
                return Vec::new();
            }
        };

        let embedding_str = format_vector(&embedding);
        let client = self.client.lock().await;

        // Use query_typed so tokio-postgres sends the vector literal as TEXT
        // and lets Postgres handle the ::vector cast in the SQL.
        let rows = match client
            .query_typed(
                RECALL_SQL,
                &[
                    (
                        &embedding_str as &(dyn tokio_postgres::types::ToSql + Sync),
                        Type::TEXT,
                    ),
                    (
                        &self.config.similarity_threshold
                            as &(dyn tokio_postgres::types::ToSql + Sync),
                        Type::FLOAT8,
                    ),
                    (
                        &(self.config.top_k as i64) as &(dyn tokio_postgres::types::ToSql + Sync),
                        Type::INT8,
                    ),
                ],
            )
            .await
        {
            Ok(rows) => rows,
            Err(e) => {
                warn!(error = %e, "mem0 recall: query failed");
                return Vec::new();
            }
        };

        let mut memories: Vec<Memory> = Vec::with_capacity(rows.len());
        let mut ids: Vec<Uuid> = Vec::with_capacity(rows.len());

        for row in &rows {
            let id: Uuid = row.get("id");
            ids.push(id);
            memories.push(Memory {
                id,
                fact: row.get("fact"),
                category: row.get("category"),
                source_session_id: row.get("source_session_id"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                access_count: row.get("access_count"),
            });
        }

        // Bump access counts in the background (best-effort)
        if !ids.is_empty() {
            if let Err(e) = client.execute(INCREMENT_ACCESS_SQL, &[&ids]).await {
                debug!(error = %e, "mem0: failed to increment access counts");
            }
        }

        debug!(count = memories.len(), "mem0 recalled memories");
        memories
    }

    /// Extract facts from a conversation exchange and store them.
    ///
    /// Designed to run as a background task after a turn completes.
    pub async fn capture(
        &self,
        user_msg: &str,
        assistant_msg: &str,
        session_id: Uuid,
    ) -> Vec<Memory> {
        if let Err(e) = self.ensure_connected().await {
            warn!(error = %e, "mem0 capture: connection check failed");
            return Vec::new();
        }
        let facts = match self.extract_facts(user_msg, assistant_msg).await {
            Ok(f) => f,
            Err(e) => {
                warn!(error = %e, "mem0 capture: fact extraction failed");
                return Vec::new();
            }
        };

        if facts.is_empty() {
            debug!("mem0 capture: no facts extracted");
            return Vec::new();
        }

        let mut stored = Vec::new();

        for fact in facts {
            match self.store_fact(&fact, session_id).await {
                Ok(Some(mem)) => stored.push(mem),
                Ok(None) => {
                    debug!(fact = %fact.fact, "mem0: deduplicated (already exists)");
                }
                Err(e) => {
                    warn!(error = %e, fact = %fact.fact, "mem0: failed to store fact");
                }
            }
        }

        info!(
            stored = stored.len(),
            "mem0 capture: facts stored for session"
        );
        stored
    }

    /// Delete a memory by its ID.
    pub async fn delete_memory(&self, id: &str) -> Result<(), String> {
        self.ensure_connected().await?;
        let client = self.client.lock().await;
        client
            .execute("DELETE FROM rune_memories WHERE id = $1::uuid", &[&id])
            .await
            .map_err(|e| format!("failed to delete memory: {e}"))?;
        Ok(())
    }

    /// Return all memories (for graph visualization and admin dashboards).
    pub async fn list_all(&self) -> Vec<Memory> {
        let client = self.client.lock().await;
        let rows = match client
            .query(
                "SELECT id, fact, category, source_session_id, created_at, updated_at, access_count FROM rune_memories ORDER BY created_at DESC",
                &[],
            )
            .await
        {
            Ok(rows) => rows,
            Err(e) => {
                warn!(error = %e, "mem0 list_all: query failed");
                return Vec::new();
            }
        };

        rows.iter()
            .map(|row| Memory {
                id: row.get("id"),
                fact: row.get("fact"),
                category: row.get("category"),
                source_session_id: row.get("source_session_id"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                access_count: row.get("access_count"),
            })
            .collect()
    }

    /// Build a knowledge graph: nodes (memories) + edges (cosine similarity above threshold).
    ///
    /// Uses a single LATERAL JOIN query to find the K nearest neighbors per node,
    /// with `a.id < b.id` to deduplicate edges at the SQL level.
    pub async fn graph(&self, edge_threshold: f64, neighbors_k: i64) -> MemoryGraph {
        if let Err(e) = self.ensure_connected().await {
            warn!(error = %e, "mem0 graph: connection check failed");
            return MemoryGraph {
                nodes: Vec::new(),
                edges: Vec::new(),
            };
        }
        let client = self.client.lock().await;

        // Get all memories
        let rows = match client
            .query(
                "SELECT id, fact, category, source_session_id, created_at, updated_at, access_count FROM rune_memories ORDER BY created_at DESC",
                &[],
            )
            .await
        {
            Ok(rows) => rows,
            Err(e) => {
                warn!(error = %e, "mem0 graph: node query failed");
                return MemoryGraph { nodes: vec![], edges: vec![] };
            }
        };

        let nodes: Vec<Memory> = rows
            .iter()
            .map(|row| Memory {
                id: row.get("id"),
                fact: row.get("fact"),
                category: row.get("category"),
                source_session_id: row.get("source_session_id"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                access_count: row.get("access_count"),
            })
            .collect();

        // Single query: for each node, find top-K neighbors with similarity > threshold.
        // a.id < b.id deduplicates edges (A-B only appears once).
        let edge_rows = match client
            .query_typed(
                r#"SELECT a.id AS source_id, b.id AS target_id,
                          1 - (a.embedding <=> b.embedding) AS similarity
                   FROM rune_memories a
                   JOIN LATERAL (
                       SELECT b.id, b.embedding
                       FROM rune_memories b
                       WHERE b.id > a.id
                         AND 1 - (a.embedding <=> b.embedding) > $1
                       ORDER BY a.embedding <=> b.embedding
                       LIMIT $2
                   ) b ON true"#,
                &[
                    (
                        &edge_threshold as &(dyn tokio_postgres::types::ToSql + Sync),
                        Type::FLOAT8,
                    ),
                    (
                        &neighbors_k as &(dyn tokio_postgres::types::ToSql + Sync),
                        Type::INT8,
                    ),
                ],
            )
            .await
        {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, "mem0 graph: edge query failed");
                return MemoryGraph {
                    nodes,
                    edges: vec![],
                };
            }
        };

        let edges: Vec<MemoryEdge> = edge_rows
            .iter()
            .map(|row| MemoryEdge {
                source: row.get("source_id"),
                target: row.get("target_id"),
                similarity: row.get("similarity"),
            })
            .collect();

        debug!(nodes = nodes.len(), edges = edges.len(), "mem0 graph built");
        MemoryGraph { nodes, edges }
    }

    /// Format recalled memories for injection into the system prompt.
    pub fn format_for_prompt(memories: &[Memory]) -> String {
        if memories.is_empty() {
            return String::new();
        }

        let mut parts = Vec::with_capacity(memories.len() + 1);
        parts.push("# Recalled Memories (auto-captured from previous sessions)\n".to_string());

        for mem in memories {
            parts.push(format!("- [{}] {}", mem.category, mem.fact));
        }

        parts.join("\n")
    }

    // ── Internal helpers ─────────────────────────────────────────────

    /// Embed a text string via the configured Azure endpoint.
    async fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
        let endpoint = self.config.embedding_endpoint.as_deref().unwrap();
        let api_key = self.config.embedding_api_key.as_deref().unwrap();
        let url = format!("{}?api-version={}", endpoint, self.config.api_version);

        let body = serde_json::json!({
            "input": text,
            "model": self.config.embedding_model,
            "dimensions": self.config.embedding_dims,
        });

        let resp = self
            .http
            .post(&url)
            .header("api-key", api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("embedding request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("embedding API returned {status}: {body}"));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("embedding response parse failed: {e}"))?;

        let embedding_vals = json
            .get("data")
            .and_then(|d| d.get(0))
            .and_then(|d| d.get("embedding"))
            .and_then(|e| e.as_array())
            .ok_or("unexpected embedding response structure")?;

        let embedding: Vec<f32> = embedding_vals
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect();

        if embedding.len() != self.config.embedding_dims {
            return Err(format!(
                "expected {} dims, got {}",
                self.config.embedding_dims,
                embedding.len()
            ));
        }

        Ok(embedding)
    }

    /// Use the extraction model to pull facts from a conversation exchange.
    async fn extract_facts(
        &self,
        user_msg: &str,
        assistant_msg: &str,
    ) -> Result<Vec<ExtractedFact>, String> {
        let user_content = format!("Conversation:\nUser: {user_msg}\nAssistant: {assistant_msg}");

        let request = rune_models::CompletionRequest {
            messages: vec![
                rune_models::ChatMessage {
                    role: rune_models::Role::System,
                    content: Some(EXTRACTION_SYSTEM_PROMPT.to_string()),
                    name: None,
                    tool_call_id: None,
                    tool_calls: None,
                },
                rune_models::ChatMessage {
                    role: rune_models::Role::User,
                    content: Some(user_content),
                    name: None,
                    tool_call_id: None,
                    tool_calls: None,
                },
            ],
            model: Some(self.config.extraction_model.clone()),
            temperature: Some(0.0),
            max_tokens: Some(1024),
            tools: None,
        };

        let response = self
            .extraction_provider
            .complete(&request)
            .await
            .map_err(|e| format!("extraction LLM call failed: {e}"))?;

        let content = response.content.as_deref().unwrap_or("[]").trim();

        // The LLM might wrap its response in a markdown code block.
        let json_str = content
            .strip_prefix("```json")
            .or_else(|| content.strip_prefix("```"))
            .and_then(|s| s.strip_suffix("```"))
            .unwrap_or(content)
            .trim();

        let facts: Vec<ExtractedFact> = serde_json::from_str(json_str)
            .map_err(|e| format!("failed to parse extraction JSON: {e} — raw: {json_str}"))?;

        Ok(facts)
    }

    /// Store a single fact, deduplicating against existing memories.
    ///
    /// Returns `Some(Memory)` if inserted or updated, `None` if an existing
    /// memory was close enough to be considered a duplicate (and was updated).
    async fn store_fact(
        &self,
        fact: &ExtractedFact,
        session_id: Uuid,
    ) -> Result<Option<Memory>, String> {
        let embedding = self.embed(&fact.fact).await?;
        let embedding_str = format_vector(&embedding);
        let client = self.client.lock().await;

        // Check for near-duplicate
        let dedup_rows = client
            .query_typed(
                DEDUP_CHECK_SQL,
                &[
                    (
                        &embedding_str as &(dyn tokio_postgres::types::ToSql + Sync),
                        Type::TEXT,
                    ),
                    (
                        &self.config.dedup_threshold as &(dyn tokio_postgres::types::ToSql + Sync),
                        Type::FLOAT8,
                    ),
                ],
            )
            .await
            .map_err(|e| format!("dedup check failed: {e}"))?;

        if let Some(existing) = dedup_rows.first() {
            let existing_id: Uuid = existing.get("id");
            let now = Utc::now();

            // Update the existing memory with the newer, potentially better phrasing
            client
                .query_typed(
                    UPDATE_MEMORY_SQL,
                    &[
                        (
                            &existing_id as &(dyn tokio_postgres::types::ToSql + Sync),
                            Type::UUID,
                        ),
                        (
                            &fact.fact as &(dyn tokio_postgres::types::ToSql + Sync),
                            Type::TEXT,
                        ),
                        (
                            &fact.category as &(dyn tokio_postgres::types::ToSql + Sync),
                            Type::TEXT,
                        ),
                        (
                            &embedding_str as &(dyn tokio_postgres::types::ToSql + Sync),
                            Type::TEXT,
                        ),
                        (
                            &now as &(dyn tokio_postgres::types::ToSql + Sync),
                            Type::TIMESTAMPTZ,
                        ),
                    ],
                )
                .await
                .map_err(|e| format!("memory update failed: {e}"))?;

            debug!(id = %existing_id, "mem0: updated existing memory (dedup)");
            return Ok(None);
        }

        // Insert new memory
        let id = Uuid::now_v7();
        let now = Utc::now();
        let session_opt = Some(session_id);

        client
            .query_typed(
                INSERT_MEMORY_SQL,
                &[
                    (
                        &id as &(dyn tokio_postgres::types::ToSql + Sync),
                        Type::UUID,
                    ),
                    (
                        &fact.fact as &(dyn tokio_postgres::types::ToSql + Sync),
                        Type::TEXT,
                    ),
                    (
                        &fact.category as &(dyn tokio_postgres::types::ToSql + Sync),
                        Type::TEXT,
                    ),
                    (
                        &embedding_str as &(dyn tokio_postgres::types::ToSql + Sync),
                        Type::TEXT,
                    ),
                    (
                        &session_opt as &(dyn tokio_postgres::types::ToSql + Sync),
                        Type::UUID,
                    ),
                    (
                        &now as &(dyn tokio_postgres::types::ToSql + Sync),
                        Type::TIMESTAMPTZ,
                    ),
                    (
                        &now as &(dyn tokio_postgres::types::ToSql + Sync),
                        Type::TIMESTAMPTZ,
                    ),
                ],
            )
            .await
            .map_err(|e| format!("memory insert failed: {e}"))?;

        Ok(Some(Memory {
            id,
            fact: fact.fact.clone(),
            category: fact.category.clone(),
            source_session_id: Some(session_id),
            created_at: now,
            updated_at: now,
            access_count: 0,
        }))
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Format a float vector as a Postgres vector literal: `[0.1,0.2,...]`
fn format_vector(v: &[f32]) -> String {
    let inner: Vec<String> = v.iter().map(|f| f.to_string()).collect();
    format!("[{}]", inner.join(","))
}

/// Connect to Postgres with TLS (required for Azure Cosmos DB for PostgreSQL
/// and most production deployments).
async fn connect_postgres(url: &str) -> Result<Client, String> {
    let tls_connector = native_tls::TlsConnector::builder()
        .build()
        .map_err(|e| format!("TLS connector build failed: {e}"))?;

    let pg_tls = postgres_native_tls::MakeTlsConnector::new(tls_connector);

    let (client, connection) = tokio_postgres::connect(url, pg_tls)
        .await
        .map_err(|e| format!("Postgres connection failed: {e}"))?;

    // Spawn the connection task — if it errors, we'll discover it on the
    // next query and can reconnect.
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            error!(error = %e, "mem0 Postgres connection task ended with error");
        }
    });

    Ok(client)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_vector_roundtrip() {
        let v = vec![0.1f32, -0.5, 1.0, 0.0];
        let s = format_vector(&v);
        assert_eq!(s, "[0.1,-0.5,1,0]");
    }

    #[test]
    fn format_for_prompt_empty() {
        assert_eq!(Mem0Engine::format_for_prompt(&[]), "");
    }

    #[test]
    fn format_for_prompt_with_memories() {
        let memories = vec![
            Memory {
                id: Uuid::nil(),
                fact: "User prefers dark mode".to_string(),
                category: "preference".to_string(),
                source_session_id: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                access_count: 3,
            },
            Memory {
                id: Uuid::nil(),
                fact: "Project uses Rust".to_string(),
                category: "project".to_string(),
                source_session_id: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                access_count: 1,
            },
        ];

        let prompt = Mem0Engine::format_for_prompt(&memories);
        assert!(prompt.contains("Recalled Memories"));
        assert!(prompt.contains("[preference] User prefers dark mode"));
        assert!(prompt.contains("[project] Project uses Rust"));
    }
}
