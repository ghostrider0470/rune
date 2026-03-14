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

use rune_config::AppConfig;
use rune_models::ModelProvider;
use rune_runtime::{
    SessionEngine, TurnExecutor,
    heartbeat::HeartbeatRunner,
    scheduler::{ReminderStore, Scheduler},
};
use rune_store::repos::{
    ApprovalRepo, SessionRepo, ToolApprovalPolicyRepo, TranscriptRepo, TurnRepo,
};

use crate::auth::bearer_auth;
use crate::error::GatewayError;
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
    pub tool_count: usize,
}

/// Start the gateway HTTP server.
///
/// Returns a [`GatewayHandle`] that can be used to await completion or trigger shutdown.
pub async fn start(services: Services) -> Result<GatewayHandle, GatewayError> {
    let (event_tx, _) = broadcast::channel::<SessionEvent>(256);

    let auth_token = services.config.gateway.auth_token.clone();
    let addr: SocketAddr = format!(
        "{}:{}",
        services.config.gateway.host, services.config.gateway.port
    )
    .parse()
    .map_err(|e: std::net::AddrParseError| GatewayError::Internal(e.to_string()))?;

    let state = AppState {
        config: Arc::new(services.config),
        started_at: Arc::new(Instant::now()),
        session_engine: services.session_engine,
        turn_executor: services.turn_executor,
        session_repo: services.session_repo,
        transcript_repo: services.transcript_repo,
        turn_repo: services.turn_repo,
        model_provider: services.model_provider,
        scheduler: services.scheduler,
        heartbeat: services.heartbeat,
        reminder_store: services.reminder_store,
        approval_repo: services.approval_repo,
        tool_approval_repo: services.tool_approval_repo,
        tool_count: services.tool_count,
        event_tx,
    };

    let supervisor_deps = SupervisorDeps {
        heartbeat: state.heartbeat.clone(),
        scheduler: state.scheduler.clone(),
        reminder_store: state.reminder_store.clone(),
        session_engine: state.session_engine.clone(),
        turn_executor: state.turn_executor.clone(),
        workspace_root: state.config.agents.defaults.workspace.clone(),
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
    let public_routes = Router::new()
        .route("/health", get(routes::health))
        .route("/ws", get(ws::ws_handler))
        .route("/webhook/telegram/{token}", post(routes::telegram_webhook))
        .with_state(state.clone());

    let protected_routes = Router::new()
        .route("/status", get(routes::status))
        .route("/dashboard", get(routes::dashboard_page))
        .route("/ui", get(routes::dashboard_page))
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
        .route("/sessions/{id}", get(routes::get_session))
        .route("/sessions/{id}/status", get(routes::get_session_status))
        .route("/sessions/{id}/messages", post(routes::send_message))
        .route("/sessions/{id}/transcript", get(routes::get_transcript))
        .route(
            "/approvals",
            get(routes::list_pending_approvals).post(routes::submit_approval_decision),
        )
        .route("/approvals/policies", get(routes::list_approval_policies))
        .route(
            "/approvals/policies/{tool}",
            get(routes::get_approval_policy)
                .put(routes::set_approval_policy)
                .delete(routes::clear_approval_policy),
        )
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
        .layer(middleware::from_fn(move |req, next| {
            bearer_auth(req, next, auth_token.clone())
        }))
        .with_state(state);

    Router::new().merge(public_routes).merge(protected_routes)
}
