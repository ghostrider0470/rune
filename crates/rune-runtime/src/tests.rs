use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;
use uuid::Uuid;

use rune_core::{SessionKind, ToolCategory};
use rune_models::{
    CompletionRequest, CompletionResponse, FinishReason, FunctionCall, ModelError, ModelProvider,
    ToolCallRequest, Usage,
};
use rune_store::StoreError;
use rune_store::models::*;
use rune_store::repos::*;
use rune_tools::{
    ToolCall, ToolDefinition as RtToolDefinition, ToolError, ToolExecutor, ToolRegistry, ToolResult,
};

use crate::compaction::NoOpCompaction;
use crate::context::ContextAssembler;
use crate::engine::SessionEngine;
use crate::executor::TurnExecutor;

// ── Fake model provider ───────────────────────────────────────────────

/// A model provider that returns pre-configured responses in order.
#[derive(Debug)]
struct FakeModelProvider {
    responses: Mutex<Vec<CompletionResponse>>,
}

impl FakeModelProvider {
    fn new(responses: Vec<CompletionResponse>) -> Self {
        Self {
            responses: Mutex::new(responses),
        }
    }

    fn text_response(content: &str) -> CompletionResponse {
        CompletionResponse {
            content: Some(content.to_string()),
            usage: Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            },
            finish_reason: Some(FinishReason::Stop),
            tool_calls: vec![],
        }
    }

    fn tool_call_response(tool_name: &str, args: &str) -> CompletionResponse {
        CompletionResponse {
            content: None,
            usage: Usage {
                prompt_tokens: 10,
                completion_tokens: 8,
                total_tokens: 18,
            },
            finish_reason: Some(FinishReason::ToolCalls),
            tool_calls: vec![ToolCallRequest {
                id: Uuid::now_v7().to_string(),
                call_type: "function".to_string(),
                function: FunctionCall {
                    name: tool_name.to_string(),
                    arguments: args.to_string(),
                },
            }],
        }
    }
}

#[async_trait]
impl ModelProvider for FakeModelProvider {
    async fn complete(
        &self,
        _request: &CompletionRequest,
    ) -> Result<CompletionResponse, ModelError> {
        let mut responses = self.responses.lock().await;
        if responses.is_empty() {
            return Err(ModelError::Provider("no more fake responses".into()));
        }
        Ok(responses.remove(0))
    }
}

/// A model provider that always fails.
#[derive(Debug)]
struct FailingModelProvider;

#[async_trait]
impl ModelProvider for FailingModelProvider {
    async fn complete(
        &self,
        _request: &CompletionRequest,
    ) -> Result<CompletionResponse, ModelError> {
        Err(ModelError::Transient("fake transient error".into()))
    }
}

// ── Fake tool executor ────────────────────────────────────────────────

struct FakeToolExecutor {
    responses: Mutex<Vec<String>>,
}

impl FakeToolExecutor {
    fn new(responses: Vec<String>) -> Self {
        Self {
            responses: Mutex::new(responses),
        }
    }
}

#[async_trait]
impl ToolExecutor for FakeToolExecutor {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let mut responses = self.responses.lock().await;
        let output = if responses.is_empty() {
            "default tool output".to_string()
        } else {
            responses.remove(0)
        };
        Ok(ToolResult {
            tool_call_id: call.tool_call_id,
            output,
            is_error: false,
        })
    }
}

// ── In-memory repos ───────────────────────────────────────────────────

struct MemSessionRepo {
    sessions: Mutex<Vec<SessionRow>>,
}

impl MemSessionRepo {
    fn new() -> Self {
        Self {
            sessions: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl SessionRepo for MemSessionRepo {
    async fn create(&self, s: NewSession) -> Result<SessionRow, StoreError> {
        let row = SessionRow {
            id: s.id,
            kind: s.kind,
            status: s.status,
            workspace_root: s.workspace_root,
            channel_ref: s.channel_ref,
            requester_session_id: s.requester_session_id,
            created_at: s.created_at,
            updated_at: s.updated_at,
            last_activity_at: s.last_activity_at,
        };
        self.sessions.lock().await.push(row.clone());
        Ok(row)
    }

    async fn find_by_id(&self, id: Uuid) -> Result<SessionRow, StoreError> {
        self.sessions
            .lock()
            .await
            .iter()
            .find(|s| s.id == id)
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
            .find(|s| s.id == id)
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

struct MemTurnRepo {
    turns: Mutex<Vec<TurnRow>>,
}

impl MemTurnRepo {
    fn new() -> Self {
        Self {
            turns: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl TurnRepo for MemTurnRepo {
    async fn create(&self, t: NewTurn) -> Result<TurnRow, StoreError> {
        let row = TurnRow {
            id: t.id,
            session_id: t.session_id,
            trigger_kind: t.trigger_kind,
            status: t.status,
            model_ref: t.model_ref,
            started_at: t.started_at,
            ended_at: t.ended_at,
            usage_prompt_tokens: t.usage_prompt_tokens,
            usage_completion_tokens: t.usage_completion_tokens,
        };
        self.turns.lock().await.push(row.clone());
        Ok(row)
    }

    async fn find_by_id(&self, id: Uuid) -> Result<TurnRow, StoreError> {
        self.turns
            .lock()
            .await
            .iter()
            .find(|t| t.id == id)
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
            .filter(|t| t.session_id == session_id)
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
            .find(|t| t.id == id)
            .ok_or(StoreError::NotFound {
                entity: "turn",
                id: id.to_string(),
            })?;
        turn.status = status.to_string();
        if ended_at.is_some() {
            turn.ended_at = ended_at;
        }
        Ok(turn.clone())
    }
}

struct MemTranscriptRepo {
    items: Mutex<Vec<TranscriptItemRow>>,
}

impl MemTranscriptRepo {
    fn new() -> Self {
        Self {
            items: Mutex::new(Vec::new()),
        }
    }
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

    async fn list_by_session(
        &self,
        session_id: Uuid,
    ) -> Result<Vec<TranscriptItemRow>, StoreError> {
        let items = self.items.lock().await;
        let mut result: Vec<_> = items
            .iter()
            .filter(|i| i.session_id == session_id)
            .cloned()
            .collect();
        result.sort_by_key(|i| i.seq);
        Ok(result)
    }
}

// ── Helper to build a TurnExecutor with fakes ─────────────────────────

struct TestHarness {
    session_repo: Arc<MemSessionRepo>,
    turn_repo: Arc<MemTurnRepo>,
    transcript_repo: Arc<MemTranscriptRepo>,
}

impl TestHarness {
    fn new() -> Self {
        Self {
            session_repo: Arc::new(MemSessionRepo::new()),
            turn_repo: Arc::new(MemTurnRepo::new()),
            transcript_repo: Arc::new(MemTranscriptRepo::new()),
        }
    }

    fn turn_executor(
        &self,
        model: Arc<dyn ModelProvider>,
        tool_executor: Arc<dyn ToolExecutor>,
        tool_registry: ToolRegistry,
    ) -> TurnExecutor {
        TurnExecutor::new(
            self.turn_repo.clone(),
            self.transcript_repo.clone(),
            model,
            tool_executor,
            Arc::new(tool_registry),
            ContextAssembler::new("You are a helpful assistant."),
            Arc::new(NoOpCompaction),
        )
    }

    fn session_engine(&self) -> SessionEngine {
        SessionEngine::new(self.session_repo.clone())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn full_turn_cycle_no_tools() {
    let h = TestHarness::new();
    let engine = h.session_engine();

    // Create session
    let session = engine
        .create_session(SessionKind::Direct, None)
        .await
        .unwrap();
    assert_eq!(session.status, "created");

    // Mark ready, then running
    engine.mark_ready(session.id).await.unwrap();
    engine.mark_running(session.id).await.unwrap();

    // Execute a turn
    let model = Arc::new(FakeModelProvider::new(vec![
        FakeModelProvider::text_response("Hello! How can I help?"),
    ]));
    let executor = h.turn_executor(
        model,
        Arc::new(FakeToolExecutor::new(vec![])),
        ToolRegistry::new(),
    );

    let (turn, usage) = executor
        .execute(session.id, "Hi there", None)
        .await
        .unwrap();

    assert_eq!(turn.status, "completed");
    assert!(turn.ended_at.is_some());
    assert_eq!(usage.model_calls, 1);
    assert_eq!(usage.prompt_tokens, 10);
    assert_eq!(usage.completion_tokens, 5);

    // Verify transcript ordering
    let transcript = h.transcript_repo.list_by_session(session.id).await.unwrap();
    assert_eq!(transcript.len(), 2);
    assert_eq!(transcript[0].kind, "user_message");
    assert_eq!(transcript[0].seq, 0);
    assert_eq!(transcript[1].kind, "assistant_message");
    assert_eq!(transcript[1].seq, 1);
}

#[tokio::test]
async fn tool_loop_with_multi_step_calls() {
    let h = TestHarness::new();
    let engine = h.session_engine();
    let session = engine
        .create_session(SessionKind::Direct, None)
        .await
        .unwrap();
    engine.mark_ready(session.id).await.unwrap();
    engine.mark_running(session.id).await.unwrap();

    // Model: tool call → tool call → final text
    let model = Arc::new(FakeModelProvider::new(vec![
        FakeModelProvider::tool_call_response("read_file", r#"{"path":"/tmp/a.txt"}"#),
        FakeModelProvider::tool_call_response("read_file", r#"{"path":"/tmp/b.txt"}"#),
        FakeModelProvider::text_response("I read both files."),
    ]));

    let mut registry = ToolRegistry::new();
    registry.register(RtToolDefinition {
        name: "read_file".to_string(),
        description: "Read a file".to_string(),
        parameters: serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        category: ToolCategory::FileRead,
        requires_approval: false,
    });

    let tool_exec = Arc::new(FakeToolExecutor::new(vec![
        "contents of a.txt".to_string(),
        "contents of b.txt".to_string(),
    ]));

    let executor = h.turn_executor(model, tool_exec, registry);
    let (turn, usage) = executor
        .execute(session.id, "Read both files", None)
        .await
        .unwrap();

    assert_eq!(turn.status, "completed");
    assert_eq!(usage.model_calls, 3);

    // Transcript: user, tool_req, tool_result, tool_req, tool_result, assistant
    let transcript = h.transcript_repo.list_by_session(session.id).await.unwrap();
    assert_eq!(transcript.len(), 6);
    let kinds: Vec<&str> = transcript.iter().map(|t| t.kind.as_str()).collect();
    assert_eq!(
        kinds,
        [
            "user_message",
            "tool_request",
            "tool_result",
            "tool_request",
            "tool_result",
            "assistant_message"
        ]
    );

    // Verify sequence ordering
    for (i, item) in transcript.iter().enumerate() {
        assert_eq!(item.seq, i as i32);
    }
}

#[tokio::test]
async fn failed_model_call_sets_turn_failed() {
    let h = TestHarness::new();
    let engine = h.session_engine();
    let session = engine
        .create_session(SessionKind::Direct, None)
        .await
        .unwrap();
    engine.mark_ready(session.id).await.unwrap();
    engine.mark_running(session.id).await.unwrap();

    let model: Arc<dyn ModelProvider> = Arc::new(FailingModelProvider);
    let executor = h.turn_executor(
        model,
        Arc::new(FakeToolExecutor::new(vec![])),
        ToolRegistry::new(),
    );

    let result = executor.execute(session.id, "Hello", None).await;
    assert!(result.is_err());

    // Verify the turn was persisted with failed status
    let turns = h.turn_repo.list_by_session(session.id).await.unwrap();
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].status, "failed");
    assert!(turns[0].ended_at.is_some());
}

#[tokio::test]
async fn session_status_transitions() {
    let h = TestHarness::new();
    let engine = h.session_engine();

    let session = engine
        .create_session(SessionKind::Direct, None)
        .await
        .unwrap();
    assert_eq!(session.status, "created");

    let session = engine.mark_ready(session.id).await.unwrap();
    assert_eq!(session.status, "ready");

    let session = engine.mark_running(session.id).await.unwrap();
    assert_eq!(session.status, "running");

    let session = engine.mark_completed(session.id).await.unwrap();
    assert_eq!(session.status, "completed");
}

#[tokio::test]
async fn invalid_session_transition_rejected() {
    let h = TestHarness::new();
    let engine = h.session_engine();

    let session = engine
        .create_session(SessionKind::Direct, None)
        .await
        .unwrap();

    // Can't go created → running (must go through ready)
    let err = engine.mark_running(session.id).await.unwrap_err();
    assert!(
        err.to_string().contains("expected ready, got created"),
        "got: {err}"
    );
}

#[tokio::test]
async fn max_tool_iterations_enforced() {
    let h = TestHarness::new();
    let engine = h.session_engine();
    let session = engine
        .create_session(SessionKind::Direct, None)
        .await
        .unwrap();
    engine.mark_ready(session.id).await.unwrap();
    engine.mark_running(session.id).await.unwrap();

    // Model always returns tool calls — should hit the limit
    let mut responses = Vec::new();
    for _ in 0..30 {
        responses.push(FakeModelProvider::tool_call_response(
            "read_file",
            r#"{"path":"x"}"#,
        ));
    }
    let model = Arc::new(FakeModelProvider::new(responses));

    let mut registry = ToolRegistry::new();
    registry.register(RtToolDefinition {
        name: "read_file".to_string(),
        description: "Read".to_string(),
        parameters: serde_json::json!({}),
        category: ToolCategory::FileRead,
        requires_approval: false,
    });

    let executor = h
        .turn_executor(model, Arc::new(FakeToolExecutor::new(vec![])), registry)
        .with_max_tool_iterations(3);

    let result = executor.execute(session.id, "loop", None).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("max tool iterations (3)"),
        "got: {err}"
    );
}

#[tokio::test]
async fn session_not_found_returns_error() {
    let h = TestHarness::new();
    let engine = h.session_engine();

    let err = engine.get_session(Uuid::now_v7()).await.unwrap_err();
    assert!(err.to_string().contains("session not found"));
}

#[tokio::test]
async fn usage_tracking_accumulates_across_tool_loop() {
    let h = TestHarness::new();
    let engine = h.session_engine();
    let session = engine
        .create_session(SessionKind::Direct, None)
        .await
        .unwrap();
    engine.mark_ready(session.id).await.unwrap();
    engine.mark_running(session.id).await.unwrap();

    let model = Arc::new(FakeModelProvider::new(vec![
        FakeModelProvider::tool_call_response("read_file", r#"{"path":"x"}"#),
        FakeModelProvider::text_response("Done"),
    ]));

    let mut registry = ToolRegistry::new();
    registry.register(RtToolDefinition {
        name: "read_file".to_string(),
        description: "Read".to_string(),
        parameters: serde_json::json!({}),
        category: ToolCategory::FileRead,
        requires_approval: false,
    });

    let executor = h.turn_executor(model, Arc::new(FakeToolExecutor::new(vec![])), registry);
    let (_, usage) = executor.execute(session.id, "read", None).await.unwrap();

    assert_eq!(usage.model_calls, 2);
    assert_eq!(usage.prompt_tokens, 20); // 10 + 10
    assert_eq!(usage.completion_tokens, 13); // 8 + 5
    assert_eq!(usage.total_tokens, 33); // 18 + 15
}
