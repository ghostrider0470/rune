//! Mem0-style auto-capture/recall memory engine.
//!
//! Provides persistent cross-session memory for the agent by:
//! - **Recall**: Before each turn, embedding the user message and searching
//!   for semantically similar past facts via the `MemoryFactRepo` trait.
//! - **Capture**: After each turn, extracting durable facts from the
//!   conversation via a cheap LLM call and storing them with embeddings.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use rune_config::Mem0Config;
use rune_models::ModelProvider;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};
use uuid::Uuid;

/// A single remembered fact.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Memory {
    pub id: Uuid,
    pub fact: String,
    pub category: String,
    pub source_session_id: Option<Uuid>,
    pub source_agent: Option<String>,
    pub trigger: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub access_count: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryCaptureMetadata {
    pub source_agent: Option<String>,
    pub trigger: Option<String>,
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

impl From<rune_store::models::MemoryFact> for Memory {
    fn from(f: rune_store::models::MemoryFact) -> Self {
        Self {
            id: f.id,
            fact: f.fact,
            category: f.category,
            source_session_id: f.source_session_id,
            source_agent: None,
            trigger: None,
            created_at: f.created_at,
            updated_at: f.updated_at,
            access_count: f.access_count,
        }
    }
}

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

/// Mem0 engine that manages the full recall/capture lifecycle.
///
/// Thread-safe and cheaply cloneable — hold one `Arc<Mem0Engine>` in the
/// `TurnExecutor` and share it across concurrent turns.
pub struct Mem0Engine {
    repo: Arc<dyn rune_store::repos::MemoryFactRepo>,
    http: reqwest::Client,
    config: Mem0Config,
    extraction_provider: Arc<dyn ModelProvider>,
}

impl Mem0Engine {
    /// Create a new Mem0Engine backed by the given repository.
    ///
    /// Returns `None` if the config is disabled or embedding config is missing,
    /// so the caller can gracefully degrade.
    pub fn try_new(
        config: &Mem0Config,
        extraction_provider: Arc<dyn ModelProvider>,
        repo: Arc<dyn rune_store::repos::MemoryFactRepo>,
    ) -> Option<Arc<Self>> {
        if !config.enabled {
            info!("mem0 disabled by configuration");
            return None;
        }

        if config.embedding_endpoint.is_none() || config.embedding_api_key.is_none() {
            warn!("mem0 enabled but embedding endpoint/key not configured — skipping");
            return None;
        }

        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("failed to build reqwest client");

        info!("mem0 engine initialized");
        Some(Arc::new(Self {
            repo,
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
        let facts = match self
            .repo
            .recall(
                &embedding_str,
                self.config.similarity_threshold,
                self.config.top_k as i64,
            )
            .await
        {
            Ok(f) => f,
            Err(e) => {
                warn!(error = %e, "mem0 recall: query failed");
                return Vec::new();
            }
        };

        let ids: Vec<Uuid> = facts.iter().map(|f| f.id).collect();
        if !ids.is_empty() {
            if let Err(e) = self.repo.increment_access(&ids).await {
                debug!(error = %e, "mem0: failed to increment access counts");
            }
        }

        let memories: Vec<Memory> = facts.into_iter().map(Memory::from).collect();
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
        self.capture_with_metadata(
            user_msg,
            assistant_msg,
            session_id,
            MemoryCaptureMetadata {
                source_agent: None,
                trigger: None,
            },
        )
        .await
    }

    pub async fn capture_with_metadata(
        &self,
        user_msg: &str,
        assistant_msg: &str,
        session_id: Uuid,
        _metadata: MemoryCaptureMetadata,
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

    /// Delete a memory by its ID.
    pub async fn delete_memory(&self, id: &str) -> Result<(), String> {
        self.repo
            .delete(id)
            .await
            .map_err(|e| format!("failed to delete memory: {e}"))
    }

    /// Return all memories (for graph visualization and admin dashboards).
    pub async fn list_all(&self) -> Vec<Memory> {
        match self.repo.list_all().await {
            Ok(facts) => facts.into_iter().map(Memory::from).collect(),
            Err(e) => {
                warn!(error = %e, "mem0 list_all: query failed");
                Vec::new()
            }
        }
    }

    /// Build a knowledge graph: nodes (memories) + similarity edges for visualization.
    pub async fn graph(&self, edge_threshold: f64, neighbors_k: i64) -> MemoryGraph {
        let nodes = self.list_all().await;
        let edges = match self.repo.graph_edges(edge_threshold, neighbors_k).await {
            Ok(e) => e
                .into_iter()
                .map(|e| MemoryEdge {
                    source: e.source,
                    target: e.target,
                    similarity: e.similarity,
                })
                .collect(),
            Err(e) => {
                warn!(error = %e, "mem0 graph: edge query failed");
                vec![]
            }
        };
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
            stable_prefix_messages: None,
            stable_prefix_tools: None,
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
    /// Returns `Some(Memory)` if inserted, `None` if an existing
    /// memory was close enough to be considered a duplicate (and was updated).
    async fn store_fact(
        &self,
        fact: &ExtractedFact,
        session_id: Uuid,
    ) -> Result<Option<Memory>, String> {
        let embedding = self.embed(&fact.fact).await?;
        let embedding_str = format_vector(&embedding);

        let dedup = self
            .repo
            .dedup_check(&embedding_str, self.config.dedup_threshold)
            .await
            .map_err(|e| format!("dedup check failed: {e}"))?;

        if let Some((existing_id, _, _)) = dedup {
            let now = Utc::now();
            self.repo
                .update(existing_id, &fact.fact, &fact.category, &embedding_str, now)
                .await
                .map_err(|e| format!("memory update failed: {e}"))?;
            debug!(id = %existing_id, "mem0: updated existing memory (dedup)");
            return Ok(None);
        }

        let id = Uuid::now_v7();
        let now = Utc::now();
        self.repo
            .insert(
                id,
                &fact.fact,
                &fact.category,
                &embedding_str,
                Some(session_id),
                now,
            )
            .await
            .map_err(|e| format!("memory insert failed: {e}"))?;

        Ok(Some(Memory {
            id,
            fact: fact.fact.clone(),
            category: fact.category.clone(),
            source_session_id: Some(session_id),
            source_agent: None,
            trigger: None,
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
                source_agent: None,
                trigger: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                access_count: 3,
            },
            Memory {
                id: Uuid::nil(),
                fact: "Project uses Rust".to_string(),
                category: "project".to_string(),
                source_session_id: None,
                source_agent: None,
                trigger: None,
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
