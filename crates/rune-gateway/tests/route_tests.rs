//! Integration tests for rune-gateway HTTP route handlers.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::Value;
use tokio::sync::{Mutex, broadcast};
use tower::ServiceExt;
use uuid::Uuid;

use rune_config::{AppConfig, ConfiguredModel, ModelProviderConfig};
use rune_models::{
    CompletionRequest, CompletionResponse, FinishReason, ModelError, ModelProvider, Usage,
};
use rune_runtime::{
    CompactionStrategy, ContextAssembler, NoOpCompaction, SessionEngine, TurnExecutor,
    heartbeat::HeartbeatRunner,
    scheduler::{ReminderStore, Scheduler},
};
use rune_store::StoreError;
use rune_store::models::*;
use rune_store::repos::*;
use rune_tools::{ToolCall, ToolError, ToolExecutor, ToolRegistry, ToolResult};

use rune_gateway::{AppState, SessionEvent, build_router};

// ── In-memory repos ───────────────────────────────────────────────────────────

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
            metadata: s.metadata,
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

    async fn find_by_channel_ref(
        &self,
        channel_ref: &str,
    ) -> Result<Option<SessionRow>, StoreError> {
        let sessions = self.sessions.lock().await;
        Ok(sessions
            .iter()
            .rev()
            .find(|s| s.channel_ref.as_deref() == Some(channel_ref))
            .cloned())
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
        Ok(session.clone())
    }

    async fn update_metadata(
        &self,
        id: Uuid,
        metadata: serde_json::Value,
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
        session.metadata = metadata;
        session.updated_at = updated_at;
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

// ── Fake model provider ───────────────────────────────────────────────────────

#[derive(Debug)]
struct FakeModelProvider;

#[async_trait]
impl ModelProvider for FakeModelProvider {
    async fn complete(
        &self,
        _request: &CompletionRequest,
    ) -> Result<CompletionResponse, ModelError> {
        Ok(CompletionResponse {
            content: Some("Hello from fake model!".to_string()),
            usage: Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            },
            finish_reason: Some(FinishReason::Stop),
            tool_calls: vec![],
        })
    }
}

// ── Fake tool approval policy repo ─────────────────────────────────────────────

struct MemToolApprovalPolicyRepo;

#[async_trait]
impl ToolApprovalPolicyRepo for MemToolApprovalPolicyRepo {
    async fn list_policies(
        &self,
    ) -> Result<Vec<rune_store::repos::ToolApprovalPolicy>, StoreError> {
        Ok(vec![])
    }
    async fn get_policy(
        &self,
        _tool_name: &str,
    ) -> Result<Option<rune_store::repos::ToolApprovalPolicy>, StoreError> {
        Ok(None)
    }
    async fn set_policy(
        &self,
        tool_name: &str,
        decision: &str,
    ) -> Result<rune_store::repos::ToolApprovalPolicy, StoreError> {
        Ok(rune_store::repos::ToolApprovalPolicy {
            tool_name: tool_name.to_string(),
            decision: decision.to_string(),
            decided_at: chrono::Utc::now(),
        })
    }
    async fn clear_policy(&self, _tool_name: &str) -> Result<bool, StoreError> {
        Ok(false)
    }
}

// ── Fake tool executor ────────────────────────────────────────────────────────

#[derive(Debug)]
struct FakeToolExecutor;

#[async_trait]
impl ToolExecutor for FakeToolExecutor {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        Ok(ToolResult {
            tool_call_id: call.tool_call_id,
            output: "fake tool output".to_string(),
            is_error: false,
        })
    }
}

// ── Test harness ──────────────────────────────────────────────────────────────

const TEST_AUTH_TOKEN: &str = "test-secret-token";

fn build_test_app(auth_token: Option<String>) -> axum::Router {
    build_test_app_with_config(AppConfig::default(), auth_token)
}

fn build_test_app_with_config(mut config: AppConfig, auth_token: Option<String>) -> axum::Router {
    let session_repo = Arc::new(MemSessionRepo::new());
    let turn_repo = Arc::new(MemTurnRepo::new());
    let transcript_repo = Arc::new(MemTranscriptRepo::new());
    let model_provider: Arc<dyn ModelProvider> = Arc::new(FakeModelProvider);
    let scheduler = Arc::new(Scheduler::new());

    let session_engine = Arc::new(SessionEngine::new(session_repo.clone()));

    let workspace_root = std::env::temp_dir().join(format!("rune-gw-test-{}", Uuid::now_v7()));
    std::fs::create_dir_all(workspace_root.join("memory")).unwrap();
    std::fs::write(workspace_root.join("AGENTS.md"), "# Test workspace").unwrap();

    let context_assembler = ContextAssembler::new("You are a test assistant.");
    let compaction: Arc<dyn CompactionStrategy> = Arc::new(NoOpCompaction);
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(FakeToolExecutor);
    let tool_registry = Arc::new(ToolRegistry::new());

    let turn_executor = Arc::new(
        TurnExecutor::new(
            session_repo.clone() as Arc<dyn SessionRepo>,
            turn_repo as Arc<dyn TurnRepo>,
            transcript_repo.clone() as Arc<dyn TranscriptRepo>,
            model_provider.clone(),
            tool_executor,
            tool_registry,
            context_assembler,
            compaction,
        )
        .with_default_model("fake-model"),
    );

    let (event_tx, _) = broadcast::channel::<SessionEvent>(64);

    config.gateway.auth_token = auth_token.clone();

    let state = AppState {
        config: Arc::new(config),
        started_at: Arc::new(Instant::now()),
        session_engine,
        turn_executor,
        session_repo: session_repo as Arc<dyn SessionRepo>,
        transcript_repo: transcript_repo as Arc<dyn TranscriptRepo>,
        model_provider,
        scheduler,
        heartbeat: Arc::new(HeartbeatRunner::new(std::env::temp_dir())),
        reminder_store: Arc::new(ReminderStore::new()),
        tool_approval_repo: Arc::new(MemToolApprovalPolicyRepo) as Arc<dyn ToolApprovalPolicyRepo>,
        tool_count: 0,
        event_tx,
    };

    build_router(state, auth_token)
}

async fn body_json(response: axum::http::Response<Body>) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn body_text(response: axum::http::Response<Body>) -> String {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn health_returns_200() {
    let app = build_test_app(None);
    let response = app
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let json = body_json(response).await;
    assert_eq!(json["status"], "ok");
    assert_eq!(json["service"], "rune-gateway");
}

#[tokio::test]
async fn status_returns_correct_shape() {
    let app = build_test_app(None);
    let response = app
        .oneshot(Request::get("/status").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let json = body_json(response).await;
    assert_eq!(json["status"], "running");
    assert!(json["version"].is_string());
    assert!(json["config_paths"].is_object());
    assert!(json["uptime_seconds"].is_number());
    assert!(json["registered_tools"].is_number());
    assert!(json["session_count"].is_number());
}

#[tokio::test]
async fn dashboard_html_requires_auth_when_enabled() {
    let app = build_test_app(Some(TEST_AUTH_TOKEN.to_string()));
    let response = app
        .oneshot(Request::get("/dashboard").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn dashboard_html_renders() {
    let app = build_test_app(None);
    let response = app
        .oneshot(Request::get("/dashboard").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_text(response).await;
    assert!(body.contains("Rune Operator Dashboard"));
    assert!(body.contains("/api/dashboard/summary"));
}

#[tokio::test]
async fn dashboard_summary_reports_runtime_counts() {
    let mut config = AppConfig::default();
    config.models.default_model = Some("openai/gpt-4.1-mini".to_string());
    config.models.providers.push(ModelProviderConfig {
        name: "openai".to_string(),
        kind: "openai".to_string(),
        base_url: "https://api.openai.example".to_string(),
        api_key: None,
        deployment_name: None,
        api_version: None,
        api_key_env: None,
        model_alias: None,
        models: vec![ConfiguredModel::Id("gpt-4.1-mini".to_string())],
    });

    let app = build_test_app_with_config(config, None);
    let response = app
        .oneshot(
            Request::get("/api/dashboard/summary")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["gateway_status"], "running");
    assert_eq!(json["default_model"], "openai/gpt-4.1-mini");
    assert_eq!(json["provider_count"], 1);
    assert_eq!(json["configured_model_count"], 1);
}

#[tokio::test]
async fn dashboard_models_lists_provider_inventory() {
    let mut config = AppConfig::default();
    config.models.default_model = Some("openai/gpt-4.1-mini".to_string());
    config.models.providers.push(ModelProviderConfig {
        name: "openai".to_string(),
        kind: "openai".to_string(),
        base_url: "https://api.openai.example".to_string(),
        api_key: None,
        deployment_name: None,
        api_version: None,
        api_key_env: None,
        model_alias: None,
        models: vec![
            ConfiguredModel::Id("gpt-4.1-mini".to_string()),
            ConfiguredModel::Id("gpt-4.1".to_string()),
        ],
    });

    let app = build_test_app_with_config(config, None);
    let response = app
        .oneshot(
            Request::get("/api/dashboard/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    let items = json.as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["provider_name"], "openai");
    assert!(items.iter().any(|item| item["is_default"] == true));
}

#[tokio::test]
async fn dashboard_diagnostics_falls_back_to_status_notes() {
    let app = build_test_app(None);
    let response = app
        .oneshot(
            Request::get("/api/dashboard/diagnostics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["structured_errors_available"], false);
    let items = json["items"].as_array().unwrap();
    assert!(!items.is_empty());
}

#[tokio::test]
async fn dashboard_sessions_includes_kind_and_activity_fields() {
    let app = build_test_app(None);

    let response = app
        .clone()
        .oneshot(
            Request::post("/sessions")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"kind":"channel","channel_ref":"telegram:ops"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let response = app
        .oneshot(
            Request::get("/api/dashboard/sessions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    let items = json.as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["kind"], "channel");
    assert_eq!(items[0]["channel_ref"], "telegram:ops");
    assert!(items[0]["last_activity_at"].is_string());
}

#[tokio::test]
async fn auth_rejection_on_missing_token() {
    let app = build_test_app(Some(TEST_AUTH_TOKEN.to_string()));

    // Protected route without auth header
    let response = app
        .oneshot(Request::get("/status").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let json = body_json(response).await;
    assert_eq!(json["code"], "unauthorized");
}

#[tokio::test]
async fn auth_rejection_on_bad_token() {
    let app = build_test_app(Some(TEST_AUTH_TOKEN.to_string()));

    let response = app
        .oneshot(
            Request::get("/status")
                .header(header::AUTHORIZATION, "Bearer wrong-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_accepts_valid_token() {
    let app = build_test_app(Some(TEST_AUTH_TOKEN.to_string()));

    let response = app
        .oneshot(
            Request::get("/status")
                .header(header::AUTHORIZATION, format!("Bearer {TEST_AUTH_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn health_is_public_even_with_auth() {
    let app = build_test_app(Some(TEST_AUTH_TOKEN.to_string()));

    let response = app
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn create_session_returns_201() {
    let app = build_test_app(None);

    let response = app
        .oneshot(
            Request::post("/sessions")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"kind":"direct"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let json = body_json(response).await;
    assert!(json["id"].is_string());
    assert_eq!(json["kind"], "direct");
    assert_eq!(json["status"], "created");
}

#[tokio::test]
async fn send_message_and_transcript_with_shared_state() {
    let session_repo = Arc::new(MemSessionRepo::new());
    let turn_repo = Arc::new(MemTurnRepo::new());
    let transcript_repo = Arc::new(MemTranscriptRepo::new());
    let model_provider: Arc<dyn ModelProvider> = Arc::new(FakeModelProvider);
    let scheduler = Arc::new(Scheduler::new());
    let session_engine = Arc::new(SessionEngine::new(session_repo.clone()));

    let workspace_root = std::env::temp_dir().join(format!("rune-gw-test-{}", Uuid::now_v7()));
    std::fs::create_dir_all(workspace_root.join("memory")).unwrap();
    std::fs::write(workspace_root.join("AGENTS.md"), "# Test").unwrap();

    let context_assembler = ContextAssembler::new("You are a test assistant.");
    let compaction: Arc<dyn CompactionStrategy> = Arc::new(NoOpCompaction);
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(FakeToolExecutor);
    let tool_registry = Arc::new(ToolRegistry::new());

    let turn_executor = Arc::new(
        TurnExecutor::new(
            session_repo.clone() as Arc<dyn SessionRepo>,
            turn_repo as Arc<dyn TurnRepo>,
            transcript_repo.clone() as Arc<dyn TranscriptRepo>,
            model_provider.clone(),
            tool_executor,
            tool_registry,
            context_assembler,
            compaction,
        )
        .with_default_model("fake-model"),
    );

    let (event_tx, _) = broadcast::channel::<SessionEvent>(64);

    let state = AppState {
        config: Arc::new(AppConfig::default()),
        started_at: Arc::new(Instant::now()),
        session_engine,
        turn_executor,
        session_repo: session_repo as Arc<dyn SessionRepo>,
        transcript_repo: transcript_repo as Arc<dyn TranscriptRepo>,
        model_provider,
        scheduler,
        heartbeat: Arc::new(HeartbeatRunner::new(std::env::temp_dir())),
        reminder_store: Arc::new(ReminderStore::new()),
        tool_approval_repo: Arc::new(MemToolApprovalPolicyRepo) as Arc<dyn ToolApprovalPolicyRepo>,
        tool_count: 0,
        event_tx,
    };

    let app = build_router(state, None);

    // 1. Create session
    let response = app
        .clone()
        .oneshot(
            Request::post("/sessions")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"kind":"direct"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let session_json = body_json(response).await;
    let session_id = session_json["id"].as_str().unwrap();

    // 2. Send message
    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/sessions/{session_id}/messages"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"content":"Hello"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let msg_json = body_json(response).await;
    assert!(msg_json["turn_id"].is_string());
    assert_eq!(msg_json["assistant_reply"], "Hello from fake model!");
    assert!(msg_json["latency_ms"].is_number());
    assert_eq!(msg_json["usage"]["prompt_tokens"], 10);
    assert_eq!(msg_json["usage"]["completion_tokens"], 5);

    // 3. Get transcript — should have items ordered by seq
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/sessions/{session_id}/transcript"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let transcript: Vec<Value> =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();

    // Should have at least user_message and assistant_message
    assert!(
        transcript.len() >= 2,
        "expected >=2 transcript items, got {}",
        transcript.len()
    );

    // Verify ordering: seq values should be ascending
    let seqs: Vec<i64> = transcript
        .iter()
        .map(|t| t["seq"].as_i64().unwrap())
        .collect();
    for window in seqs.windows(2) {
        assert!(
            window[0] <= window[1],
            "transcript not ordered by seq: {seqs:?}"
        );
    }

    // First item should be user message
    assert_eq!(transcript[0]["kind"], "user_message");
}

#[tokio::test]
async fn get_session_not_found() {
    let app = build_test_app(None);
    let fake_id = Uuid::now_v7();
    let response = app
        .oneshot(
            Request::get(format!("/sessions/{fake_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn send_message_session_not_found() {
    let app = build_test_app(None);
    let fake_id = Uuid::now_v7();
    let response = app
        .oneshot(
            Request::post(format!("/sessions/{fake_id}/messages"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"content":"Hello"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn transcript_session_not_found() {
    let app = build_test_app(None);
    let fake_id = Uuid::now_v7();
    let response = app
        .oneshot(
            Request::get(format!("/sessions/{fake_id}/transcript"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn list_sessions_empty() {
    let app = build_test_app(None);
    let response = app
        .oneshot(Request::get("/sessions").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert!(json.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn list_sessions_filters_by_channel_and_activity() {
    let session_repo = Arc::new(MemSessionRepo::new());
    let now = chrono::Utc::now();

    session_repo
        .create(NewSession {
            id: Uuid::now_v7(),
            kind: "channel".into(),
            status: "running".into(),
            workspace_root: None,
            channel_ref: Some("telegram".into()),
            requester_session_id: None,
            metadata: serde_json::json!({}),
            created_at: now,
            updated_at: now,
            last_activity_at: now,
        })
        .await
        .unwrap();

    session_repo
        .create(NewSession {
            id: Uuid::now_v7(),
            kind: "channel".into(),
            status: "running".into(),
            workspace_root: None,
            channel_ref: Some("discord".into()),
            requester_session_id: None,
            metadata: serde_json::json!({}),
            created_at: now - chrono::Duration::minutes(90),
            updated_at: now - chrono::Duration::minutes(90),
            last_activity_at: now - chrono::Duration::minutes(90),
        })
        .await
        .unwrap();

    let turn_repo = Arc::new(MemTurnRepo::new());
    let transcript_repo = Arc::new(MemTranscriptRepo::new());
    let model_provider: Arc<dyn ModelProvider> = Arc::new(FakeModelProvider);
    let scheduler = Arc::new(Scheduler::new());
    let session_engine = Arc::new(SessionEngine::new(session_repo.clone()));
    let context_assembler = ContextAssembler::new("You are a test assistant.");
    let compaction: Arc<dyn CompactionStrategy> = Arc::new(NoOpCompaction);
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(FakeToolExecutor);
    let tool_registry = Arc::new(ToolRegistry::new());
    let turn_executor = Arc::new(
        TurnExecutor::new(
            session_repo.clone() as Arc<dyn SessionRepo>,
            turn_repo as Arc<dyn TurnRepo>,
            transcript_repo.clone() as Arc<dyn TranscriptRepo>,
            model_provider.clone(),
            tool_executor,
            tool_registry,
            context_assembler,
            compaction,
        )
        .with_default_model("fake-model"),
    );
    let (event_tx, _) = broadcast::channel::<SessionEvent>(64);
    let state = AppState {
        config: Arc::new(AppConfig::default()),
        started_at: Arc::new(Instant::now()),
        session_engine,
        turn_executor,
        session_repo: session_repo as Arc<dyn SessionRepo>,
        transcript_repo: transcript_repo as Arc<dyn TranscriptRepo>,
        model_provider,
        scheduler,
        heartbeat: Arc::new(HeartbeatRunner::new(std::env::temp_dir())),
        reminder_store: Arc::new(ReminderStore::new()),
        tool_approval_repo: Arc::new(MemToolApprovalPolicyRepo) as Arc<dyn ToolApprovalPolicyRepo>,
        tool_count: 0,
        event_tx,
    };

    let app = build_router(state, None);
    let response = app
        .oneshot(
            Request::get("/sessions?channel=telegram&active=30&limit=10")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    let items = json.as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["channel"], "telegram");
}
