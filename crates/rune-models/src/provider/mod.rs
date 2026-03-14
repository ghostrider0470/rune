pub mod anthropic;
pub mod azure;
pub mod azure_foundry;
pub mod bedrock;
pub mod deepseek;
pub mod google;
pub mod groq;
pub mod mistral;
pub mod ollama;
pub mod openai;
pub(crate) mod response;

use async_trait::async_trait;

use crate::error::ModelError;
use crate::types::{CompletionRequest, CompletionResponse};

/// Trait for model providers (Azure OpenAI, OpenAI, etc.).
#[async_trait]
pub trait ModelProvider: Send + Sync + std::fmt::Debug {
    /// Send a completion request and return the response.
    async fn complete(&self, request: &CompletionRequest)
    -> Result<CompletionResponse, ModelError>;
}
