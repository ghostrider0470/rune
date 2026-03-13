//! Thin gateway binary — loads config and starts the Rune gateway daemon.
//!
//! This keeps a zero-config runnable path alive during the rewrite by wiring the
//! existing runtime stack with in-memory repos and a canned model provider.
//! It is intentionally transitional: PostgreSQL-backed service wiring remains
//! the release-target path, but the binary now actually boots and serves the
//! current HTTP/WS surface instead of exiting immediately.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::Mutex;
use uuid::Uuid;

use rune_config::AppConfig;
use rune_gateway::{init_logging, start, Services};
use rune_models::{CompletionRequest, CompletionResponse, FinishReason, ModelError, ModelProvider, Usage};
use rune_runtime::{ContextAssembler, NoOpCompaction, SessionEngine, TurnExecutor};
use rune_store::models::{NewSession, NewTranscriptItem, NewTurn, SessionRow, TranscriptItemRow, TurnRow};
use rune_store::repos::{SessionRepo, TranscriptRepo, TurnRepo};
use rune_store::StoreError;
use rune_tools::{register_builtin_stubs, StubExecutor, ToolRegistry};

#[tokio::main]
async fn main() -> Result<()> {
    let config_path = std::env::var("RUNE_CONFIG").ok();
    let config = AppConfig::load(config_path.as_deref())?;
    init_logging(&config.logging);

    let session_repo: Arc<dyn SessionRepo> = Arc::new(MemSessionRepo::default());
    let turn_repo: Arc<dyn TurnRepo> = Arc::new(MemTurnRepo::default());
    let transcript_repo: Arc<dyn TranscriptRepo> = Arc::new(MemTranscriptRepo::default());

    let session_engine = Arc::new(SessionEngine::new(session_repo.clone()));

    let mut registry = ToolRegistry::new();
    register_builtin_stubs(&mut registry);
    let tool_registry = Arc::new(registry);

    let model_provider: Arc<dyn ModelProvider> = Arc::new(EchoModelProvider);
    let tool_executor = Arc::new(StubExecutor);
    let context_assembler = ContextAssembler::new(
        "You are Rune, an OpenClaw-compatible runtime in active parity implementation.",
    );
    let compaction = Arc::new(NoOpCompaction);

    let turn_executor = Arc::new(TurnExecutor::new(
        turn_repo,
        transcript_repo.clone(),
        model_provider.clone(),
        tool_executor,
        tool_registry,
        context_assembler,
        compaction,
    ));

    let handle = start(Services {
        config,
        session_engine,
        turn_executor,
        session_repo,
        transcript_repo,
        model_provider,
    })
    .await?;

    handle.wait().await.map_err(anyhow::Error::from)
}

#[derive(Debug)]
struct EchoModelProvider;

#[async_trait]
impl ModelProvider for EchoModelProvider {
    async fn complete(&self, request: &CompletionRequest) -> Result<CompletionResponse, ModelError> {
        let last_user = request
            .messages
            .iter()
            .rev()
            .find(|m| matches!(m.role, rune_models::Role::User))
            .and_then(|m| m.content.clone())
            .unwrap_or_else(|| "(no user input)".to_string());

        Ok(CompletionResponse {
            content: Some(format!("Echo: {last_user}")),
            usage: Usage {
                prompt_tokens: request.messages.len() as u32 * 8,
                completion_tokens: 6,
                total_tokens: request.messages.len() as u32 * 8 + 6,
            },
            finish_reason: Some(FinishReason::Stop),
            tool_calls: Vec::new(),
        })
    }
}

#[derive(Default)]
struct MemSessionRepo {
    sessions: Mutex<Vec<SessionRow>>,
}

#[async_trait]
impl SessionRepo for MemSessionRepo {
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
        Ok(sessions
            .iter()
            .skip(offset as usize)
            .take(limit as usize)
            .cloned()
            .collect())
    }

    async fn update_status(
        &self,
        id: Uuid,
        status: &str,
        updated_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<SessionRow, StoreError> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
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

#[derive(Default)]
struct MemTurnRepo {
    turns: Mutex<Vec<TurnRow>>,
}

#[async_trait]
impl TurnRepo for MemTurnRepo {
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
        ended_at: Option<chrono::DateTime<chrono::Utc>>,
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
        if let Some(ended_at) = ended_at {
            turn.ended_at = Some(ended_at);
        }
        Ok(turn.clone())
    }
}

#[derive(Default)]
struct MemTranscriptRepo {
    items: Mutex<Vec<TranscriptItemRow>>,
}

#[async_trait]
impl TranscriptRepo for MemTranscriptRepo {
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

    async fn list_by_session(&self, session_id: Uuid) -> Result<Vec<TranscriptItemRow>, StoreError> {
        let mut rows: Vec<_> = self
            .items
            .lock()
            .await
            .iter()
            .filter(|item| item.session_id == session_id)
            .cloned()
            .collect();
        rows.sort_by_key(|item| item.seq);
        Ok(rows)
    }
}
