//! Thin gateway binary — loads config and starts the Rune gateway daemon.
//!
//! When `database.database_url` is configured, uses PostgreSQL-backed
//! repositories. Otherwise falls back to in-memory repos for zero-config
//! local development.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use tokio::signal;
use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;

use rune_config::AppConfig;
use rune_gateway::{Services, init_logging, start};
use rune_models::{
    CompletionRequest, CompletionResponse, FinishReason, ModelError, ModelProvider, Usage,
};
use rune_runtime::{
    ContextAssembler, NoOpCompaction, SessionEngine, TurnExecutor, scheduler::Scheduler,
};
use rune_store::StoreError;
use rune_store::models::{
    NewSession, NewTranscriptItem, NewTurn, SessionRow, TranscriptItemRow, TurnRow,
};
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

    let services = build_services(config)?;
    let handle = start(services).await.context("failed to start gateway")?;

    shutdown_signal().await;
    info!("shutdown signal received");
    handle.shutdown();

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
        "postgres"
    } else {
        "in-memory"
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

fn build_services(config: AppConfig) -> Result<Services> {
    let (session_repo, turn_repo, transcript_repo): (
        Arc<dyn SessionRepo>,
        Arc<dyn TurnRepo>,
        Arc<dyn TranscriptRepo>,
    ) = if let Some(ref database_url) = config.database.database_url {
        info!("using PostgreSQL persistence");
        if config.database.run_migrations {
            info!("running pending database migrations");
            rune_store::pool::run_migrations(database_url)?;
        }
        let pool =
            rune_store::pool::create_pool(database_url, config.database.max_connections as usize)?;
        (
            Arc::new(rune_store::pg::PgSessionRepo::new(pool.clone())),
            Arc::new(rune_store::pg::PgTurnRepo::new(pool.clone())),
            Arc::new(rune_store::pg::PgTranscriptRepo::new(pool)),
        )
    } else {
        warn!("no DATABASE_URL configured — using in-memory persistence");
        (
            Arc::new(InMemorySessionRepo::default()),
            Arc::new(InMemoryTurnRepo::default()),
            Arc::new(InMemoryTranscriptRepo::default()),
        )
    };

    let session_engine = Arc::new(SessionEngine::new(session_repo.clone()));

    let model_provider: Arc<dyn ModelProvider> = Arc::new(EchoModelProvider::new());
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

    Ok(Services {
        config,
        session_engine,
        turn_executor,
        session_repo,
        transcript_repo,
        model_provider,
        scheduler,
        tool_count,
    })
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

#[derive(Debug, Default)]
struct InMemorySessionRepo {
    sessions: Mutex<Vec<SessionRow>>,
}

#[async_trait]
impl SessionRepo for InMemorySessionRepo {
    async fn create(&self, session: NewSession) -> Result<SessionRow, StoreError> {
        let row = SessionRow {
            id: session.id,
            kind: session.kind,
            status: session.status,
            workspace_root: session.workspace_root,
            channel_ref: session.channel_ref,
            requester_session_id: session.requester_session_id,
            created_at: session.created_at,
            updated_at: session.updated_at,
            last_activity_at: session.last_activity_at,
        };
        self.sessions.lock().await.push(row.clone());
        Ok(row)
    }

    async fn find_by_id(&self, id: Uuid) -> Result<SessionRow, StoreError> {
        self.sessions
            .lock()
            .await
            .iter()
            .find(|session| session.id == id)
            .cloned()
            .ok_or(StoreError::NotFound {
                entity: "session",
                id: id.to_string(),
            })
    }

    async fn list(&self, limit: i64, offset: i64) -> Result<Vec<SessionRow>, StoreError> {
        let sessions = self.sessions.lock().await;
        let start = offset.max(0) as usize;
        let end = (start + limit.max(0) as usize).min(sessions.len());
        Ok(sessions[start..end].iter().rev().cloned().collect())
    }

    async fn update_status(
        &self,
        id: Uuid,
        status: &str,
        updated_at: chrono::DateTime<Utc>,
    ) -> Result<SessionRow, StoreError> {
        let mut sessions = self.sessions.lock().await;
        let session =
            sessions
                .iter_mut()
                .find(|session| session.id == id)
                .ok_or(StoreError::NotFound {
                    entity: "session",
                    id: id.to_string(),
                })?;
        session.status = status.to_string();
        session.updated_at = updated_at;
        session.last_activity_at = updated_at;
        Ok(session.clone())
    }
}

#[derive(Debug, Default)]
struct InMemoryTurnRepo {
    turns: Mutex<Vec<TurnRow>>,
}

#[async_trait]
impl TurnRepo for InMemoryTurnRepo {
    async fn create(&self, turn: NewTurn) -> Result<TurnRow, StoreError> {
        let row = TurnRow {
            id: turn.id,
            session_id: turn.session_id,
            trigger_kind: turn.trigger_kind,
            status: turn.status,
            model_ref: turn.model_ref,
            started_at: turn.started_at,
            ended_at: turn.ended_at,
            usage_prompt_tokens: turn.usage_prompt_tokens,
            usage_completion_tokens: turn.usage_completion_tokens,
        };
        self.turns.lock().await.push(row.clone());
        Ok(row)
    }

    async fn find_by_id(&self, id: Uuid) -> Result<TurnRow, StoreError> {
        self.turns
            .lock()
            .await
            .iter()
            .find(|turn| turn.id == id)
            .cloned()
            .ok_or(StoreError::NotFound {
                entity: "turn",
                id: id.to_string(),
            })
    }

    async fn list_by_session(&self, session_id: Uuid) -> Result<Vec<TurnRow>, StoreError> {
        Ok(self
            .turns
            .lock()
            .await
            .iter()
            .filter(|turn| turn.session_id == session_id)
            .cloned()
            .collect())
    }

    async fn update_status(
        &self,
        id: Uuid,
        status: &str,
        ended_at: Option<chrono::DateTime<Utc>>,
    ) -> Result<TurnRow, StoreError> {
        let mut turns = self.turns.lock().await;
        let turn = turns
            .iter_mut()
            .find(|turn| turn.id == id)
            .ok_or(StoreError::NotFound {
                entity: "turn",
                id: id.to_string(),
            })?;
        turn.status = status.to_string();
        turn.ended_at = ended_at;
        Ok(turn.clone())
    }
}

#[derive(Debug, Default)]
struct InMemoryTranscriptRepo {
    items: Mutex<Vec<TranscriptItemRow>>,
}

#[async_trait]
impl TranscriptRepo for InMemoryTranscriptRepo {
    async fn append(&self, item: NewTranscriptItem) -> Result<TranscriptItemRow, StoreError> {
        let row = TranscriptItemRow {
            id: item.id,
            session_id: item.session_id,
            turn_id: item.turn_id,
            seq: item.seq,
            kind: item.kind,
            payload: item.payload,
            created_at: item.created_at,
        };
        self.items.lock().await.push(row.clone());
        Ok(row)
    }

    async fn list_by_session(
        &self,
        session_id: Uuid,
    ) -> Result<Vec<TranscriptItemRow>, StoreError> {
        let mut items: Vec<_> = self
            .items
            .lock()
            .await
            .iter()
            .filter(|item| item.session_id == session_id)
            .cloned()
            .collect();
        items.sort_by_key(|item| item.seq);
        Ok(items)
    }
}

#[derive(Debug)]
struct EchoModelProvider;

impl EchoModelProvider {
    const fn new() -> Self {
        Self
    }
}

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
