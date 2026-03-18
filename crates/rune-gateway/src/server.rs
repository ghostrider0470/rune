//! Axum server startup and graceful shutdown.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use axum::Router;
use axum::middleware;
use axum::routing::{delete, get, post};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tracing::info;

use rune_config::{AppConfig, Capabilities};
use rune_models::ModelProvider;
use rune_runtime::{
    SessionEngine, SkillLoader, SkillRegistry, TurnExecutor,
    heartbeat::HeartbeatRunner,
    scheduler::{ReminderStore, Scheduler},
};
use rune_store::repos::{
    ApprovalRepo, DeviceRepo, SessionRepo, ToolApprovalPolicyRepo, TranscriptRepo, TurnRepo,
};
use rune_stt::SttEngine;
use rune_stt::openai::OpenAiStt;
use rune_tools::process_tool::ProcessManager;
use rune_tts::TtsEngine;
use rune_tts::elevenlabs::ElevenLabsTts;
use rune_tts::openai::OpenAiTts;
use tokio::sync::RwLock;

use crate::auth::bearer_auth;
use crate::error::GatewayError;
use crate::pairing::DeviceRegistry;
use crate::routes;
use crate::state::{AppState, SessionEvent};
use crate::supervisor::{BackgroundSupervisor, SupervisorDeps};
use crate::ws;

/// Handle returned by [`start`] to allow callers to await server completion.
pub struct GatewayHandle {
    server_handle: tokio::task::JoinHandle<Result<(), GatewayError>>,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    supervisor: BackgroundSupervisor,
}

impl GatewayHandle {
    /// Block until the server shuts down.
    pub async fn wait(self) -> Result<(), GatewayError> {
        self.server_handle
            .await
            .map_err(|e| GatewayError::Internal(e.to_string()))?
    }

    /// Initiate graceful shutdown.
    pub fn shutdown(mut self) {
        self.supervisor.shutdown();
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
    }
}

/// Service dependencies for constructing the gateway.
pub struct Services {
    pub config: AppConfig,
    pub session_engine: Arc<SessionEngine>,
    pub turn_executor: Arc<TurnExecutor>,
    pub session_repo: Arc<dyn SessionRepo>,
    pub transcript_repo: Arc<dyn TranscriptRepo>,
    pub turn_repo: Arc<dyn TurnRepo>,
    pub model_provider: Arc<dyn ModelProvider>,
    pub scheduler: Arc<Scheduler>,
    pub heartbeat: Arc<HeartbeatRunner>,
    pub reminder_store: Arc<ReminderStore>,
    pub approval_repo: Arc<dyn ApprovalRepo>,
    pub tool_approval_repo: Arc<dyn ToolApprovalPolicyRepo>,
    pub process_manager: ProcessManager,
    pub capabilities: Capabilities,
    pub device_repo: Arc<dyn DeviceRepo>,
}

/// Start the gateway HTTP server.
///
/// Returns a [`GatewayHandle`] that can be used to await completion or trigger shutdown.
pub async fn start(services: Services) -> Result<GatewayHandle, GatewayError> {
    let (event_tx, _) = broadcast::channel::<SessionEvent>(256);

    // Extract values from config before wrapping in RwLock.
    let auth_token = services.config.gateway.auth_token.clone();
    let addr: SocketAddr = format!(
        "{}:{}",
        services.config.gateway.host, services.config.gateway.port
    )
    .parse()
    .map_err(|e: std::net::AddrParseError| GatewayError::Internal(e.to_string()))?;

    let skills_dir = services.config.paths.skills_dir.clone();
    let workspace_root = services.config.agents.defaults.workspace.clone();

    // Build TTS engine if an API key is configured.
    let tts_engine = services
        .config
        .media
        .tts
        .api_key
        .as_deref()
        .filter(|k| !k.is_empty())
        .map(|key| {
            let provider: Box<dyn rune_tts::TtsProvider> =
                if services.config.media.tts.provider == "elevenlabs" {
                    Box::new(ElevenLabsTts::new(key))
                } else {
                    Box::new(OpenAiTts::new(key))
                };
            Arc::new(RwLock::new(TtsEngine::new(
                provider,
                services.config.media.tts.clone(),
            )))
        });

    // Build STT engine if an API key is configured.
    let stt_engine = services
        .config
        .media
        .stt
        .api_key
        .as_deref()
        .filter(|k| !k.is_empty())
        .map(|key| {
            let provider: Box<dyn rune_stt::SttProvider> = Box::new(OpenAiStt::new(key));
            Arc::new(RwLock::new(SttEngine::new(
                provider,
                services.config.media.stt.clone(),
            )))
        });

    let config = Arc::new(RwLock::new(services.config));
    let skill_registry = Arc::new(SkillRegistry::new());
    let skill_loader = Arc::new(SkillLoader::new(skills_dir, skill_registry.clone()));
    let _ = skill_loader.scan().await;

    let turn_executor = Arc::new(
        Arc::try_unwrap(services.turn_executor)
            .unwrap_or_else(|executor| (*executor).clone())
            .with_skill_registry(skill_registry.clone()),
    );

    let state = AppState {
        config,
        started_at: Arc::new(Instant::now()),
        session_engine: services.session_engine,
        turn_executor,
        session_repo: services.session_repo,
        transcript_repo: services.transcript_repo,
        turn_repo: services.turn_repo,
        model_provider: services.model_provider,
        scheduler: services.scheduler,
        heartbeat: services.heartbeat,
        reminder_store: services.reminder_store,
        approval_repo: services.approval_repo,
        tool_approval_repo: services.tool_approval_repo,
        process_manager: services.process_manager,
        capabilities: Arc::new(services.capabilities),
        device_repo: services.device_repo.clone(),
        device_registry: Arc::new(DeviceRegistry::new(services.device_repo)),
        skill_registry,
        skill_loader,
        event_tx,
        tts_engine,
        stt_engine,
    };

    let supervisor_deps = SupervisorDeps {
        heartbeat: state.heartbeat.clone(),
        scheduler: state.scheduler.clone(),
        reminder_store: state.reminder_store.clone(),
        session_engine: state.session_engine.clone(),
        turn_executor: state.turn_executor.clone(),
        workspace_root,
        device_registry: state.device_registry.clone(),
    };

    let app = build_router(state, auth_token);

    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| GatewayError::Internal(e.to_string()))?;

    info!(%addr, "gateway listening");

    let mut supervisor = BackgroundSupervisor::new();
    supervisor.start(supervisor_deps);

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server_handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .map_err(|e| GatewayError::Internal(e.to_string()))
    });

    Ok(GatewayHandle {
        server_handle,
        shutdown_tx: Some(shutdown_tx),
        supervisor,
    })
}

/// Build the gateway router. Public for integration testing.
pub fn build_router(state: AppState, auth_token: Option<String>) -> Router {
    let device_registry = state.device_registry.clone();

    let public_routes = Router::new()
        .route("/health", get(routes::health))
        .route("/ws", get(ws::ws_handler))
        .route("/assets/{path}", get(routes::branded_asset))
        .route("/webhook/telegram/{token}", post(routes::telegram_webhook))
        .route("/devices/pair/request", post(routes::device_pair_request))
        .with_state(state.clone());

    let protected_routes = Router::new()
        .route("/status", get(routes::status))
        .route("/dashboard", get(routes::spa_index))
        .route("/ui", get(routes::spa_index))
        .route("/api/dashboard/summary", get(routes::dashboard_summary))
        .route("/api/dashboard/models", get(routes::dashboard_models))
        .route("/api/dashboard/sessions", get(routes::dashboard_sessions))
        .route(
            "/api/dashboard/diagnostics",
            get(routes::dashboard_diagnostics),
        )
        .route("/gateway/health", get(routes::health))
        .route("/gateway/start", post(routes::gateway_start))
        .route("/gateway/stop", post(routes::gateway_stop))
        .route("/gateway/restart", post(routes::gateway_restart))
        .route("/cron/status", get(routes::cron_status))
        .route("/cron", get(routes::cron_list).post(routes::cron_add))
        .route("/cron/wake", post(routes::cron_wake))
        .route(
            "/cron/{id}",
            post(routes::cron_update).delete(routes::cron_remove),
        )
        .route("/cron/{id}/run", post(routes::cron_run))
        .route("/cron/{id}/runs", get(routes::cron_runs))
        .route(
            "/sessions",
            get(routes::list_sessions).post(routes::create_session),
        )
        .route(
            "/sessions/{id}",
            get(routes::get_session)
                .patch(routes::patch_session)
                .delete(routes::delete_session),
        )
        .route("/sessions/{id}/status", get(routes::get_session_status))
        .route("/sessions/{id}/messages", post(routes::send_message))
        .route("/sessions/{id}/transcript", get(routes::get_transcript))
        .route(
            "/approvals",
            get(routes::list_pending_approvals).post(routes::submit_approval_decision),
        )
        .route("/approvals/policies", get(routes::list_approval_policies))
        .route("/processes", get(routes::list_processes))
        .route("/processes/{id}", get(routes::get_process))
        .route("/processes/{id}/log", get(routes::get_process_log))
        .route(
            "/approvals/policies/{tool}",
            get(routes::get_approval_policy)
                .put(routes::set_approval_policy)
                .delete(routes::clear_approval_policy),
        )
        // Device pairing routes
        .route("/devices/pair/approve", post(routes::device_pair_approve))
        .route("/devices/pair/reject", post(routes::device_pair_reject))
        .route("/devices/pair/pending", get(routes::device_pair_pending))
        .route("/devices", get(routes::device_list))
        .route("/devices/{id}", delete(routes::device_revoke))
        .route(
            "/devices/{id}/rotate-token",
            post(routes::device_rotate_token),
        )
        // Model routes
        .route("/models", get(routes::list_models))
        .route("/models/scan", post(routes::scan_models))
        // Skill routes
        .route("/skills", get(routes::list_skills))
        .route("/skills/reload", post(routes::reload_skills))
        .route("/skills/{name}/enable", post(routes::enable_skill))
        .route("/skills/{name}/disable", post(routes::disable_skill))
        // Heartbeat routes
        .route("/heartbeat/status", get(routes::heartbeat_status))
        .route("/heartbeat/enable", post(routes::heartbeat_enable))
        .route("/heartbeat/disable", post(routes::heartbeat_disable))
        // Reminder routes
        .route(
            "/reminders",
            get(routes::reminders_list).post(routes::reminders_add),
        )
        .route("/reminders/{id}", delete(routes::reminders_cancel))
        // TTS routes
        .route("/tts/status", get(routes::tts_status))
        .route("/tts/synthesize", post(routes::tts_synthesize))
        .route("/tts/enable", post(routes::tts_enable))
        .route("/tts/disable", post(routes::tts_disable))
        // STT routes
        .route("/stt/status", get(routes::stt_status))
        .route("/stt/transcribe", post(routes::stt_transcribe))
        .route("/stt/enable", post(routes::stt_enable))
        .route("/stt/disable", post(routes::stt_disable))
        // Config editor routes
        .route(
            "/config",
            get(routes::get_config).put(routes::update_config),
        )
        // Turn routes
        .route("/api/turns", get(routes::list_turns))
        .route("/api/turns/{id}", get(routes::get_turn))
        // Tool routes
        .route("/api/tools", get(routes::list_tools))
        .route("/api/tools/{id}", get(routes::get_tool_execution))
        // Auth routes
        .route("/api/auth", get(routes::auth_token_info))
        // Channel routes
        .route("/api/channels", get(routes::list_channels))
        .route("/api/channels/status", get(routes::channels_status))
        // Memory routes
        .route("/api/memory/status", get(routes::memory_status))
        .route("/api/memory/search", get(routes::memory_search))
        // Log routes
        .route("/api/logs", get(routes::query_logs))
        // Doctor routes
        .route("/api/doctor/run", post(routes::doctor_run))
        .route("/api/doctor/results", get(routes::doctor_results))
        .layer(middleware::from_fn(move |req, next| {
            bearer_auth(req, next, auth_token.clone(), device_registry.clone())
        }))
        .with_state(state);

    let spa_routes = Router::new()
        .route("/", get(routes::spa_handler))
        .fallback(routes::spa_handler);

    Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .merge(spa_routes)
}
