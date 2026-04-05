//! Mem0-style auto-capture/recall memory engine.
//!
//! Provides persistent cross-session memory for the agent by:
//! - **Recall**: Before each turn, embedding the user message and searching
//!   for semantically similar past facts via the `MemoryFactRepo` trait.
//! - **Capture**: After each turn, extracting durable facts from the
//!   conversation via a cheap LLM call and storing them with embeddings.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use rune_config::Mem0Config;
use rune_models::ModelProvider;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::mem0_vault::{VaultSyncReport, VaultSyncer};

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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCaptureAction {
    Inserted,
    SkippedDuplicate,
    UpdatedExact,
    Merged,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryCaptureDecision {
    pub action: MemoryCaptureAction,
    pub memory: Option<Memory>,
    pub matched_memory_id: Option<Uuid>,
    pub matched_fact: Option<String>,
    pub similarity: Option<f64>,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MemoryHierarchyMetrics {
    pub recall_hits: u64,
    pub warm_memories: u64,
    pub hot_memories: u64,
    pub cold_memories: u64,
    pub total_memories: u64,
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
    vault: Option<Arc<VaultSyncer>>,
}

impl Mem0Engine {
    /// Create a new Mem0Engine backed by the given repository.
    ///
    /// Returns `None` if the config is disabled or embedding config is missing,
    /// so the caller can gracefully degrade.
    ///
    /// Pass `vault_dir` to enable the markdown vault sync layer. When enabled,
    /// every stored/deleted fact is projected to a `.md` file in the background
    /// with zero impact on the LLM recall/capture hot path.
    pub async fn try_new(
        config: &Mem0Config,
        extraction_provider: Arc<dyn ModelProvider>,
        repo: Arc<dyn rune_store::repos::MemoryFactRepo>,
        vault_dir: Option<PathBuf>,
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

        let vault = if config.vault_enabled {
            if let Some(dir) = vault_dir {
                match VaultSyncer::new(dir.clone(), config.vault_link_threshold).await {
                    Ok(v) => {
                        info!(dir = %dir.display(), "mem0 vault sync enabled");
                        Some(Arc::new(v))
                    }
                    Err(e) => {
                        warn!(error = %e, "mem0 vault init failed — continuing without vault");
                        None
                    }
                }
            } else {
                warn!("mem0 vault enabled but no vault_dir resolved — skipping");
                None
            }
        } else {
            None
        };

        info!("mem0 engine initialized");
        Some(Arc::new(Self {
            repo,
            http,
            config: config.clone(),
            extraction_provider,
            vault,
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
        metadata: MemoryCaptureMetadata,
    ) -> Vec<Memory> {
        let decisions = self
            .capture_with_decisions(user_msg, assistant_msg, session_id, metadata)
            .await;

        let stored: Vec<Memory> = decisions
            .into_iter()
            .filter_map(|decision| match decision.action {
                MemoryCaptureAction::Inserted
                | MemoryCaptureAction::UpdatedExact
                | MemoryCaptureAction::Merged => decision.memory,
                MemoryCaptureAction::SkippedDuplicate => None,
            })
            .collect();

        info!(
            stored = stored.len(),
            "mem0 capture: facts stored for session"
        );
        stored
    }

    pub async fn capture_with_decisions(
        &self,
        user_msg: &str,
        assistant_msg: &str,
        session_id: Uuid,
        _metadata: MemoryCaptureMetadata,
    ) -> Vec<MemoryCaptureDecision> {
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

        let mut decisions = Vec::new();
        let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();

        for fact in facts {
            match self.store_fact(&fact, session_id).await {
                Ok(decision) => {
                    let key = match decision.action {
                        MemoryCaptureAction::Inserted => "inserted",
                        MemoryCaptureAction::SkippedDuplicate => "skipped_duplicate",
                        MemoryCaptureAction::UpdatedExact => "updated_exact",
                        MemoryCaptureAction::Merged => "merged",
                    };
                    *counts.entry(key).or_default() += 1;
                    decisions.push(decision);
                }
                Err(e) => {
                    warn!(error = %e, fact = %fact.fact, "mem0: failed to store fact");
                }
            }
        }

        info!(
            inserted = counts.get("inserted").copied().unwrap_or(0),
            skipped_duplicate = counts.get("skipped_duplicate").copied().unwrap_or(0),
            updated_exact = counts.get("updated_exact").copied().unwrap_or(0),
            merged = counts.get("merged").copied().unwrap_or(0),
            "mem0 capture: decisions recorded for session"
        );

        decisions
    }

    /// Delete a memory by its ID.
    pub async fn delete_memory(&self, id: &str) -> Result<(), String> {
        self.repo
            .delete(id)
            .await
            .map_err(|e| format!("failed to delete memory: {e}"))?;

        // Background vault cleanup (fire-and-forget, never blocks LLM).
        if let Some(ref vault) = self.vault {
            if let Ok(uid) = Uuid::parse_str(id) {
                let vault = vault.clone();
                tokio::spawn(async move {
                    if let Err(e) = vault.delete_fact(&uid).await {
                        warn!(error = %e, id = %uid, "vault delete failed");
                    }
                });
            }
        }

        Ok(())
    }

    /// Return all memories (for graph visualization and admin dashboards).
    pub async fn memory_hierarchy_metrics(
        &self,
    ) -> Result<MemoryHierarchyMetrics, rune_store::StoreError> {
        let memories = self.repo.list_all().await?;
        let total_memories = memories.len() as u64;
        let warm_memories = memories
            .iter()
            .filter(|memory| memory.access_count > 0)
            .count() as u64;
        let hot_memories = memories
            .iter()
            .filter(|memory| memory.access_count >= 3)
            .count() as u64;
        let cold_memories = memories
            .iter()
            .filter(|memory| memory.access_count <= 0)
            .count() as u64;
        let recall_hits = memories
            .iter()
            .map(|memory| u64::try_from(memory.access_count.max(0)).unwrap_or(0))
            .sum();

        Ok(MemoryHierarchyMetrics {
            recall_hits,
            warm_memories,
            hot_memories,
            cold_memories,
            total_memories,
        })
    }

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

    /// Perform a full vault sync: re-export every memory as a `.md` file with
    /// `[[wikilinks]]` derived from the similarity graph, pruning orphaned files.
    ///
    /// Intended for the `/api/v1/memory/vault/sync` endpoint and one-off CLI use.
    pub async fn vault_full_sync(&self) -> Result<VaultSyncReport, String> {
        let vault = self
            .vault
            .as_ref()
            .ok_or_else(|| "vault not enabled".to_string())?;
        let memories = self.list_all().await;
        let graph = self.graph(vault.link_threshold(), 5).await;
        vault
            .full_sync(&memories, &graph)
            .await
            .map_err(|e| format!("vault sync failed: {e}"))
    }

    /// Whether the vault sync layer is active.
    pub fn vault_enabled(&self) -> bool {
        self.vault.is_some()
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
                    content_parts: None,
                    name: None,
                    tool_call_id: None,
                    tool_calls: None,
                },
                rune_models::ChatMessage {
                    role: rune_models::Role::User,
                    content: Some(user_content),
                    content_parts: None,
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

    /// Store a single fact using a conservative dedup/update policy.
    async fn store_fact(
        &self,
        fact: &ExtractedFact,
        session_id: Uuid,
    ) -> Result<MemoryCaptureDecision, String> {
        let embedding = self.embed(&fact.fact).await?;
        let embedding_str = format_vector(&embedding);

        let dedup = self
            .repo
            .dedup_check(&embedding_str, self.config.dedup_threshold)
            .await
            .map_err(|e| format!("dedup check failed: {e}"))?;

        if let Some((existing_id, existing_fact, similarity)) = dedup.clone() {
            let normalized_existing = existing_fact.trim().to_ascii_lowercase();
            let normalized_new = fact.fact.trim().to_ascii_lowercase();

            if normalized_existing == normalized_new {
                let now = Utc::now();
                self.repo
                    .update(existing_id, &fact.fact, &fact.category, &embedding_str, now)
                    .await
                    .map_err(|e| format!("memory update failed: {e}"))?;
                debug!(id = %existing_id, similarity, "mem0: updated exact memory match");

                let memory = Memory {
                    id: existing_id,
                    fact: fact.fact.clone(),
                    category: fact.category.clone(),
                    source_session_id: Some(session_id),
                    source_agent: None,
                    trigger: None,
                    created_at: now,
                    updated_at: now,
                    access_count: 0,
                };

                if let Some(ref vault) = self.vault {
                    let vault = vault.clone();
                    let mem = memory.clone();
                    tokio::spawn(async move {
                        if let Err(e) = vault.sync_fact_simple(&mem).await {
                            warn!(error = %e, "vault sync failed for exact-match updated fact");
                        }
                    });
                }

                return Ok(MemoryCaptureDecision {
                    action: MemoryCaptureAction::UpdatedExact,
                    memory: Some(memory),
                    matched_memory_id: Some(existing_id),
                    matched_fact: Some(existing_fact),
                    similarity: Some(similarity),
                    reason: "exact normalized fact match updated existing memory".to_string(),
                });
            }

            debug!(id = %existing_id, similarity, existing_fact = %existing_fact, new_fact = %fact.fact, "mem0: preserving distinct fact despite approximate similarity");
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

        let memory = Memory {
            id,
            fact: fact.fact.clone(),
            category: fact.category.clone(),
            source_session_id: Some(session_id),
            source_agent: None,
            trigger: None,
            created_at: now,
            updated_at: now,
            access_count: 0,
        };

        // Background vault sync (fire-and-forget, never blocks LLM).
        if let Some(ref vault) = self.vault {
            let vault = vault.clone();
            let mem = memory.clone();
            tokio::spawn(async move {
                if let Err(e) = vault.sync_fact_simple(&mem).await {
                    warn!(error = %e, "vault sync failed for new fact");
                }
            });
        }

        Ok(MemoryCaptureDecision {
            action: MemoryCaptureAction::Inserted,
            memory: Some(memory),
            matched_memory_id: dedup.as_ref().map(|(id, _, _)| *id),
            matched_fact: dedup.as_ref().map(|(_, fact, _)| fact.clone()),
            similarity: dedup.as_ref().map(|(_, _, similarity)| *similarity),
            reason: if dedup.is_some() {
                "approximate similarity alone is not enough to overwrite an existing fact"
                    .to_string()
            } else {
                "no similar existing memory exceeded the dedup threshold".to_string()
            },
        })
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
    fn memory_capture_action_serializes_snake_case() {
        let value = serde_json::to_value(MemoryCaptureAction::UpdatedExact).unwrap();
        assert_eq!(value, serde_json::json!("updated_exact"));
    }

    #[test]
    fn memory_capture_decision_records_insert_reason() {
        let decision = MemoryCaptureDecision {
            action: MemoryCaptureAction::Inserted,
            memory: None,
            matched_memory_id: None,
            matched_fact: None,
            similarity: None,
            reason: "no similar existing memory exceeded the dedup threshold".to_string(),
        };

        let value = serde_json::to_value(&decision).unwrap();
        assert_eq!(value["action"], "inserted");
        assert_eq!(
            value["reason"],
            "no similar existing memory exceeded the dedup threshold"
        );
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
