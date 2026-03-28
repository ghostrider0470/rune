//! Markdown vault sync for Mem0 facts.
//!
//! Projects each vector-stored memory fact to a human-readable `.md` file
//! with YAML frontmatter and `[[wikilinks]]` to related facts. The vector
//! store remains the source of truth; the vault is a derived view.
//!
//! All sync operations are designed to run in the background via
//! `tokio::spawn` — they never block the LLM recall/capture hot path.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::mem0::{Memory, MemoryGraph};

// ── Vault sync report ───────────────────────────────────────────────

/// Summary of a full vault sync operation.
#[derive(Clone, Debug, Default, Serialize)]
pub struct VaultSyncReport {
    pub created: usize,
    pub updated: usize,
    pub deleted: usize,
    pub errors: usize,
}

// ── Vault index ─────────────────────────────────────────────────────

/// Bidirectional UUID ↔ slug index, persisted as JSON.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct VaultIndex {
    entries: HashMap<Uuid, String>,
}

impl VaultIndex {
    async fn load(vault_dir: &Path) -> Self {
        let path = vault_dir.join(".vault-index.json");
        match tokio::fs::read_to_string(&path).await {
            Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    async fn save(&self, vault_dir: &Path) -> Result<(), String> {
        let path = vault_dir.join(".vault-index.json");
        let tmp = vault_dir.join(".vault-index.json.tmp");
        let data = serde_json::to_string_pretty(self)
            .map_err(|e| format!("index serialize: {e}"))?;
        tokio::fs::write(&tmp, data)
            .await
            .map_err(|e| format!("index write: {e}"))?;
        tokio::fs::rename(&tmp, &path)
            .await
            .map_err(|e| format!("index rename: {e}"))?;
        Ok(())
    }

    fn get_slug(&self, id: &Uuid) -> Option<&str> {
        self.entries.get(id).map(|s| s.as_str())
    }

    fn slug_exists(&self, slug: &str) -> bool {
        self.entries.values().any(|s| s == slug)
    }

    fn insert(&mut self, id: Uuid, slug: String) {
        self.entries.insert(id, slug);
    }

    fn remove(&mut self, id: &Uuid) -> Option<String> {
        self.entries.remove(id)
    }
}

// ── Slug generation ─────────────────────────────────────────────────

/// Generate a URL/filename-safe slug from fact text.
///
/// "User prefers dark mode" → "user-prefers-dark-mode"
fn generate_slug(fact: &str) -> String {
    let slug: String = fact
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();

    // Collapse consecutive hyphens, trim edges.
    let mut result = String::with_capacity(slug.len());
    let mut prev_hyphen = true; // skip leading hyphens
    for c in slug.chars() {
        if c == '-' {
            if !prev_hyphen {
                result.push('-');
            }
            prev_hyphen = true;
        } else {
            result.push(c);
            prev_hyphen = false;
        }
    }

    // Trim trailing hyphen.
    if result.ends_with('-') {
        result.pop();
    }

    // Truncate at ~60 chars on a word boundary.
    if result.len() > 60 {
        if let Some(pos) = result[..60].rfind('-') {
            result.truncate(pos);
        } else {
            result.truncate(60);
        }
    }

    if result.is_empty() {
        "fact".to_string()
    } else {
        result
    }
}

/// Resolve a unique slug, appending `-2`, `-3` etc. on collision.
fn resolve_slug(base: &str, id: Uuid, index: &VaultIndex) -> String {
    // If this UUID already has a slug, keep it stable.
    if let Some(existing) = index.get_slug(&id) {
        return existing.to_string();
    }

    if !index.slug_exists(base) {
        return base.to_string();
    }

    for n in 2..1000 {
        let candidate = format!("{base}-{n}");
        if !index.slug_exists(&candidate) {
            return candidate;
        }
    }
    // Extremely unlikely fallback.
    format!("{base}-{}", &id.to_string()[..8])
}

// ── Markdown rendering ──────────────────────────────────────────────

fn render_fact_md(memory: &Memory, related: &[(String, f64)]) -> String {
    let mut md = String::with_capacity(512);

    // Frontmatter
    md.push_str("---\n");
    md.push_str(&format!("id: \"{}\"\n", memory.id));
    md.push_str(&format!("category: \"{}\"\n", memory.category));
    md.push_str(&format!("created_at: \"{}\"\n", memory.created_at.to_rfc3339()));
    md.push_str(&format!("updated_at: \"{}\"\n", memory.updated_at.to_rfc3339()));
    md.push_str(&format!("access_count: {}\n", memory.access_count));
    if let Some(ref sid) = memory.source_session_id {
        md.push_str(&format!("source_session_id: \"{sid}\"\n"));
    }
    md.push_str("tags:\n");
    md.push_str("  - mem0\n");
    md.push_str(&format!("  - {}\n", memory.category));
    md.push_str("---\n\n");

    // Body
    md.push_str(&memory.fact);
    md.push('\n');

    // Related links
    if !related.is_empty() {
        md.push_str("\n## Related\n\n");
        for (slug, similarity) in related {
            md.push_str(&format!(
                "- [[{slug}]] ({:.0}%)\n",
                similarity * 100.0
            ));
        }
    }

    md
}

fn render_categories_md(categories: &HashMap<String, Vec<String>>) -> String {
    let mut md = String::with_capacity(1024);
    md.push_str("---\ngenerated: true\n---\n\n");
    md.push_str("# Memory Vault\n\n");

    let mut cats: Vec<_> = categories.keys().collect();
    cats.sort();

    for cat in cats {
        md.push_str(&format!("## {cat}\n\n"));
        if let Some(slugs) = categories.get(cat.as_str()) {
            for slug in slugs {
                md.push_str(&format!("- [[{slug}]]\n"));
            }
        }
        md.push('\n');
    }

    md
}

// ── VaultSyncer ─────────────────────────────────────────────────────

/// Syncs Mem0 facts to a directory of markdown files.
///
/// All methods are safe to call from `tokio::spawn` — they never block
/// and gracefully handle I/O errors without panicking.
pub struct VaultSyncer {
    vault_dir: PathBuf,
    link_threshold: f64,
    index: Arc<RwLock<VaultIndex>>,
}

impl VaultSyncer {
    /// Create a new syncer, loading or creating the index.
    pub async fn new(vault_dir: PathBuf, link_threshold: f64) -> Result<Self, String> {
        tokio::fs::create_dir_all(&vault_dir)
            .await
            .map_err(|e| format!("create vault dir: {e}"))?;

        let index = VaultIndex::load(&vault_dir).await;
        Ok(Self {
            vault_dir,
            link_threshold,
            index: Arc::new(RwLock::new(index)),
        })
    }

    /// The configured link threshold (used by callers to build the graph).
    pub fn link_threshold(&self) -> f64 {
        self.link_threshold
    }

    /// Write a single fact to the vault (without Related links).
    ///
    /// Fast path for incremental sync after `store_fact()`.
    pub async fn sync_fact_simple(&self, memory: &Memory) -> Result<(), String> {
        let slug = {
            let mut idx = self.index.write().await;
            let base = generate_slug(&memory.fact);
            let slug = resolve_slug(&base, memory.id, &idx);
            idx.insert(memory.id, slug.clone());
            idx.save(&self.vault_dir).await?;
            slug
        };

        let md = render_fact_md(memory, &[]);
        let path = self.vault_dir.join(format!("{slug}.md"));
        tokio::fs::write(&path, md)
            .await
            .map_err(|e| format!("write {slug}.md: {e}"))?;

        debug!(id = %memory.id, slug = %slug, "vault: synced fact");
        Ok(())
    }

    /// Remove a fact's markdown file from the vault.
    pub async fn delete_fact(&self, id: &Uuid) -> Result<(), String> {
        let slug = {
            let mut idx = self.index.write().await;
            let slug = idx.remove(id);
            idx.save(&self.vault_dir).await?;
            slug
        };

        if let Some(slug) = slug {
            let path = self.vault_dir.join(format!("{slug}.md"));
            if path.exists() {
                tokio::fs::remove_file(&path)
                    .await
                    .map_err(|e| format!("delete {slug}.md: {e}"))?;
            }
            debug!(id = %id, slug = %slug, "vault: deleted fact");
        }

        Ok(())
    }

    /// Full sync: rebuild all markdown files from the current memory state.
    ///
    /// This is the only path that generates `[[wikilinks]]` in the Related
    /// section, since it has access to the full graph.
    pub async fn full_sync(
        &self,
        memories: &[Memory],
        graph: &MemoryGraph,
    ) -> Result<VaultSyncReport, String> {
        let mut report = VaultSyncReport::default();

        // Build edge lookup: id → [(target_id, similarity)]
        let mut edge_map: HashMap<Uuid, Vec<(Uuid, f64)>> = HashMap::new();
        for edge in &graph.edges {
            edge_map
                .entry(edge.source)
                .or_default()
                .push((edge.target, edge.similarity));
            edge_map
                .entry(edge.target)
                .or_default()
                .push((edge.source, edge.similarity));
        }

        // Assign slugs to all memories first (so wikilinks can resolve).
        let mut idx = self.index.write().await;
        let mut id_to_slug: HashMap<Uuid, String> = HashMap::new();

        for mem in memories {
            let base = generate_slug(&mem.fact);
            let slug = resolve_slug(&base, mem.id, &idx);
            idx.insert(mem.id, slug.clone());
            id_to_slug.insert(mem.id, slug);
        }

        idx.save(&self.vault_dir).await?;
        drop(idx);

        // Write each fact file with resolved wikilinks.
        let mut categories: HashMap<String, Vec<String>> = HashMap::new();

        for mem in memories {
            let slug = match id_to_slug.get(&mem.id) {
                Some(s) => s,
                None => continue,
            };

            // Build related links for this fact.
            let related: Vec<(String, f64)> = edge_map
                .get(&mem.id)
                .map(|edges| {
                    let mut links: Vec<(String, f64)> = edges
                        .iter()
                        .filter_map(|(target_id, sim)| {
                            id_to_slug.get(target_id).map(|s| (s.clone(), *sim))
                        })
                        .collect();
                    links.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                    links
                })
                .unwrap_or_default();

            let md = render_fact_md(mem, &related);
            let path = self.vault_dir.join(format!("{slug}.md"));

            let existed = path.exists();
            match tokio::fs::write(&path, md).await {
                Ok(()) => {
                    if existed {
                        report.updated += 1;
                    } else {
                        report.created += 1;
                    }
                }
                Err(e) => {
                    warn!(slug = %slug, error = %e, "vault: failed to write fact");
                    report.errors += 1;
                }
            }

            categories
                .entry(mem.category.clone())
                .or_default()
                .push(slug.clone());
        }

        // Prune orphaned files (in index but not in current memories).
        let live_ids: std::collections::HashSet<Uuid> =
            memories.iter().map(|m| m.id).collect();
        let mut idx = self.index.write().await;
        let orphans: Vec<Uuid> = idx
            .entries
            .keys()
            .filter(|id| !live_ids.contains(id))
            .copied()
            .collect();

        for orphan_id in orphans {
            if let Some(slug) = idx.remove(&orphan_id) {
                let path = self.vault_dir.join(format!("{slug}.md"));
                if path.exists() {
                    let _ = tokio::fs::remove_file(&path).await;
                    report.deleted += 1;
                }
            }
        }
        idx.save(&self.vault_dir).await?;
        drop(idx);

        // Write category index.
        let cat_md = render_categories_md(&categories);
        let _ = tokio::fs::write(self.vault_dir.join("_categories.md"), cat_md).await;

        debug!(
            created = report.created,
            updated = report.updated,
            deleted = report.deleted,
            "vault: full sync complete"
        );

        Ok(report)
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem0::MemoryEdge;
    use chrono::Utc;

    fn make_memory(fact: &str, category: &str) -> Memory {
        Memory {
            id: Uuid::now_v7(),
            fact: fact.to_string(),
            category: category.to_string(),
            source_session_id: None,
            source_agent: None,
            trigger: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            access_count: 0,
        }
    }

    #[test]
    fn slug_basic() {
        assert_eq!(generate_slug("User prefers dark mode"), "user-prefers-dark-mode");
    }

    #[test]
    fn slug_special_chars() {
        assert_eq!(generate_slug("C++ 17 standard!"), "c-17-standard");
    }

    #[test]
    fn slug_collapses_hyphens() {
        assert_eq!(generate_slug("hello   ---   world"), "hello-world");
    }

    #[test]
    fn slug_truncates_at_word_boundary() {
        let long = "this is a very long fact that should be truncated at approximately sixty characters or so to keep filenames reasonable";
        let slug = generate_slug(long);
        assert!(slug.len() <= 60);
        assert!(!slug.ends_with('-'));
    }

    #[test]
    fn slug_empty_input() {
        assert_eq!(generate_slug(""), "fact");
        assert_eq!(generate_slug("---"), "fact");
    }

    #[test]
    fn resolve_slug_no_collision() {
        let index = VaultIndex::default();
        let slug = resolve_slug("user-prefers-dark-mode", Uuid::now_v7(), &index);
        assert_eq!(slug, "user-prefers-dark-mode");
    }

    #[test]
    fn resolve_slug_with_collision() {
        let mut index = VaultIndex::default();
        let other_id = Uuid::now_v7();
        index.insert(other_id, "user-prefers-dark-mode".to_string());

        let new_id = Uuid::now_v7();
        let slug = resolve_slug("user-prefers-dark-mode", new_id, &index);
        assert_eq!(slug, "user-prefers-dark-mode-2");
    }

    #[test]
    fn resolve_slug_stable_for_existing_id() {
        let mut index = VaultIndex::default();
        let id = Uuid::now_v7();
        index.insert(id, "my-existing-slug".to_string());

        let slug = resolve_slug("completely-different-text", id, &index);
        assert_eq!(slug, "my-existing-slug");
    }

    #[tokio::test]
    async fn index_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let mut index = VaultIndex::default();
        let id = Uuid::now_v7();
        index.insert(id, "test-slug".to_string());
        index.save(tmp.path()).await.unwrap();

        let loaded = VaultIndex::load(tmp.path()).await;
        assert_eq!(loaded.get_slug(&id), Some("test-slug"));
    }

    #[tokio::test]
    async fn sync_fact_creates_file() {
        let tmp = tempfile::tempdir().unwrap();
        let syncer = VaultSyncer::new(tmp.path().to_path_buf(), 0.45)
            .await
            .unwrap();

        let mem = make_memory("User prefers dark mode", "preference");
        syncer.sync_fact_simple(&mem).await.unwrap();

        let path = tmp.path().join("user-prefers-dark-mode.md");
        assert!(path.exists());

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("category: \"preference\""));
        assert!(content.contains("User prefers dark mode"));
        assert!(content.contains("- mem0"));
    }

    #[tokio::test]
    async fn sync_fact_updates_keep_slug() {
        let tmp = tempfile::tempdir().unwrap();
        let syncer = VaultSyncer::new(tmp.path().to_path_buf(), 0.45)
            .await
            .unwrap();

        let mut mem = make_memory("User prefers dark mode", "preference");
        syncer.sync_fact_simple(&mem).await.unwrap();

        // Update the fact text (simulating dedup merge).
        mem.fact = "User strongly prefers dark mode across all tools".to_string();
        mem.access_count = 5;
        syncer.sync_fact_simple(&mem).await.unwrap();

        // Should still use the original slug.
        let path = tmp.path().join("user-prefers-dark-mode.md");
        assert!(path.exists());
        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("strongly prefers"));
        assert!(content.contains("access_count: 5"));
    }

    #[tokio::test]
    async fn delete_fact_removes_file() {
        let tmp = tempfile::tempdir().unwrap();
        let syncer = VaultSyncer::new(tmp.path().to_path_buf(), 0.45)
            .await
            .unwrap();

        let mem = make_memory("Temporary fact", "ops");
        syncer.sync_fact_simple(&mem).await.unwrap();
        assert!(tmp.path().join("temporary-fact.md").exists());

        syncer.delete_fact(&mem.id).await.unwrap();
        assert!(!tmp.path().join("temporary-fact.md").exists());
    }

    #[tokio::test]
    async fn full_sync_with_wikilinks() {
        let tmp = tempfile::tempdir().unwrap();
        let syncer = VaultSyncer::new(tmp.path().to_path_buf(), 0.45)
            .await
            .unwrap();

        let mem_a = make_memory("Rune is written in Rust", "technical");
        let mem_b = make_memory("Project uses Tokio for async", "technical");

        let graph = MemoryGraph {
            nodes: vec![mem_a.clone(), mem_b.clone()],
            edges: vec![MemoryEdge {
                source: mem_a.id,
                target: mem_b.id,
                similarity: 0.72,
            }],
        };

        let report = syncer.full_sync(&graph.nodes, &graph).await.unwrap();
        assert_eq!(report.created, 2);

        // Check wikilinks are present.
        let content_a = tokio::fs::read_to_string(
            tmp.path().join("rune-is-written-in-rust.md"),
        )
        .await
        .unwrap();
        assert!(content_a.contains("[[project-uses-tokio-for-async]]"));
        assert!(content_a.contains("72%"));

        // Check categories index.
        let cats = tokio::fs::read_to_string(tmp.path().join("_categories.md"))
            .await
            .unwrap();
        assert!(cats.contains("## technical"));
    }

    #[tokio::test]
    async fn full_sync_prunes_orphans() {
        let tmp = tempfile::tempdir().unwrap();
        let syncer = VaultSyncer::new(tmp.path().to_path_buf(), 0.45)
            .await
            .unwrap();

        let mem_a = make_memory("Fact A", "ops");
        let mem_b = make_memory("Fact B", "ops");

        let graph = MemoryGraph {
            nodes: vec![mem_a.clone(), mem_b.clone()],
            edges: vec![],
        };
        syncer.full_sync(&graph.nodes, &graph).await.unwrap();
        assert!(tmp.path().join("fact-a.md").exists());
        assert!(tmp.path().join("fact-b.md").exists());

        // Second sync with only mem_a — mem_b should be pruned.
        let graph2 = MemoryGraph {
            nodes: vec![mem_a.clone()],
            edges: vec![],
        };
        let report = syncer.full_sync(&graph2.nodes, &graph2).await.unwrap();
        assert_eq!(report.deleted, 1);
        assert!(tmp.path().join("fact-a.md").exists());
        assert!(!tmp.path().join("fact-b.md").exists());
    }

    #[test]
    fn render_fact_md_valid_frontmatter() {
        let mem = make_memory("Test fact", "technical");
        let md = render_fact_md(&mem, &[("related-slug".to_string(), 0.85)]);

        // Should start and end frontmatter correctly.
        assert!(md.starts_with("---\n"));
        assert!(md.contains("---\n\nTest fact\n"));
        assert!(md.contains("[[related-slug]]"));
        assert!(md.contains("85%"));
    }
}
