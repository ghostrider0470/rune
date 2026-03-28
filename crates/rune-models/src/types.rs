use serde::{Deserialize, Serialize};

/// Content block for multimodal chat messages.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessagePart {
    Text {
        text: String,
    },
    ImageUrl {
        image_url: ImageUrlPart,
    },
}

/// OpenAI-compatible image URL content block.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImageUrlPart {
    pub url: String,
}

/// Role of a message participant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// A single chat message.
#[derive(Clone, Debug, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "serialize_message_content")]
    pub content: Option<String>,
    #[serde(skip)]
    pub content_parts: Option<Vec<MessagePart>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallRequest>>,
}

/// A tool call requested by the model.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCallRequest {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

/// Function call details within a tool call.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

/// Tool definition provided to the model.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

/// Function definition within a tool.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Request to a model provider.
#[derive(Clone, Debug, Serialize)]
pub struct CompletionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stable_prefix_messages: Option<Vec<ChatMessage>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stable_prefix_tools: Option<Vec<ToolDefinition>>,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
}

/// Reason the model stopped generating.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
}

/// Token usage information.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_prompt_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uncached_prompt_tokens: Option<u32>,
}

/// Response from a model provider.
#[derive(Clone, Debug)]
pub struct CompletionResponse {
    pub content: Option<String>,
    pub usage: Usage,
    pub finish_reason: Option<FinishReason>,
    pub tool_calls: Vec<ToolCallRequest>,
}

/// Events emitted during streaming completion.
#[derive(Clone, Debug)]
pub enum StreamEvent {
    /// A chunk of text content from the model.
    TextDelta(String),
    /// Streaming is complete; carries the assembled final response.
    Done(CompletionResponse),
}

impl Serialize for ChatMessage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("role", &self.role)?;

        if let Some(content_parts) = &self.content_parts {
            map.serialize_entry("content", content_parts)?;
        } else if let Some(content) = &self.content {
            map.serialize_entry("content", content)?;
        }

        if let Some(name) = &self.name {
            map.serialize_entry("name", name)?;
        }
        if let Some(tool_call_id) = &self.tool_call_id {
            map.serialize_entry("tool_call_id", tool_call_id)?;
        }
        if let Some(tool_calls) = &self.tool_calls {
            map.serialize_entry("tool_calls", tool_calls)?;
        }

        map.end()
    }
}
