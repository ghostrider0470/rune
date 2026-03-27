use std::path::{Path, PathBuf};

use async_trait::async_trait;
use rune_core::ToolCategory;
use rune_tools::{ToolCall, ToolDefinition, ToolError, ToolExecutor, ToolResult};
use serde::Deserialize;

use crate::{PatternQuery, rust_pattern, validate_rune_codebase};

pub struct RustPatternsToolExecutor {
    workspace_root: PathBuf,
}

impl RustPatternsToolExecutor {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
        }
    }

    fn resolve_path(&self, tool: &str, raw: &str) -> Result<PathBuf, ToolError> {
        let candidate = Path::new(raw);
        if candidate.is_absolute() {
            return Err(ToolError::InvalidArguments {
                tool: tool.to_string(),
                reason: "absolute paths are not allowed".into(),
            });
        }

        let workspace_root = self
            .workspace_root
            .canonicalize()
            .map_err(|e| ToolError::ExecutionFailed(format!("workspace root invalid: {e}")))?;
        let joined = workspace_root.join(candidate);
        let canonical = if joined.exists() {
            joined
                .canonicalize()
                .map_err(|e| ToolError::ExecutionFailed(format!("path resolution failed: {e}")))?
        } else {
            return Err(ToolError::InvalidArguments {
                tool: tool.to_string(),
                reason: "path does not exist".into(),
            });
        };
        if !canonical.starts_with(&workspace_root) {
            return Err(ToolError::InvalidArguments {
                tool: tool.to_string(),
                reason: "path escapes workspace boundary".into(),
            });
        }
        Ok(canonical)
    }

    fn optional_path(&self, call: &ToolCall, key: &str) -> Result<Option<PathBuf>, ToolError> {
        call.arguments
            .get(key)
            .and_then(|value| value.as_str())
            .map(|raw| self.resolve_path(&call.tool_name, raw))
            .transpose()
    }

    fn parse_query(&self, call: &ToolCall) -> Result<PatternQuery, ToolError> {
        #[derive(Deserialize)]
        struct RawQuery {
            topic: Option<String>,
            tags: Option<Vec<String>>,
            context_file: Option<String>,
            task_description: Option<String>,
            error_message: Option<String>,
            patterns_dir: Option<String>,
            max_results: Option<usize>,
        }

        let raw: RawQuery = serde_json::from_value(call.arguments.clone()).map_err(|e| {
            ToolError::InvalidArguments {
                tool: call.tool_name.clone(),
                reason: format!("invalid rust_pattern arguments: {e}"),
            }
        })?;

        let context_file = raw
            .context_file
            .as_deref()
            .map(|value| self.resolve_path(&call.tool_name, value))
            .transpose()?;
        let patterns_dir = raw
            .patterns_dir
            .as_deref()
            .map(|value| self.resolve_path(&call.tool_name, value))
            .transpose()?;

        Ok(PatternQuery {
            topic: raw.topic,
            tags: raw.tags,
            context_file,
            task_description: raw.task_description,
            error_message: raw.error_message,
            patterns_dir,
            max_results: raw.max_results.unwrap_or(3),
        })
    }
}

#[async_trait]
impl ToolExecutor for RustPatternsToolExecutor {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        match call.tool_name.as_str() {
            "rust_pattern" => {
                let query = self.parse_query(&call)?;
                let result =
                    rust_pattern(query).map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
                let output = serde_json::to_string_pretty(&result).map_err(|e| {
                    ToolError::ExecutionFailed(format!(
                        "failed to serialize rust_pattern result: {e}"
                    ))
                })?;
                Ok(ToolResult {
                    tool_call_id: call.tool_call_id,
                    output,
                    is_error: false,
                    tool_execution_id: None,
                })
            }
            "rust_pattern_validate" => {
                let root = self
                    .optional_path(&call, "path")?
                    .unwrap_or_else(|| self.workspace_root.clone());
                let report = validate_rune_codebase(&root);
                let output = serde_json::to_string_pretty(&report).map_err(|e| {
                    ToolError::ExecutionFailed(format!(
                        "failed to serialize rust_pattern_validate result: {e}"
                    ))
                })?;
                Ok(ToolResult {
                    tool_call_id: call.tool_call_id,
                    output,
                    is_error: false,
                    tool_execution_id: None,
                })
            }
            other => Err(ToolError::UnknownTool {
                name: other.to_string(),
            }),
        }
    }
}

pub fn rust_patterns_validate_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: "rust_pattern_validate".into(),
        description: "Scan Rust files for common anti-patterns from the rust-patterns spell, including unwrap() in non-test code and blocking sleep in async contexts.".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Optional workspace-relative directory or Rust file path to validate; defaults to the workspace root"
                }
            }
        }),
        category: ToolCategory::FileRead,
        requires_approval: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune_core::ToolCallId;
    use tempfile::tempdir;

    fn tool_call(name: &str, arguments: serde_json::Value) -> ToolCall {
        ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: name.to_string(),
            arguments,
        }
    }

    #[tokio::test]
    async fn validate_tool_returns_report_for_workspace_relative_path() {
        let tmp = tempdir().expect("tempdir");
        let file = tmp.path().join("sample.rs");
        std::fs::write(&file, "fn demo() { let _ = Some(1).unwrap(); }\n").expect("write");

        let executor = RustPatternsToolExecutor::new(tmp.path());
        let result = executor
            .execute(tool_call(
                "rust_pattern_validate",
                serde_json::json!({ "path": "." }),
            ))
            .await
            .expect("tool execution should succeed");

        assert!(!result.is_error);
        assert!(result.output.contains("unwrap"));
    }

    #[tokio::test]
    async fn validate_tool_rejects_absolute_paths() {
        let tmp = tempdir().expect("tempdir");
        let executor = RustPatternsToolExecutor::new(tmp.path());
        let absolute = tmp.path().to_string_lossy().to_string();

        let error = executor
            .execute(tool_call(
                "rust_pattern_validate",
                serde_json::json!({ "path": absolute }),
            ))
            .await
            .expect_err("absolute paths should be rejected");

        match error {
            ToolError::InvalidArguments { tool, .. } => {
                assert_eq!(tool, "rust_pattern_validate");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
