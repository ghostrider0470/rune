//! Shared application state for Axum handlers.

use std::sync::Arc;
use std::time::Instant;

use rune_config::AppConfig;
use rune_models::ModelProvider;
use rune_runtime::{SessionEngine, TurnExecutor, heartbeat::HeartbeatRunner, scheduler::{ReminderStore, Scheduler}};
use rune_store::repos::{SessionRepo, ToolApprovalPolicyRepo, TranscriptRepo};
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
    /// Process start time for uptime/status reporting.
    pub started_at: Arc<Instant>,
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
    /// In-memory scheduler backing the current cron operator surface.
    pub scheduler: Arc<Scheduler>,
    /// Heartbeat runner for periodic check-ins.
    pub heartbeat: Arc<HeartbeatRunner>,
    /// In-memory reminder store.
    pub reminder_store: Arc<ReminderStore>,
    /// Tool approval policy repository.
    pub tool_approval_repo: Arc<dyn ToolApprovalPolicyRepo>,
    /// Number of registered tools in the runtime graph.
    pub tool_count: usize,
    /// Broadcast channel for session events (WebSocket fan-out).
    pub event_tx: broadcast::Sender<SessionEvent>,
}
