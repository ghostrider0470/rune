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
    HookRegistry, PluginLoader, PluginRegistry,
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
use crate::webchat;
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
    let tls_config = services.config.gateway.tls.clone();
    let auth_token = services.config.gateway.auth_token.clone();
    let addr: SocketAddr = format!(
        "{}:{}",
        services.config.gateway.host, services.config.gateway.port
    )
    .parse()
    .map_err(|e: std::net::AddrParseError| GatewayError::Internal(e.to_string()))?;

    let skills_dir = services.config.paths.skills_dir.clone();
    let plugins_dir = services.config.paths.plugins_dir.clone();
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
            let provider: Box<dyn rune_stt::SttProvider> = Box::new(OpenAiStt::new(
                key,
                services.config.media.stt.base_url.clone().unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
                services.config.media.stt.api_version.clone(),
                services.config.media.stt.model.clone(),
            ));
            Arc::new(RwLock::new(SttEngine::new(
                provider,
                services.config.media.stt.clone(),
            )))
        });

    // Clone telegram token before services.config is moved
    let tg_token_for_delivery = services.config.channels.telegram_token.clone();
    let config = Arc::new(RwLock::new(services.config));
    let skill_registry = Arc::new(SkillRegistry::new());
    let skill_loader = Arc::new(SkillLoader::new(skills_dir, skill_registry.clone()));
    let _ = skill_loader.scan().await;

    // Plugin and hook system initialization
    let plugin_registry = Arc::new(PluginRegistry::new());
    let plugin_loader = Arc::new(PluginLoader::new(plugins_dir, plugin_registry.clone()));
    let _ = plugin_loader.scan().await;
    let hook_registry = Arc::new(HookRegistry::new());
    plugin_registry.register_hooks(&hook_registry).await;

    let turn_executor = Arc::new(
        Arc::try_unwrap(services.turn_executor)
            .unwrap_or_else(|executor| (*executor).clone())
            .with_skill_registry(skill_registry.clone())
            .with_tool_approval_policy_repo(services.tool_approval_repo.clone())
            .with_hook_registry(hook_registry.clone()),
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
        plugin_registry,
        plugin_loader,
        hook_registry,
        event_tx,
        tts_engine,
        stt_engine,
    };

    // Build operator delivery for heartbeat/scheduled output to Telegram.
    // Looks up the operator's chat_id from the most recent Channel session's
    // channel_ref (format: "{chat_id}:{sender}").
    let operator_delivery: Option<Arc<dyn crate::supervisor::OperatorDelivery>> =
        if let Some(ref tg_token) = tg_token_for_delivery {
            let session_repo = &state.session_repo;
            let chat_id = session_repo
                .list(50, 0)
                .await
                .ok()
                .and_then(|sessions| {
                    sessions.into_iter()
                        .find(|s| s.channel_ref.is_some())
                        .and_then(|s| s.channel_ref)
                        .and_then(|r| r.split(':').next().map(String::from))
                });
            if let Some(chat_id) = chat_id {
                info!(chat_id = %chat_id, "heartbeat delivery target resolved from session DB");
                Some(Arc::new(crate::supervisor::TelegramOperatorDelivery::new(
                    tg_token.clone(),
                    chat_id,
                )))
            } else {
                info!("no Telegram sessions found yet — heartbeat delivery will be configured after first message");
                None
            }
        } else {
            None
        };

    let supervisor_deps = SupervisorDeps {
        heartbeat: state.heartbeat.clone(),
        scheduler: state.scheduler.clone(),
        reminder_store: state.reminder_store.clone(),
        session_engine: state.session_engine.clone(),
        turn_executor: state.turn_executor.clone(),
        workspace_root,
        device_registry: state.device_registry.clone(),
        event_tx: state.event_tx.clone(),
        operator_delivery,
    };

    let app = build_router(state, auth_token);

    // Validate TLS config before attempting to bind.
    tls_config
        .validate()
        .map_err(GatewayError::Internal)?;

    let mut supervisor = BackgroundSupervisor::new();
    supervisor.start(supervisor_deps);

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let server_handle = if tls_config.enabled {
        let rustls_config = build_rustls_config(
            tls_config.cert_path.as_deref().unwrap(),
            tls_config.key_path.as_deref().unwrap(),
        )?;

        info!(%addr, "gateway listening (HTTPS)");

        let tls_cfg = axum_server::tls_rustls::RustlsConfig::from_config(rustls_config);
        let handle = axum_server::Handle::new();
        let shutdown_handle = handle.clone();
        tokio::spawn(async move {
            // Spawn a task to trigger graceful shutdown on signal.
            tokio::spawn(async move {
                let _ = shutdown_rx.await;
                shutdown_handle.graceful_shutdown(None);
            });
            axum_server::bind_rustls(addr, tls_cfg)
                .handle(handle)
                .serve(app.into_make_service())
                .await
                .map_err(|e| GatewayError::Internal(e.to_string()))
        })
    } else {
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| GatewayError::Internal(e.to_string()))?;

        info!(%addr, "gateway listening (HTTP)");

        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .map_err(|e| GatewayError::Internal(e.to_string()))
        })
    };

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
        .route("/chat", get(routes::spa_index))
        .route("/webchat", get(webchat::webchat_handler))
        .route("/ws", get(ws::ws_handler))
        .route("/assets/{path}", get(routes::branded_asset))
        .route("/webhook/telegram/{token}", post(routes::telegram_webhook))
        .route("/devices/pair/request", post(routes::device_pair_request))
        .with_state(state.clone());

    let protected_routes = Router::new()
        .route("/gateway/health", get(routes::health))
        .route("/status", get(routes::status))
        .route("/dashboard", get(routes::spa_index))
        .route("/ui", get(routes::spa_index))
        .route("/gateway/start", post(routes::gateway_start))
        .route("/gateway/stop", post(routes::gateway_stop))
        .route("/gateway/restart", post(routes::gateway_restart))
        .route("/api/dashboard/summary", get(routes::dashboard_summary))
        .route("/api/dashboard/models", get(routes::dashboard_models))
        .route("/api/dashboard/sessions", get(routes::dashboard_sessions))
        .route("/api/dashboard/diagnostics", get(routes::dashboard_diagnostics))
        .route("/cron/status", get(routes::cron_status))
        .route("/cron", get(routes::cron_list).post(routes::cron_add))
        .route("/cron/wake", post(routes::cron_wake))
        .route(
            "/cron/{id}",
            get(routes::cron_get)
                .post(routes::cron_update)
                .delete(routes::cron_remove),
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
        .route("/sessions/{id}/tree", get(routes::get_session_tree))
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
        .route("/processes/{id}/kill", post(routes::kill_process))
        .route(
            "/approvals/policies/{tool}",
            get(routes::get_approval_policy)
                .put(routes::set_approval_policy)
                .delete(routes::clear_approval_policy),
        )
        // Agent (subagent) control routes
        .route("/agents/{id}/steer", post(routes::agent_steer))
        .route("/agents/{id}/kill", post(routes::agent_kill))
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
        .route("/skills/{name}", get(routes::get_skill))
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
        // Configure / Setup routes
        .route("/configure", post(routes::configure))
        .route("/setup", post(routes::setup))
        .layer(middleware::from_fn(move |req, next| {
            bearer_auth(req, next, auth_token.clone(), device_registry.clone())
        }))
        .with_state(state);

    let spa_routes = Router::new()
        .route("/", get(routes::spa_handler))
        .fallback(routes::spa_handler);

    // Content-negotiation layer: browser navigation (Accept: text/html)
    // on API paths gets the SPA instead of JSON.
    let content_negotiate = middleware::from_fn(content_negotiate_spa);

    Router::new()
        .merge(public_routes)
        .merge(protected_routes.layer(content_negotiate))
        .merge(spa_routes)
}

/// Content-negotiation middleware: if a browser navigates to an API path
/// (Accept header contains text/html), serve the SPA index instead of JSON.
/// This lets SPA client-side routes like `/sessions`, `/cron`, `/models` work
/// on hard refresh without conflicting with the API handlers.
async fn content_negotiate_spa(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::http::header;

    // Only intercept GET requests (not POST/PUT/DELETE API calls)
    if request.method() != axum::http::Method::GET {
        return next.run(request).await;
    }

    // Skip paths that are clearly API-only (have /api/ prefix or are known API-only paths)
    let path = request.uri().path();
    if path.starts_with("/api/")
        || path.starts_with("/gateway/")
        || path.starts_with("/webhook/")
        || path.starts_with("/devices/")
        || path == "/health"
        || path == "/ws"
    {
        return next.run(request).await;
    }

    // Check if browser is requesting HTML (navigation)
    let accepts_html = request
        .headers()
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("text/html"))
        .unwrap_or(false);

    if accepts_html {
        // Browser navigation — serve SPA index
        return routes::spa_index().await;
    }

    // API/fetch request — pass through to the handler
    next.run(request).await
}

/// Build a [`rustls::ServerConfig`] from PEM-encoded cert chain and private key files.
fn build_rustls_config(
    cert_path: &str,
    key_path: &str,
) -> Result<Arc<rustls::ServerConfig>, GatewayError> {
    use rustls::ServerConfig;
    use rustls_pemfile::{certs, private_key};
    use std::fs::File;
    use std::io::BufReader;

    let cert_file = File::open(cert_path)
        .map_err(|e| GatewayError::Internal(format!("cannot open TLS cert {cert_path}: {e}")))?;
    let key_file = File::open(key_path)
        .map_err(|e| GatewayError::Internal(format!("cannot open TLS key {key_path}: {e}")))?;

    let cert_chain: Vec<_> = certs(&mut BufReader::new(cert_file))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| GatewayError::Internal(format!("invalid TLS certificate: {e}")))?;

    let key = private_key(&mut BufReader::new(key_file))
        .map_err(|e| GatewayError::Internal(format!("invalid TLS private key: {e}")))?
        .ok_or_else(|| GatewayError::Internal("no private key found in key file".into()))?;

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)
        .map_err(|e| GatewayError::Internal(format!("TLS config error: {e}")))?;

    Ok(Arc::new(config))
}

#[cfg(test)]
mod tests {
    use rune_config::TlsConfig;

    #[test]
    fn tls_config_disabled_validates() {
        let cfg = TlsConfig::default();
        assert!(!cfg.enabled);
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn tls_config_enabled_missing_cert() {
        let cfg = TlsConfig {
            enabled: true,
            cert_path: None,
            key_path: Some("/tmp/key.pem".into()),
        };
        let err = cfg.validate().unwrap_err();
        assert!(err.contains("cert_path"));
    }

    #[test]
    fn tls_config_enabled_missing_key() {
        let cfg = TlsConfig {
            enabled: true,
            cert_path: Some("/tmp/cert.pem".into()),
            key_path: None,
        };
        let err = cfg.validate().unwrap_err();
        assert!(err.contains("key_path"));
    }

    #[test]
    fn tls_config_enabled_valid() {
        let cfg = TlsConfig {
            enabled: true,
            cert_path: Some("/tmp/cert.pem".into()),
            key_path: Some("/tmp/key.pem".into()),
        };
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn tls_config_deserializes() {
        let json = serde_json::json!({
            "enabled": true,
            "cert_path": "/etc/rune/tls/cert.pem",
            "key_path": "/etc/rune/tls/key.pem"
        });
        let cfg: TlsConfig = serde_json::from_value(json).unwrap();
        assert!(cfg.enabled);
        assert_eq!(cfg.cert_path.as_deref(), Some("/etc/rune/tls/cert.pem"));
        assert_eq!(cfg.key_path.as_deref(), Some("/etc/rune/tls/key.pem"));
    }

    #[test]
    fn tls_config_defaults_when_omitted() {
        let json = serde_json::json!({});
        let cfg: TlsConfig = serde_json::from_value(json).unwrap();
        assert!(!cfg.enabled);
        assert!(cfg.cert_path.is_none());
        assert!(cfg.key_path.is_none());
    }

    #[test]
    fn build_rustls_config_rejects_missing_files() {
        let result = super::build_rustls_config("/nonexistent/cert.pem", "/nonexistent/key.pem");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("cannot open TLS cert"));
    }
}
