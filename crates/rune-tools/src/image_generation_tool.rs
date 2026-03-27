//! Image generation tool definition and lightweight executor scaffolding.
//!
//! This lane ships the Rune tool contract for image generation so the runtime
//! can advertise a first-class `image_generation` capability with medium-risk
//! approval semantics and concrete parameters for prompt, size, quality, style,
//! model, and output directory.

use std::path::PathBuf;

use async_trait::async_trait;

use crate::definition::{ToolCall, ToolDefinition, ToolResult};
use crate::error::ToolError;
use crate::executor::ToolExecutor;

/// Executor scaffold for `image_generation`.
///
/// Full provider-backed generation is intentionally left for the next slice;
/// this issue lane establishes the tool contract and output shape expected by
/// the runtime and channels.
pub struct ImageGenerationToolExecutor {
    default_output_dir: PathBuf,
}

impl ImageGenerationToolExecutor {
    #[must_use]
    pub fn new(default_output_dir: impl Into<PathBuf>) -> Self {
        Self {
            default_output_dir: default_output_dir.into(),
        }
    }
}

#[async_trait]
impl ToolExecutor for ImageGenerationToolExecutor {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        match call.tool_name.as_str() {
            "image_generation" => Ok(ToolResult {
                tool_call_id: call.tool_call_id,
                output: format!(
                    "image_generation is registered but not yet wired to a provider; default_output_dir={}",
                    self.default_output_dir.display()
                ),
                is_error: true,
                tool_execution_id: None,
            }),
            other => Err(ToolError::NotFound(other.into())),
        }
    }
}

/// Return the `ToolDefinition` for registration in the tool registry.
pub fn image_generation_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: "image_generation".into(),
        description: "Generate an image from a text prompt using the configured image model/provider. Supports prompt, optional size/quality/style parameters, and saving results to a configurable output directory. Returns the saved file path or provider URL for downstream channel delivery.".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Text prompt describing the image to generate"
                },
                "model": {
                    "type": "string",
                    "description": "Optional image model override (defaults to configured default_image_model)"
                },
                "size": {
                    "type": "string",
                    "description": "Requested image size such as 1024x1024, 1024x1792, or 1792x1024"
                },
                "quality": {
                    "type": "string",
                    "description": "Requested render quality such as standard or hd"
                },
                "style": {
                    "type": "string",
                    "description": "Requested render style such as vivid or natural"
                },
                "output_dir": {
                    "type": "string",
                    "description": "Directory where generated images should be saved"
                },
                "filename": {
                    "type": "string",
                    "description": "Optional filename to use when saving the generated image"
                }
            },
            "required": ["prompt"]
        }),
        category: rune_core::ToolCategory::External,
        requires_approval: true,
    }
}

#[cfg(test)]
mod tests {
    use rune_core::ToolCallId;

    use super::*;

    #[test]
    fn definition_has_required_prompt_and_medium_risk_shape() {
        let def = image_generation_tool_definition();
        assert_eq!(def.name, "image_generation");
        assert_eq!(def.category, rune_core::ToolCategory::External);
        assert!(def.requires_approval);
        let required = def.parameters["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v.as_str() == Some("prompt")));
        assert!(def.parameters["properties"].get("size").is_some());
        assert!(def.parameters["properties"].get("quality").is_some());
        assert!(def.parameters["properties"].get("style").is_some());
        assert!(def.parameters["properties"].get("output_dir").is_some());
    }

    #[tokio::test]
    async fn executor_returns_not_yet_wired_error_result() {
        let exec = ImageGenerationToolExecutor::new("generated-images");
        let call = ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: "image_generation".into(),
            arguments: serde_json::json!({"prompt": "draw a rune logo"}),
        };

        let result = exec.execute(call).await.unwrap();
        assert!(result.is_error);
        assert!(result.output.contains("not yet wired"));
        assert!(result.output.contains("generated-images"));
    }

    #[tokio::test]
    async fn executor_rejects_unknown_tool_name() {
        let exec = ImageGenerationToolExecutor::new("generated-images");
        let call = ToolCall {
            tool_call_id: ToolCallId::new(),
            tool_name: "not_image_generation".into(),
            arguments: serde_json::json!({}),
        };

        let err = exec.execute(call).await.unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
    }
}
