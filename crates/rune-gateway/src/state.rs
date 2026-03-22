//! Shared application state for Axum handlers.

use std::sync::Arc;
use std::time::Instant;

use rune_config::{AppConfig, Capabilities};
use rune_models::ModelProvider;
use rune_runtime::{
    HookRegistry, PluginLoader, PluginRegistry, SessionEngine, SkillLoader, SkillRegistry,
    TurnExecutor,
    heartbeat::HeartbeatRunner,
    scheduler::{ReminderStore, Scheduler},
};
use rune_store::repos::{
    ApprovalRepo, DeviceRepo, SessionRepo, ToolApprovalPolicyRepo, TranscriptRepo, TurnRepo,
};
use rune_stt::SttEngine;
use rune_tools::process_tool::ProcessManager;
use rune_tts::TtsEngine;
use tokio::sync::{RwLock, broadcast};

use crate::ms365::{Ms365MailService, Ms365PlannerService, Ms365TodoService};
use crate::pairing::DeviceRegistry;

/// Events emitted for WebSocket subscribers.
#[derive(Clone, Debug, serde::Serialize)]
pub struct SessionEvent {
    /// The session this event belongs to.
    pub session_id: String,
    /// Event kind: `transcript_item`, `status_change`, etc.
    pub kind: String,
    /// Arbitrary JSON payload.
    pub payload: serde_json::Value,
    /// Whether this event should bump the connection-visible state version.
    pub state_changed: bool,
}

/// Shared state accessible from all route handlers.
#[derive(Clone)]
pub struct AppState {
    /// Resolved application configuration (behind RwLock for live editing).
    pub config: Arc<RwLock<AppConfig>>,
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
    /// Turn repository for session-level aggregates.
    pub turn_repo: Arc<dyn TurnRepo>,
    /// Model provider for status reporting.
    pub model_provider: Arc<dyn ModelProvider>,
    /// In-memory scheduler backing the current cron operator surface.
    pub scheduler: Arc<Scheduler>,
    /// Heartbeat runner for periodic check-ins.
    pub heartbeat: Arc<HeartbeatRunner>,
    /// In-memory reminder store.
    pub reminder_store: Arc<ReminderStore>,
    /// Durable approval request repository.
    pub approval_repo: Arc<dyn ApprovalRepo>,
    /// Tool approval policy repository.
    pub tool_approval_repo: Arc<dyn ToolApprovalPolicyRepo>,
    /// Background process manager for operator inspection/control surfaces.
    pub process_manager: ProcessManager,
    /// Consolidated runtime capabilities (immutable after boot).
    pub capabilities: Arc<Capabilities>,
    /// Persistent device pairing repository.
    pub device_repo: Arc<dyn DeviceRepo>,
    /// Device pairing registry for Ed25519 challenge-response auth.
    pub device_registry: Arc<DeviceRegistry>,
    /// Dynamic skill registry populated from scanned `SKILL.md` directories.
    pub skill_registry: Arc<SkillRegistry>,
    /// Skill loader used for explicit reloads and background scanning.
    pub skill_loader: Arc<SkillLoader>,
    /// Dynamic plugin registry populated from scanned `PLUGIN.md` directories.
    pub plugin_registry: Arc<PluginRegistry>,
    /// Plugin loader used for discovery and background scanning.
    pub plugin_loader: Arc<PluginLoader>,
    /// Hook registry for plugin event handlers.
    pub hook_registry: Arc<HookRegistry>,
    /// Broadcast channel for session events (WebSocket fan-out).
    pub event_tx: broadcast::Sender<SessionEvent>,
    /// Text-to-speech engine (constructed when TTS API key is configured).
    pub tts_engine: Option<Arc<RwLock<TtsEngine>>>,
    /// Speech-to-text engine (constructed when STT API key is configured).
    pub stt_engine: Option<Arc<RwLock<SttEngine>>>,
    /// Microsoft 365 Planner mutation backend.
    pub ms365_planner_service: Arc<dyn Ms365PlannerService>,
    /// Microsoft 365 To-Do mutation backend.
    pub ms365_todo_service: Arc<dyn Ms365TodoService>,
    /// Microsoft 365 Mail mutation backend.
    pub ms365_mail_service: Arc<dyn Ms365MailService>,
}
