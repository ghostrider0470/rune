//! Real implementations of file-system tools (read, write, edit, list).

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tracing::instrument;

use crate::definition::{ToolCall, ToolResult};
use crate::error::ToolError;
use crate::executor::ToolExecutor;

/// Executor for file-system tools rooted at a workspace directory.
///
/// All tool paths are resolved relative to `workspace_root`. Absolute paths and
/// traversal outside the workspace boundary are rejected.
pub struct FileToolExecutor {
    workspace_root: PathBuf,
}

impl FileToolExecutor {
    /// Create a new executor for the provided workspace root.
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
        }
    }

    fn invalid_arguments(&self, tool: &str, reason: impl Into<String>) -> ToolError {
        ToolError::InvalidArguments {
            tool: tool.to_string(),
            reason: reason.into(),
        }
    }

    fn execution_failed(message: impl Into<String>) -> ToolError {
        ToolError::ExecutionFailed(message.into())
    }

    /// Resolve a workspace-relative path and ensure it remains inside the workspace.
    fn resolve_path(&self, tool: &str, raw: &str) -> Result<PathBuf, ToolError> {
        let candidate = Path::new(raw);

        if candidate.is_absolute() {
            return Err(self.invalid_arguments(
                tool,
                "absolute paths are not allowed; use workspace-relative paths",
            ));
        }

        let workspace_root = self
            .workspace_root
            .canonicalize()
            .map_err(|e| Self::execution_failed(format!("workspace root invalid: {e}")))?;

        let joined = workspace_root.join(candidate);

        let canonical = if joined.exists() {
            joined
                .canonicalize()
                .map_err(|e| Self::execution_failed(format!("path resolution failed: {e}")))?
        } else {
            let parent = joined
                .parent()
                .ok_or_else(|| self.invalid_arguments(tool, "cannot determine parent directory"))?;

            let canonical_parent = parent.canonicalize().map_err(|e| {
                Self::execution_failed(format!("parent path resolution failed: {e}"))
            })?;

            let file_name = joined.file_name().ok_or_else(|| {
                self.invalid_arguments(tool, "cannot determine file name for target path")
            })?;

            canonical_parent.join(file_name)
        };

        if !canonical.starts_with(&workspace_root) {
            return Err(self.invalid_arguments(tool, "path escapes workspace boundary"));
        }

        Ok(canonical)
    }

    fn required_str<'a>(call: &'a ToolCall, key: &str) -> Result<&'a str, ToolError> {
        call.arguments
            .get(key)
            .and_then(|value| value.as_str())
            .ok_or_else(|| ToolError::InvalidArguments {
                tool: call.tool_name.clone(),
                reason: format!("missing required parameter: {key}"),
            })
    }

    fn required_str_any<'a>(call: &'a ToolCall, keys: &[&str]) -> Result<&'a str, ToolError> {
        keys.iter()
            .find_map(|key| call.arguments.get(*key).and_then(|value| value.as_str()))
            .ok_or_else(|| ToolError::InvalidArguments {
                tool: call.tool_name.clone(),
                reason: format!("missing required parameter: one of {}", keys.join(", ")),
            })
    }

    fn optional_u64_any(call: &ToolCall, keys: &[&str]) -> Option<u64> {
        keys.iter()
            .find_map(|key| call.arguments.get(*key).and_then(|value| value.as_u64()))
    }

    #[instrument(skip(self, call), fields(tool = %call.tool_name))]
    async fn read_file(&self, call: &ToolCall) -> Result<ToolResult, ToolError> {
        let path_str = Self::required_str_any(call, &["path", "file_path"])?;
        let path = self.resolve_path(&call.tool_name, path_str)?;

        let content = tokio::fs::read_to_string(&path).await.map_err(|e| {
            Self::execution_failed(format!("failed to read {}: {e}", path.display()))
        })?;

        let offset = Self::optional_u64_any(call, &["offset", "from"]).unwrap_or(1) as usize;
        let limit = Self::optional_u64_any(call, &["limit", "lines"]).map(|v| v as usize);

        let lines: Vec<&str> = content.lines().collect();
        let start = offset.saturating_sub(1).min(lines.len());
        let end = limit.map_or(lines.len(), |value| (start + value).min(lines.len()));
        let output = lines[start..end].join("\n");

        Ok(ToolResult {
            tool_call_id: call.tool_call_id,
            output: truncate_read_output(&output),
            is_error: false,
        })
    }

    #[instrument(skip(self, call), fields(tool = %call.tool_name))]
    async fn write_file(&self, call: &ToolCall) -> Result<ToolResult, ToolError> {
        let path_str = Self::required_str_any(call, &["path", "file_path"])?;
        let content = Self::required_str(call, "content")?;
        let path = self.resolve_path(&call.tool_name, path_str)?;

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                Self::execution_failed(format!(
                    "failed to create directories for {}: {e}",
                    path.display()
                ))
            })?;
        }

        tokio::fs::write(&path, content).await.map_err(|e| {
            Self::execution_failed(format!("failed to write {}: {e}", path.display()))
        })?;

        Ok(ToolResult {
            tool_call_id: call.tool_call_id,
            output: format!("wrote {} bytes to {}", content.len(), path_str),
            is_error: false,
        })
    }

    #[instrument(skip(self, call), fields(tool = %call.tool_name))]
    async fn edit_file(&self, call: &ToolCall) -> Result<ToolResult, ToolError> {
        let path_str = Self::required_str_any(call, &["path", "file_path"])?;
        let old_string = Self::required_str_any(call, &["old_string", "oldText", "old_text"])?;
        let new_string = Self::required_str_any(call, &["new_string", "newText", "new_text"])?;
        let path = self.resolve_path(&call.tool_name, path_str)?;

        let content = tokio::fs::read_to_string(&path).await.map_err(|e| {
            Self::execution_failed(format!("failed to read {}: {e}", path.display()))
        })?;

        if !content.contains(old_string) {
            return Ok(ToolResult {
                tool_call_id: call.tool_call_id,
                output: "old_string not found in file — no changes made".into(),
                is_error: true,
            });
        }

        let match_count = content.matches(old_string).count();
        if match_count != 1 {
            return Ok(ToolResult {
                tool_call_id: call.tool_call_id,
                output: format!(
                    "old_string matched {match_count} times — must match exactly once for safe edit"
                ),
                is_error: true,
            });
        }

        let updated = content.replacen(old_string, new_string, 1);
        tokio::fs::write(&path, updated).await.map_err(|e| {
            Self::execution_failed(format!("failed to write {}: {e}", path.display()))
        })?;

        Ok(ToolResult {
            tool_call_id: call.tool_call_id,
            output: format!("edited {path_str}: replaced 1 occurrence"),
            is_error: false,
        })
    }

    #[instrument(skip(self, call), fields(tool = %call.tool_name))]
    async fn list_files(&self, call: &ToolCall) -> Result<ToolResult, ToolError> {
        let path_str = call
            .arguments
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or(".");
        let path = self.resolve_path(&call.tool_name, path_str)?;

        let mut entries = tokio::fs::read_dir(&path).await.map_err(|e| {
            Self::execution_failed(format!("failed to list {}: {e}", path.display()))
        })?;

        let mut names = Vec::new();
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| Self::execution_failed(format!("failed to read directory entry: {e}")))?
        {
            let name = entry.file_name().to_string_lossy().to_string();
            let file_type = entry.file_type().await.map_err(|e| {
                Self::execution_failed(format!("failed to read file type for {name}: {e}"))
            })?;
            let suffix = if file_type.is_dir() { "/" } else { "" };
            names.push(format!("{name}{suffix}"));
        }

        names.sort();

        Ok(ToolResult {
            tool_call_id: call.tool_call_id,
            output: names.join("\n"),
            is_error: false,
        })
    }
}

fn truncate_read_output(content: &str) -> String {
    const MAX_BYTES: usize = 50_000;
    const MAX_LINES: usize = 2_000;

    let mut truncated = content
        .lines()
        .take(MAX_LINES)
        .collect::<Vec<_>>()
        .join("\n");

    if truncated.len() > MAX_BYTES {
        truncated.truncate(MAX_BYTES);
    }

    truncated
}

#[async_trait]
impl ToolExecutor for FileToolExecutor {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        match call.tool_name.as_str() {
            "read_file" | "read" => self.read_file(&call).await,
            "write_file" | "write" => self.write_file(&call).await,
            "edit_file" | "edit" => self.edit_file(&call).await,
            "list_files" => self.list_files(&call).await,
            other => Err(ToolError::UnknownTool {
                name: other.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use rune_core::ToolCallId;
    use tempfile::TempDir;

    use super::*;

    fn make_call(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: name.into(),
            arguments: args,
        }
    }

    #[tokio::test]
    async fn read_write_roundtrip() {
        let tmp = TempDir::new().expect("tempdir");
        let exec = FileToolExecutor::new(tmp.path());

        let write_call = make_call(
            "write_file",
            serde_json::json!({"path": "test.txt", "content": "hello world"}),
        );
        let result = exec.execute(write_call).await.expect("write succeeds");
        assert!(!result.is_error);

        let read_call = make_call("read_file", serde_json::json!({"path": "test.txt"}));
        let result = exec.execute(read_call).await.expect("read succeeds");
        assert_eq!(result.output, "hello world");
    }

    #[tokio::test]
    async fn read_with_offset_and_limit() {
        let tmp = TempDir::new().expect("tempdir");
        let exec = FileToolExecutor::new(tmp.path());

        let write_call = make_call(
            "write_file",
            serde_json::json!({"path": "lines.txt", "content": "line1\nline2\nline3\nline4\nline5"}),
        );
        exec.execute(write_call).await.expect("write succeeds");

        let read_call = make_call(
            "read_file",
            serde_json::json!({"path": "lines.txt", "offset": 2, "limit": 2}),
        );
        let result = exec.execute(read_call).await.expect("read succeeds");
        assert_eq!(result.output, "line2\nline3");
    }

    #[tokio::test]
    async fn read_accepts_openclaw_aliases() {
        let tmp = TempDir::new().expect("tempdir");
        let exec = FileToolExecutor::new(tmp.path());

        let write_call = make_call(
            "write",
            serde_json::json!({"file_path": "alias.txt", "content": "a\nb\nc\nd"}),
        );
        exec.execute(write_call).await.expect("write succeeds");

        let read_call = make_call(
            "read",
            serde_json::json!({"file_path": "alias.txt", "from": 2, "lines": 2}),
        );
        let result = exec.execute(read_call).await.expect("read succeeds");
        assert_eq!(result.output, "b\nc");
    }

    #[tokio::test]
    async fn edit_replaces_exact_match() {
        let tmp = TempDir::new().expect("tempdir");
        let exec = FileToolExecutor::new(tmp.path());

        let write_call = make_call(
            "write_file",
            serde_json::json!({"path": "edit.txt", "content": "foo bar baz"}),
        );
        exec.execute(write_call).await.expect("write succeeds");

        let edit_call = make_call(
            "edit_file",
            serde_json::json!({"path": "edit.txt", "old_string": "bar", "new_string": "qux"}),
        );
        let result = exec.execute(edit_call).await.expect("edit succeeds");
        assert!(!result.is_error);

        let read_call = make_call("read_file", serde_json::json!({"path": "edit.txt"}));
        let result = exec.execute(read_call).await.expect("read succeeds");
        assert_eq!(result.output, "foo qux baz");
    }

    #[tokio::test]
    async fn edit_accepts_openclaw_aliases() {
        let tmp = TempDir::new().expect("tempdir");
        let exec = FileToolExecutor::new(tmp.path());

        let write_call = make_call(
            "write",
            serde_json::json!({"file_path": "edit_alias.txt", "content": "one two three"}),
        );
        exec.execute(write_call).await.expect("write succeeds");

        let edit_call = make_call(
            "edit",
            serde_json::json!({"file_path": "edit_alias.txt", "oldText": "two", "newText": "TWO"}),
        );
        let result = exec.execute(edit_call).await.expect("edit succeeds");
        assert!(!result.is_error);

        let read_call = make_call("read", serde_json::json!({"path": "edit_alias.txt"}));
        let result = exec.execute(read_call).await.expect("read succeeds");
        assert_eq!(result.output, "one TWO three");
    }

    #[tokio::test]
    async fn edit_rejects_missing_match() {
        let tmp = TempDir::new().expect("tempdir");
        let exec = FileToolExecutor::new(tmp.path());

        let write_call = make_call(
            "write_file",
            serde_json::json!({"path": "edit.txt", "content": "foo bar baz"}),
        );
        exec.execute(write_call).await.expect("write succeeds");

        let edit_call = make_call(
            "edit_file",
            serde_json::json!({"path": "edit.txt", "old_string": "nope", "new_string": "qux"}),
        );
        let result = exec
            .execute(edit_call)
            .await
            .expect("edit returns tool result");
        assert!(result.is_error);
        assert!(result.output.contains("not found"));
    }

    #[tokio::test]
    async fn edit_rejects_multiple_matches() {
        let tmp = TempDir::new().expect("tempdir");
        let exec = FileToolExecutor::new(tmp.path());

        let write_call = make_call(
            "write_file",
            serde_json::json!({"path": "dup.txt", "content": "aaa bbb aaa"}),
        );
        exec.execute(write_call).await.expect("write succeeds");

        let edit_call = make_call(
            "edit_file",
            serde_json::json!({"path": "dup.txt", "old_string": "aaa", "new_string": "ccc"}),
        );
        let result = exec
            .execute(edit_call)
            .await
            .expect("edit returns tool result");
        assert!(result.is_error);
        assert!(result.output.contains("2 times"));
    }

    #[tokio::test]
    async fn absolute_path_rejected() {
        let tmp = TempDir::new().expect("tempdir");
        let exec = FileToolExecutor::new(tmp.path());

        let call = make_call("read_file", serde_json::json!({"path": "/etc/passwd"}));
        let err = exec
            .execute(call)
            .await
            .expect_err("absolute path rejected");
        assert!(matches!(err, ToolError::InvalidArguments { .. }));
    }

    #[tokio::test]
    async fn traversal_rejected() {
        let tmp = TempDir::new().expect("tempdir");
        let exec = FileToolExecutor::new(tmp.path());

        let call = make_call(
            "read_file",
            serde_json::json!({"path": "../../../etc/passwd"}),
        );
        let err = exec.execute(call).await.expect_err("traversal rejected");
        assert!(matches!(err, ToolError::InvalidArguments { .. }));
    }

    #[tokio::test]
    async fn list_files_returns_sorted_entries() {
        let tmp = TempDir::new().expect("tempdir");
        let exec = FileToolExecutor::new(tmp.path());

        for name in ["zebra.txt", "alpha.txt", "middle.txt"] {
            let call = make_call(
                "write_file",
                serde_json::json!({"path": name, "content": "x"}),
            );
            exec.execute(call).await.expect("write succeeds");
        }

        tokio::fs::create_dir(tmp.path().join("nested"))
            .await
            .expect("create dir succeeds");

        let call = make_call("list_files", serde_json::json!({"path": "."}));
        let result = exec.execute(call).await.expect("list succeeds");
        assert_eq!(result.output, "alpha.txt\nmiddle.txt\nnested/\nzebra.txt");
    }

    #[test]
    fn truncate_read_output_caps_lines_and_bytes() {
        let many_lines = (0..2_100).map(|i| format!("line-{i}")).collect::<Vec<_>>().join("\n");
        let truncated = truncate_read_output(&many_lines);
        assert_eq!(truncated.lines().count(), 2_000);

        let huge = "x".repeat(60_000);
        let truncated = truncate_read_output(&huge);
        assert_eq!(truncated.len(), 50_000);
    }
}
