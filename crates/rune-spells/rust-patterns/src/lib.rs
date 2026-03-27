use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RustPatternsError {
    #[error("pattern directory not found: {0}")]
    PatternDirMissing(PathBuf),
    #[error("failed to read {path}: {source}")]
    ReadFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse TOML {path}: {source}")]
    ParseToml {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Pattern {
    pub topic: String,
    pub name: String,
    pub when: String,
    pub code: String,
    pub rationale: String,
    pub anti_pattern: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternQuery {
    pub topic: Option<String>,
    pub tags: Option<Vec<String>>,
    pub context_file: Option<PathBuf>,
    pub task_description: Option<String>,
    pub error_message: Option<String>,
    pub max_results: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RustImportContext {
    pub path: PathBuf,
    pub imports: Vec<String>,
}

impl Default for PatternQuery {
    fn default() -> Self {
        Self {
            topic: None,
            tags: None,
            context_file: None,
            task_description: None,
            error_message: None,
            max_results: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub patterns: Vec<Pattern>,
    pub import_context: Option<RustImportContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationFinding {
    pub file: String,
    pub line: usize,
    pub issue: String,
    pub recommendation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ValidationReport {
    pub findings: Vec<ValidationFinding>,
    pub scanned_files: usize,
}

#[derive(Debug, Deserialize)]
struct PatternFile {
    meta: PatternMeta,
    patterns: Vec<PatternEntry>,
}

#[derive(Debug, Deserialize)]
struct PatternMeta {
    topic: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    relevance_signals: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PatternEntry {
    name: String,
    when: String,
    code: String,
    rationale: String,
    anti_pattern: String,
}

#[derive(Debug, Clone)]
struct LoadedTopic {
    topic: String,
    tags: Vec<String>,
    relevance_signals: Vec<String>,
    patterns: Vec<Pattern>,
}

pub fn rust_patterns_tool_definition() -> rune_tools::ToolDefinition {
    rune_tools::ToolDefinition {
        name: "rust_pattern".into(),
        description: "Query the Rust patterns spell by topic, tags, task description, error text, or a Rust source file's imports.".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "topic": { "type": "string", "description": "Pattern topic, e.g. error_handling" },
                "tags": { "type": "array", "items": { "type": "string" }, "description": "Tags to match, e.g. [\"tokio\", \"select\"]" },
                "context_file": { "type": "string", "description": "Rust file path used for import-based pattern detection" },
                "task_description": { "type": "string", "description": "Free-form task text used for relevance matching" },
                "error_message": { "type": "string", "description": "Error text used for debugging-oriented relevance matching" },
                "max_results": { "type": "integer", "description": "Maximum number of patterns to return (default: 3)" }
            }
        }),
        category: rune_core::ToolCategory::FileRead,
        requires_approval: false,
    }
}

pub fn rust_pattern(query: PatternQuery) -> Result<QueryResult, RustPatternsError> {
    let topics = load_pattern_library(default_patterns_dir())?;
    let mut scored = Vec::new();

    let requested_topic = query.topic.as_ref().map(|t| normalize(t));
    let requested_tags: Vec<String> = query
        .tags
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(|tag| normalize(&tag))
        .collect();
    let import_context = query
        .context_file
        .as_ref()
        .and_then(|path| collect_import_signals(path).ok())
        .map(|imports| RustImportContext {
            path: query
                .context_file
                .clone()
                .expect("context_file exists when import context is built"),
            imports,
        });
    let import_signals = import_context
        .as_ref()
        .map(|ctx| ctx.imports.clone())
        .unwrap_or_default();
    let task_terms = tokenize(query.task_description.as_deref().unwrap_or_default());
    let error_terms = tokenize(query.error_message.as_deref().unwrap_or_default());

    for topic in topics {
        let topic_norm = normalize(&topic.topic);
        let topic_tags: Vec<String> = topic.tags.iter().map(|tag| normalize(tag)).collect();
        let topic_signals: Vec<String> = topic
            .relevance_signals
            .iter()
            .map(|signal| normalize(signal))
            .collect();

        let mut score = 0usize;
        if requested_topic
            .as_ref()
            .is_some_and(|wanted| wanted == &topic_norm)
        {
            score += 100;
        }
        for tag in &requested_tags {
            if topic_tags.iter().any(|candidate| candidate == tag)
                || topic_signals
                    .iter()
                    .any(|candidate| candidate.contains(tag) || tag.contains(candidate))
            {
                score += 20;
            }
        }
        for signal in &import_signals {
            if topic_tags.iter().any(|candidate| candidate == signal)
                || topic_signals
                    .iter()
                    .any(|candidate| candidate.contains(signal) || signal.contains(candidate))
            {
                score += 15;
            }
        }
        for term in task_terms.iter().chain(error_terms.iter()) {
            if topic_tags.iter().any(|candidate| candidate == term)
                || topic_signals
                    .iter()
                    .any(|candidate| candidate.contains(term) || term.contains(candidate))
                || topic_norm.contains(term)
            {
                score += 5;
            }
        }

        if score == 0
            && requested_topic.is_none()
            && requested_tags.is_empty()
            && import_signals.is_empty()
            && task_terms.is_empty()
            && error_terms.is_empty()
        {
            score = 1;
        }

        for pattern in topic.patterns {
            if score > 0 {
                scored.push((score, pattern));
            }
        }
    }

    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.topic.cmp(&b.1.topic))
            .then_with(|| a.1.name.cmp(&b.1.name))
    });
    scored.dedup_by(|a, b| a.1.topic == b.1.topic && a.1.name == b.1.name);

    Ok(QueryResult {
        patterns: scored
            .into_iter()
            .take(query.max_results.max(1))
            .map(|(_, pattern)| pattern)
            .collect(),
        import_context,
    })
}

pub fn validate_rune_codebase(root: &Path) -> ValidationReport {
    let mut findings = Vec::new();
    let mut scanned_files = 0;
    collect_validation_findings(root, &mut findings, &mut scanned_files);
    ValidationReport {
        findings,
        scanned_files,
    }
}

fn collect_validation_findings(
    root: &Path,
    findings: &mut Vec<ValidationFinding>,
    scanned_files: &mut usize,
) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let skip = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| matches!(name, "target" | ".git" | "node_modules"));
            if !skip {
                collect_validation_findings(&path, findings, scanned_files);
            }
            continue;
        }

        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }

        if path
            .components()
            .any(|component| component.as_os_str() == "tests")
        {
            continue;
        }

        if let Ok(content) = fs::read_to_string(&path) {
            *scanned_files += 1;
            let async_and_blocking =
                content.contains("async fn") && content.contains("std::thread::sleep");
            for (idx, line) in content.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.contains(".unwrap()") {
                    findings.push(ValidationFinding {
                        file: path.display().to_string(),
                        line: idx + 1,
                        issue: "unwrap() in non-test Rust code".into(),
                        recommendation: "Prefer ? or a typed error path from the rust-patterns anti-pattern library.".into(),
                    });
                }
                if async_and_blocking && trimmed.starts_with("async fn") {
                    findings.push(ValidationFinding {
                        file: path.display().to_string(),
                        line: idx + 1,
                        issue: "blocking std::thread::sleep inside async context".into(),
                        recommendation:
                            "Use tokio::time::sleep or spawn_blocking depending on the workload."
                                .into(),
                    });
                }
            }
        }
    }
}

fn load_pattern_library(dir: PathBuf) -> Result<Vec<LoadedTopic>, RustPatternsError> {
    if !dir.exists() {
        return Err(RustPatternsError::PatternDirMissing(dir));
    }

    let mut topics = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|source| RustPatternsError::ReadFile {
        path: dir.clone(),
        source,
    })? {
        let entry = entry.map_err(|source| RustPatternsError::ReadFile {
            path: dir.clone(),
            source,
        })?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
            continue;
        }

        let raw = fs::read_to_string(&path).map_err(|source| RustPatternsError::ReadFile {
            path: path.clone(),
            source,
        })?;
        let parsed: PatternFile =
            toml::from_str(&raw).map_err(|source| RustPatternsError::ParseToml {
                path: path.clone(),
                source,
            })?;

        let patterns = parsed
            .patterns
            .into_iter()
            .map(|pattern| Pattern {
                topic: parsed.meta.topic.clone(),
                name: pattern.name,
                when: pattern.when,
                code: pattern.code,
                rationale: pattern.rationale,
                anti_pattern: pattern.anti_pattern,
                tags: parsed.meta.tags.clone(),
            })
            .collect();

        topics.push(LoadedTopic {
            topic: parsed.meta.topic,
            tags: parsed.meta.tags,
            relevance_signals: parsed.meta.relevance_signals,
            patterns,
        });
    }

    topics.sort_by(|a, b| a.topic.cmp(&b.topic));
    Ok(topics)
}

fn default_patterns_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("patterns")
}

fn collect_import_signals(path: &Path) -> Result<Vec<String>, RustPatternsError> {
    let raw = fs::read_to_string(path).map_err(|source| RustPatternsError::ReadFile {
        path: path.to_path_buf(),
        source,
    })?;
    let mut signals = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("use ") {
            let crate_name = rest
                .split([':', ';', ' '])
                .next()
                .unwrap_or_default()
                .trim();
            if !crate_name.is_empty() {
                signals.push(normalize(crate_name));
            }
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("extern crate ") {
            let crate_name = rest
                .split([';', ' '])
                .next()
                .unwrap_or_default()
                .trim();
            if !crate_name.is_empty() {
                signals.push(normalize(crate_name));
            }
            continue;
        }
    }
    Ok(signals)
}

fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|part| !part.is_empty())
        .map(normalize)
        .collect()
}

fn normalize(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('-', "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_definition_name_is_stable() {
        let def = rust_patterns_tool_definition();
        assert_eq!(def.name, "rust_pattern");
    }

    #[test]
    fn loads_all_seed_topics() {
        let topics =
            load_pattern_library(default_patterns_dir()).expect("seed patterns should load");
        assert!(topics.iter().any(|topic| topic.topic == "ownership"));
        assert!(topics.iter().any(|topic| topic.topic == "error_handling"));
        assert!(topics.iter().any(|topic| topic.topic == "async_tokio"));
        assert!(topics.iter().any(|topic| topic.topic == "axum_web"));
        assert!(topics.iter().any(|topic| topic.topic == "concurrency"));
        assert!(topics.iter().any(|topic| topic.topic == "database"));
        assert!(topics.iter().any(|topic| topic.topic == "cli_clap"));
        assert!(topics.iter().any(|topic| topic.topic == "wasm"));
        assert!(topics.iter().any(|topic| topic.topic == "pyo3"));
        assert!(topics.iter().any(|topic| topic.topic == "anti_patterns"));
    }

    #[test]
    fn every_seed_topic_has_at_least_one_pattern() {
        let topics =
            load_pattern_library(default_patterns_dir()).expect("seed patterns should load");
        assert!(topics.iter().all(|topic| !topic.patterns.is_empty()));
    }

    #[test]
    fn query_by_topic_returns_matching_patterns() {
        let result = rust_pattern(PatternQuery {
            topic: Some("error_handling".into()),
            ..Default::default()
        })
        .expect("query should work");

        assert!(!result.patterns.is_empty());
        assert!(
            result
                .patterns
                .iter()
                .all(|pattern| pattern.topic == "error_handling")
        );
    }

    #[test]
    fn query_by_tags_and_task_description_prefers_tokio() {
        let result = rust_pattern(PatternQuery {
            tags: Some(vec!["tokio".into(), "select".into()]),
            task_description: Some("Need an async timeout with tokio select".into()),
            ..Default::default()
        })
        .expect("query should work");

        assert!(!result.patterns.is_empty());
        assert_eq!(result.patterns[0].topic, "async_tokio");
    }

    #[test]
    fn query_by_imports_prefers_axum_patterns() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("handler.rs");
        fs::write(
            &file,
            "use axum::Json;\nuse axum::extract::State;\nasync fn handler() {}\n",
        )
        .expect("write file");

        let result = rust_pattern(PatternQuery {
            context_file: Some(file),
            ..Default::default()
        })
        .expect("query should work");

        assert!(!result.patterns.is_empty());
        assert_eq!(result.patterns[0].topic, "axum_web");
    }


    #[test]
    fn query_by_extern_crate_imports_prefers_pyo3_patterns() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("bindings.rs");
        fs::write(
            &file,
            "extern crate pyo3;\nuse pyo3::prelude::*;\n#[pyfunction] fn demo() {}\n",
        )
        .expect("write file");

        let result = rust_pattern(PatternQuery {
            context_file: Some(file.clone()),
            ..Default::default()
        })
        .expect("query should work");

        assert!(!result.patterns.is_empty());
        assert_eq!(result.patterns[0].topic, "pyo3");
        let import_context = result.import_context.expect("import context");
        assert!(import_context.imports.contains(&"pyo3".to_string()));
    }

    #[test]
    fn query_returns_import_context_when_context_file_is_used() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("handler.rs");
        fs::write(
            &file,
            "use tokio::time::sleep;
use axum::Json;
",
        )
        .expect("write file");

        let result = rust_pattern(PatternQuery {
            context_file: Some(file.clone()),
            ..Default::default()
        })
        .expect("query should work");

        let import_context = result.import_context.expect("import context");
        assert_eq!(import_context.path, file);
        assert!(import_context.imports.contains(&"tokio".to_string()));
        assert!(import_context.imports.contains(&"axum".to_string()));
    }

    #[test]
    fn validation_flags_unwrap_in_non_test_code() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("sample.rs");
        fs::write(&file, "fn demo() { let _ = Some(1).unwrap(); }\n").expect("write file");

        let report = validate_rune_codebase(tmp.path());
        assert_eq!(report.scanned_files, 1);
        assert_eq!(report.findings.len(), 1);
        assert!(report.findings[0].issue.contains("unwrap"));
    }
}
