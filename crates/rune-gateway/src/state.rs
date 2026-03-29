//! Shared application state for Axum handlers.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;

use rune_config::{AppConfig, Capabilities};
use rune_models::ModelProvider;
use rune_runtime::CommsClient;
use rune_runtime::{
    HookRegistry, PluginLoader, PluginManager, PluginRegistry, SessionEngine, SkillLoader,
    SkillRegistry, TurnExecutor,
    heartbeat::HeartbeatRunner,
    scheduler::{ReminderStore, Scheduler},
};
use rune_store::repos::{
    ApprovalRepo, DeviceRepo, SessionRepo, ToolApprovalPolicyRepo, ToolExecutionRepo,
    TranscriptRepo, TurnRepo,
};
use rune_stt::SttEngine;
use rune_tools::process_tool::ProcessManager;
use rune_tts::TtsEngine;
use tokio::sync::{Mutex, RwLock, broadcast};

use crate::logging::LogStore;
use crate::ms365::{
    Ms365CalendarService, Ms365FilesService, Ms365MailService, Ms365PlannerService,
    Ms365TodoService, Ms365UsersService,
};
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

#[derive(Clone, Debug)]
pub struct WebChatRateLimitEntry {
    pub window_started_at: Instant,
    pub count: u32,
    pub retry_after: Duration,
}

#[derive(Clone, Debug)]
pub struct WebChatRateLimiter {
    window: Duration,
    max_requests: u32,
    entries: Arc<Mutex<HashMap<String, WebChatRateLimitEntry>>>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct TokenMetricsSnapshot {
    pub provider: String,
    pub model: String,
    pub total_input_tokens: u64,
    pub cached_tokens: u64,
    pub uncached_tokens: u64,
    pub cache_hit_ratio_percent: f64,
}

#[derive(Clone, Debug, Default)]
struct TokenMetricsEntry {
    total_input_tokens: u64,
    cached_tokens: u64,
    uncached_tokens: u64,
}

#[derive(Clone, Debug, Default)]
pub struct TokenMetricsStore {
    inner: Arc<Mutex<HashMap<(String, String), TokenMetricsEntry>>>,
}

#[derive(Clone, Debug)]
pub struct DelegationTaskRecord {
    pub request: crate::routes::DelegationTaskRequest,
    pub result: crate::routes::DelegationTaskResultResponse,
}

#[derive(Clone, Debug, Default)]
pub struct DelegationTaskStore {
    inner: Arc<Mutex<HashMap<String, DelegationTaskRecord>>>,
}

impl DelegationTaskStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn submit(
        &self,
        request: crate::routes::DelegationTaskRequest,
    ) -> Result<crate::routes::DelegationTaskResultResponse, String> {
        let mut inner = self.inner.lock().await;
        match inner.entry(request.task_id.clone()) {
            Entry::Occupied(_) => Err(format!(
                "delegation task '{}' already exists",
                request.task_id
            )),
            Entry::Vacant(slot) => {
                let accepted_at = chrono::Utc::now().to_rfc3339();
                let result = crate::routes::DelegationTaskResultResponse {
                    task_id: request.task_id.clone(),
                    status: "accepted".to_string(),
                    accepted_at,
                    started_at: None,
                    output: None,
                    artifacts: request.task.artifacts.clone(),
                    error: None,
                    finished_at: None,
                };
                slot.insert(DelegationTaskRecord {
                    request,
                    result: result.clone(),
                });
                Ok(result)
            }
        }
    }

    pub async fn get(&self, task_id: &str) -> Option<DelegationTaskRecord> {
        let inner = self.inner.lock().await;
        inner.get(task_id).cloned()
    }
}

impl TokenMetricsStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn record(
        &self,
        provider: impl Into<String>,
        model: impl Into<String>,
        total_input_tokens: u32,
        cached_tokens: Option<u32>,
        uncached_tokens: Option<u32>,
    ) {
        let provider = provider.into();
        let model = model.into();
        let mut inner = self.inner.lock().await;
        let entry = inner.entry((provider, model)).or_default();

        let total_input_tokens = u64::from(total_input_tokens);
        let cached_tokens = u64::from(cached_tokens.unwrap_or(0));
        let uncached_tokens = uncached_tokens
            .map(u64::from)
            .unwrap_or_else(|| total_input_tokens.saturating_sub(cached_tokens));

        entry.total_input_tokens = entry.total_input_tokens.saturating_add(total_input_tokens);
        entry.cached_tokens = entry.cached_tokens.saturating_add(cached_tokens);
        entry.uncached_tokens = entry.uncached_tokens.saturating_add(uncached_tokens);
    }

    pub async fn snapshot(&self) -> Vec<TokenMetricsSnapshot> {
        let inner = self.inner.lock().await;
        Self::rows_from_inner(&inner)
    }
    fn rows_from_inner(
        inner: &HashMap<(String, String), TokenMetricsEntry>,
    ) -> Vec<TokenMetricsSnapshot> {
        let mut rows = inner
            .iter()
            .map(|((provider, model), entry)| {
                let ratio = if entry.total_input_tokens == 0 {
                    0.0
                } else {
                    (entry.cached_tokens as f64 / entry.total_input_tokens as f64) * 100.0
                };
                TokenMetricsSnapshot {
                    provider: provider.clone(),
                    model: model.clone(),
                    total_input_tokens: entry.total_input_tokens,
                    cached_tokens: entry.cached_tokens,
                    uncached_tokens: entry.uncached_tokens,
                    cache_hit_ratio_percent: ratio,
                }
            })
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| {
            a.provider
                .cmp(&b.provider)
                .then_with(|| a.model.cmp(&b.model))
        });
        rows
    }
}

impl WebChatRateLimiter {
    pub fn new(window: Duration, max_requests: u32) -> Self {
        Self {
            window,
            max_requests: max_requests.max(1),
            entries: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn check(&self, key: impl Into<String>) -> Result<(), Duration> {
        let key = key.into();
        let now = Instant::now();
        let mut entries = self.entries.lock().await;

        entries.retain(|_, entry| now.duration_since(entry.window_started_at) < self.window);

        let entry = entries.entry(key).or_insert_with(|| WebChatRateLimitEntry {
            window_started_at: now,
            count: 0,
            retry_after: Duration::from_secs(0),
        });

        if now.duration_since(entry.window_started_at) >= self.window {
            entry.window_started_at = now;
            entry.count = 0;
            entry.retry_after = Duration::from_secs(0);
        }

        if entry.count >= self.max_requests {
            let retry_after = self
                .window
                .saturating_sub(now.duration_since(entry.window_started_at));
            entry.retry_after = retry_after;
            return Err(retry_after);
        }

        entry.count += 1;
        entry.retry_after = Duration::from_secs(0);
        Ok(())
    }
}

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
    /// Tool execution audit repository.
    pub tool_execution_repo: Arc<dyn ToolExecutionRepo>,
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
    /// Plugin lifecycle manager (enable/disable/reload).
    pub plugin_manager: Option<Arc<PluginManager>>,
    /// Broadcast channel for session events (WebSocket fan-out).
    pub event_tx: broadcast::Sender<SessionEvent>,
    /// In-memory structured log buffer + fan-out for the admin log viewer.
    pub log_store: LogStore,
    /// Per-browser WebChat send limiter used to smooth reconnect storms and abuse.
    pub webchat_rate_limiter: Arc<WebChatRateLimiter>,
    /// Text-to-speech engine (constructed when TTS API key is configured).
    pub tts_engine: Option<Arc<RwLock<TtsEngine>>>,
    /// Speech-to-text engine (constructed when STT API key is configured).
    pub stt_engine: Option<Arc<RwLock<SttEngine>>>,
    /// Microsoft 365 Calendar mutation backend.
    pub ms365_calendar_service: Arc<dyn Ms365CalendarService>,
    /// Microsoft 365 Planner mutation backend.
    pub ms365_planner_service: Arc<dyn Ms365PlannerService>,
    /// Microsoft 365 To-Do mutation backend.
    pub ms365_todo_service: Arc<dyn Ms365TodoService>,
    /// Microsoft 365 Mail mutation backend.
    pub ms365_mail_service: Arc<dyn Ms365MailService>,
    /// Microsoft 365 Files read backend.
    pub ms365_files_service: Arc<dyn Ms365FilesService>,
    /// Microsoft 365 Users read backend.
    pub ms365_users_service: Arc<dyn Ms365UsersService>,
    /// Optional filesystem mailbox client for native inter-agent comms.
    pub comms_client: Option<Arc<CommsClient>>,
    /// Rolling in-memory prompt cache metrics grouped by provider/model.
    pub token_metrics: TokenMetricsStore,
    /// In-memory registry for delegated cross-instance tasks.
    pub delegation_tasks: DelegationTaskStore,
}
