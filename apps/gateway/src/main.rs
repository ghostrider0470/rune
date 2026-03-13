//! Thin gateway binary — loads config and starts the Rune gateway daemon.
//!
//! When `database.database_url` is configured, connects to the external
//! PostgreSQL instance. Otherwise bootstraps an embedded PostgreSQL
//! server via `postgresql_embedded` for zero-config local development.
//! Data in both cases is durable and persisted to disk.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::signal;
use tracing::{info, warn};

use rune_config::AppConfig;
use rune_gateway::{Services, init_logging, start};
use rune_models::{
    CompletionRequest, CompletionResponse, FinishReason, ModelError, ModelProvider, Usage,
};
use rune_runtime::{
    ContextAssembler, NoOpCompaction, SessionEngine, TurnExecutor, scheduler::Scheduler,
};
use rune_store::EmbeddedPg;
use rune_store::repos::{SessionRepo, TranscriptRepo, TurnRepo};
use rune_tools::{StubExecutor, ToolRegistry, register_builtin_stubs};

#[tokio::main]
async fn main() -> Result<()> {
    let config_path = discover_config_path();
    let config = AppConfig::load(config_path.as_deref()).with_context(|| {
        format!(
            "failed to load config from {}",
            display_config_path(config_path.as_deref())
        )
    })?;

    init_logging(&config.logging);
    emit_startup_banner(&config, config_path.as_deref());

    if let Err(errors) = config.validate_paths() {
        for error in errors {
            warn!(error = %error, "path validation warning");
        }
    }

    let (services, _embedded_pg) = build_services(config).await?;
    let handle = start(services).await.context("failed to start gateway")?;

    shutdown_signal().await;
    info!("shutdown signal received");
    handle.shutdown();

    // Explicitly stop embedded PG if it was started (also happens on drop,
    // but this lets us log any errors).
    if let Some(epg) = _embedded_pg {
        if let Err(e) = epg.stop().await {
            warn!(error = %e, "error stopping embedded PostgreSQL");
        }
    }

    Ok(())
}

fn discover_config_path() -> Option<PathBuf> {
    std::env::args_os()
        .skip(1)
        .collect::<Vec<_>>()
        .windows(2)
        .find_map(|window| {
            if window[0] == "--config" {
                Some(PathBuf::from(&window[1]))
            } else {
                None
            }
        })
        .or_else(|| std::env::var_os("RUNE_CONFIG").map(PathBuf::from))
}

fn display_config_path(path: Option<&Path>) -> String {
    path.map(|p| p.display().to_string())
        .unwrap_or_else(|| "<defaults+env>".to_string())
}

fn emit_startup_banner(config: &AppConfig, config_path: Option<&Path>) {
    let store_backend = if config.database.database_url.is_some() {
        "postgres (external)"
    } else {
        "postgres (embedded)"
    };

    info!(
        version = env!("CARGO_PKG_VERSION"),
        host = %config.gateway.host,
        port = config.gateway.port,
        config_path = %display_config_path(config_path),
        store_backend,
        model_backend = if config.models.providers.is_empty() { "demo-echo" } else { "configured-provider-or-demo-fallback" },
        "starting rune gateway"
    );
}

/// Build all services, returning an optional `EmbeddedPg` handle that
/// must be kept alive for the lifetime of the process.
async fn build_services(config: AppConfig) -> Result<(Services, Option<EmbeddedPg>)> {
    // Resolve the database URL — either from config or by starting embedded PG.
    let (database_url, embedded_pg) = if let Some(ref url) = config.database.database_url {
        info!("using external PostgreSQL");
        (url.clone(), None)
    } else {
        info!("no DATABASE_URL configured — starting embedded PostgreSQL");
        let epg = EmbeddedPg::start(&config.paths.db_dir, "rune")
            .await
            .context("failed to start embedded PostgreSQL")?;
        let url = epg.database_url().to_owned();
        (url, Some(epg))
    };

    // Run migrations.
    if config.database.run_migrations {
        info!("running pending database migrations");
        rune_store::pool::run_migrations(&database_url)?;
    }

    // Build connection pool.
    let pool = rune_store::pool::create_pool(
        &database_url,
        config.database.max_connections as usize,
    )?;

    let session_repo: Arc<dyn SessionRepo> =
        Arc::new(rune_store::pg::PgSessionRepo::new(pool.clone()));
    let turn_repo: Arc<dyn TurnRepo> =
        Arc::new(rune_store::pg::PgTurnRepo::new(pool.clone()));
    let transcript_repo: Arc<dyn TranscriptRepo> =
        Arc::new(rune_store::pg::PgTranscriptRepo::new(pool));

    let session_engine = Arc::new(SessionEngine::new(session_repo.clone()));

    let model_provider: Arc<dyn ModelProvider> = Arc::new(EchoModelProvider);
    let scheduler = Arc::new(Scheduler::new());

    let mut registry = ToolRegistry::new();
    register_builtin_stubs(&mut registry);
    let tool_count = registry.len();
    let tool_registry = Arc::new(registry);
    let tool_executor = Arc::new(StubExecutor);

    let turn_executor = Arc::new(TurnExecutor::new(
        turn_repo,
        transcript_repo.clone(),
        model_provider.clone(),
        tool_executor,
        tool_registry,
        ContextAssembler::new(
            "You are Rune, the Rust gateway runtime for OpenClaw parity testing.",
        ),
        Arc::new(NoOpCompaction),
    ));

    let services = Services {
        config,
        session_engine,
        turn_executor,
        session_repo,
        transcript_repo,
        model_provider,
        scheduler,
        tool_count,
    };

    Ok((services, embedded_pg))
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(mut stream) => {
                let _ = stream.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

/// Demo model provider that echoes back user messages.
/// Used when no real model providers are configured.
#[derive(Debug)]
struct EchoModelProvider;

#[async_trait]
impl ModelProvider for EchoModelProvider {
    async fn complete(
        &self,
        request: &CompletionRequest,
    ) -> Result<CompletionResponse, ModelError> {
        let latest_user = request
            .messages
            .iter()
            .rev()
            .find(|message| matches!(message.role, rune_models::Role::User))
            .and_then(|message| message.content.as_deref())
            .unwrap_or("(empty message)");

        Ok(CompletionResponse {
            content: Some(format!("Echo: {latest_user}")),
            usage: Usage {
                prompt_tokens: latest_user.len() as u32,
                completion_tokens: (latest_user.len() as u32) + 6,
                total_tokens: (latest_user.len() as u32) * 2 + 6,
            },
            finish_reason: Some(FinishReason::Stop),
            tool_calls: Vec::new(),
        })
    }
}
