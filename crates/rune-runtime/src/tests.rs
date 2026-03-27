use std::sync::Arc;

use async_trait::async_trait;
use std::path::PathBuf;

use tokio::sync::Mutex;
use uuid::Uuid;

use rune_core::{SessionKind, SessionStatus, ToolCategory};
use rune_models::{
    CompletionRequest, CompletionResponse, FinishReason, FunctionCall, ModelError, ModelProvider,
    ToolCallRequest, Usage,
};
use rune_store::StoreError;
use rune_store::models::*;
use rune_store::repos::*;
use rune_tools::{
    ToolCall, ToolDefinition as RtToolDefinition, ToolError, ToolExecutor, ToolRegistry,
    ToolResult,
    approval::{ApprovalRequest, ApprovalScope, RiskLevel},
};

use crate::compaction::NoOpCompaction;
use crate::context::ContextAssembler;
use crate::engine::SessionEngine;
use crate::executor::TurnExecutor;
use crate::skill::{Skill, SkillRegistry};

#[derive(Debug)]
struct FakeModelProvider {
    responses: Mutex<Vec<CompletionResponse>>,
    requests: Mutex<Vec<CompletionRequest>>,
}

impl FakeModelProvider {
    fn new(responses: Vec<CompletionResponse>) -> Self {
        Self {
            responses: Mutex::new(responses),
            requests: Mutex::new(Vec::new()),
        }
    }

    async fn requests(&self) -> Vec<CompletionRequest> {
        self.requests.lock().await.clone()
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
        request: &CompletionRequest,
    ) -> Result<CompletionResponse, ModelError> {
        self.requests.lock().await.push(request.clone());
        let mut responses = self.responses.lock().await;
        if responses.is_empty() {
            return Err(ModelError::Provider("no more fake responses".into()));
        }
        Ok(responses.remove(0))
    }
}

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

#[derive(Debug)]
struct FailAfterFirstCallModelProvider {
    calls: Mutex<usize>,
}

impl FailAfterFirstCallModelProvider {
    fn new() -> Self {
        Self {
            calls: Mutex::new(0),
        }
    }
}

#[async_trait]
impl ModelProvider for FailAfterFirstCallModelProvider {
    async fn complete(
        &self,
        _request: &CompletionRequest,
    ) -> Result<CompletionResponse, ModelError> {
        let mut calls = self.calls.lock().await;
        *calls += 1;
        if *calls == 1 {
            Ok(FakeModelProvider::tool_call_response(
                "exec",
                r#"{"command":"echo hi"}"#,
            ))
        } else {
            Err(ModelError::Transient(
                "fake transient error after approval resume".into(),
            ))
        }
    }
}

enum FakeToolStep {
    Output(String),
    OutputWithExecutionId {
        output: String,
        tool_execution_id: Uuid,
    },
    Error(ToolError),
    ApprovalRequired {
        tool: String,
        details: String,
    },
}

struct FakeToolExecutor {
    responses: Mutex<Vec<FakeToolStep>>,
}

impl FakeToolExecutor {
    fn new(responses: Vec<String>) -> Self {
        Self {
            responses: Mutex::new(
                responses
                    .into_iter()
                    .map(FakeToolStep::Output)
                    .collect::<Vec<_>>(),
            ),
        }
    }

    fn with_steps(steps: Vec<FakeToolStep>) -> Self {
        Self {
            responses: Mutex::new(steps),
        }
    }
}

#[async_trait]
impl ToolExecutor for FakeToolExecutor {
    async fn execute(&self, call: ToolCall) -> Result<ToolResult, ToolError> {
        let mut responses = self.responses.lock().await;
        let step = if responses.is_empty() {
            FakeToolStep::Output("default tool output".to_string())
        } else {
            responses.remove(0)
        };
        match step {
            FakeToolStep::Output(output) => Ok(ToolResult {
                tool_call_id: call.tool_call_id,
                output,
                is_error: false,
                tool_execution_id: None,
            }),
            FakeToolStep::OutputWithExecutionId {
                output,
                tool_execution_id,
            } => Ok(ToolResult {
                tool_call_id: call.tool_call_id,
                output,
                is_error: false,
                tool_execution_id: Some(tool_execution_id),
            }),
            FakeToolStep::Error(error) => Err(error),
            FakeToolStep::ApprovalRequired { tool, details } => {
                Err(ToolError::ApprovalRequired { tool, details })
            }
        }
    }
}

struct MemSessionRepo {
    sessions: Mutex<Vec<SessionRow>>,
}

impl MemSessionRepo {
    fn new() -> Self {
        Self {
            sessions: Mutex::new(Vec::new()),
        }
    }

    async fn set_metadata(&self, id: Uuid, metadata: serde_json::Value) {
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.iter_mut().find(|session| session.id == id) {
            session.metadata = metadata;
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
            runtime_profile: s.runtime_profile,
            policy_profile: s.policy_profile,
            latest_turn_id: s.latest_turn_id,
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
        let terminal = ["completed", "failed", "cancelled"];
        Ok(sessions
            .iter()
            .rev()
            .find(|s| {
                s.channel_ref.as_deref() == Some(channel_ref)
                    && !terminal.contains(&s.status.as_str())
            })
            .cloned())
    }

    async fn update_status(
        &self,
        id: Uuid,
        status: &str,
        updated_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<SessionRow, StoreError> {
        let target: SessionStatus = status
            .parse()
            .map_err(|e: rune_core::CoreError| StoreError::InvalidTransition(e.to_string()))?;

        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or(StoreError::NotFound {
                entity: "session",
                id: id.to_string(),
            })?;

        let current: SessionStatus = session
            .status
            .parse()
            .map_err(|e: rune_core::CoreError| StoreError::InvalidTransition(e.to_string()))?;
        current
            .transition(target)
            .map_err(|e| StoreError::InvalidTransition(e.to_string()))?;

        session.status = status.to_string();
        session.updated_at = updated_at;
        session.last_activity_at = updated_at;
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
        session.last_activity_at = updated_at;
        Ok(session.clone())
    }

    async fn update_latest_turn(
        &self,
        id: Uuid,
        turn_id: Uuid,
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
        session.latest_turn_id = Some(turn_id);
        session.updated_at = updated_at;
        session.last_activity_at = updated_at;
        Ok(session.clone())
    }

    async fn delete(&self, id: Uuid) -> Result<bool, StoreError> {
        let mut sessions = self.sessions.lock().await;
        let before = sessions.len();
        sessions.retain(|session| session.id != id);
        Ok(sessions.len() != before)
    }

    async fn list_active_channel_sessions(&self) -> Result<Vec<SessionRow>, StoreError> {
        let sessions = self.sessions.lock().await;
        let terminal = ["completed", "failed", "cancelled"];
        Ok(sessions
            .iter()
            .filter(|s| {
                s.kind == "Channel"
                    && s.channel_ref.is_some()
                    && !terminal.contains(&s.status.as_str())
            })
            .cloned()
            .collect())
    }

    async fn mark_stale_completed(&self, _stale_secs: i64) -> Result<u64, StoreError> {
        Ok(0)
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

    async fn update_usage(
        &self,
        id: Uuid,
        prompt_tokens: i32,
        completion_tokens: i32,
    ) -> Result<TurnRow, StoreError> {
        let mut turns = self.turns.lock().await;
        let turn = turns
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or(StoreError::NotFound {
                entity: "turn",
                id: id.to_string(),
            })?;
        turn.usage_prompt_tokens = Some(prompt_tokens);
        turn.usage_completion_tokens = Some(completion_tokens);
        Ok(turn.clone())
    }

    async fn mark_stale_failed(&self, _stale_secs: i64) -> Result<u64, StoreError> {
        Ok(0)
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

    async fn delete_by_session(&self, session_id: Uuid) -> Result<usize, StoreError> {
        let mut items = self.items.lock().await;
        let before = items.len();
        items.retain(|item| item.session_id != session_id);
        Ok(before - items.len())
    }
}

struct MemApprovalRepo {
    approvals: Mutex<Vec<ApprovalRow>>,
}

impl MemApprovalRepo {
    fn new() -> Self {
        Self {
            approvals: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl ApprovalRepo for MemApprovalRepo {
    async fn create(&self, approval: NewApproval) -> Result<ApprovalRow, StoreError> {
        let row = ApprovalRow {
            id: approval.id,
            subject_type: approval.subject_type,
            subject_id: approval.subject_id,
            reason: approval.reason,
            decision: None,
            decided_by: None,
            decided_at: None,
            presented_payload: approval.presented_payload,
            created_at: approval.created_at,
            handle_ref: approval.handle_ref,
            host_ref: approval.host_ref,
        };
        self.approvals.lock().await.push(row.clone());
        Ok(row)
    }

    async fn find_by_id(&self, id: Uuid) -> Result<ApprovalRow, StoreError> {
        self.approvals
            .lock()
            .await
            .iter()
            .find(|approval| approval.id == id)
            .cloned()
            .ok_or(StoreError::NotFound {
                entity: "approval",
                id: id.to_string(),
            })
    }

    async fn list(&self, pending_only: bool) -> Result<Vec<ApprovalRow>, StoreError> {
        let approvals = self.approvals.lock().await;
        Ok(approvals
            .iter()
            .filter(|approval| !pending_only || approval.decision.is_none())
            .cloned()
            .collect())
    }

    async fn decide(
        &self,
        id: Uuid,
        decision: &str,
        decided_by: &str,
        decided_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<ApprovalRow, StoreError> {
        let mut approvals = self.approvals.lock().await;
        let approval = approvals
            .iter_mut()
            .find(|approval| approval.id == id)
            .ok_or(StoreError::NotFound {
                entity: "approval",
                id: id.to_string(),
            })?;
        approval.decision = Some(decision.to_string());
        approval.decided_by = Some(decided_by.to_string());
        approval.decided_at = Some(decided_at);
        Ok(approval.clone())
    }

    async fn update_presented_payload(
        &self,
        id: Uuid,
        presented_payload: serde_json::Value,
    ) -> Result<ApprovalRow, StoreError> {
        let mut approvals = self.approvals.lock().await;
        let approval = approvals
            .iter_mut()
            .find(|approval| approval.id == id)
            .ok_or(StoreError::NotFound {
                entity: "approval",
                id: id.to_string(),
            })?;
        approval.presented_payload = presented_payload;
        Ok(approval.clone())
    }
}

struct MemToolApprovalPolicyRepo {
    policies: Mutex<Vec<ToolApprovalPolicy>>,
}

impl MemToolApprovalPolicyRepo {
    fn new() -> Self {
        Self {
            policies: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl ToolApprovalPolicyRepo for MemToolApprovalPolicyRepo {
    async fn list_policies(&self) -> Result<Vec<ToolApprovalPolicy>, StoreError> {
        Ok(self.policies.lock().await.clone())
    }

    async fn get_policy(&self, tool_name: &str) -> Result<Option<ToolApprovalPolicy>, StoreError> {
        let policies = self.policies.lock().await;
        Ok(policies.iter().find(|p| p.tool_name == tool_name).cloned())
    }

    async fn set_policy(
        &self,
        tool_name: &str,
        decision: &str,
    ) -> Result<ToolApprovalPolicy, StoreError> {
        let mut policies = self.policies.lock().await;
        policies.retain(|p| p.tool_name != tool_name);
        let policy = ToolApprovalPolicy {
            tool_name: tool_name.to_string(),
            decision: decision.to_string(),
            decided_at: chrono::Utc::now(),
        };
        policies.push(policy.clone());
        Ok(policy)
    }

    async fn clear_policy(&self, tool_name: &str) -> Result<bool, StoreError> {
        let mut policies = self.policies.lock().await;
        let before = policies.len();
        policies.retain(|p| p.tool_name != tool_name);
        Ok(policies.len() < before)
    }
}

struct TestHarness {
    session_repo: Arc<MemSessionRepo>,
    turn_repo: Arc<MemTurnRepo>,
    transcript_repo: Arc<MemTranscriptRepo>,
    approval_repo: Arc<MemApprovalRepo>,
    workspace_root: PathBuf,
}

impl TestHarness {
    fn new() -> Self {
        let workspace_root =
            std::env::temp_dir().join(format!("rune-runtime-test-{}", Uuid::now_v7()));
        std::fs::create_dir_all(workspace_root.join("memory")).unwrap();
        std::fs::write(
            workspace_root.join("AGENTS.md"),
            "# AGENTS\nWorkspace rules.",
        )
        .unwrap();
        std::fs::write(workspace_root.join("SOUL.md"), "# SOUL\nBe sharp.").unwrap();
        std::fs::write(workspace_root.join("USER.md"), "# USER\nHamza").unwrap();
        std::fs::write(
            workspace_root.join("MEMORY.md"),
            "# MEMORY\nLong-term fact.",
        )
        .unwrap();
        let today = chrono::Utc::now().date_naive();
        std::fs::write(
            workspace_root.join(format!("memory/{}.md", today.format("%Y-%m-%d"))),
            "# Today\nRuntime note.",
        )
        .unwrap();

        Self {
            session_repo: Arc::new(MemSessionRepo::new()),
            turn_repo: Arc::new(MemTurnRepo::new()),
            transcript_repo: Arc::new(MemTranscriptRepo::new()),
            approval_repo: Arc::new(MemApprovalRepo::new()),
            workspace_root,
        }
    }

    fn turn_executor(
        &self,
        model: Arc<dyn ModelProvider>,
        tool_executor: Arc<dyn ToolExecutor>,
        tool_registry: ToolRegistry,
    ) -> TurnExecutor {
        self.turn_executor_with_skill_registry(model, tool_executor, tool_registry, None)
    }

    fn turn_executor_with_skill_registry(
        &self,
        model: Arc<dyn ModelProvider>,
        tool_executor: Arc<dyn ToolExecutor>,
        tool_registry: ToolRegistry,
        skill_registry: Option<Arc<SkillRegistry>>,
    ) -> TurnExecutor {
        let executor = TurnExecutor::new(
            self.session_repo.clone(),
            self.turn_repo.clone(),
            self.transcript_repo.clone(),
            self.approval_repo.clone(),
            model,
            tool_executor,
            Arc::new(tool_registry),
            ContextAssembler::new("You are a helpful assistant."),
            Arc::new(NoOpCompaction),
        );

        if let Some(skill_registry) = skill_registry {
            executor.with_skill_registry(skill_registry)
        } else {
            executor
        }
    }

    fn turn_executor_with_policy_repo(
        &self,
        model: Arc<dyn ModelProvider>,
        tool_executor: Arc<dyn ToolExecutor>,
        tool_registry: ToolRegistry,
        policy_repo: Arc<dyn ToolApprovalPolicyRepo>,
    ) -> TurnExecutor {
        TurnExecutor::new(
            self.session_repo.clone(),
            self.turn_repo.clone(),
            self.transcript_repo.clone(),
            self.approval_repo.clone(),
            model,
            tool_executor,
            Arc::new(tool_registry),
            ContextAssembler::new("You are a helpful assistant."),
            Arc::new(NoOpCompaction),
        )
        .with_tool_approval_policy_repo(policy_repo)
    }

    fn session_engine(&self) -> SessionEngine {
        SessionEngine::new(self.session_repo.clone())
            .with_transcript_repo(self.transcript_repo.clone())
    }
}

#[tokio::test]
async fn full_turn_cycle_no_tools() {
    let h = TestHarness::new();
    let engine = h.session_engine();

    let session = engine
        .create_session(
            SessionKind::Direct,
            Some(h.workspace_root.to_string_lossy().to_string()),
        )
        .await
        .unwrap();
    // Auto-transitions created → ready on creation.
    assert_eq!(session.status, "ready");

    engine.mark_ready(session.id).await.unwrap(); // idempotent
    engine.mark_running(session.id).await.unwrap();

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

    let persisted_turn = h.turn_repo.find_by_id(turn.id).await.unwrap();
    assert_eq!(persisted_turn.usage_prompt_tokens, Some(10));
    assert_eq!(persisted_turn.usage_completion_tokens, Some(5));

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
        .create_session(
            SessionKind::Direct,
            Some(h.workspace_root.to_string_lossy().to_string()),
        )
        .await
        .unwrap();
    engine.mark_ready(session.id).await.unwrap();
    engine.mark_running(session.id).await.unwrap();

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

    for (i, item) in transcript.iter().enumerate() {
        assert_eq!(item.seq, i as i32);
    }
}

#[tokio::test]
async fn failed_model_call_sets_turn_failed() {
    let h = TestHarness::new();
    let engine = h.session_engine();
    let session = engine
        .create_session(
            SessionKind::Direct,
            Some(h.workspace_root.to_string_lossy().to_string()),
        )
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
        .create_session(
            SessionKind::Direct,
            Some(h.workspace_root.to_string_lossy().to_string()),
        )
        .await
        .unwrap();
    // Auto-transitions created → ready on creation.
    assert_eq!(session.status, "ready");

    let session = engine.mark_ready(session.id).await.unwrap(); // idempotent
    assert_eq!(session.status, "ready");

    let session = engine.mark_running(session.id).await.unwrap();
    assert_eq!(session.status, "running");

    let session = engine.mark_completed(session.id).await.unwrap();
    assert_eq!(session.status, "completed");
}

#[tokio::test]
async fn patch_metadata_merges_and_removes_null_fields() {
    let h = TestHarness::new();
    let engine = h.session_engine();

    let session = engine
        .create_session(
            SessionKind::Direct,
            Some(h.workspace_root.to_string_lossy().to_string()),
        )
        .await
        .unwrap();

    let patched = engine
        .patch_metadata(
            session.id,
            serde_json::json!({
                "label": "ops",
                "nested": { "a": 1, "b": 2 }
            }),
        )
        .await
        .unwrap();
    assert_eq!(patched.metadata["label"], "ops");
    assert_eq!(patched.metadata["nested"]["a"], 1);

    let patched = engine
        .patch_metadata(
            session.id,
            serde_json::json!({
                "nested": { "b": null, "c": 3 }
            }),
        )
        .await
        .unwrap();
    assert!(patched.metadata["nested"].get("b").is_none());
    assert_eq!(patched.metadata["nested"]["c"], 3);
}

#[tokio::test]
async fn delete_session_removes_session_and_transcript() {
    let h = TestHarness::new();
    let engine = h.session_engine();

    let session = engine
        .create_session(
            SessionKind::Direct,
            Some(h.workspace_root.to_string_lossy().to_string()),
        )
        .await
        .unwrap();

    h.transcript_repo
        .append(NewTranscriptItem {
            id: Uuid::now_v7(),
            session_id: session.id,
            turn_id: None,
            seq: 0,
            kind: "status_note".to_string(),
            payload: serde_json::json!({"message": "hello"}),
            created_at: chrono::Utc::now(),
        })
        .await
        .unwrap();

    engine.delete_session(session.id).await.unwrap();
    assert!(engine.get_session(session.id).await.is_err());
    let transcript = h.transcript_repo.list_by_session(session.id).await.unwrap();
    assert!(transcript.is_empty());
}

#[tokio::test]
async fn mark_running_allows_resuming_from_waiting_states() {
    let h = TestHarness::new();
    let engine = h.session_engine();

    for status in [
        "waiting_for_tool",
        "waiting_for_approval",
        "waiting_for_subagent",
    ] {
        let session = engine
            .create_session(
                SessionKind::Direct,
                Some(h.workspace_root.to_string_lossy().to_string()),
            )
            .await
            .unwrap();
        engine.mark_running(session.id).await.unwrap();
        h.session_repo
            .update_status(session.id, status, chrono::Utc::now())
            .await
            .unwrap();

        let resumed = engine.mark_running(session.id).await.unwrap();
        assert_eq!(resumed.status, "running", "failed for status {status}");
    }
}

#[tokio::test]
async fn ready_session_can_transition_to_terminal_states() {
    let h = TestHarness::new();
    let engine = h.session_engine();

    for (target, expected) in [
        (SessionStatus::Completed, "completed"),
        (SessionStatus::Failed, "failed"),
        (SessionStatus::Cancelled, "cancelled"),
    ] {
        let session = engine
            .create_session(
                SessionKind::Direct,
                Some(h.workspace_root.to_string_lossy().to_string()),
            )
            .await
            .unwrap();
        assert_eq!(session.status, "ready");

        let updated = match target {
            SessionStatus::Completed => engine.mark_completed(session.id).await.unwrap(),
            SessionStatus::Failed => engine.mark_failed(session.id).await.unwrap(),
            SessionStatus::Cancelled => h
                .session_repo
                .update_status(session.id, "cancelled", chrono::Utc::now())
                .await
                .unwrap(),
            _ => unreachable!(),
        };

        assert_eq!(updated.status, expected);
    }
}

#[tokio::test]
async fn invalid_session_transition_rejected() {
    let h = TestHarness::new();
    let engine = h.session_engine();

    let session = engine
        .create_session(
            SessionKind::Direct,
            Some(h.workspace_root.to_string_lossy().to_string()),
        )
        .await
        .unwrap();
    assert_eq!(session.status, "ready");

    let err = engine.mark_running(session.id).await.unwrap();
    assert_eq!(err.status, "running");

    let completed = engine.mark_completed(session.id).await.unwrap();
    assert_eq!(completed.status, "completed");

    let err = engine.mark_running(session.id).await.unwrap_err();
    assert!(err.to_string().contains("running"), "got: {err}");
}

#[tokio::test]
async fn approval_wait_transition_requires_running_session() {
    let h = TestHarness::new();
    let session = h
        .session_engine()
        .create_session(
            SessionKind::Direct,
            Some(h.workspace_root.to_string_lossy().to_string()),
        )
        .await
        .unwrap();

    let err = h
        .session_repo
        .update_status(session.id, "waiting_for_approval", chrono::Utc::now())
        .await
        .unwrap_err();

    assert!(err.to_string().contains("invalid transition"), "got: {err}");
}

#[tokio::test]
async fn approval_wait_transition_allowed_from_running_session() {
    let h = TestHarness::new();
    let engine = h.session_engine();
    let session = engine
        .create_session(
            SessionKind::Direct,
            Some(h.workspace_root.to_string_lossy().to_string()),
        )
        .await
        .unwrap();

    engine.mark_running(session.id).await.unwrap();

    let updated = h
        .session_repo
        .update_status(session.id, "waiting_for_approval", chrono::Utc::now())
        .await
        .unwrap();

    assert_eq!(updated.status, "waiting_for_approval");
}

#[tokio::test]
async fn turn_executor_rejects_completed_session_restart() {
    let h = TestHarness::new();
    let engine = h.session_engine();
    let session = engine
        .create_session(
            SessionKind::Direct,
            Some(h.workspace_root.to_string_lossy().to_string()),
        )
        .await
        .unwrap();

    engine.mark_running(session.id).await.unwrap();
    engine.mark_completed(session.id).await.unwrap();

    let model: Arc<dyn ModelProvider> = Arc::new(FakeModelProvider::new(vec![
        FakeModelProvider::text_response("should not run"),
    ]));
    let executor = h.turn_executor(
        model,
        Arc::new(FakeToolExecutor::new(vec![])),
        ToolRegistry::new(),
    );

    let err = executor
        .execute(session.id, "Hello", None)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("invalid"), "got: {err}");
}

#[tokio::test]
async fn turn_executor_requires_running_before_waiting_for_approval() {
    let h = TestHarness::new();
    let session = h
        .session_engine()
        .create_session(
            SessionKind::Direct,
            Some(h.workspace_root.to_string_lossy().to_string()),
        )
        .await
        .unwrap();

    let err = h
        .session_repo
        .update_status(session.id, "waiting_for_approval", chrono::Utc::now())
        .await
        .unwrap_err();

    assert!(err.to_string().contains("invalid transition"), "got: {err}");
}

#[tokio::test]
async fn max_tool_iterations_enforced() {
    let h = TestHarness::new();
    let engine = h.session_engine();
    let session = engine
        .create_session(
            SessionKind::Direct,
            Some(h.workspace_root.to_string_lossy().to_string()),
        )
        .await
        .unwrap();
    engine.mark_ready(session.id).await.unwrap();
    engine.mark_running(session.id).await.unwrap();

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
async fn approval_required_is_attributed_in_transcript() {
    let h = TestHarness::new();
    let engine = h.session_engine();
    let session = engine
        .create_session(
            SessionKind::Direct,
            Some(h.workspace_root.to_string_lossy().to_string()),
        )
        .await
        .unwrap();
    engine.mark_ready(session.id).await.unwrap();
    engine.mark_running(session.id).await.unwrap();

    let model = Arc::new(FakeModelProvider::new(vec![
        FakeModelProvider::tool_call_response("exec", r#"{"command":"rm -rf /tmp/demo"}"#),
        FakeModelProvider::text_response("Execution blocked pending approval."),
    ]));

    let mut registry = ToolRegistry::new();
    registry.register(RtToolDefinition {
        name: "exec".to_string(),
        description: "Execute a shell command".to_string(),
        parameters: serde_json::json!({"type": "object"}),
        category: ToolCategory::ProcessExec,
        requires_approval: true,
    });

    let executor = h.turn_executor(
        model,
        Arc::new(FakeToolExecutor::with_steps(vec![
            FakeToolStep::ApprovalRequired {
                tool: "exec".to_string(),
                details: serde_json::to_string(&ApprovalRequest {
                    tool_name: "exec".to_string(),
                    risk_level: RiskLevel::Medium,
                    scope: ApprovalScope::ExactCall,
                    presented_payload: serde_json::json!({
                        "command": "rm -rf /tmp/demo",
                        "workdir": "/workspace",
                        "background": false,
                        "timeout": 30,
                        "pty": false,
                        "elevated": false,
                        "ask": "always",
                        "security": "allowlist"
                    }),
                    command: Some("rm -rf /tmp/demo".to_string()),
                })
                .unwrap(),
            },
        ])),
        registry,
    );
    let (turn, _) = executor.execute(session.id, "Run it", None).await.unwrap();
    assert_eq!(turn.status, "tool_executing");
    assert!(turn.ended_at.is_none());

    let session_row = h.session_repo.find_by_id(session.id).await.unwrap();
    assert_eq!(session_row.status, "waiting_for_approval");

    let transcript = h.transcript_repo.list_by_session(session.id).await.unwrap();
    let kinds: Vec<&str> = transcript.iter().map(|t| t.kind.as_str()).collect();
    assert_eq!(kinds, ["user_message", "tool_request", "approval_request"]);

    let approval_request_item: rune_core::TranscriptItem =
        serde_json::from_value(transcript[2].payload.clone()).unwrap();
    match approval_request_item {
        rune_core::TranscriptItem::ApprovalRequest { command, .. } => {
            assert_eq!(command.as_deref(), Some("rm -rf /tmp/demo"));
        }
        other => panic!("unexpected transcript item: {other:?}"),
    }

    let approvals = h.approval_repo.list(true).await.unwrap();
    assert_eq!(approvals.len(), 1);
    assert_eq!(approvals[0].subject_type, "tool_call");
    assert_eq!(approvals[0].reason, "exec");
    assert!(approvals[0].decision.is_none());
    assert_eq!(approvals[0].presented_payload["tool_name"], "exec");
    assert_eq!(approvals[0].presented_payload["resume_status"], "pending");
    assert_eq!(approvals[0].presented_payload["approval_status"], "pending");
    assert!(
        approvals[0]
            .presented_payload
            .get("approval_status_updated_at")
            .is_none()
    );
    assert_eq!(
        approvals[0].presented_payload["command"],
        "rm -rf /tmp/demo"
    );
}

#[tokio::test]
async fn approval_allow_once_resumes_blocked_turn() {
    let h = TestHarness::new();
    let engine = h.session_engine();
    let session = engine
        .create_session(
            SessionKind::Direct,
            Some(h.workspace_root.to_string_lossy().to_string()),
        )
        .await
        .unwrap();
    engine.mark_ready(session.id).await.unwrap();
    engine.mark_running(session.id).await.unwrap();

    let model = Arc::new(FakeModelProvider::new(vec![
        FakeModelProvider::tool_call_response("exec", r#"{"command":"echo hi"}"#),
        FakeModelProvider::text_response("done after approval"),
    ]));

    let mut registry = ToolRegistry::new();
    registry.register(RtToolDefinition {
        name: "exec".to_string(),
        description: "Execute a shell command".to_string(),
        parameters: serde_json::json!({"type": "object"}),
        category: ToolCategory::ProcessExec,
        requires_approval: true,
    });

    let executor = h.turn_executor(
        model,
        Arc::new(FakeToolExecutor::with_steps(vec![
            FakeToolStep::ApprovalRequired {
                tool: "exec".to_string(),
                details: serde_json::to_string(&ApprovalRequest {
                    tool_name: "exec".to_string(),
                    risk_level: RiskLevel::Medium,
                    scope: ApprovalScope::ExactCall,
                    presented_payload: serde_json::json!({"command": "echo hi"}),
                    command: Some("echo hi".to_string()),
                })
                .unwrap(),
            },
            FakeToolStep::Output("approved output".to_string()),
        ])),
        registry,
    );

    let (turn, _) = executor.execute(session.id, "run it", None).await.unwrap();
    assert_eq!(turn.status, "tool_executing");
    assert!(turn.ended_at.is_none());

    let approvals = h.approval_repo.list(true).await.unwrap();
    assert_eq!(approvals.len(), 1);
    let decided = h
        .approval_repo
        .decide(
            approvals[0].id,
            "allow_once",
            "operator",
            chrono::Utc::now(),
        )
        .await
        .unwrap();
    assert_eq!(decided.decision.as_deref(), Some("allow_once"));

    let (resumed_turn, usage) = executor.resume_approval(approvals[0].id).await.unwrap();
    assert_eq!(resumed_turn.id, turn.id);
    assert_eq!(resumed_turn.status, "completed");
    assert_eq!(usage.model_calls, 1);

    let approval_after_resume = h.approval_repo.find_by_id(approvals[0].id).await.unwrap();
    assert_eq!(
        approval_after_resume.presented_payload["resume_status"],
        "completed"
    );
    assert_eq!(
        approval_after_resume.presented_payload["approval_status"],
        "completed"
    );
    assert!(
        approval_after_resume
            .presented_payload
            .get("resumed_at")
            .is_some()
    );
    assert!(
        approval_after_resume
            .presented_payload
            .get("approval_status_updated_at")
            .is_some()
    );
    assert!(
        approval_after_resume
            .presented_payload
            .get("completed_at")
            .is_some()
    );
    assert_eq!(
        approval_after_resume.presented_payload["resume_result_summary"],
        "approved output"
    );

    let transcript = h.transcript_repo.list_by_session(session.id).await.unwrap();
    let kinds: Vec<&str> = transcript.iter().map(|t| t.kind.as_str()).collect();
    assert_eq!(
        kinds,
        [
            "user_message",
            "tool_request",
            "approval_request",
            "approval_response",
            "tool_result",
            "assistant_message"
        ]
    );

    let tool_result_item: rune_core::TranscriptItem =
        serde_json::from_value(transcript[4].payload.clone()).unwrap();
    match tool_result_item {
        rune_core::TranscriptItem::ToolResult {
            output,
            tool_execution_id,
            ..
        } => {
            assert_eq!(output, "approved output");
            assert!(tool_execution_id.is_none());
        }
        other => panic!("unexpected transcript item: {other:?}"),
    }

    let session_row = h.session_repo.find_by_id(session.id).await.unwrap();
    assert_eq!(session_row.status, "running");
}

#[tokio::test]
async fn tool_result_transcript_preserves_tool_execution_id() {
    let h = TestHarness::new();
    let engine = h.session_engine();
    let session = engine
        .create_session(
            SessionKind::Direct,
            Some(h.workspace_root.to_string_lossy().to_string()),
        )
        .await
        .unwrap();
    engine.mark_ready(session.id).await.unwrap();

    let model = Arc::new(FakeModelProvider::new(vec![
        FakeModelProvider::tool_call_response("read", r#"{"path":"Cargo.toml"}"#),
        FakeModelProvider::text_response("done"),
    ]));

    let mut registry = ToolRegistry::new();
    registry.register(RtToolDefinition {
        name: "read".to_string(),
        description: "Read a file".to_string(),
        parameters: serde_json::json!({"type": "object"}),
        category: ToolCategory::FileRead,
        requires_approval: false,
    });

    let execution_id = Uuid::now_v7();
    let executor = h.turn_executor(
        model,
        Arc::new(FakeToolExecutor::with_steps(vec![
            FakeToolStep::OutputWithExecutionId {
                output: "file contents".to_string(),
                tool_execution_id: execution_id,
            },
        ])),
        registry,
    );

    executor
        .execute(session.id, "read the file", None)
        .await
        .unwrap();

    let transcript = h.transcript_repo.list_by_session(session.id).await.unwrap();
    let tool_result_row = transcript
        .iter()
        .find(|item| item.kind == "tool_result")
        .expect("tool_result transcript item present");
    let tool_result_item: rune_core::TranscriptItem =
        serde_json::from_value(tool_result_row.payload.clone()).unwrap();
    match tool_result_item {
        rune_core::TranscriptItem::ToolResult {
            output,
            is_error,
            tool_execution_id,
            ..
        } => {
            assert_eq!(output, "file contents");
            assert!(!is_error);
            assert_eq!(tool_execution_id, Some(execution_id));
        }
        other => panic!("unexpected transcript item: {other:?}"),
    }
}

#[tokio::test]
async fn approval_resume_does_not_mark_completed_at_before_completion() {
    let h = TestHarness::new();
    let engine = h.session_engine();
    let session = engine
        .create_session(
            SessionKind::Direct,
            Some(h.workspace_root.to_string_lossy().to_string()),
        )
        .await
        .unwrap();
    engine.mark_ready(session.id).await.unwrap();
    engine.mark_running(session.id).await.unwrap();

    let model = Arc::new(FakeModelProvider::new(vec![
        FakeModelProvider::tool_call_response("exec", r#"{"command":"echo hi"}"#),
        FakeModelProvider::text_response("done after approval"),
    ]));

    let mut registry = ToolRegistry::new();
    registry.register(RtToolDefinition {
        name: "exec".to_string(),
        description: "Execute a shell command".to_string(),
        parameters: serde_json::json!({"type": "object"}),
        category: ToolCategory::ProcessExec,
        requires_approval: true,
    });

    let executor = h.turn_executor(
        model,
        Arc::new(FakeToolExecutor::with_steps(vec![
            FakeToolStep::ApprovalRequired {
                tool: "exec".to_string(),
                details: serde_json::to_string(&ApprovalRequest {
                    tool_name: "exec".to_string(),
                    risk_level: RiskLevel::Medium,
                    scope: ApprovalScope::ExactCall,
                    presented_payload: serde_json::json!({"command": "echo hi"}),
                    command: Some("echo hi".to_string()),
                })
                .unwrap(),
            },
            FakeToolStep::Output("approved output".to_string()),
        ])),
        registry,
    );

    executor.execute(session.id, "run it", None).await.unwrap();
    let approvals = h.approval_repo.list(true).await.unwrap();
    h.approval_repo
        .decide(
            approvals[0].id,
            "allow_once",
            "operator",
            chrono::Utc::now(),
        )
        .await
        .unwrap();

    let decided = h.approval_repo.find_by_id(approvals[0].id).await.unwrap();
    assert!(decided.presented_payload.get("completed_at").is_none());

    executor.resume_approval(approvals[0].id).await.unwrap();

    let completed = h.approval_repo.find_by_id(approvals[0].id).await.unwrap();
    assert!(completed.presented_payload.get("completed_at").is_some());
}

#[tokio::test]
async fn approval_resume_marks_failed_when_post_tool_continuation_fails() {
    let h = TestHarness::new();
    let engine = h.session_engine();
    let session = engine
        .create_session(
            SessionKind::Direct,
            Some(h.workspace_root.to_string_lossy().to_string()),
        )
        .await
        .unwrap();
    engine.mark_ready(session.id).await.unwrap();
    engine.mark_running(session.id).await.unwrap();

    let model = Arc::new(FailAfterFirstCallModelProvider::new());

    let mut registry = ToolRegistry::new();
    registry.register(RtToolDefinition {
        name: "exec".to_string(),
        description: "Execute a shell command".to_string(),
        parameters: serde_json::json!({"type": "object"}),
        category: ToolCategory::ProcessExec,
        requires_approval: true,
    });

    let executor = h.turn_executor(
        model,
        Arc::new(FakeToolExecutor::with_steps(vec![
            FakeToolStep::ApprovalRequired {
                tool: "exec".to_string(),
                details: serde_json::to_string(&ApprovalRequest {
                    tool_name: "exec".to_string(),
                    risk_level: RiskLevel::Medium,
                    scope: ApprovalScope::ExactCall,
                    presented_payload: serde_json::json!({"command": "echo hi"}),
                    command: Some("echo hi".to_string()),
                })
                .unwrap(),
            },
            FakeToolStep::Output("approved output".to_string()),
        ])),
        registry,
    );

    let (blocked_turn, _) = executor.execute(session.id, "run it", None).await.unwrap();
    assert_eq!(blocked_turn.status, "tool_executing");

    let approvals = h.approval_repo.list(true).await.unwrap();
    h.approval_repo
        .decide(
            approvals[0].id,
            "allow_once",
            "operator",
            chrono::Utc::now(),
        )
        .await
        .unwrap();

    let err = executor.resume_approval(approvals[0].id).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("fake transient error after approval resume")
    );

    let approval = h.approval_repo.find_by_id(approvals[0].id).await.unwrap();
    assert_eq!(approval.presented_payload["approval_status"], "failed");
    assert_eq!(approval.presented_payload["resume_status"], "failed");
    assert!(approval.presented_payload.get("completed_at").is_some());
    assert!(
        approval.presented_payload["resume_result_summary"]
            .as_str()
            .unwrap()
            .contains("post-approval continuation failed")
    );

    let failed_turn = h.turn_repo.find_by_id(blocked_turn.id).await.unwrap();
    assert_eq!(failed_turn.status, "failed");
}

#[tokio::test]
async fn resume_approval_rejects_redeciding_completed_approval() {
    let h = TestHarness::new();
    let engine = h.session_engine();
    let session = engine
        .create_session(
            SessionKind::Direct,
            Some(h.workspace_root.to_string_lossy().to_string()),
        )
        .await
        .unwrap();
    engine.mark_ready(session.id).await.unwrap();
    engine.mark_running(session.id).await.unwrap();

    let model = Arc::new(FakeModelProvider::new(vec![
        FakeModelProvider::tool_call_response("exec", r#"{"command":"echo hi"}"#),
        FakeModelProvider::text_response("done"),
    ]));

    let mut registry = ToolRegistry::new();
    registry.register(RtToolDefinition {
        name: "exec".to_string(),
        description: "Execute a shell command".to_string(),
        parameters: serde_json::json!({"type": "object"}),
        category: ToolCategory::ProcessExec,
        requires_approval: true,
    });

    let executor = h.turn_executor(
        model,
        Arc::new(FakeToolExecutor::with_steps(vec![
            FakeToolStep::ApprovalRequired {
                tool: "exec".to_string(),
                details: serde_json::to_string(&ApprovalRequest {
                    tool_name: "exec".to_string(),
                    risk_level: RiskLevel::Medium,
                    scope: ApprovalScope::ExactCall,
                    presented_payload: serde_json::json!({"command": "echo hi"}),
                    command: Some("echo hi".to_string()),
                })
                .unwrap(),
            },
            FakeToolStep::Output("approved output".to_string()),
        ])),
        registry,
    );

    let (blocked_turn, _) = executor.execute(session.id, "run it", None).await.unwrap();
    assert_eq!(blocked_turn.status, "tool_executing");

    let approvals = h.approval_repo.list(true).await.unwrap();
    let approval_id = approvals[0].id;
    h.approval_repo
        .decide(approval_id, "allow_once", "operator", chrono::Utc::now())
        .await
        .unwrap();

    let (resumed_turn, _) = executor.resume_approval(approval_id).await.unwrap();
    assert_eq!(resumed_turn.status, "completed");

    let err = executor.resume_approval(approval_id).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("approval already resumed with status completed")
    );
}

#[tokio::test]
async fn approval_resume_retains_reapproval_linkage_for_followup_approval() {
    let h = TestHarness::new();
    let engine = h.session_engine();
    let session = engine
        .create_session(
            SessionKind::Direct,
            Some(h.workspace_root.to_string_lossy().to_string()),
        )
        .await
        .unwrap();
    engine.mark_ready(session.id).await.unwrap();
    engine.mark_running(session.id).await.unwrap();

    let model = Arc::new(FakeModelProvider::new(vec![
        FakeModelProvider::tool_call_response("exec", r#"{"command":"echo hi"}"#),
        FakeModelProvider::text_response("done"),
    ]));

    let mut registry = ToolRegistry::new();
    registry.register(RtToolDefinition {
        name: "exec".to_string(),
        description: "Execute a shell command".to_string(),
        parameters: serde_json::json!({"type": "object"}),
        category: ToolCategory::ProcessExec,
        requires_approval: true,
    });

    let executor = h.turn_executor(
        model,
        Arc::new(FakeToolExecutor::with_steps(vec![
            FakeToolStep::ApprovalRequired {
                tool: "exec".to_string(),
                details: serde_json::to_string(&ApprovalRequest {
                    tool_name: "exec".to_string(),
                    risk_level: RiskLevel::Medium,
                    scope: ApprovalScope::ExactCall,
                    presented_payload: serde_json::json!({"command": "echo hi"}),
                    command: Some("echo hi".to_string()),
                })
                .unwrap(),
            },
            FakeToolStep::Error(ToolError::ApprovalRequired {
                tool: "exec".to_string(),
                details: serde_json::to_string(&ApprovalRequest {
                    tool_name: "exec".to_string(),
                    risk_level: RiskLevel::High,
                    scope: ApprovalScope::ExactCall,
                    presented_payload: serde_json::json!({"command": "echo hi", "phase": "resume"}),
                    command: Some("echo hi".to_string()),
                })
                .unwrap(),
            }),
        ])),
        registry,
    );

    let (blocked_turn, _) = executor.execute(session.id, "run it", None).await.unwrap();
    assert_eq!(blocked_turn.status, "tool_executing");

    let approvals = h.approval_repo.list(false).await.unwrap();
    assert_eq!(approvals.len(), 1);
    let first_approval = approvals[0].clone();
    let session_id = session.id.to_string();
    let turn_id = blocked_turn.id.to_string();
    assert_eq!(
        first_approval.handle_ref.as_deref(),
        Some(session_id.as_str())
    );
    assert_eq!(first_approval.host_ref.as_deref(), Some(turn_id.as_str()));

    h.approval_repo
        .decide(
            first_approval.id,
            "allow_once",
            "operator",
            chrono::Utc::now(),
        )
        .await
        .unwrap();

    let resumed_turn = executor.resume_approval(first_approval.id).await.unwrap().0;
    assert_eq!(resumed_turn.status, "tool_executing");

    let approvals = h.approval_repo.list(false).await.unwrap();
    assert_eq!(approvals.len(), 2);
    let followup = approvals
        .into_iter()
        .find(|approval| approval.id != first_approval.id)
        .expect("follow-up approval created");
    assert_eq!(followup.handle_ref.as_deref(), Some(session_id.as_str()));
    assert_eq!(followup.host_ref.as_deref(), Some(turn_id.as_str()));
    assert_eq!(followup.presented_payload["session_id"], session_id);
    assert_eq!(followup.presented_payload["turn_id"], turn_id);
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
        .create_session(
            SessionKind::Direct,
            Some(h.workspace_root.to_string_lossy().to_string()),
        )
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
    assert_eq!(usage.prompt_tokens, 20);
    assert_eq!(usage.completion_tokens, 13);
    assert_eq!(usage.total_tokens, 33);
}

#[tokio::test]
async fn session_parent_linkage() {
    let h = TestHarness::new();
    let engine = h.session_engine();

    let parent = engine
        .create_session(SessionKind::Direct, Some("/workspace".to_string()))
        .await
        .unwrap();

    let child = engine
        .create_session_full(
            SessionKind::Subagent,
            Some("/workspace".to_string()),
            Some(parent.id),
            None,
            None,
        )
        .await
        .unwrap();

    assert_eq!(child.kind, "subagent");
    assert_eq!(child.requester_session_id, Some(parent.id));
    assert!(child.channel_ref.is_none());

    let channel_session = engine
        .create_session_full(
            SessionKind::Channel,
            None,
            None,
            Some("telegram".to_string()),
            None,
        )
        .await
        .unwrap();

    assert_eq!(channel_session.kind, "channel");
    assert_eq!(channel_session.channel_ref, Some("telegram".to_string()));
    assert!(channel_session.requester_session_id.is_none());

    let scheduled_main = engine
        .create_session_full(
            SessionKind::Scheduled,
            Some("/workspace".to_string()),
            None,
            Some("system:scheduled-main".to_string()),
            None,
        )
        .await
        .unwrap();

    let isolated_run = engine
        .create_session_full(
            SessionKind::Subagent,
            Some("/workspace".to_string()),
            Some(scheduled_main.id),
            None,
            None,
        )
        .await
        .unwrap();

    let resumed_main = engine
        .get_session_by_channel_ref("system:scheduled-main")
        .await
        .unwrap()
        .expect("scheduled main session should resolve by channel ref");

    assert_eq!(resumed_main.id, scheduled_main.id);
    assert_eq!(resumed_main.kind, "scheduled");
    assert_eq!(isolated_run.kind, "subagent");
    assert_eq!(isolated_run.requester_session_id, Some(scheduled_main.id));

    assert!(parent.requester_session_id.is_none());
    assert!(parent.channel_ref.is_none());
}

#[tokio::test]
async fn direct_session_prompt_includes_workspace_and_memory_context() {
    let h = TestHarness::new();
    let engine = h.session_engine();

    let session = engine
        .create_session(
            SessionKind::Direct,
            Some(h.workspace_root.to_string_lossy().to_string()),
        )
        .await
        .unwrap();

    let model = Arc::new(FakeModelProvider::new(vec![
        FakeModelProvider::text_response("Context loaded"),
    ]));
    let model_handle = model.clone();
    let executor = h.turn_executor(
        model,
        Arc::new(FakeToolExecutor::new(vec![])),
        ToolRegistry::new(),
    );

    executor.execute(session.id, "hello", None).await.unwrap();

    let requests = model_handle.requests().await;
    let system = requests[0].messages[0].content.clone().unwrap();
    assert!(system.contains("AGENTS.md"));
    assert!(system.contains("SOUL.md"));
    assert!(system.contains("USER.md"));
    assert!(system.contains("Long-term Memory"));
    assert!(system.contains("Today's Notes"));
    assert!(!system.contains("HEARTBEAT.md"));
}

#[tokio::test]
async fn channel_session_prompt_excludes_long_term_memory() {
    let h = TestHarness::new();
    let engine = h.session_engine();

    let session = engine
        .create_session_full(
            SessionKind::Channel,
            Some(h.workspace_root.to_string_lossy().to_string()),
            None,
            Some("telegram".to_string()),
        )
        .await
        .unwrap();

    let model = Arc::new(FakeModelProvider::new(vec![
        FakeModelProvider::text_response("Channel context"),
    ]));
    let model_handle = model.clone();
    let executor = h.turn_executor(
        model,
        Arc::new(FakeToolExecutor::new(vec![])),
        ToolRegistry::new(),
    );

    executor.execute(session.id, "ping", None).await.unwrap();

    let requests = model_handle.requests().await;
    let system = requests[0].messages[0].content.clone().unwrap();
    assert!(system.contains("AGENTS.md"));
    assert!(system.contains("Today's Notes"));
    assert!(!system.contains("Long-term Memory"));
    assert!(!system.contains("Long-term fact."));
    assert!(!system.contains("HEARTBEAT.md"));
}

#[tokio::test]
async fn enabled_skills_are_injected_into_system_prompt() {
    let h = TestHarness::new();
    let engine = h.session_engine();

    let session = engine
        .create_session(
            SessionKind::Direct,
            Some(h.workspace_root.to_string_lossy().to_string()),
        )
        .await
        .unwrap();

    let skill_registry = Arc::new(SkillRegistry::new());
    skill_registry
        .register(Skill {
            name: "skill-alpha".into(),
            description: "Alpha description".into(),
            parameters: serde_json::json!({}),
            binary_path: Some(PathBuf::from("/tmp/skill-alpha")),
            source_dir: PathBuf::from("/tmp/skills/skill-alpha"),
            enabled: true,
            prompt_body: None,
            model: None,
            allowed_tools: None,
            user_invocable: false,
            namespace: None,
            version: None,
            kind: Default::default(),
            requires: vec![],
            tags: vec![],
            triggers: vec![],
        })
        .await;
    skill_registry
        .register(Skill {
            name: "skill-beta".into(),
            description: "Beta description".into(),
            parameters: serde_json::json!({}),
            binary_path: None,
            source_dir: PathBuf::from("/tmp/skills/skill-beta"),
            enabled: false,
            prompt_body: None,
            model: None,
            allowed_tools: None,
            user_invocable: false,
            namespace: None,
            version: None,
            kind: Default::default(),
            requires: vec![],
            tags: vec![],
            triggers: vec![],
        })
        .await;

    let model = Arc::new(FakeModelProvider::new(vec![
        FakeModelProvider::text_response("skills loaded"),
    ]));
    let model_handle = model.clone();
    let executor = h.turn_executor_with_skill_registry(
        model,
        Arc::new(FakeToolExecutor::new(vec![])),
        ToolRegistry::new(),
        Some(skill_registry),
    );

    executor.execute(session.id, "hello", None).await.unwrap();

    let requests = model_handle.requests().await;
    let system = requests[0].messages[0].content.clone().unwrap();
    assert!(system.contains("## Available Spells"));
    assert!(system.contains("skill-alpha"));
    assert!(system.contains("Alpha description"));
    assert!(system.contains("/tmp/skill-alpha"));
    assert!(!system.contains("skill-beta"));
    assert!(!system.contains("Beta description"));
}

#[tokio::test]
async fn selected_model_metadata_is_used_for_future_turns() {
    let h = TestHarness::new();
    let engine = h.session_engine();

    let session = engine
        .create_session(
            SessionKind::Direct,
            Some(h.workspace_root.to_string_lossy().to_string()),
        )
        .await
        .unwrap();

    h.session_repo
        .set_metadata(
            session.id,
            serde_json::json!({"selected_model": "azure/gpt-5.4"}),
        )
        .await;

    let model = Arc::new(FakeModelProvider::new(vec![
        FakeModelProvider::text_response("Model remembered"),
    ]));
    let model_handle = model.clone();
    let executor = h.turn_executor(
        model,
        Arc::new(FakeToolExecutor::new(vec![])),
        ToolRegistry::new(),
    );

    executor
        .execute(session.id, "hello again", None)
        .await
        .unwrap();

    let requests = model_handle.requests().await;
    assert_eq!(requests[0].model.as_deref(), Some("azure/gpt-5.4"));
}

// ── allow-always reuse tests ──────────────────────────────────────────

#[tokio::test]
async fn allow_always_policy_auto_approves_matching_tool_call() {
    let h = TestHarness::new();
    let engine = h.session_engine();
    let session = engine
        .create_session(
            SessionKind::Direct,
            Some(h.workspace_root.to_string_lossy().to_string()),
        )
        .await
        .unwrap();
    engine.mark_ready(session.id).await.unwrap();
    engine.mark_running(session.id).await.unwrap();

    // Model makes a tool call, then receives the result and finishes.
    let model = Arc::new(FakeModelProvider::new(vec![
        FakeModelProvider::tool_call_response("exec", r#"{"command":"ls"}"#),
        FakeModelProvider::text_response("done after auto-approval"),
    ]));

    let mut registry = ToolRegistry::new();
    registry.register(RtToolDefinition {
        name: "exec".to_string(),
        description: "Execute a shell command".to_string(),
        parameters: serde_json::json!({"type": "object"}),
        category: ToolCategory::ProcessExec,
        requires_approval: true,
    });

    // Pre-seed an allow-always policy for the "exec" tool.
    let policy_repo = Arc::new(MemToolApprovalPolicyRepo::new());
    policy_repo
        .set_policy("exec", "allow_always")
        .await
        .unwrap();

    // The tool executor returns ApprovalRequired on the first call, then
    // succeeds on the second (the auto-approved retry with __approval_resume).
    let executor = h.turn_executor_with_policy_repo(
        model,
        Arc::new(FakeToolExecutor::with_steps(vec![
            FakeToolStep::ApprovalRequired {
                tool: "exec".to_string(),
                details: serde_json::to_string(&ApprovalRequest {
                    tool_name: "exec".to_string(),
                    risk_level: RiskLevel::Medium,
                    scope: ApprovalScope::ExactCall,
                    presented_payload: serde_json::json!({
                        "command": "ls",
                    }),
                    command: Some("ls".to_string()),
                })
                .unwrap(),
            },
            FakeToolStep::Output("file1.txt\nfile2.txt".to_string()),
        ])),
        registry,
        policy_repo,
    );

    let (turn, _) = executor
        .execute(session.id, "list files", None)
        .await
        .unwrap();
    // Turn should complete (not halt at waiting_for_approval).
    assert_eq!(turn.status, "completed");

    // Session should NOT be stuck in waiting_for_approval.
    let session_row = h.session_repo.find_by_id(session.id).await.unwrap();
    assert_ne!(session_row.status, "waiting_for_approval");

    // No pending approval records should have been created.
    let approvals = h.approval_repo.list(true).await.unwrap();
    assert!(approvals.is_empty(), "expected no pending approvals");

    // Transcript should contain an auto-approval audit entry.
    let items = h.transcript_repo.list_by_session(session.id).await.unwrap();
    let kinds: Vec<&str> = items.iter().map(|i| i.kind.as_str()).collect();
    assert!(
        kinds.contains(&"approval_response"),
        "transcript should contain auto-approval audit entry, got: {kinds:?}"
    );

    // Verify the auto-approval note mentions the policy.
    let approval_item = items
        .iter()
        .find(|i| i.kind == "approval_response")
        .unwrap();
    let note = approval_item.payload["note"].as_str().unwrap_or("");
    assert!(
        note.contains("auto-approved") && note.contains("allow-always"),
        "auto-approval note should mention policy, got: {note}"
    );
}

#[tokio::test]
async fn no_allow_always_policy_still_halts_for_approval() {
    // When no policy repo has a matching allow-always entry, the executor
    // should behave exactly as before: halt and wait for approval.
    let h = TestHarness::new();
    let engine = h.session_engine();
    let session = engine
        .create_session(
            SessionKind::Direct,
            Some(h.workspace_root.to_string_lossy().to_string()),
        )
        .await
        .unwrap();
    engine.mark_ready(session.id).await.unwrap();
    engine.mark_running(session.id).await.unwrap();

    let model = Arc::new(FakeModelProvider::new(vec![
        FakeModelProvider::tool_call_response("exec", r#"{"command":"rm -rf /"}"#),
        FakeModelProvider::text_response("should not reach here"),
    ]));

    let mut registry = ToolRegistry::new();
    registry.register(RtToolDefinition {
        name: "exec".to_string(),
        description: "Execute a shell command".to_string(),
        parameters: serde_json::json!({"type": "object"}),
        category: ToolCategory::ProcessExec,
        requires_approval: true,
    });

    // Policy repo exists but is empty — no allow-always policy.
    let policy_repo = Arc::new(MemToolApprovalPolicyRepo::new());

    let executor = h.turn_executor_with_policy_repo(
        model,
        Arc::new(FakeToolExecutor::with_steps(vec![
            FakeToolStep::ApprovalRequired {
                tool: "exec".to_string(),
                details: serde_json::to_string(&ApprovalRequest {
                    tool_name: "exec".to_string(),
                    risk_level: RiskLevel::High,
                    scope: ApprovalScope::ExactCall,
                    presented_payload: serde_json::json!({
                        "command": "rm -rf /",
                    }),
                    command: Some("rm -rf /".to_string()),
                })
                .unwrap(),
            },
        ])),
        registry,
        policy_repo,
    );

    let (turn, _) = executor.execute(session.id, "nuke it", None).await.unwrap();
    assert_eq!(turn.status, "tool_executing");

    let session_row = h.session_repo.find_by_id(session.id).await.unwrap();
    assert_eq!(session_row.status, "waiting_for_approval");

    let approvals = h.approval_repo.list(true).await.unwrap();
    assert_eq!(approvals.len(), 1);
}

#[tokio::test]
async fn allow_always_policy_does_not_affect_different_tool() {
    // An allow-always policy for tool "exec" should NOT auto-approve
    // a different tool like "file_write".
    let h = TestHarness::new();
    let engine = h.session_engine();
    let session = engine
        .create_session(
            SessionKind::Direct,
            Some(h.workspace_root.to_string_lossy().to_string()),
        )
        .await
        .unwrap();
    engine.mark_ready(session.id).await.unwrap();
    engine.mark_running(session.id).await.unwrap();

    let model = Arc::new(FakeModelProvider::new(vec![
        FakeModelProvider::tool_call_response("file_write", r#"{"path":"/tmp/x"}"#),
        FakeModelProvider::text_response("should not reach here"),
    ]));

    let mut registry = ToolRegistry::new();
    registry.register(RtToolDefinition {
        name: "file_write".to_string(),
        description: "Write a file".to_string(),
        parameters: serde_json::json!({"type": "object"}),
        category: ToolCategory::FileWrite,
        requires_approval: true,
    });

    // Policy is for "exec", NOT "file_write".
    let policy_repo = Arc::new(MemToolApprovalPolicyRepo::new());
    policy_repo
        .set_policy("exec", "allow_always")
        .await
        .unwrap();

    let executor = h.turn_executor_with_policy_repo(
        model,
        Arc::new(FakeToolExecutor::with_steps(vec![
            FakeToolStep::ApprovalRequired {
                tool: "file_write".to_string(),
                details: serde_json::to_string(&ApprovalRequest {
                    tool_name: "file_write".to_string(),
                    risk_level: RiskLevel::Medium,
                    scope: ApprovalScope::ExactCall,
                    presented_payload: serde_json::json!({
                        "path": "/tmp/x",
                    }),
                    command: None,
                })
                .unwrap(),
            },
        ])),
        registry,
        policy_repo,
    );

    let (turn, _) = executor
        .execute(session.id, "write file", None)
        .await
        .unwrap();
    // Should halt — the allow-always policy is for a different tool.
    assert_eq!(turn.status, "tool_executing");

    let session_row = h.session_repo.find_by_id(session.id).await.unwrap();
    assert_eq!(session_row.status, "waiting_for_approval");
}

#[derive(Debug)]
struct SharedSentChannelAdapter {
    sent: Arc<Mutex<Vec<rune_channels::OutboundAction>>>,
}

#[async_trait]
impl rune_channels::ChannelAdapter for SharedSentChannelAdapter {
    async fn receive(
        &mut self,
    ) -> Result<rune_channels::InboundEvent, rune_channels::ChannelError> {
        Err(rune_channels::ChannelError::ConnectionLost {
            reason: "not used in this test".to_string(),
        })
    }

    async fn send(
        &self,
        action: rune_channels::OutboundAction,
    ) -> Result<rune_channels::DeliveryReceipt, rune_channels::ChannelError> {
        self.sent.lock().await.push(action);
        Ok(rune_channels::DeliveryReceipt {
            provider_message_id: Uuid::now_v7().to_string(),
            delivered_at: chrono::Utc::now(),
        })
    }
}

#[tokio::test]
async fn resumed_session_notice_skips_non_restored_sessions() {
    let h = TestHarness::new();
    let engine = Arc::new(h.session_engine());
    let existing = engine
        .create_session_full(
            SessionKind::Channel,
            Some(h.workspace_root.to_string_lossy().to_string()),
            None,
            Some("chat-2:user-2".to_string()),
            None,
        )
        .await
        .unwrap();

    let sent = Arc::new(Mutex::new(Vec::new()));
    let adapter = SharedSentChannelAdapter { sent: sent.clone() };
    let session_loop = crate::session_loop::SessionLoop::new(
        engine.clone(),
        Arc::new(h.turn_executor(
            Arc::new(FakeModelProvider::new(vec![])),
            Arc::new(FakeToolExecutor::new(vec![])),
            ToolRegistry::new(),
        )),
        h.session_repo.clone(),
        Box::new(adapter),
        rune_config::AgentsConfig::default(),
        rune_config::ModelsConfig::default(),
    );

    let msg = rune_channels::ChannelMessage {
        channel_id: rune_core::ChannelId::new(),
        raw_chat_id: "chat-2".to_string(),
        sender: "user-2".to_string(),
        content: "hello".to_string(),
        attachments: vec![],
        timestamp: chrono::Utc::now(),
        provider_message_id: "msg-2".to_string(),
    };

    session_loop
        .maybe_send_resumed_session_notice(&msg, "chat-2:user-2", &existing)
        .await;

    let sent = sent.lock().await.clone();
    assert!(sent.is_empty(), "notice should not be sent for sessions that were not restored during startup");
}

#[tokio::test]
async fn resumed_session_notice_only_for_restored_channel_sessions() {
    let h = TestHarness::new();
    let engine = Arc::new(h.session_engine());
    let existing = engine
        .create_session_full(
            SessionKind::Channel,
            Some(h.workspace_root.to_string_lossy().to_string()),
            None,
            Some("chat-1:user-1".to_string()),
            None,
        )
        .await
        .unwrap();

    let sent = Arc::new(Mutex::new(Vec::new()));
    let adapter = SharedSentChannelAdapter { sent: sent.clone() };
    let session_loop = crate::session_loop::SessionLoop::new(
        engine.clone(),
        Arc::new(h.turn_executor(
            Arc::new(FakeModelProvider::new(vec![])),
            Arc::new(FakeToolExecutor::new(vec![])),
            ToolRegistry::new(),
        )),
        h.session_repo.clone(),
        Box::new(adapter),
        rune_config::AgentsConfig::default(),
        rune_config::ModelsConfig::default(),
    );

    let msg = rune_channels::ChannelMessage {
        channel_id: rune_core::ChannelId::new(),
        raw_chat_id: "chat-1".to_string(),
        sender: "user-1".to_string(),
        content: "hello again".to_string(),
        attachments: vec![],
        timestamp: chrono::Utc::now(),
        provider_message_id: "msg-1".to_string(),
    };

    session_loop
        .maybe_send_resumed_session_notice(&msg, "chat-1:user-1", &existing)
        .await;
    session_loop
        .maybe_send_resumed_session_notice(&msg, "chat-1:user-1", &existing)
        .await;

    let sent = sent.lock().await.clone();
    assert_eq!(
        sent.len(),
        1,
        "notice should be sent once per restored session state"
    );
    match &sent[0] {
        rune_channels::OutboundAction::Reply { content, .. } => {
            assert!(content.contains("Resumed session"));
            assert!(content.contains(&existing.id.to_string()));
            assert!(content.contains("do not resume in place"));
        }
        other => panic!("expected reply notice, got {other:?}"),
    }
}

#[tokio::test]
async fn create_session_full_persists_mode_in_metadata() {
    let h = TestHarness::new();
    let engine = h.session_engine();

    let session = engine
        .create_session_full(
            SessionKind::Direct,
            Some("/workspace".to_string()),
            None,
            None,
            Some("architect".to_string()),
        )
        .await
        .unwrap();

    assert_eq!(
        session.metadata.get("mode").and_then(|value| value.as_str()),
        Some("architect")
    );
}
