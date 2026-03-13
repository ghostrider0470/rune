//! Shared application state for Axum handlers.

use std::sync::Arc;

use rune_config::AppConfig;
use rune_models::ModelProvider;
use rune_runtime::{SessionEngine, TurnExecutor};
use rune_store::repos::{SessionRepo, TranscriptRepo};
use tokio::sync::broadcast;

/// Events emitted for WebSocket subscribers.
#[derive(Clone, Debug, serde::Serialize)]
pub struct SessionEvent {
    /// The session this event belongs to.
    pub session_id: String,
    /// Event kind: `transcript_item`, `status_change`, etc.
    pub kind: String,
    /// Arbitrary JSON payload.
    pub payload: serde_json::Value,
}

/// Shared state accessible from all route handlers.
#[derive(Clone)]
pub struct AppState {
    /// Resolved application configuration.
    pub config: Arc<AppConfig>,
    /// Session engine for lifecycle management.
    pub session_engine: Arc<SessionEngine>,
    /// Turn executor for processing messages.
    pub turn_executor: Arc<TurnExecutor>,
    /// Session repository for direct queries.
    pub session_repo: Arc<dyn SessionRepo>,
    /// Transcript repository for transcript queries.
    pub transcript_repo: Arc<dyn TranscriptRepo>,
    /// Model provider for status reporting.
    pub model_provider: Arc<dyn ModelProvider>,
    /// Broadcast channel for session events (WebSocket fan-out).
    pub event_tx: broadcast::Sender<SessionEvent>,
}
