use std::sync::Arc;

use async_trait::async_trait;
use rune_core::ToolCategory;
use rune_tools::{ToolCall, ToolDefinition, ToolError, ToolExecutor, ToolResult};
use serde::Deserialize;

use crate::browser::BrowserPool;
use crate::error::BrowserError;
use crate::snapshot::{BrowserSnapshot, SnapshotOptions};

/// Parameters for the `browse` tool.
#[derive(Clone, Debug, Deserialize)]
pub struct BrowseParams {
    /// URL to browse.
    pub url: String,
    /// Reserved for future DOM stabilization support.
    pub wait_for: Option<String>,
    /// Optional maximum characters in the returned snapshot.
    pub max_chars: Option<usize>,
}

/// Backend abstraction for browse tool tests.
#[async_trait]
pub trait BrowseBackend: Send + Sync {
    async fn browse(
        &self,
        url: &str,
        options: &SnapshotOptions,
    ) -> Result<BrowserSnapshot, BrowserError>;
}

#[async_trait]
impl BrowseBackend for BrowserPool {
    async fn browse(
        &self,
        url: &str,
        options: &SnapshotOptions,
    ) -> Result<BrowserSnapshot, BrowserError> {
        BrowserPool::browse(self, url, options).await
    }
}

/// Tool executor that exposes semantic browser snapshots as the `browse` tool.
pub struct BrowseTool<B = BrowserPool> {
    backend: Arc<B>,
    default_options: SnapshotOptions,
    default_max_chars: usize,
}

impl<B> BrowseTool<B> {
    #[must_use]
    pub fn new(
        backend: Arc<B>,
        default_options: SnapshotOptions,
        default_max_chars: usize,
    ) -> Self {
        Self {
            backend,
            default_options,
            default_max_chars: default_max_chars.max(1),
        }
    }
}

pub fn browse_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: "browse".to_string(),
        description: "Browse a URL and return a semantic text snapshot of the page content with numeric references for interactive elements.".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "required": ["url"],
            "properties": {
                "url": {
                    "type": "string",
                    "description": "HTTP or HTTPS URL to browse"
                },
                "wait_for": {
                    "type": "string",
                    "description": "Reserved selector hint for dynamic pages"
                },
                "max_chars": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Maximum characters in the returned snapshot"
                }
            }
        }),
        category: ToolCategory::External,
        requires_approval: false,
    }
}

#[async_trait]
impl<B> ToolExecutor for BrowseTool<B>
where
    B: BrowseBackend + 'static,
{
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let params: BrowseParams =
            serde_json::from_value(call.arguments).map_err(|err| ToolError::InvalidArguments {
                tool: call.tool_name.clone(),
                reason: err.to_string(),
            })?;

        let options = self.default_options.clone();
        if let Some(wait_for) = params.wait_for.as_deref() {
            let _ = wait_for;
        }
        let max_chars = params.max_chars.unwrap_or(self.default_max_chars).max(1);

        match self.backend.browse(&params.url, &options).await {
            Ok(snapshot) => Ok(ToolResult {
                tool_call_id: call.tool_call_id,
                output: truncate_snapshot_text(&snapshot.text, max_chars),
                is_error: false,
                tool_execution_id: None,
            }),
            Err(BrowserError::InvalidUrl { reason, .. }) => Err(ToolError::InvalidArguments {
                tool: call.tool_name,
                reason,
            }),
            Err(BrowserError::UrlBlocked { .. }) => Ok(ToolResult {
                tool_call_id: call.tool_call_id,
                output: format!("Error: {}", BrowserError::UrlBlocked { url: params.url }),
                is_error: true,
                tool_execution_id: None,
            }),
            Err(err) => Ok(ToolResult {
                tool_call_id: call.tool_call_id,
                output: format!("Error: {err}"),
                is_error: true,
                tool_execution_id: None,
            }),
        }
    }
}

fn truncate_snapshot_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }

    let truncated = text.chars().take(max_chars).collect::<String>();
    format!("{truncated}\n[snapshot truncated at {max_chars} chars]")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune_core::ToolCallId;
    use std::sync::Mutex;

    struct StubBackend {
        response: Mutex<Option<Result<BrowserSnapshot, BrowserError>>>,
    }

    #[async_trait]
    impl BrowseBackend for StubBackend {
        async fn browse(
            &self,
            _url: &str,
            _options: &SnapshotOptions,
        ) -> Result<BrowserSnapshot, BrowserError> {
            self.response
                .lock()
                .expect("stub backend mutex poisoned")
                .take()
                .expect("stub backend response already consumed")
        }
    }

    fn tool_call(arguments: serde_json::Value) -> ToolCall {
        ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: "browse".to_string(),
            arguments,
        }
    }

    #[test]
    fn browse_tool_definition_exposes_expected_schema() {
        let definition = browse_tool_definition();
        assert_eq!(definition.name, "browse");
        assert_eq!(definition.category, ToolCategory::External);
        assert!(!definition.requires_approval);
        assert_eq!(
            definition.parameters["required"],
            serde_json::json!(["url"])
        );
    }

    #[tokio::test]
    async fn browse_tool_rejects_invalid_url_arguments() {
        let tool = BrowseTool::new(
            Arc::new(StubBackend {
                response: Mutex::new(Some(Err(BrowserError::InvalidUrl {
                    url: "ftp://example.com".to_string(),
                    reason: "unsupported scheme 'ftp'".to_string(),
                }))),
            }),
            SnapshotOptions::default(),
            30_000,
        );

        let err = tool
            .execute(tool_call(serde_json::json!({ "url": "ftp://example.com" })))
            .await
            .unwrap_err();

        assert!(matches!(err, ToolError::InvalidArguments { .. }));
    }

    #[tokio::test]
    async fn browse_tool_returns_blocked_url_as_tool_error_result() {
        let tool = BrowseTool::new(
            Arc::new(StubBackend {
                response: Mutex::new(Some(Err(BrowserError::UrlBlocked {
                    url: "https://blocked.example.com".to_string(),
                }))),
            }),
            SnapshotOptions::default(),
            30_000,
        );

        let result = tool
            .execute(tool_call(
                serde_json::json!({ "url": "https://blocked.example.com" }),
            ))
            .await
            .expect("blocked URL should return a tool result");

        assert!(result.is_error);
        assert!(result.output.contains("URL blocked by policy"));
    }

    #[tokio::test]
    async fn browse_tool_truncates_snapshot_output() {
        let tool = BrowseTool::new(
            Arc::new(StubBackend {
                response: Mutex::new(Some(Ok(BrowserSnapshot {
                    url: "https://example.com".to_string(),
                    title: "Example".to_string(),
                    elements: Vec::new(),
                    text: "0123456789abcdef".to_string(),
                }))),
            }),
            SnapshotOptions::default(),
            8,
        );

        let result = tool
            .execute(tool_call(
                serde_json::json!({ "url": "https://example.com" }),
            ))
            .await
            .expect("snapshot should succeed");

        assert!(!result.output.is_empty());
        assert!(result.output.contains("[snapshot truncated at 8 chars]"));
    }
}
