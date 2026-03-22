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
use crate::types::{CompletionRequest, CompletionResponse, StreamEvent};

/// Trait for model providers (Azure OpenAI, OpenAI, etc.).
#[async_trait]
pub trait ModelProvider: Send + Sync + std::fmt::Debug {
    /// Send a completion request and return the response.
    async fn complete(&self, request: &CompletionRequest)
    -> Result<CompletionResponse, ModelError>;

    /// Stream a completion request, yielding text deltas as they arrive.
    ///
    /// The default implementation falls back to non-streaming [`complete`](Self::complete),
    /// emitting the full content as a single [`StreamEvent::TextDelta`] followed by
    /// [`StreamEvent::Done`].
    async fn complete_stream(
        &self,
        request: &CompletionRequest,
    ) -> Result<tokio::sync::mpsc::Receiver<StreamEvent>, ModelError> {
        let response = self.complete(request).await?;
        let (tx, rx) = tokio::sync::mpsc::channel(2);
        if let Some(ref content) = response.content {
            let _ = tx.send(StreamEvent::TextDelta(content.clone())).await;
        }
        let _ = tx.send(StreamEvent::Done(response)).await;
        Ok(rx)
    }
}
