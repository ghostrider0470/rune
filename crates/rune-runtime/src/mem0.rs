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

/// Mem0 engine that manages the full recall/capture lifecycle.
///
/// Thread-safe and cheaply cloneable — hold one `Arc<Mem0Engine>` in the
/// `TurnExecutor` and share it across concurrent turns.
pub struct Mem0Engine {
    client: tokio::sync::Mutex<Client>,
    http: reqwest::Client,
    config: Mem0Config,
    extraction_provider: Arc<dyn ModelProvider>,
}

// ── SQL constants ────────────────────────────────────────────────────

const ENSURE_EXTENSION_SQL: &str = "CREATE EXTENSION IF NOT EXISTS vector";

const ENSURE_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS rune_memories (
    id UUID PRIMARY KEY,
    fact TEXT NOT NULL,
    category TEXT NOT NULL DEFAULT 'general',
    embedding vector(3072),
    source_session_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    access_count INTEGER NOT NULL DEFAULT 0
)
"#;

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

        // Ensure schema
        if let Err(e) = client.batch_execute(ENSURE_EXTENSION_SQL).await {
            warn!(error = %e, "mem0: failed to create vector extension (may already exist or lack permissions)");
            // Continue — the extension might already be loaded.
        }

        if let Err(e) = client.batch_execute(ENSURE_TABLE_SQL).await {
            error!(error = %e, "mem0: failed to create rune_memories table — disabling");
            return None;
        }

        if let Err(e) = client.batch_execute(ENSURE_INDEX_SQL).await {
            warn!(error = %e, "mem0: failed to create HNSW index (may already exist)");
        }

        info!("mem0 engine connected and schema ensured");

        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("failed to build reqwest client");

        Some(Arc::new(Self {
            client: tokio::sync::Mutex::new(client),
            http,
            config: config.clone(),
            extraction_provider,
        }))
    }

    // ── Public API ───────────────────────────────────────────────────

    /// Recall memories semantically similar to the query.
    ///
    /// Returns an empty vec on any error (never blocks the turn).
    pub async fn recall(&self, query: &str) -> Vec<Memory> {
        let embedding = match self.embed(query).await {
            Ok(e) => e,
            Err(e) => {
                warn!(error = %e, "mem0 recall: embedding failed");
                return Vec::new();
            }
        };

        let embedding_str = format_vector(&embedding);
        let client = self.client.lock().await;

        let rows = match client
            .query(
                RECALL_SQL,
                &[
                    &embedding_str,
                    &self.config.similarity_threshold,
                    &(self.config.top_k as i64),
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
            return Err(format!(
                "embedding API returned {status}: {body}"
            ));
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
        let user_content = format!(
            "Conversation:\nUser: {user_msg}\nAssistant: {assistant_msg}"
        );

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
            model: None, // use the provider's default
            temperature: Some(0.0),
            max_tokens: Some(1024),
            tools: None,
        };

        let response = self
            .extraction_provider
            .complete(&request)
            .await
            .map_err(|e| format!("extraction LLM call failed: {e}"))?;

        let content = response
            .content
            .as_deref()
            .unwrap_or("[]")
            .trim();

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
            .query(
                DEDUP_CHECK_SQL,
                &[&embedding_str, &self.config.dedup_threshold],
            )
            .await
            .map_err(|e| format!("dedup check failed: {e}"))?;

        if let Some(existing) = dedup_rows.first() {
            let existing_id: Uuid = existing.get("id");
            let now = Utc::now();

            // Update the existing memory with the newer, potentially better phrasing
            client
                .execute(
                    UPDATE_MEMORY_SQL,
                    &[
                        &existing_id,
                        &fact.fact,
                        &fact.category,
                        &embedding_str,
                        &now,
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

        client
            .execute(
                INSERT_MEMORY_SQL,
                &[
                    &id,
                    &fact.fact,
                    &fact.category,
                    &embedding_str,
                    &Some(session_id),
                    &now,
                    &now,
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
