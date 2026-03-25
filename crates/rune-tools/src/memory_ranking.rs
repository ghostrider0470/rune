//! Ranking and re-ranking utilities for memory search results.
//!
//! Provides:
//! - **Temporal decay**: older memories score lower (configurable half-life).
//! - **MMR diversity**: Maximal Marginal Relevance re-ranking to reduce redundancy.
//! - **Query expansion**: converts conversational queries into keyword-friendly search terms.

use chrono::{NaiveDate, Utc};
use std::collections::HashSet;
use tracing::debug;

// ---------------------------------------------------------------------------
// Core data type
// ---------------------------------------------------------------------------

/// A memory search hit carrying a relevance score.
#[derive(Clone, Debug)]
pub struct MemoryHit {
    /// Relative file path within the workspace (e.g. `memory/2026-03-13.md`).
    pub file_path: String,
    /// The matched text content.
    pub chunk_text: String,
    /// Relevance score (higher is better). Modified in-place by ranking stages.
    pub score: f64,
}

// ---------------------------------------------------------------------------
// I2: Temporal decay
// ---------------------------------------------------------------------------

/// Default half-life in days for temporal decay.
pub const DEFAULT_HALF_LIFE_DAYS: f64 = 30.0;

/// Apply temporal decay to memory search results.
///
/// - Extracts date from filename (`memory/YYYY-MM-DD.md`) or treats as evergreen.
/// - `MEMORY.md` is evergreen (no decay).
/// - Daily notes decay with configurable half-life.
/// - Score multiplier = `0.5^(age_days / half_life_days)`.
pub fn apply_temporal_decay(results: &mut [MemoryHit], half_life_days: f64) {
    let today = Utc::now().date_naive();
    for hit in results.iter_mut() {
        if let Some(decay) = compute_decay_factor(&hit.file_path, today, half_life_days) {
            hit.score *= decay;
        }
        // else: evergreen file, no decay applied
    }
}

/// Compute the decay factor for a given file path.
///
/// Returns `None` for evergreen files (MEMORY.md, files without a date in the name).
/// Returns `Some(factor)` in `(0.0, 1.0]` for dated files.
fn compute_decay_factor(file_path: &str, today: NaiveDate, half_life_days: f64) -> Option<f64> {
    // MEMORY.md is always evergreen
    let filename = file_path
        .rsplit('/')
        .next()
        .or_else(|| file_path.rsplit('\\').next())
        .unwrap_or(file_path);

    if filename.eq_ignore_ascii_case("memory.md") {
        return None;
    }

    // Try to extract YYYY-MM-DD from the filename
    let date = extract_date_from_filename(filename)?;
    let age_days = (today - date).num_days().max(0) as f64;
    let factor = 0.5_f64.powf(age_days / half_life_days);
    Some(factor)
}

/// Extract a `NaiveDate` from a filename like `2026-03-13.md`.
fn extract_date_from_filename(filename: &str) -> Option<NaiveDate> {
    // Strip extension
    let stem = filename.strip_suffix(".md").unwrap_or(filename);
    NaiveDate::parse_from_str(stem, "%Y-%m-%d").ok()
}

// ---------------------------------------------------------------------------
// I3: MMR diversity
// ---------------------------------------------------------------------------

/// Default lambda for MMR: balances relevance vs. diversity.
pub const DEFAULT_MMR_LAMBDA: f64 = 0.7;

/// Re-rank results using Maximal Marginal Relevance.
///
/// After initial scoring, iteratively selects results that maximize:
///   `lambda * relevance - (1 - lambda) * max_similarity_to_already_selected`
///
/// Uses Jaccard similarity on word tokens for similarity detection.
pub fn mmr_rerank(results: Vec<MemoryHit>, lambda: f64, top_k: usize) -> Vec<MemoryHit> {
    if results.is_empty() || top_k == 0 {
        return Vec::new();
    }

    let top_k = top_k.min(results.len());

    // Pre-tokenize all results
    let tokenized: Vec<HashSet<String>> = results
        .iter()
        .map(|hit| tokenize(&hit.chunk_text))
        .collect();

    // Normalize scores to [0, 1] for fair comparison with similarity
    let max_score = results
        .iter()
        .map(|h| h.score)
        .fold(f64::NEG_INFINITY, f64::max);
    let min_score = results
        .iter()
        .map(|h| h.score)
        .fold(f64::INFINITY, f64::min);
    let score_range = max_score - min_score;

    let normalized_scores: Vec<f64> = if score_range > f64::EPSILON {
        results
            .iter()
            .map(|h| (h.score - min_score) / score_range)
            .collect()
    } else {
        vec![1.0; results.len()]
    };

    let mut selected: Vec<usize> = Vec::with_capacity(top_k);
    let mut remaining: Vec<usize> = (0..results.len()).collect();

    for _ in 0..top_k {
        let mut best_idx = None;
        let mut best_mmr = f64::NEG_INFINITY;

        for &candidate in &remaining {
            let relevance = normalized_scores[candidate];

            let max_sim = selected
                .iter()
                .map(|&sel| jaccard_similarity(&tokenized[candidate], &tokenized[sel]))
                .fold(0.0_f64, f64::max);

            let mmr_score = lambda * relevance - (1.0 - lambda) * max_sim;

            if mmr_score > best_mmr {
                best_mmr = mmr_score;
                best_idx = Some(candidate);
            }
        }

        match best_idx {
            Some(idx) => {
                selected.push(idx);
                remaining.retain(|&i| i != idx);
            }
            None => break,
        }
    }

    debug!(
        selected_count = selected.len(),
        total = results.len(),
        "MMR re-ranking complete"
    );

    // Collect selected results in order, preserving their original (decayed) scores
    selected.into_iter().map(|i| results[i].clone()).collect()
}

/// Tokenize text into a set of lowercase words.
fn tokenize(text: &str) -> HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

/// Jaccard similarity between two token sets: |A ∩ B| / |A ∪ B|.
fn jaccard_similarity(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let intersection = a.intersection(b).count() as f64;
    let union = a.union(b).count() as f64;
    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

// ---------------------------------------------------------------------------
// I4: Query expansion
// ---------------------------------------------------------------------------

/// Common English stop words to filter from conversational queries.
const STOP_WORDS: &[&str] = &[
    "a", "about", "an", "and", "any", "are", "as", "at", "be", "been", "but", "by", "can", "could",
    "did", "do", "does", "for", "from", "had", "has", "have", "he", "her", "him", "his", "how",
    "i", "if", "in", "into", "is", "it", "its", "just", "me", "might", "my", "no", "not", "of",
    "on", "or", "our", "out", "own", "re", "say", "she", "should", "so", "some", "than", "that",
    "the", "their", "them", "then", "there", "these", "they", "this", "those", "to", "too", "up",
    "us", "very", "was", "we", "were", "what", "when", "where", "which", "while", "who", "whom",
    "why", "will", "with", "would", "you", "your",
];

/// Expand a conversational query into search-friendly keywords.
///
/// - Lowercases and splits on whitespace/punctuation
/// - Filters common stop words
/// - Keeps meaningful terms
/// - Returns expanded query string for keyword search
pub fn expand_query(query: &str) -> String {
    let stop_set: HashSet<&str> = STOP_WORDS.iter().copied().collect();

    let keywords: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .filter(|w| w.len() > 1 && !stop_set.contains(w.as_str()))
        .collect();

    if keywords.is_empty() {
        // Fallback: return original query lowercased if all words were stop words
        query.to_lowercase()
    } else {
        keywords.join(" ")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Temporal decay tests --

    #[test]
    fn decay_memory_md_is_evergreen() {
        let today = NaiveDate::from_ymd_opt(2026, 3, 22).unwrap();
        assert!(compute_decay_factor("MEMORY.md", today, 30.0).is_none());
        assert!(compute_decay_factor("memory/MEMORY.md", today, 30.0).is_none());
    }

    #[test]
    fn decay_today_file_has_no_decay() {
        let today = NaiveDate::from_ymd_opt(2026, 3, 22).unwrap();
        let factor = compute_decay_factor("memory/2026-03-22.md", today, 30.0).unwrap();
        assert!((factor - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn decay_30_day_old_file_is_half() {
        let today = NaiveDate::from_ymd_opt(2026, 3, 22).unwrap();
        let factor = compute_decay_factor("memory/2026-02-20.md", today, 30.0).unwrap();
        assert!((factor - 0.5).abs() < 0.01);
    }

    #[test]
    fn decay_60_day_old_file_is_quarter() {
        let today = NaiveDate::from_ymd_opt(2026, 3, 22).unwrap();
        let factor = compute_decay_factor("memory/2026-01-21.md", today, 30.0).unwrap();
        assert!((factor - 0.25).abs() < 0.01);
    }

    #[test]
    fn decay_non_dated_file_is_evergreen() {
        let today = NaiveDate::from_ymd_opt(2026, 3, 22).unwrap();
        assert!(compute_decay_factor("memory/project-notes.md", today, 30.0).is_none());
    }

    #[test]
    fn apply_temporal_decay_modifies_scores() {
        let mut hits = vec![
            MemoryHit {
                file_path: "MEMORY.md".into(),
                chunk_text: "evergreen".into(),
                score: 1.0,
            },
            MemoryHit {
                file_path: "memory/2026-03-22.md".into(),
                chunk_text: "today".into(),
                score: 1.0,
            },
            MemoryHit {
                file_path: "memory/2026-02-20.md".into(),
                chunk_text: "old".into(),
                score: 1.0,
            },
        ];

        apply_temporal_decay(&mut hits, 30.0);

        // MEMORY.md: no decay
        assert!((hits[0].score - 1.0).abs() < f64::EPSILON);
        // Today: no decay (age 0 → factor 1.0), but depends on actual date
        // Old: should be decayed
        assert!(hits[2].score < 1.0);
    }

    // -- MMR tests --

    #[test]
    fn mmr_empty_input() {
        let result = mmr_rerank(vec![], 0.7, 5);
        assert!(result.is_empty());
    }

    #[test]
    fn mmr_returns_top_k() {
        let hits: Vec<MemoryHit> = (0..10)
            .map(|i| MemoryHit {
                file_path: format!("memory/{i}.md"),
                chunk_text: format!("unique content {i}"),
                score: 10.0 - i as f64,
            })
            .collect();

        let result = mmr_rerank(hits, 0.7, 3);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn mmr_prefers_diversity_over_duplicates() {
        let hits = vec![
            MemoryHit {
                file_path: "a.md".into(),
                chunk_text: "rust memory system design".into(),
                score: 1.0,
            },
            MemoryHit {
                file_path: "b.md".into(),
                chunk_text: "rust memory system design pattern".into(),
                score: 0.95,
            },
            MemoryHit {
                file_path: "c.md".into(),
                chunk_text: "python web framework comparison".into(),
                score: 0.9,
            },
        ];

        // Lambda=0.3 strongly favors diversity. The near-duplicate b.md
        // gets penalized heavily for its high Jaccard similarity to a.md,
        // while the diverse c.md has zero similarity and wins.
        let result = mmr_rerank(hits, 0.3, 2);
        let paths: Vec<&str> = result.iter().map(|h| h.file_path.as_str()).collect();
        assert!(paths.contains(&"a.md"));
        assert!(paths.contains(&"c.md"));
    }

    #[test]
    fn mmr_high_lambda_favors_relevance() {
        let hits = vec![
            MemoryHit {
                file_path: "a.md".into(),
                chunk_text: "rust memory system".into(),
                score: 1.0,
            },
            MemoryHit {
                file_path: "b.md".into(),
                chunk_text: "rust memory system design".into(),
                score: 0.99,
            },
            MemoryHit {
                file_path: "c.md".into(),
                chunk_text: "unrelated topic completely different".into(),
                score: 0.1,
            },
        ];

        let result = mmr_rerank(hits, 1.0, 2);
        // Lambda=1.0 means pure relevance, no diversity penalty
        assert_eq!(result[0].file_path, "a.md");
        assert_eq!(result[1].file_path, "b.md");
    }

    // -- Jaccard tests --

    #[test]
    fn jaccard_identical_sets() {
        let a: HashSet<String> = ["hello", "world"].iter().map(|s| s.to_string()).collect();
        assert!((jaccard_similarity(&a, &a) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn jaccard_disjoint_sets() {
        let a: HashSet<String> = ["hello"].iter().map(|s| s.to_string()).collect();
        let b: HashSet<String> = ["world"].iter().map(|s| s.to_string()).collect();
        assert!((jaccard_similarity(&a, &b)).abs() < f64::EPSILON);
    }

    #[test]
    fn jaccard_empty_sets() {
        let a: HashSet<String> = HashSet::new();
        let b: HashSet<String> = HashSet::new();
        assert!((jaccard_similarity(&a, &b) - 1.0).abs() < f64::EPSILON);
    }

    // -- Query expansion tests --

    #[test]
    fn expand_removes_stop_words() {
        let expanded = expand_query("what did we decide about the database?");
        assert!(!expanded.contains("what"));
        assert!(!expanded.contains("did"));
        assert!(!expanded.contains("the"));
        assert!(expanded.contains("decide"));
        assert!(expanded.contains("database"));
    }

    #[test]
    fn expand_preserves_meaningful_terms() {
        let expanded = expand_query("memory search temporal decay");
        assert!(expanded.contains("memory"));
        assert!(expanded.contains("search"));
        assert!(expanded.contains("temporal"));
        assert!(expanded.contains("decay"));
    }

    #[test]
    fn expand_lowercases() {
        let expanded = expand_query("PostgreSQL Migration");
        assert!(expanded.contains("postgresql"));
        assert!(expanded.contains("migration"));
    }

    #[test]
    fn expand_all_stop_words_returns_original() {
        let expanded = expand_query("is it a");
        // All words are stop words or too short, should return original lowercased
        assert_eq!(expanded, "is it a");
    }

    #[test]
    fn expand_handles_punctuation() {
        let expanded = expand_query("what's the auth-middleware status?");
        assert!(expanded.contains("auth"));
        assert!(expanded.contains("middleware"));
        assert!(expanded.contains("status"));
    }
}
