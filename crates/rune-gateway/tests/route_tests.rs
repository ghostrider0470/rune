//! Integration tests for rune-gateway HTTP route handlers.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::Value;
use tokio::sync::{Mutex, RwLock, broadcast};
use tower::ServiceExt;
use uuid::Uuid;

use chrono::Timelike;

use rune_config::{
    AppConfig, Capabilities, ConfiguredModel, LaneQueueConfig, ModelProviderConfig, RuntimeMode,
};
use rune_models::{
    CompletionRequest, CompletionResponse, FinishReason, ModelError, ModelProvider, Usage,
};
use rune_runtime::{
    CompactionStrategy, ContextAssembler, HookRegistry, NoOpCompaction, PluginLoader,
    PluginRegistry, SessionEngine, SkillLoader, SkillRegistry, TurnExecutor,
    heartbeat::HeartbeatRunner,
    scheduler::{ReminderStore, Scheduler},
};
use rune_store::StoreError;
use rune_store::models::*;
use rune_store::repos::*;
use rune_tools::process_tool::ProcessManager;
use rune_tools::{ToolCall, ToolError, ToolExecutor, ToolRegistry, ToolResult};
use std::collections::HashMap;

use rune_gateway::{AppState, SessionEvent, build_router, pairing::DeviceRegistry};

fn test_capabilities(tool_count: usize) -> Arc<Capabilities> {
    Arc::new(Capabilities {
        mode: RuntimeMode::Standalone,
        storage_backend: "test".to_string(),
        pgvector: false,
        memory_mode: "disabled".to_string(),
        browser: false,
        mcp_servers: 0,
        tts: false,
        stt: false,
        tool_count,
        channels: vec![],
        approval_mode: "prompt".to_string(),
        security_posture: "sandboxed".to_string(),
    })
}

fn test_plugins() -> (Arc<PluginRegistry>, Arc<PluginLoader>, Arc<HookRegistry>) {
    let plugin_registry = Arc::new(PluginRegistry::new());
    let plugin_loader = Arc::new(PluginLoader::new(
        std::env::temp_dir(),
        plugin_registry.clone(),
    ));
    let hook_registry = Arc::new(HookRegistry::new());
    (plugin_registry, plugin_loader, hook_registry)
}

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
        Ok(session.clone())
    }

    async fn delete(&self, id: Uuid) -> Result<bool, StoreError> {
        let mut sessions = self.sessions.lock().await;
        let before = sessions.len();
        sessions.retain(|session| session.id != id);
        Ok(sessions.len() != before)
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
            handle_ref: None,
            host_ref: None,
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
    policies: Mutex<HashMap<String, rune_store::repos::ToolApprovalPolicy>>,
}

impl MemToolApprovalPolicyRepo {
    fn new() -> Self {
        Self {
            policies: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl ToolApprovalPolicyRepo for MemToolApprovalPolicyRepo {
    async fn list_policies(
        &self,
    ) -> Result<Vec<rune_store::repos::ToolApprovalPolicy>, StoreError> {
        let mut values: Vec<_> = self.policies.lock().await.values().cloned().collect();
        values.sort_by(|a, b| a.tool_name.cmp(&b.tool_name));
        Ok(values)
    }
    async fn get_policy(
        &self,
        tool_name: &str,
    ) -> Result<Option<rune_store::repos::ToolApprovalPolicy>, StoreError> {
        Ok(self.policies.lock().await.get(tool_name).cloned())
    }
    async fn set_policy(
        &self,
        tool_name: &str,
        decision: &str,
    ) -> Result<rune_store::repos::ToolApprovalPolicy, StoreError> {
        let policy = rune_store::repos::ToolApprovalPolicy {
            tool_name: tool_name.to_string(),
            decision: decision.to_string(),
            decided_at: chrono::Utc::now(),
        };
        self.policies
            .lock()
            .await
            .insert(tool_name.to_string(), policy.clone());
        Ok(policy)
    }
    async fn clear_policy(&self, tool_name: &str) -> Result<bool, StoreError> {
        Ok(self.policies.lock().await.remove(tool_name).is_some())
    }
}

struct MemDeviceRepo {
    devices: Mutex<Vec<PairedDeviceRow>>,
    requests: Mutex<Vec<PairingRequestRow>>,
}

impl MemDeviceRepo {
    fn new() -> Self {
        Self {
            devices: Mutex::new(Vec::new()),
            requests: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl DeviceRepo for MemDeviceRepo {
    async fn create_device(&self, device: NewPairedDevice) -> Result<PairedDeviceRow, StoreError> {
        let row = PairedDeviceRow {
            id: device.id,
            name: device.name,
            public_key: device.public_key,
            role: device.role,
            scopes: device.scopes,
            token_hash: device.token_hash,
            token_expires_at: device.token_expires_at,
            paired_at: device.paired_at,
            last_seen_at: None,
            created_at: device.created_at,
        };
        self.devices.lock().await.push(row.clone());
        Ok(row)
    }

    async fn find_device_by_id(&self, id: Uuid) -> Result<PairedDeviceRow, StoreError> {
        self.devices
            .lock()
            .await
            .iter()
            .find(|device| device.id == id)
            .cloned()
            .ok_or(StoreError::NotFound {
                entity: "paired_device",
                id: id.to_string(),
            })
    }

    async fn find_device_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<PairedDeviceRow>, StoreError> {
        Ok(self
            .devices
            .lock()
            .await
            .iter()
            .find(|device| device.token_hash == token_hash)
            .cloned())
    }

    async fn find_device_by_public_key(
        &self,
        public_key: &str,
    ) -> Result<Option<PairedDeviceRow>, StoreError> {
        Ok(self
            .devices
            .lock()
            .await
            .iter()
            .find(|device| device.public_key == public_key)
            .cloned())
    }

    async fn list_devices(&self) -> Result<Vec<PairedDeviceRow>, StoreError> {
        Ok(self.devices.lock().await.clone())
    }

    async fn update_token(
        &self,
        id: Uuid,
        token_hash: &str,
        token_expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<PairedDeviceRow, StoreError> {
        let mut devices = self.devices.lock().await;
        let device =
            devices
                .iter_mut()
                .find(|device| device.id == id)
                .ok_or(StoreError::NotFound {
                    entity: "paired_device",
                    id: id.to_string(),
                })?;
        device.token_hash = token_hash.to_string();
        device.token_expires_at = token_expires_at;
        Ok(device.clone())
    }

    async fn update_role(
        &self,
        id: Uuid,
        role: &str,
        scopes: serde_json::Value,
    ) -> Result<PairedDeviceRow, StoreError> {
        let mut devices = self.devices.lock().await;
        let device =
            devices
                .iter_mut()
                .find(|device| device.id == id)
                .ok_or(StoreError::NotFound {
                    entity: "paired_device",
                    id: id.to_string(),
                })?;
        device.role = role.to_string();
        device.scopes = scopes;
        Ok(device.clone())
    }

    async fn touch_last_seen(
        &self,
        id: Uuid,
        last_seen_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), StoreError> {
        let mut devices = self.devices.lock().await;
        let device =
            devices
                .iter_mut()
                .find(|device| device.id == id)
                .ok_or(StoreError::NotFound {
                    entity: "paired_device",
                    id: id.to_string(),
                })?;
        device.last_seen_at = Some(last_seen_at);
        Ok(())
    }

    async fn delete_device(&self, id: Uuid) -> Result<bool, StoreError> {
        let mut devices = self.devices.lock().await;
        let before = devices.len();
        devices.retain(|device| device.id != id);
        Ok(devices.len() != before)
    }

    async fn create_pairing_request(
        &self,
        request: NewPairingRequest,
    ) -> Result<PairingRequestRow, StoreError> {
        let row = PairingRequestRow {
            id: request.id,
            device_name: request.device_name,
            public_key: request.public_key,
            challenge: request.challenge,
            created_at: request.created_at,
            expires_at: request.expires_at,
        };
        self.requests.lock().await.push(row.clone());
        Ok(row)
    }

    async fn take_pairing_request(
        &self,
        id: Uuid,
    ) -> Result<Option<PairingRequestRow>, StoreError> {
        let mut requests = self.requests.lock().await;
        if let Some(index) = requests.iter().position(|request| request.id == id) {
            Ok(Some(requests.remove(index)))
        } else {
            Ok(None)
        }
    }

    async fn delete_pairing_request(&self, id: Uuid) -> Result<bool, StoreError> {
        let mut requests = self.requests.lock().await;
        let before = requests.len();
        requests.retain(|request| request.id != id);
        Ok(requests.len() != before)
    }

    async fn list_pending_requests(&self) -> Result<Vec<PairingRequestRow>, StoreError> {
        let now = chrono::Utc::now();
        Ok(self
            .requests
            .lock()
            .await
            .iter()
            .filter(|request| request.expires_at > now)
            .cloned()
            .collect())
    }

    async fn prune_expired_requests(&self) -> Result<usize, StoreError> {
        let now = chrono::Utc::now();
        let mut requests = self.requests.lock().await;
        let before = requests.len();
        requests.retain(|request| request.expires_at > now);
        Ok(before - requests.len())
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
            tool_execution_id: None,
        })
    }
}

// ── Test harness ──────────────────────────────────────────────────────────────

const TEST_AUTH_TOKEN: &str = "test-secret-token";

fn build_test_app(auth_token: Option<String>) -> axum::Router {
    build_test_app_parts(AppConfig::default(), auth_token).0
}

fn build_test_app_with_config(config: AppConfig, auth_token: Option<String>) -> axum::Router {
    build_test_app_parts(config, auth_token).0
}

fn build_test_app_parts(
    mut config: AppConfig,
    auth_token: Option<String>,
) -> (axum::Router, Arc<MemDeviceRepo>) {
    let session_repo = Arc::new(MemSessionRepo::new());
    let turn_repo = Arc::new(MemTurnRepo::new());
    let transcript_repo = Arc::new(MemTranscriptRepo::new());
    let approval_repo = Arc::new(MemApprovalRepo::new());
    let model_provider: Arc<dyn ModelProvider> = Arc::new(FakeModelProvider);
    let scheduler = Arc::new(Scheduler::new());

    let session_engine = Arc::new(
        SessionEngine::new(session_repo.clone()).with_transcript_repo(transcript_repo.clone()),
    );

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
            turn_repo.clone() as Arc<dyn TurnRepo>,
            transcript_repo.clone() as Arc<dyn TranscriptRepo>,
            approval_repo.clone() as Arc<dyn ApprovalRepo>,
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
    let skills_dir = config.paths.skills_dir.clone();
    let config = Arc::new(RwLock::new(config));
    let skill_registry = Arc::new(SkillRegistry::new());
    let skill_loader = Arc::new(SkillLoader::new(skills_dir, skill_registry.clone()));
    let device_repo = Arc::new(MemDeviceRepo::new());
    let device_registry = Arc::new(DeviceRegistry::new(device_repo.clone()));
    let (plugin_registry, plugin_loader, hook_registry) = test_plugins();

    let state = AppState {
        config,
        started_at: Arc::new(Instant::now()),
        session_engine,
        turn_executor,
        session_repo: session_repo as Arc<dyn SessionRepo>,
        transcript_repo: transcript_repo as Arc<dyn TranscriptRepo>,
        turn_repo: turn_repo.clone() as Arc<dyn TurnRepo>,
        model_provider,
        scheduler,
        heartbeat: Arc::new(HeartbeatRunner::new(std::env::temp_dir())),
        reminder_store: Arc::new(ReminderStore::new()),
        approval_repo: approval_repo as Arc<dyn ApprovalRepo>,
        tool_approval_repo: Arc::new(MemToolApprovalPolicyRepo::new())
            as Arc<dyn ToolApprovalPolicyRepo>,
        process_manager: ProcessManager::new(),
        capabilities: test_capabilities(0),
        device_repo: device_repo.clone() as Arc<dyn DeviceRepo>,
        device_registry,
        skill_registry,
        skill_loader,
        plugin_registry,
        plugin_loader,
        hook_registry,
        event_tx,
        tts_engine: None,
        stt_engine: None,
    };

    (build_router(state, auth_token), device_repo)
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
async fn ws_rpc_status_matches_http_status_basics() {
    use rune_gateway::ws_rpc::RpcDispatcher;
    use rune_runtime::{Lane, LaneQueue};

    let session_repo = Arc::new(MemSessionRepo::new());
    let turn_repo = Arc::new(MemTurnRepo::new());
    let transcript_repo = Arc::new(MemTranscriptRepo::new());
    let approval_repo = Arc::new(MemApprovalRepo::new());
    let model_provider: Arc<dyn ModelProvider> = Arc::new(FakeModelProvider);
    let scheduler = Arc::new(Scheduler::new());
    let session_engine = Arc::new(
        SessionEngine::new(session_repo.clone()).with_transcript_repo(transcript_repo.clone()),
    );
    let context_assembler = ContextAssembler::new("You are a test assistant.");
    let compaction: Arc<dyn CompactionStrategy> = Arc::new(NoOpCompaction);
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(FakeToolExecutor);
    let tool_registry = Arc::new(ToolRegistry::new());
    let lane_queue = Arc::new(LaneQueue::with_capacities(4, 8, 16));
    let turn_executor = Arc::new(
        TurnExecutor::new(
            session_repo.clone() as Arc<dyn SessionRepo>,
            turn_repo.clone() as Arc<dyn TurnRepo>,
            transcript_repo.clone() as Arc<dyn TranscriptRepo>,
            approval_repo.clone() as Arc<dyn ApprovalRepo>,
            model_provider.clone(),
            tool_executor,
            tool_registry,
            context_assembler,
            compaction,
        )
        .with_default_model("fake-model")
        .with_lane_queue(lane_queue.clone()),
    );
    let (event_tx, _) = broadcast::channel::<SessionEvent>(64);
    let skill_registry = Arc::new(SkillRegistry::new());
    let skill_loader = Arc::new(SkillLoader::new(
        std::env::temp_dir(),
        skill_registry.clone(),
    ));
    let device_repo = Arc::new(MemDeviceRepo::new());
    let device_registry = Arc::new(DeviceRegistry::new(device_repo.clone()));
    let (plugin_registry, plugin_loader, hook_registry) = test_plugins();

    let state = AppState {
        config: Arc::new(RwLock::new(AppConfig::default())),
        started_at: Arc::new(Instant::now()),
        session_engine,
        turn_executor,
        session_repo: session_repo as Arc<dyn SessionRepo>,
        transcript_repo: transcript_repo as Arc<dyn TranscriptRepo>,
        turn_repo: turn_repo as Arc<dyn TurnRepo>,
        model_provider,
        scheduler,
        heartbeat: Arc::new(HeartbeatRunner::new(std::env::temp_dir())),
        reminder_store: Arc::new(ReminderStore::new()),
        approval_repo: approval_repo as Arc<dyn ApprovalRepo>,
        tool_approval_repo: Arc::new(MemToolApprovalPolicyRepo::new())
            as Arc<dyn ToolApprovalPolicyRepo>,
        process_manager: ProcessManager::new(),
        capabilities: test_capabilities(3),
        device_repo: device_repo.clone() as Arc<dyn DeviceRepo>,
        device_registry,
        skill_registry,
        skill_loader,
        plugin_registry,
        plugin_loader,
        hook_registry,
        event_tx,
        tts_engine: None,
        stt_engine: None,
    };

    let main_permit = lane_queue.acquire(Lane::Main).await;

    let dispatcher = RpcDispatcher::new(state);
    let payload = dispatcher
        .dispatch("status", serde_json::json!({}))
        .await
        .unwrap();

    assert_eq!(payload["status"], "running");
    assert!(payload["version"].is_string());
    assert_eq!(payload["registered_tools"], 3);
    assert!(payload["ws_subscribers"].is_number());
    assert!(payload["config_paths"].is_object());
    assert_eq!(payload["lane_stats"]["main_active"], 1);
    assert_eq!(payload["lane_stats"]["main_capacity"], 4);
    assert_eq!(payload["lane_stats"]["subagent_active"], 0);
    assert_eq!(payload["lane_stats"]["cron_capacity"], 16);

    drop(main_permit);
}

#[tokio::test]
async fn ws_rpc_skills_reload_and_toggle_round_trip() {
    use rune_gateway::ws_rpc::RpcDispatcher;

    let session_repo = Arc::new(MemSessionRepo::new());
    let turn_repo = Arc::new(MemTurnRepo::new());
    let transcript_repo = Arc::new(MemTranscriptRepo::new());
    let approval_repo = Arc::new(MemApprovalRepo::new());
    let model_provider: Arc<dyn ModelProvider> = Arc::new(FakeModelProvider);
    let scheduler = Arc::new(Scheduler::new());
    let session_engine = Arc::new(
        SessionEngine::new(session_repo.clone()).with_transcript_repo(transcript_repo.clone()),
    );
    let context_assembler = ContextAssembler::new("You are a test assistant.");
    let compaction: Arc<dyn CompactionStrategy> = Arc::new(NoOpCompaction);
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(FakeToolExecutor);
    let tool_registry = Arc::new(ToolRegistry::new());
    let turn_executor = Arc::new(
        TurnExecutor::new(
            session_repo.clone() as Arc<dyn SessionRepo>,
            turn_repo.clone() as Arc<dyn TurnRepo>,
            transcript_repo.clone() as Arc<dyn TranscriptRepo>,
            approval_repo.clone() as Arc<dyn ApprovalRepo>,
            model_provider.clone(),
            tool_executor,
            tool_registry,
            context_assembler,
            compaction,
        )
        .with_default_model("fake-model"),
    );
    let (event_tx, _) = broadcast::channel::<SessionEvent>(64);
    let skill_registry = Arc::new(SkillRegistry::new());
    let skills_dir = std::env::temp_dir().join(format!("rune-ws-skills-{}", Uuid::now_v7()));
    std::fs::create_dir_all(skills_dir.join("rpc-skill")).unwrap();
    std::fs::write(
        skills_dir.join("rpc-skill/SKILL.md"),
        r#"---
name: rpc-skill
description: RPC managed skill
enabled: true
---

# RPC Skill
"#,
    )
    .unwrap();
    let skill_loader = Arc::new(SkillLoader::new(skills_dir, skill_registry.clone()));
    let device_repo = Arc::new(MemDeviceRepo::new());
    let device_registry = Arc::new(DeviceRegistry::new(device_repo.clone()));
    let (plugin_registry, plugin_loader, hook_registry) = test_plugins();

    let state = AppState {
        config: Arc::new(RwLock::new(AppConfig::default())),
        started_at: Arc::new(Instant::now()),
        session_engine,
        turn_executor,
        session_repo: session_repo as Arc<dyn SessionRepo>,
        transcript_repo: transcript_repo as Arc<dyn TranscriptRepo>,
        turn_repo: turn_repo as Arc<dyn TurnRepo>,
        model_provider,
        scheduler,
        heartbeat: Arc::new(HeartbeatRunner::new(std::env::temp_dir())),
        reminder_store: Arc::new(ReminderStore::new()),
        approval_repo: approval_repo as Arc<dyn ApprovalRepo>,
        tool_approval_repo: Arc::new(MemToolApprovalPolicyRepo::new())
            as Arc<dyn ToolApprovalPolicyRepo>,
        process_manager: ProcessManager::new(),
        capabilities: test_capabilities(0),
        device_repo: device_repo.clone() as Arc<dyn DeviceRepo>,
        device_registry,
        skill_registry,
        skill_loader,
        plugin_registry,
        plugin_loader,
        hook_registry,
        event_tx,
        tts_engine: None,
        stt_engine: None,
    };

    let dispatcher = RpcDispatcher::new(state);

    let reload = dispatcher
        .dispatch("skills.reload", serde_json::json!({}))
        .await
        .unwrap();
    assert_eq!(reload["success"], true);
    assert_eq!(reload["loaded"], 1);

    let listed = dispatcher
        .dispatch("skills.list", serde_json::json!({}))
        .await
        .unwrap();
    let items = listed.as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["name"], "rpc-skill");
    assert_eq!(items[0]["enabled"], true);

    let disabled = dispatcher
        .dispatch("skills.disable", serde_json::json!({"name": "rpc-skill"}))
        .await
        .unwrap();
    assert_eq!(disabled["enabled"], false);

    let enabled = dispatcher
        .dispatch("skills.enable", serde_json::json!({"name": "rpc-skill"}))
        .await
        .unwrap();
    assert_eq!(enabled["enabled"], true);

    let err = dispatcher
        .dispatch("skills.enable", serde_json::json!({"name": "missing"}))
        .await
        .unwrap_err();
    assert_eq!(err.code, "not_found");
}

#[tokio::test]
async fn status_reports_configured_lane_capacities() {
    let session_repo = Arc::new(MemSessionRepo::new());
    let turn_repo = Arc::new(MemTurnRepo::new());
    let transcript_repo = Arc::new(MemTranscriptRepo::new());
    let approval_repo = Arc::new(MemApprovalRepo::new());
    let model_provider: Arc<dyn ModelProvider> = Arc::new(FakeModelProvider);
    let scheduler = Arc::new(Scheduler::new());
    let session_engine = Arc::new(
        SessionEngine::new(session_repo.clone()).with_transcript_repo(transcript_repo.clone()),
    );
    let context_assembler = ContextAssembler::new("You are a test assistant.");
    let compaction: Arc<dyn CompactionStrategy> = Arc::new(NoOpCompaction);
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(FakeToolExecutor);
    let tool_registry = Arc::new(ToolRegistry::new());
    let lane_queue = Arc::new(rune_runtime::LaneQueue::with_capacities(6, 9, 128));
    let turn_executor = Arc::new(
        TurnExecutor::new(
            session_repo.clone() as Arc<dyn SessionRepo>,
            turn_repo.clone() as Arc<dyn TurnRepo>,
            transcript_repo.clone() as Arc<dyn TranscriptRepo>,
            approval_repo.clone() as Arc<dyn ApprovalRepo>,
            model_provider.clone(),
            tool_executor,
            tool_registry,
            context_assembler,
            compaction,
        )
        .with_default_model("fake-model")
        .with_lane_queue(lane_queue),
    );
    let (event_tx, _) = broadcast::channel::<SessionEvent>(64);
    let skill_registry = Arc::new(SkillRegistry::new());
    let skill_loader = Arc::new(SkillLoader::new(
        std::env::temp_dir(),
        skill_registry.clone(),
    ));

    let mut config = AppConfig::default();
    config.runtime.lanes = LaneQueueConfig {
        main_capacity: 6,
        subagent_capacity: 9,
        cron_capacity: 128,
    };
    let device_repo = Arc::new(MemDeviceRepo::new());
    let device_registry = Arc::new(DeviceRegistry::new(device_repo.clone()));
    let (plugin_registry, plugin_loader, hook_registry) = test_plugins();

    let state = AppState {
        config: Arc::new(RwLock::new(config)),
        started_at: Arc::new(Instant::now()),
        session_engine,
        turn_executor,
        session_repo: session_repo as Arc<dyn SessionRepo>,
        transcript_repo: transcript_repo as Arc<dyn TranscriptRepo>,
        turn_repo: turn_repo as Arc<dyn TurnRepo>,
        model_provider,
        scheduler,
        heartbeat: Arc::new(HeartbeatRunner::new(std::env::temp_dir())),
        reminder_store: Arc::new(ReminderStore::new()),
        approval_repo: approval_repo as Arc<dyn ApprovalRepo>,
        tool_approval_repo: Arc::new(MemToolApprovalPolicyRepo::new())
            as Arc<dyn ToolApprovalPolicyRepo>,
        process_manager: ProcessManager::new(),
        capabilities: test_capabilities(0),
        device_repo: device_repo.clone() as Arc<dyn DeviceRepo>,
        device_registry,
        skill_registry,
        skill_loader,
        plugin_registry,
        plugin_loader,
        hook_registry,
        event_tx,
        tts_engine: None,
        stt_engine: None,
    };

    let app = build_router(state, None);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload["lane_stats"]["main_capacity"], 6);
    assert_eq!(payload["lane_stats"]["subagent_capacity"], 9);
    assert_eq!(payload["lane_stats"]["cron_capacity"], 128);
}

#[tokio::test]
async fn ws_rpc_runtime_lanes_reports_lane_queue_stats() {
    use rune_gateway::ws_rpc::RpcDispatcher;
    use rune_runtime::{Lane, LaneQueue};

    let session_repo = Arc::new(MemSessionRepo::new());
    let turn_repo = Arc::new(MemTurnRepo::new());
    let transcript_repo = Arc::new(MemTranscriptRepo::new());
    let approval_repo = Arc::new(MemApprovalRepo::new());
    let model_provider: Arc<dyn ModelProvider> = Arc::new(FakeModelProvider);
    let scheduler = Arc::new(Scheduler::new());
    let session_engine = Arc::new(
        SessionEngine::new(session_repo.clone()).with_transcript_repo(transcript_repo.clone()),
    );
    let context_assembler = ContextAssembler::new("You are a test assistant.");
    let compaction: Arc<dyn CompactionStrategy> = Arc::new(NoOpCompaction);
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(FakeToolExecutor);
    let tool_registry = Arc::new(ToolRegistry::new());
    let lane_queue = Arc::new(LaneQueue::with_capacities(2, 3, 4));
    let turn_executor = Arc::new(
        TurnExecutor::new(
            session_repo.clone() as Arc<dyn SessionRepo>,
            turn_repo.clone() as Arc<dyn TurnRepo>,
            transcript_repo.clone() as Arc<dyn TranscriptRepo>,
            approval_repo.clone() as Arc<dyn ApprovalRepo>,
            model_provider.clone(),
            tool_executor,
            tool_registry,
            context_assembler,
            compaction,
        )
        .with_default_model("fake-model")
        .with_lane_queue(lane_queue.clone()),
    );
    let (event_tx, _) = broadcast::channel::<SessionEvent>(64);
    let skill_registry = Arc::new(SkillRegistry::new());
    let skill_loader = Arc::new(SkillLoader::new(
        std::env::temp_dir(),
        skill_registry.clone(),
    ));
    let device_repo = Arc::new(MemDeviceRepo::new());
    let device_registry = Arc::new(DeviceRegistry::new(device_repo.clone()));
    let (plugin_registry, plugin_loader, hook_registry) = test_plugins();

    let state = AppState {
        config: Arc::new(RwLock::new(AppConfig::default())),
        started_at: Arc::new(Instant::now()),
        session_engine,
        turn_executor,
        session_repo: session_repo as Arc<dyn SessionRepo>,
        transcript_repo: transcript_repo as Arc<dyn TranscriptRepo>,
        turn_repo: turn_repo as Arc<dyn TurnRepo>,
        model_provider,
        scheduler,
        heartbeat: Arc::new(HeartbeatRunner::new(std::env::temp_dir())),
        reminder_store: Arc::new(ReminderStore::new()),
        approval_repo: approval_repo as Arc<dyn ApprovalRepo>,
        tool_approval_repo: Arc::new(MemToolApprovalPolicyRepo::new())
            as Arc<dyn ToolApprovalPolicyRepo>,
        process_manager: ProcessManager::new(),
        capabilities: test_capabilities(0),
        device_repo: device_repo.clone() as Arc<dyn DeviceRepo>,
        device_registry,
        skill_registry,
        skill_loader,
        plugin_registry,
        plugin_loader,
        hook_registry,
        event_tx,
        tts_engine: None,
        stt_engine: None,
    };

    let main_permit = lane_queue.acquire(Lane::Main).await;
    let subagent_permit = lane_queue.acquire(Lane::Subagent).await;

    let dispatcher = RpcDispatcher::new(state);
    let payload = dispatcher
        .dispatch("runtime.lanes", serde_json::json!({}))
        .await
        .unwrap();

    assert_eq!(payload["enabled"], true);
    assert_eq!(payload["lanes"]["main"]["active"], 1);
    assert_eq!(payload["lanes"]["main"]["capacity"], 2);
    assert_eq!(payload["lanes"]["subagent"]["active"], 1);
    assert_eq!(payload["lanes"]["subagent"]["capacity"], 3);
    assert_eq!(payload["lanes"]["cron"]["active"], 0);
    assert_eq!(payload["lanes"]["cron"]["capacity"], 4);

    drop(main_permit);
    drop(subagent_permit);
}

#[tokio::test]
async fn ws_rpc_health_reports_session_count() {
    use rune_gateway::ws_rpc::RpcDispatcher;

    let session_repo = Arc::new(MemSessionRepo::new());
    let turn_repo = Arc::new(MemTurnRepo::new());
    let transcript_repo = Arc::new(MemTranscriptRepo::new());
    let approval_repo = Arc::new(MemApprovalRepo::new());
    let model_provider: Arc<dyn ModelProvider> = Arc::new(FakeModelProvider);
    let scheduler = Arc::new(Scheduler::new());
    let session_engine = Arc::new(
        SessionEngine::new(session_repo.clone()).with_transcript_repo(transcript_repo.clone()),
    );
    let context_assembler = ContextAssembler::new("You are a test assistant.");
    let compaction: Arc<dyn CompactionStrategy> = Arc::new(NoOpCompaction);
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(FakeToolExecutor);
    let tool_registry = Arc::new(ToolRegistry::new());
    let turn_executor = Arc::new(
        TurnExecutor::new(
            session_repo.clone() as Arc<dyn SessionRepo>,
            turn_repo.clone() as Arc<dyn TurnRepo>,
            transcript_repo.clone() as Arc<dyn TranscriptRepo>,
            approval_repo.clone() as Arc<dyn ApprovalRepo>,
            model_provider.clone(),
            tool_executor,
            tool_registry,
            context_assembler,
            compaction,
        )
        .with_default_model("fake-model"),
    );
    let (event_tx, _) = broadcast::channel::<SessionEvent>(64);
    let skill_registry = Arc::new(SkillRegistry::new());
    let skill_loader = Arc::new(SkillLoader::new(
        std::env::temp_dir(),
        skill_registry.clone(),
    ));

    let now = chrono::Utc::now();
    session_repo
        .create(NewSession {
            id: Uuid::now_v7(),
            kind: "direct".into(),
            status: "created".into(),
            workspace_root: None,
            channel_ref: None,
            requester_session_id: None,
            latest_turn_id: None,
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
            kind: "subagent".into(),
            status: "running".into(),
            workspace_root: None,
            channel_ref: None,
            requester_session_id: None,
            latest_turn_id: None,
            metadata: serde_json::json!({}),
            created_at: now,
            updated_at: now,
            last_activity_at: now,
        })
        .await
        .unwrap();
    let device_repo = Arc::new(MemDeviceRepo::new());
    let device_registry = Arc::new(DeviceRegistry::new(device_repo.clone()));
    let (plugin_registry, plugin_loader, hook_registry) = test_plugins();

    let state = AppState {
        config: Arc::new(RwLock::new(AppConfig::default())),
        started_at: Arc::new(Instant::now()),
        session_engine,
        turn_executor,
        session_repo: session_repo as Arc<dyn SessionRepo>,
        transcript_repo: transcript_repo as Arc<dyn TranscriptRepo>,
        turn_repo: turn_repo as Arc<dyn TurnRepo>,
        model_provider,
        scheduler,
        heartbeat: Arc::new(HeartbeatRunner::new(std::env::temp_dir())),
        reminder_store: Arc::new(ReminderStore::new()),
        approval_repo: approval_repo as Arc<dyn ApprovalRepo>,
        tool_approval_repo: Arc::new(MemToolApprovalPolicyRepo::new())
            as Arc<dyn ToolApprovalPolicyRepo>,
        process_manager: ProcessManager::new(),
        capabilities: test_capabilities(0),
        device_repo: device_repo.clone() as Arc<dyn DeviceRepo>,
        device_registry,
        skill_registry,
        skill_loader,
        plugin_registry,
        plugin_loader,
        hook_registry,
        event_tx,
        tts_engine: None,
        stt_engine: None,
    };

    let dispatcher = RpcDispatcher::new(state);
    let payload = dispatcher
        .dispatch("health", serde_json::json!({}))
        .await
        .unwrap();

    assert_eq!(payload["status"], "ok");
    assert_eq!(payload["service"], "rune-gateway");
    assert_eq!(payload["session_count"], 2);
    assert_eq!(payload["ws_connections"], 0);
}

#[tokio::test]
async fn ws_rpc_cron_list_and_get_surface_delivery_mode() {
    use rune_gateway::ws_rpc::RpcDispatcher;

    let session_repo = Arc::new(MemSessionRepo::new());
    let turn_repo = Arc::new(MemTurnRepo::new());
    let transcript_repo = Arc::new(MemTranscriptRepo::new());
    let approval_repo = Arc::new(MemApprovalRepo::new());
    let model_provider: Arc<dyn ModelProvider> = Arc::new(FakeModelProvider);
    let scheduler = Arc::new(Scheduler::new());
    let session_engine = Arc::new(
        SessionEngine::new(session_repo.clone()).with_transcript_repo(transcript_repo.clone()),
    );
    let context_assembler = ContextAssembler::new("You are a test assistant.");
    let compaction: Arc<dyn CompactionStrategy> = Arc::new(NoOpCompaction);
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(FakeToolExecutor);
    let tool_registry = Arc::new(ToolRegistry::new());
    let turn_executor = Arc::new(
        TurnExecutor::new(
            session_repo.clone() as Arc<dyn SessionRepo>,
            turn_repo.clone() as Arc<dyn TurnRepo>,
            transcript_repo.clone() as Arc<dyn TranscriptRepo>,
            approval_repo.clone() as Arc<dyn ApprovalRepo>,
            model_provider.clone(),
            tool_executor,
            tool_registry,
            context_assembler,
            compaction,
        )
        .with_default_model("fake-model"),
    );
    let (event_tx, _) = broadcast::channel::<SessionEvent>(64);
    let skill_registry = Arc::new(SkillRegistry::new());
    let skill_loader = Arc::new(SkillLoader::new(
        std::env::temp_dir(),
        skill_registry.clone(),
    ));
    let device_repo = Arc::new(MemDeviceRepo::new());
    let device_registry = Arc::new(DeviceRegistry::new(device_repo.clone()));
    let (plugin_registry, plugin_loader, hook_registry) = test_plugins();
    let now = chrono::Utc::now();
    let job_id = scheduler
        .add_job(rune_runtime::scheduler::Job {
            id: rune_core::JobId::new(),
            name: Some("daily-check".into()),
            schedule: rune_runtime::scheduler::Schedule::Cron {
                expr: "0 0 9 * * *".into(),
                tz: Some("UTC".into()),
            },
            payload: rune_runtime::scheduler::JobPayload::SystemEvent {
                text: "run daily check".into(),
            },
            delivery_mode: rune_core::SchedulerDeliveryMode::Announce,
            webhook_url: None,
            session_target: rune_runtime::scheduler::SessionTarget::Main,
            enabled: true,
            created_at: now,
            last_run_at: None,
            next_run_at: Some(now),
            run_count: 0,
        })
        .await;

    let state = AppState {
        config: Arc::new(RwLock::new(AppConfig::default())),
        started_at: Arc::new(Instant::now()),
        session_engine,
        turn_executor,
        session_repo: session_repo as Arc<dyn SessionRepo>,
        transcript_repo: transcript_repo as Arc<dyn TranscriptRepo>,
        turn_repo: turn_repo as Arc<dyn TurnRepo>,
        model_provider,
        scheduler,
        heartbeat: Arc::new(HeartbeatRunner::new(std::env::temp_dir())),
        reminder_store: Arc::new(ReminderStore::new()),
        approval_repo: approval_repo as Arc<dyn ApprovalRepo>,
        tool_approval_repo: Arc::new(MemToolApprovalPolicyRepo::new())
            as Arc<dyn ToolApprovalPolicyRepo>,
        process_manager: ProcessManager::new(),
        capabilities: test_capabilities(0),
        device_repo: device_repo.clone() as Arc<dyn DeviceRepo>,
        device_registry,
        skill_registry,
        skill_loader,
        plugin_registry,
        plugin_loader,
        hook_registry,
        event_tx,
        tts_engine: None,
        stt_engine: None,
    };

    let dispatcher = RpcDispatcher::new(state);
    let list = dispatcher
        .dispatch("cron.list", serde_json::json!({ "includeDisabled": true }))
        .await
        .unwrap();
    let items = list.as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["delivery_mode"], "announce");
    assert_eq!(items[0]["schedule"]["kind"], "cron");
    assert_eq!(items[0]["payload"]["kind"], "system_event");

    let detail = dispatcher
        .dispatch("cron.get", serde_json::json!({ "id": job_id.to_string() }))
        .await
        .unwrap();
    assert_eq!(detail["id"], job_id.to_string());
    assert_eq!(detail["delivery_mode"], "announce");
    assert_eq!(detail["session_target"], "main");
}

#[tokio::test]
async fn ws_rpc_session_status_surfaces_defaults_and_usage() {
    use rune_gateway::ws_rpc::RpcDispatcher;

    let session_repo = Arc::new(MemSessionRepo::new());
    let turn_repo = Arc::new(MemTurnRepo::new());
    let transcript_repo = Arc::new(MemTranscriptRepo::new());
    let approval_repo = Arc::new(MemApprovalRepo::new());
    let model_provider: Arc<dyn ModelProvider> = Arc::new(FakeModelProvider);
    let scheduler = Arc::new(Scheduler::new());
    let session_engine = Arc::new(
        SessionEngine::new(session_repo.clone()).with_transcript_repo(transcript_repo.clone()),
    );
    let context_assembler = ContextAssembler::new("You are a test assistant.");
    let compaction: Arc<dyn CompactionStrategy> = Arc::new(NoOpCompaction);
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(FakeToolExecutor);
    let tool_registry = Arc::new(ToolRegistry::new());
    let turn_executor = Arc::new(
        TurnExecutor::new(
            session_repo.clone() as Arc<dyn SessionRepo>,
            turn_repo.clone() as Arc<dyn TurnRepo>,
            transcript_repo.clone() as Arc<dyn TranscriptRepo>,
            approval_repo.clone() as Arc<dyn ApprovalRepo>,
            model_provider.clone(),
            tool_executor,
            tool_registry,
            context_assembler,
            compaction,
        )
        .with_default_model("fake-model"),
    );
    let (event_tx, _) = broadcast::channel::<SessionEvent>(64);
    let skill_registry = Arc::new(SkillRegistry::new());
    let skill_loader = Arc::new(SkillLoader::new(
        std::env::temp_dir(),
        skill_registry.clone(),
    ));

    let session_id = Uuid::now_v7();
    let now = chrono::Utc::now();
    session_repo
        .create(NewSession {
            id: session_id,
            kind: "direct".into(),
            status: "created".into(),
            workspace_root: None,
            channel_ref: Some("telegram:chat-1".into()),
            requester_session_id: None,
            latest_turn_id: None,
            metadata: serde_json::json!({}),
            created_at: now,
            updated_at: now,
            last_activity_at: now,
        })
        .await
        .unwrap();
    turn_repo
        .create(NewTurn {
            id: Uuid::now_v7(),
            session_id,
            trigger_kind: "user_message".into(),
            status: "completed".into(),
            model_ref: Some("fake-model".into()),
            started_at: now,
            ended_at: Some(now),
            usage_prompt_tokens: Some(12),
            usage_completion_tokens: Some(7),
        })
        .await
        .unwrap();
    let device_repo = Arc::new(MemDeviceRepo::new());
    let device_registry = Arc::new(DeviceRegistry::new(device_repo.clone()));
    let (plugin_registry, plugin_loader, hook_registry) = test_plugins();

    let state = AppState {
        config: Arc::new(RwLock::new(AppConfig::default())),
        started_at: Arc::new(Instant::now()),
        session_engine,
        turn_executor,
        session_repo: session_repo as Arc<dyn SessionRepo>,
        transcript_repo: transcript_repo as Arc<dyn TranscriptRepo>,
        turn_repo: turn_repo.clone() as Arc<dyn TurnRepo>,
        model_provider,
        scheduler,
        heartbeat: Arc::new(HeartbeatRunner::new(std::env::temp_dir())),
        reminder_store: Arc::new(ReminderStore::new()),
        approval_repo: approval_repo as Arc<dyn ApprovalRepo>,
        tool_approval_repo: Arc::new(MemToolApprovalPolicyRepo::new())
            as Arc<dyn ToolApprovalPolicyRepo>,
        process_manager: ProcessManager::new(),
        capabilities: test_capabilities(0),
        device_repo: device_repo.clone() as Arc<dyn DeviceRepo>,
        device_registry,
        skill_registry,
        skill_loader,
        plugin_registry,
        plugin_loader,
        hook_registry,
        event_tx,
        tts_engine: None,
        stt_engine: None,
    };

    let dispatcher = RpcDispatcher::new(state);
    let payload = dispatcher
        .dispatch(
            "session.status",
            serde_json::json!({ "session_id": session_id }),
        )
        .await
        .unwrap();

    assert_eq!(payload["session_id"], session_id.to_string());
    assert_eq!(payload["channel_ref"], "telegram:chat-1");
    assert_eq!(
        payload["runtime"],
        "kind=direct | channel=telegram:chat-1 | status=created"
    );
    assert_eq!(payload["current_model"], "fake-model");
    assert_eq!(payload["model_override"], Value::Null);
    assert_eq!(payload["turn_count"], 1);
    assert_eq!(payload["prompt_tokens"], 12);
    assert_eq!(payload["completion_tokens"], 7);
    assert_eq!(payload["total_tokens"], 19);
    assert_eq!(payload["estimated_cost"], "not available");
    assert_eq!(payload["approval_mode"], "on-miss");
    assert_eq!(payload["security_mode"], "allowlist");
    assert_eq!(payload["reasoning"], "off");
    assert_eq!(payload["verbose"], false);
    assert_eq!(payload["elevated"], false);
    assert!(payload["last_turn_started_at"].is_string());
    assert!(payload["last_turn_ended_at"].is_string());
    let unresolved = payload["unresolved"].as_array().unwrap();
    assert!(unresolved.iter().any(|item| item.as_str()
        == Some("cost posture is estimate-only; provider pricing is not wired yet")));
    assert!(unresolved.iter().any(|item| item.as_str() == Some("approval requests and operator-triggered resume are durable, but restart-safe continuation for mid-resume approval flows is not parity-complete yet")));
    assert!(unresolved.iter().any(|item| item.as_str()
        == Some("host/node/sandbox parity and PTY fidelity are not yet parity-complete")));
}

#[tokio::test]
async fn ws_rpc_session_get_includes_last_turn_timestamps() {
    use rune_gateway::ws_rpc::RpcDispatcher;

    let session_repo = Arc::new(MemSessionRepo::new());
    let turn_repo = Arc::new(MemTurnRepo::new());
    let transcript_repo = Arc::new(MemTranscriptRepo::new());
    let approval_repo = Arc::new(MemApprovalRepo::new());
    let model_provider: Arc<dyn ModelProvider> = Arc::new(FakeModelProvider);
    let scheduler = Arc::new(Scheduler::new());
    let session_engine = Arc::new(
        SessionEngine::new(session_repo.clone()).with_transcript_repo(transcript_repo.clone()),
    );
    let context_assembler = ContextAssembler::new("You are a test assistant.");
    let compaction: Arc<dyn CompactionStrategy> = Arc::new(NoOpCompaction);
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(FakeToolExecutor);
    let tool_registry = Arc::new(ToolRegistry::new());
    let turn_executor = Arc::new(
        TurnExecutor::new(
            session_repo.clone() as Arc<dyn SessionRepo>,
            turn_repo.clone() as Arc<dyn TurnRepo>,
            transcript_repo.clone() as Arc<dyn TranscriptRepo>,
            approval_repo.clone() as Arc<dyn ApprovalRepo>,
            model_provider.clone(),
            tool_executor,
            tool_registry,
            context_assembler,
            compaction,
        )
        .with_default_model("fake-model"),
    );
    let (event_tx, _) = broadcast::channel::<SessionEvent>(64);
    let skill_registry = Arc::new(SkillRegistry::new());
    let skill_loader = Arc::new(SkillLoader::new(
        std::env::temp_dir(),
        skill_registry.clone(),
    ));

    let session_id = Uuid::now_v7();
    let now = chrono::Utc::now();
    session_repo
        .create(NewSession {
            id: session_id,
            kind: "direct".into(),
            status: "created".into(),
            workspace_root: None,
            channel_ref: None,
            requester_session_id: None,
            latest_turn_id: None,
            metadata: serde_json::json!({}),
            created_at: now,
            updated_at: now,
            last_activity_at: now,
        })
        .await
        .unwrap();
    turn_repo
        .create(NewTurn {
            id: Uuid::now_v7(),
            session_id,
            trigger_kind: "user_message".into(),
            status: "completed".into(),
            model_ref: Some("fake-model".into()),
            started_at: now,
            ended_at: Some(now),
            usage_prompt_tokens: Some(3),
            usage_completion_tokens: Some(4),
        })
        .await
        .unwrap();
    let device_repo = Arc::new(MemDeviceRepo::new());
    let device_registry = Arc::new(DeviceRegistry::new(device_repo.clone()));
    let (plugin_registry, plugin_loader, hook_registry) = test_plugins();

    let state = AppState {
        config: Arc::new(RwLock::new(AppConfig::default())),
        started_at: Arc::new(Instant::now()),
        session_engine,
        turn_executor,
        session_repo: session_repo as Arc<dyn SessionRepo>,
        transcript_repo: transcript_repo as Arc<dyn TranscriptRepo>,
        turn_repo: turn_repo.clone() as Arc<dyn TurnRepo>,
        model_provider,
        scheduler,
        heartbeat: Arc::new(HeartbeatRunner::new(std::env::temp_dir())),
        reminder_store: Arc::new(ReminderStore::new()),
        approval_repo: approval_repo as Arc<dyn ApprovalRepo>,
        tool_approval_repo: Arc::new(MemToolApprovalPolicyRepo::new())
            as Arc<dyn ToolApprovalPolicyRepo>,
        process_manager: ProcessManager::new(),
        capabilities: test_capabilities(0),
        device_repo: device_repo.clone() as Arc<dyn DeviceRepo>,
        device_registry,
        skill_registry,
        skill_loader,
        plugin_registry,
        plugin_loader,
        hook_registry,
        event_tx,
        tts_engine: None,
        stt_engine: None,
    };

    let dispatcher = RpcDispatcher::new(state);
    let payload = dispatcher
        .dispatch(
            "session.get",
            serde_json::json!({ "session_id": session_id.to_string() }),
        )
        .await
        .unwrap();

    assert_eq!(payload["id"], session_id.to_string());
    assert_eq!(payload["latest_model"], "fake-model");
    assert_eq!(payload["usage_prompt_tokens"], 3);
    assert_eq!(payload["usage_completion_tokens"], 4);
    assert!(payload["last_turn_started_at"].is_string());
    assert!(payload["last_turn_ended_at"].is_string());
}

#[tokio::test]
async fn ws_rpc_session_status_rejects_invalid_uuid() {
    use rune_gateway::ws_rpc::RpcDispatcher;

    let session_repo = Arc::new(MemSessionRepo::new());
    let turn_repo = Arc::new(MemTurnRepo::new());
    let transcript_repo = Arc::new(MemTranscriptRepo::new());
    let approval_repo = Arc::new(MemApprovalRepo::new());
    let model_provider: Arc<dyn ModelProvider> = Arc::new(FakeModelProvider);
    let scheduler = Arc::new(Scheduler::new());
    let session_engine = Arc::new(
        SessionEngine::new(session_repo.clone()).with_transcript_repo(transcript_repo.clone()),
    );
    let context_assembler = ContextAssembler::new("You are a test assistant.");
    let compaction: Arc<dyn CompactionStrategy> = Arc::new(NoOpCompaction);
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(FakeToolExecutor);
    let tool_registry = Arc::new(ToolRegistry::new());
    let turn_executor = Arc::new(
        TurnExecutor::new(
            session_repo.clone() as Arc<dyn SessionRepo>,
            turn_repo.clone() as Arc<dyn TurnRepo>,
            transcript_repo.clone() as Arc<dyn TranscriptRepo>,
            approval_repo.clone() as Arc<dyn ApprovalRepo>,
            model_provider.clone(),
            tool_executor,
            tool_registry,
            context_assembler,
            compaction,
        )
        .with_default_model("fake-model"),
    );
    let (event_tx, _) = broadcast::channel::<SessionEvent>(64);
    let skill_registry = Arc::new(SkillRegistry::new());
    let skill_loader = Arc::new(SkillLoader::new(
        std::env::temp_dir(),
        skill_registry.clone(),
    ));
    let device_repo = Arc::new(MemDeviceRepo::new());
    let device_registry = Arc::new(DeviceRegistry::new(device_repo.clone()));
    let (plugin_registry, plugin_loader, hook_registry) = test_plugins();

    let state = AppState {
        config: Arc::new(RwLock::new(AppConfig::default())),
        started_at: Arc::new(Instant::now()),
        session_engine,
        turn_executor,
        session_repo: session_repo as Arc<dyn SessionRepo>,
        transcript_repo: transcript_repo as Arc<dyn TranscriptRepo>,
        turn_repo: turn_repo as Arc<dyn TurnRepo>,
        model_provider,
        scheduler,
        heartbeat: Arc::new(HeartbeatRunner::new(std::env::temp_dir())),
        reminder_store: Arc::new(ReminderStore::new()),
        approval_repo: approval_repo as Arc<dyn ApprovalRepo>,
        tool_approval_repo: Arc::new(MemToolApprovalPolicyRepo::new())
            as Arc<dyn ToolApprovalPolicyRepo>,
        process_manager: ProcessManager::new(),
        capabilities: test_capabilities(0),
        device_repo: device_repo.clone() as Arc<dyn DeviceRepo>,
        device_registry,
        skill_registry,
        skill_loader,
        plugin_registry,
        plugin_loader,
        hook_registry,
        event_tx,
        tts_engine: None,
        stt_engine: None,
    };

    let dispatcher = RpcDispatcher::new(state);
    let err = dispatcher
        .dispatch(
            "session.status",
            serde_json::json!({ "session_id": "not-a-uuid" }),
        )
        .await
        .unwrap_err();

    assert_eq!(err.code, "bad_request");
    assert!(err.message.contains("invalid UUID for session_id"));
}

#[tokio::test]
async fn ws_handle_text_message_subscribe_unsubscribe_and_errors() {
    use rune_gateway::ws::{ConnState, handle_text_message};
    use rune_gateway::ws_rpc::RpcDispatcher;

    let session_repo = Arc::new(MemSessionRepo::new());
    let turn_repo = Arc::new(MemTurnRepo::new());
    let transcript_repo = Arc::new(MemTranscriptRepo::new());
    let approval_repo = Arc::new(MemApprovalRepo::new());
    let model_provider: Arc<dyn ModelProvider> = Arc::new(FakeModelProvider);
    let scheduler = Arc::new(Scheduler::new());
    let session_engine = Arc::new(
        SessionEngine::new(session_repo.clone()).with_transcript_repo(transcript_repo.clone()),
    );
    let context_assembler = ContextAssembler::new("You are a test assistant.");
    let compaction: Arc<dyn CompactionStrategy> = Arc::new(NoOpCompaction);
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(FakeToolExecutor);
    let tool_registry = Arc::new(ToolRegistry::new());
    let turn_executor = Arc::new(
        TurnExecutor::new(
            session_repo.clone() as Arc<dyn SessionRepo>,
            turn_repo.clone() as Arc<dyn TurnRepo>,
            transcript_repo.clone() as Arc<dyn TranscriptRepo>,
            approval_repo.clone() as Arc<dyn ApprovalRepo>,
            model_provider.clone(),
            tool_executor,
            tool_registry,
            context_assembler,
            compaction,
        )
        .with_default_model("fake-model"),
    );
    let (event_tx, _) = broadcast::channel::<SessionEvent>(64);
    let skill_registry = Arc::new(SkillRegistry::new());
    let skill_loader = Arc::new(SkillLoader::new(
        std::env::temp_dir(),
        skill_registry.clone(),
    ));
    let device_repo = Arc::new(MemDeviceRepo::new());
    let device_registry = Arc::new(DeviceRegistry::new(device_repo.clone()));
    let (plugin_registry, plugin_loader, hook_registry) = test_plugins();

    let state = AppState {
        config: Arc::new(RwLock::new(AppConfig::default())),
        started_at: Arc::new(Instant::now()),
        session_engine,
        turn_executor,
        session_repo: session_repo as Arc<dyn SessionRepo>,
        transcript_repo: transcript_repo as Arc<dyn TranscriptRepo>,
        turn_repo: turn_repo as Arc<dyn TurnRepo>,
        model_provider,
        scheduler,
        heartbeat: Arc::new(HeartbeatRunner::new(std::env::temp_dir())),
        reminder_store: Arc::new(ReminderStore::new()),
        approval_repo: approval_repo as Arc<dyn ApprovalRepo>,
        tool_approval_repo: Arc::new(MemToolApprovalPolicyRepo::new())
            as Arc<dyn ToolApprovalPolicyRepo>,
        process_manager: ProcessManager::new(),
        capabilities: test_capabilities(0),
        device_repo: device_repo.clone() as Arc<dyn DeviceRepo>,
        device_registry,
        skill_registry,
        skill_loader,
        plugin_registry,
        plugin_loader,
        hook_registry,
        event_tx,
        tts_engine: None,
        stt_engine: None,
    };

    let dispatcher = RpcDispatcher::new(state);
    let mut conn = ConnState::new();
    let state_version = Arc::new(AtomicU64::new(1));
    // stateVersion in responses is driven by the process-global counter, not this per-call seed.

    let subscribe: String = handle_text_message(
        r#"{"type":"req","id":"1","method":"subscribe","params":{"session_id":"sess-1"}}"#,
        &mut conn,
        &dispatcher,
        &state_version,
    )
    .await
    .unwrap()
    .to_string();
    let subscribe_json: Value = serde_json::from_str(&subscribe).unwrap();
    assert_eq!(subscribe_json["type"], "res");
    assert_eq!(subscribe_json["id"], "1");
    assert_eq!(subscribe_json["ok"], true);
    assert_eq!(
        subscribe_json["payload"]["subscribed"]["session_id"],
        "sess-1"
    );
    assert_eq!(subscribe_json["payload"]["subscribed"]["all"], false);
    assert!(subscribe_json["stateVersion"].as_u64().unwrap() >= 1);

    let unsubscribe: String = handle_text_message(
        r#"{"type":"req","id":"2","method":"unsubscribe","params":{"session_id":"sess-1"}}"#,
        &mut conn,
        &dispatcher,
        &state_version,
    )
    .await
    .unwrap()
    .to_string();
    let unsubscribe_json: Value = serde_json::from_str(&unsubscribe).unwrap();
    assert_eq!(unsubscribe_json["ok"], true);
    assert_eq!(
        unsubscribe_json["payload"]["unsubscribed"]["session_id"],
        "sess-1"
    );
    assert_eq!(unsubscribe_json["payload"]["unsubscribed"]["all"], false);
    assert!(unsubscribe_json["stateVersion"].as_u64().unwrap() >= 1);

    let missing_session: String = handle_text_message(
        r#"{"type":"req","id":"3","method":"subscribe","params":{}}"#,
        &mut conn,
        &dispatcher,
        &state_version,
    )
    .await
    .unwrap()
    .to_string();
    let missing_json: Value = serde_json::from_str(&missing_session).unwrap();
    assert_eq!(missing_json["ok"], false);
    assert_eq!(missing_json["error"]["code"], "bad_request");
    assert!(
        missing_json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("missing subscription target")
    );

    let parse_error: String =
        handle_text_message("not-json", &mut conn, &dispatcher, &state_version)
            .await
            .unwrap()
            .to_string();
    let parse_json: Value = serde_json::from_str(&parse_error).unwrap();
    assert_eq!(parse_json["ok"], false);
    assert_eq!(parse_json["id"], "unknown");
    assert_eq!(parse_json["error"]["code"], "parse_error");
}

#[tokio::test]
async fn ws_handle_text_message_supports_event_and_global_subscriptions() {
    use rune_gateway::ws::{ConnState, handle_text_message};
    use rune_gateway::ws_rpc::RpcDispatcher;

    let session_repo = Arc::new(MemSessionRepo::new());
    let turn_repo = Arc::new(MemTurnRepo::new());
    let transcript_repo = Arc::new(MemTranscriptRepo::new());
    let approval_repo = Arc::new(MemApprovalRepo::new());
    let model_provider: Arc<dyn ModelProvider> = Arc::new(FakeModelProvider);
    let scheduler = Arc::new(Scheduler::new());
    let session_engine = Arc::new(
        SessionEngine::new(session_repo.clone()).with_transcript_repo(transcript_repo.clone()),
    );
    let context_assembler = ContextAssembler::new("You are a test assistant.");
    let compaction: Arc<dyn CompactionStrategy> = Arc::new(NoOpCompaction);
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(FakeToolExecutor);
    let tool_registry = Arc::new(ToolRegistry::new());
    let turn_executor = Arc::new(
        TurnExecutor::new(
            session_repo.clone() as Arc<dyn SessionRepo>,
            turn_repo.clone() as Arc<dyn TurnRepo>,
            transcript_repo.clone() as Arc<dyn TranscriptRepo>,
            approval_repo.clone() as Arc<dyn ApprovalRepo>,
            model_provider.clone(),
            tool_executor,
            tool_registry,
            context_assembler,
            compaction,
        )
        .with_default_model("fake-model"),
    );
    let (event_tx, _) = broadcast::channel::<SessionEvent>(64);
    let skill_registry = Arc::new(SkillRegistry::new());
    let skill_loader = Arc::new(SkillLoader::new(
        std::env::temp_dir(),
        skill_registry.clone(),
    ));
    let device_repo = Arc::new(MemDeviceRepo::new());
    let device_registry = Arc::new(DeviceRegistry::new(device_repo.clone()));
    let (plugin_registry, plugin_loader, hook_registry) = test_plugins();

    let state = AppState {
        config: Arc::new(RwLock::new(AppConfig::default())),
        started_at: Arc::new(Instant::now()),
        session_engine,
        turn_executor,
        session_repo: session_repo as Arc<dyn SessionRepo>,
        transcript_repo: transcript_repo as Arc<dyn TranscriptRepo>,
        turn_repo: turn_repo as Arc<dyn TurnRepo>,
        model_provider,
        scheduler,
        heartbeat: Arc::new(HeartbeatRunner::new(std::env::temp_dir())),
        reminder_store: Arc::new(ReminderStore::new()),
        approval_repo: approval_repo as Arc<dyn ApprovalRepo>,
        tool_approval_repo: Arc::new(MemToolApprovalPolicyRepo::new())
            as Arc<dyn ToolApprovalPolicyRepo>,
        process_manager: ProcessManager::new(),
        capabilities: test_capabilities(0),
        device_repo: device_repo.clone() as Arc<dyn DeviceRepo>,
        device_registry,
        skill_registry,
        skill_loader,
        plugin_registry,
        plugin_loader,
        hook_registry,
        event_tx,
        tts_engine: None,
        stt_engine: None,
    };

    let dispatcher = RpcDispatcher::new(state);
    let mut conn = ConnState::new();
    let state_version = Arc::new(AtomicU64::new(25));

    let subscribe_event = handle_text_message(
        r#"{"type":"req","id":"11","method":"subscribe","params":{"event":"wake_event"}}"#,
        &mut conn,
        &dispatcher,
        &state_version,
    )
    .await
    .unwrap()
    .to_string();
    let subscribe_event_json: Value = serde_json::from_str(&subscribe_event).unwrap();
    assert_eq!(subscribe_event_json["ok"], true);
    assert_eq!(
        subscribe_event_json["payload"]["subscribed"]["event"],
        "wake_event"
    );
    assert_eq!(subscribe_event_json["payload"]["subscribed"]["all"], false);
    assert!(subscribe_event_json["stateVersion"].as_u64().unwrap() >= 1);

    let subscribe_all = handle_text_message(
        r#"{"type":"req","id":"12","method":"subscribe","params":{"all":true}}"#,
        &mut conn,
        &dispatcher,
        &state_version,
    )
    .await
    .unwrap()
    .to_string();
    let subscribe_all_json: Value = serde_json::from_str(&subscribe_all).unwrap();
    assert_eq!(subscribe_all_json["ok"], true);
    assert_eq!(subscribe_all_json["payload"]["subscribed"]["all"], true);
    assert!(subscribe_all_json["stateVersion"].as_u64().unwrap() >= 1);

    let unsubscribe_event = handle_text_message(
        r#"{"type":"req","id":"13","method":"unsubscribe","params":{"event":"wake_event"}}"#,
        &mut conn,
        &dispatcher,
        &state_version,
    )
    .await
    .unwrap()
    .to_string();
    let unsubscribe_event_json: Value = serde_json::from_str(&unsubscribe_event).unwrap();
    assert_eq!(unsubscribe_event_json["ok"], true);
    assert_eq!(
        unsubscribe_event_json["payload"]["unsubscribed"]["event"],
        "wake_event"
    );
    assert!(unsubscribe_event_json["stateVersion"].as_u64().unwrap() >= 1);

    let unsubscribe_all = handle_text_message(
        r#"{"type":"req","id":"14","method":"unsubscribe","params":{"all":true}}"#,
        &mut conn,
        &dispatcher,
        &state_version,
    )
    .await
    .unwrap()
    .to_string();
    let unsubscribe_all_json: Value = serde_json::from_str(&unsubscribe_all).unwrap();
    assert_eq!(unsubscribe_all_json["ok"], true);
    assert_eq!(unsubscribe_all_json["payload"]["unsubscribed"]["all"], true);
    assert!(unsubscribe_all_json["stateVersion"].as_u64().unwrap() >= 1);
}

#[tokio::test]
async fn ws_subscribe_bumps_state_version_once_and_non_subscription_rpc_does_not() {
    use rune_gateway::ws::ConnState;
    use rune_gateway::ws::handle_text_message;
    use rune_gateway::ws_rpc::RpcDispatcher;

    let session_repo = Arc::new(MemSessionRepo::new());
    let turn_repo = Arc::new(MemTurnRepo::new());
    let transcript_repo = Arc::new(MemTranscriptRepo::new());
    let approval_repo = Arc::new(MemApprovalRepo::new());
    let model_provider: Arc<dyn ModelProvider> = Arc::new(FakeModelProvider);
    let scheduler = Arc::new(Scheduler::new());
    let session_engine = Arc::new(
        SessionEngine::new(session_repo.clone()).with_transcript_repo(transcript_repo.clone()),
    );
    let context_assembler = ContextAssembler::new("You are a test assistant.");
    let compaction: Arc<dyn CompactionStrategy> = Arc::new(NoOpCompaction);
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(FakeToolExecutor);
    let tool_registry = Arc::new(ToolRegistry::new());
    let turn_executor = Arc::new(
        TurnExecutor::new(
            session_repo.clone() as Arc<dyn SessionRepo>,
            turn_repo.clone() as Arc<dyn TurnRepo>,
            transcript_repo.clone() as Arc<dyn TranscriptRepo>,
            approval_repo.clone() as Arc<dyn ApprovalRepo>,
            model_provider.clone(),
            tool_executor,
            tool_registry,
            context_assembler,
            compaction,
        )
        .with_default_model("fake-model"),
    );
    let (event_tx, _) = broadcast::channel::<SessionEvent>(64);
    let skill_registry = Arc::new(SkillRegistry::new());
    let skill_loader = Arc::new(SkillLoader::new(
        std::env::temp_dir(),
        skill_registry.clone(),
    ));
    let device_repo = Arc::new(MemDeviceRepo::new());
    let device_registry = Arc::new(DeviceRegistry::new(device_repo.clone()));
    let (plugin_registry, plugin_loader, hook_registry) = test_plugins();

    let state = AppState {
        config: Arc::new(RwLock::new(AppConfig::default())),
        started_at: Arc::new(Instant::now()),
        session_engine,
        turn_executor,
        session_repo: session_repo as Arc<dyn SessionRepo>,
        transcript_repo: transcript_repo as Arc<dyn TranscriptRepo>,
        turn_repo: turn_repo as Arc<dyn TurnRepo>,
        model_provider,
        scheduler,
        heartbeat: Arc::new(HeartbeatRunner::new(std::env::temp_dir())),
        reminder_store: Arc::new(ReminderStore::new()),
        approval_repo: approval_repo as Arc<dyn ApprovalRepo>,
        tool_approval_repo: Arc::new(MemToolApprovalPolicyRepo::new())
            as Arc<dyn ToolApprovalPolicyRepo>,
        process_manager: ProcessManager::new(),
        capabilities: test_capabilities(0),
        device_repo: device_repo.clone() as Arc<dyn DeviceRepo>,
        device_registry,
        skill_registry,
        skill_loader,
        plugin_registry,
        plugin_loader,
        hook_registry,
        event_tx,
        tts_engine: None,
        stt_engine: None,
    };

    let dispatcher = RpcDispatcher::new(state);
    let mut conn = ConnState::new();
    let state_version = Arc::new(AtomicU64::new(101));

    let subscribe = handle_text_message(
        r#"{"type":"req","id":"sub","method":"subscribe","params":{"session_id":"sess-1"}}"#,
        &mut conn,
        &dispatcher,
        &state_version,
    )
    .await
    .unwrap();
    let subscribe_json: Value = serde_json::from_str(&subscribe).unwrap();
    let subscribed_version = subscribe_json["stateVersion"].as_u64().unwrap();
    assert_eq!(subscribed_version, 102);
    assert_eq!(state_version.load(Ordering::Relaxed), 102);

    let status = handle_text_message(
        r#"{"type":"req","id":"status","method":"status","params":{}}"#,
        &mut conn,
        &dispatcher,
        &state_version,
    )
    .await
    .unwrap();
    let status_json: Value = serde_json::from_str(&status).unwrap();
    assert_eq!(status_json["stateVersion"], 102);
    assert_eq!(state_version.load(Ordering::Relaxed), 102);
    assert_eq!(
        status_json["stateVersion"].as_u64().unwrap(),
        subscribed_version
    );
}

#[tokio::test]
async fn ws_handle_text_message_dispatches_rpc_errors() {
    use rune_gateway::ws::{ConnState, handle_text_message};
    use rune_gateway::ws_rpc::RpcDispatcher;

    let session_repo = Arc::new(MemSessionRepo::new());
    let turn_repo = Arc::new(MemTurnRepo::new());
    let transcript_repo = Arc::new(MemTranscriptRepo::new());
    let approval_repo = Arc::new(MemApprovalRepo::new());
    let model_provider: Arc<dyn ModelProvider> = Arc::new(FakeModelProvider);
    let scheduler = Arc::new(Scheduler::new());
    let session_engine = Arc::new(
        SessionEngine::new(session_repo.clone()).with_transcript_repo(transcript_repo.clone()),
    );
    let context_assembler = ContextAssembler::new("You are a test assistant.");
    let compaction: Arc<dyn CompactionStrategy> = Arc::new(NoOpCompaction);
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(FakeToolExecutor);
    let tool_registry = Arc::new(ToolRegistry::new());
    let turn_executor = Arc::new(
        TurnExecutor::new(
            session_repo.clone() as Arc<dyn SessionRepo>,
            turn_repo.clone() as Arc<dyn TurnRepo>,
            transcript_repo.clone() as Arc<dyn TranscriptRepo>,
            approval_repo.clone() as Arc<dyn ApprovalRepo>,
            model_provider.clone(),
            tool_executor,
            tool_registry,
            context_assembler,
            compaction,
        )
        .with_default_model("fake-model"),
    );
    let (event_tx, _) = broadcast::channel::<SessionEvent>(64);
    let skill_registry = Arc::new(SkillRegistry::new());
    let skill_loader = Arc::new(SkillLoader::new(
        std::env::temp_dir(),
        skill_registry.clone(),
    ));
    let device_repo = Arc::new(MemDeviceRepo::new());
    let device_registry = Arc::new(DeviceRegistry::new(device_repo.clone()));
    let (plugin_registry, plugin_loader, hook_registry) = test_plugins();

    let state = AppState {
        config: Arc::new(RwLock::new(AppConfig::default())),
        started_at: Arc::new(Instant::now()),
        session_engine,
        turn_executor,
        session_repo: session_repo as Arc<dyn SessionRepo>,
        transcript_repo: transcript_repo as Arc<dyn TranscriptRepo>,
        turn_repo: turn_repo as Arc<dyn TurnRepo>,
        model_provider,
        scheduler,
        heartbeat: Arc::new(HeartbeatRunner::new(std::env::temp_dir())),
        reminder_store: Arc::new(ReminderStore::new()),
        approval_repo: approval_repo as Arc<dyn ApprovalRepo>,
        tool_approval_repo: Arc::new(MemToolApprovalPolicyRepo::new())
            as Arc<dyn ToolApprovalPolicyRepo>,
        process_manager: ProcessManager::new(),
        capabilities: test_capabilities(0),
        device_repo: device_repo.clone() as Arc<dyn DeviceRepo>,
        device_registry,
        skill_registry,
        skill_loader,
        plugin_registry,
        plugin_loader,
        hook_registry,
        event_tx,
        tts_engine: None,
        stt_engine: None,
    };

    let dispatcher = RpcDispatcher::new(state);
    let mut conn = ConnState::new();
    let state_version = Arc::new(AtomicU64::new(10));

    let method_not_found: String = handle_text_message(
        r#"{"type":"req","id":"404","method":"unknown.method","params":{}}"#,
        &mut conn,
        &dispatcher,
        &state_version,
    )
    .await
    .unwrap()
    .to_string();
    let method_json: Value = serde_json::from_str(&method_not_found).unwrap();
    assert_eq!(method_json["ok"], false);
    assert_eq!(method_json["error"]["code"], "method_not_found");
    assert!(
        method_json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("unknown method: unknown.method")
    );
    assert_eq!(method_json["stateVersion"], 10);

    let invalid_uuid: String = handle_text_message(
        r#"{"type":"req","id":"bad","method":"session.status","params":{"session_id":"nope"}}"#,
        &mut conn,
        &dispatcher,
        &state_version,
    )
    .await
    .unwrap()
    .to_string();
    let invalid_uuid_json: Value = serde_json::from_str(&invalid_uuid).unwrap();
    assert_eq!(invalid_uuid_json["ok"], false);
    assert_eq!(invalid_uuid_json["error"]["code"], "bad_request");
    assert!(
        invalid_uuid_json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("invalid UUID for session_id")
    );
}

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
    assert_eq!(json["ws_connections"], 0);
    assert_eq!(json["mode"], "standalone");
    assert_eq!(json["storage_backend"], "test");
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
    assert!(json["capabilities"].is_object());
    assert_eq!(json["capabilities"]["mode"], "standalone");
    assert_eq!(json["capabilities"]["storage_backend"], "test");
    assert_eq!(json["capabilities"]["pgvector"], false);
    assert_eq!(json["capabilities"]["memory_mode"], "disabled");
    assert_eq!(json["capabilities"]["browser"], false);
    assert_eq!(json["capabilities"]["mcp_servers"], 0);
    assert_eq!(json["capabilities"]["tts"], false);
    assert_eq!(json["capabilities"]["stt"], false);
    assert_eq!(json["capabilities"]["channels"], serde_json::json!([]));
}

#[tokio::test]
async fn status_includes_skill_status_shape() {
    let app = build_test_app(None);
    let response = app
        .oneshot(Request::get("/status").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let json = body_json(response).await;
    assert!(json["skills"].is_object());
    assert!(json["skills"]["loaded"].is_number());
    assert!(json["skills"]["enabled"].is_number());
    assert!(json["skills"]["skills_dir"].is_string());
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
    assert!(body.contains("<!doctype html>") || body.contains("<html"));
    assert!(body.contains("/assets/") || body.contains("/favicon") || body.contains("index-"));
}

#[tokio::test]
async fn processes_routes_surface_empty_inventory() {
    let app = build_test_app(None);

    let response = app
        .clone()
        .oneshot(Request::get("/processes").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json, serde_json::json!([]));

    let response = app
        .oneshot(
            Request::get("/processes/missing")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let json = body_json(response).await;
    assert_eq!(json["code"], "bad_request");
    assert!(
        json["message"]
            .as_str()
            .unwrap()
            .contains("process not found")
    );
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
async fn dashboard_api_requires_auth_when_enabled() {
    let app = build_test_app(Some(TEST_AUTH_TOKEN.to_string()));

    let response = app
        .clone()
        .oneshot(
            Request::get("/api/dashboard/summary")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .oneshot(
            Request::get("/api/dashboard/summary")
                .header(header::AUTHORIZATION, format!("Bearer {TEST_AUTH_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
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
async fn ms365_auth_status_returns_unconfigured_response() {
    let app = build_test_app(None);

    let response = app
        .oneshot(
            Request::get("/ms365/auth/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let json = body_json(response).await;
    assert_eq!(json["authenticated"], false);
    assert_eq!(json["tenant_id"], Value::Null);
    assert_eq!(json["client_id"], Value::Null);
    assert_eq!(json["user_principal"], Value::Null);
    assert_eq!(json["scopes"], serde_json::json!([]));
    assert_eq!(json["token_valid"], false);
    assert_eq!(json["token_expires_at"], Value::Null);
}

#[tokio::test]
async fn ms365_auth_status_returns_configured_response() {
    let mut config = AppConfig::default();
    config.ms365.tenant_id = Some("tenant-123".to_string());
    config.ms365.client_id = Some("client-456".to_string());
    config.ms365.user_principal = Some("user@example.com".to_string());
    config.ms365.scopes = vec!["Mail.Read".to_string(), "Calendars.Read".to_string()];

    let app = build_test_app_with_config(config, None);

    let response = app
        .oneshot(
            Request::get("/ms365/auth/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let json = body_json(response).await;
    assert_eq!(json["authenticated"], false);
    assert_eq!(json["tenant_id"], "tenant-123");
    assert_eq!(json["client_id"], "client-456");
    assert_eq!(json["user_principal"], "user@example.com");
    assert_eq!(
        json["scopes"],
        serde_json::json!(["Mail.Read", "Calendars.Read"])
    );
    assert_eq!(json["token_valid"], false);
    assert_eq!(json["token_expires_at"], Value::Null);
}

#[tokio::test]
async fn ms365_auth_status_requires_auth_when_gateway_token_enabled() {
    let app = build_test_app(Some(TEST_AUTH_TOKEN.to_string()));

    let response = app
        .clone()
        .oneshot(
            Request::get("/ms365/auth/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .oneshot(
            Request::get("/ms365/auth/status")
                .header(header::AUTHORIZATION, format!("Bearer {TEST_AUTH_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn device_management_requires_gateway_token_even_when_auth_enabled() {
    let app = build_test_app(Some(TEST_AUTH_TOKEN.to_string()));

    let response = app
        .clone()
        .oneshot(Request::get("/devices").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .oneshot(
            Request::get("/devices")
                .header(header::AUTHORIZATION, "Bearer wrong-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn device_pair_request_is_public_but_approve_requires_gateway_token() {
    use ed25519_dalek::{Signer, SigningKey};

    let app = build_test_app(Some(TEST_AUTH_TOKEN.to_string()));
    let signing_key = SigningKey::from_bytes(&[7u8; 32]);
    let public_key = hex::encode(signing_key.verifying_key().as_bytes());

    let response = app
        .clone()
        .oneshot(
            Request::post("/devices/pair/request")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "device_name": "paired-phone",
                        "public_key": public_key,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let pairing_request = body_json(response).await;
    let request_id = pairing_request["request_id"].as_str().unwrap().to_string();
    let challenge = pairing_request["challenge"].as_str().unwrap().to_string();
    assert!(pairing_request["expires_at"].as_str().is_some());

    let signature = signing_key.sign(&hex::decode(&challenge).unwrap());

    let response = app
        .clone()
        .oneshot(
            Request::post("/devices/pair/approve")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "request_id": request_id,
                        "challenge_response": hex::encode(signature.to_bytes()),
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .oneshot(
            Request::post("/devices/pair/approve")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {TEST_AUTH_TOKEN}"))
                .body(Body::from(
                    serde_json::json!({
                        "request_id": request_id,
                        "challenge_response": hex::encode(signature.to_bytes()),
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let approved = body_json(response).await;
    assert_eq!(approved["name"], "paired-phone");
    assert_eq!(approved["role"], "operator");
    assert!(approved["token"].as_str().unwrap().len() >= 32);
    assert!(approved["token_expires_at"].as_str().is_some());
}

#[tokio::test]
async fn paired_device_token_can_access_general_protected_routes_but_not_device_management() {
    use ed25519_dalek::{Signer, SigningKey};

    let (app, device_repo) =
        build_test_app_parts(AppConfig::default(), Some(TEST_AUTH_TOKEN.to_string()));
    let signing_key = SigningKey::from_bytes(&[8u8; 32]);
    let public_key = hex::encode(signing_key.verifying_key().as_bytes());

    let response = app
        .clone()
        .oneshot(
            Request::post("/devices/pair/request")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "device_name": "ops-tablet",
                        "public_key": public_key,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let pairing_request = body_json(response).await;
    let request_id = pairing_request["request_id"].as_str().unwrap().to_string();
    let challenge = pairing_request["challenge"].as_str().unwrap().to_string();
    let signature = signing_key.sign(&hex::decode(&challenge).unwrap());

    let response = app
        .clone()
        .oneshot(
            Request::post("/devices/pair/approve")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {TEST_AUTH_TOKEN}"))
                .body(Body::from(
                    serde_json::json!({
                        "request_id": request_id,
                        "challenge_response": hex::encode(signature.to_bytes()),
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let approved = body_json(response).await;
    let device_token = approved["token"].as_str().unwrap().to_string();

    let response = app
        .clone()
        .oneshot(
            Request::get("/status")
                .header(header::AUTHORIZATION, format!("Bearer {device_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = body_json(response).await;
    assert_eq!(payload["status"], "running");

    let response = app
        .clone()
        .oneshot(
            Request::get("/devices")
                .header(header::AUTHORIZATION, format!("Bearer {device_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let payload = body_json(response).await;
    assert_eq!(payload["code"], "unauthorized");

    let response = app
        .oneshot(
            Request::get("/devices")
                .header(header::AUTHORIZATION, format!("Bearer {TEST_AUTH_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let devices = device_repo.devices.lock().await;
    let device = devices
        .iter()
        .find(|device| device.public_key == public_key)
        .expect("paired device should be stored");
    assert!(
        device.last_seen_at.is_some(),
        "device token auth should touch last_seen_at"
    );
}

#[tokio::test]
async fn expired_paired_device_token_is_rejected_without_refreshing_last_seen() {
    use chrono::{Duration, Utc};
    use ed25519_dalek::{Signer, SigningKey};

    let (app, device_repo) =
        build_test_app_parts(AppConfig::default(), Some(TEST_AUTH_TOKEN.to_string()));
    let signing_key = SigningKey::from_bytes(&[18u8; 32]);
    let public_key = hex::encode(signing_key.verifying_key().as_bytes());

    let response = app
        .clone()
        .oneshot(
            Request::post("/devices/pair/request")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "device_name": "expired-tablet",
                        "public_key": public_key,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let pairing_request = body_json(response).await;
    let request_id = pairing_request["request_id"].as_str().unwrap().to_string();
    let challenge = pairing_request["challenge"].as_str().unwrap().to_string();
    let signature = signing_key.sign(&hex::decode(&challenge).unwrap());

    let response = app
        .clone()
        .oneshot(
            Request::post("/devices/pair/approve")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {TEST_AUTH_TOKEN}"))
                .body(Body::from(
                    serde_json::json!({
                        "request_id": request_id,
                        "challenge_response": hex::encode(signature.to_bytes()),
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let approved = body_json(response).await;
    let device_token = approved["token"].as_str().unwrap().to_string();

    {
        let mut devices = device_repo.devices.lock().await;
        let device = devices
            .iter_mut()
            .find(|device| device.public_key == public_key)
            .expect("paired device should be stored");
        device.token_expires_at = Utc::now() - Duration::minutes(1);
        device.last_seen_at = None;
    }

    let response = app
        .oneshot(
            Request::get("/status")
                .header(header::AUTHORIZATION, format!("Bearer {device_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let devices = device_repo.devices.lock().await;
    let device = devices
        .iter()
        .find(|device| device.public_key == public_key)
        .expect("paired device should remain stored");
    assert!(
        device.last_seen_at.is_none(),
        "expired device token must not touch last_seen_at"
    );
}

#[tokio::test]
async fn device_pair_approve_rejects_wrong_signature() {
    use ed25519_dalek::SigningKey;

    let app = build_test_app(Some(TEST_AUTH_TOKEN.to_string()));
    let signing_key = SigningKey::from_bytes(&[9u8; 32]);
    let public_key = hex::encode(signing_key.verifying_key().as_bytes());

    let response = app
        .clone()
        .oneshot(
            Request::post("/devices/pair/request")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "device_name": "bad-sig-device",
                        "public_key": public_key,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let pairing_request = body_json(response).await;
    let request_id = pairing_request["request_id"].as_str().unwrap().to_string();

    let response = app
        .oneshot(
            Request::post("/devices/pair/approve")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {TEST_AUTH_TOKEN}"))
                .body(Body::from(
                    serde_json::json!({
                        "request_id": request_id,
                        "challenge_response": "aa".repeat(64),
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let payload = body_json(response).await;
    assert_eq!(payload["code"], "bad_request");
    assert_eq!(
        payload["message"],
        "bad request: challenge response verification failed"
    );
}

#[tokio::test]
async fn device_pair_approve_rejects_expired_request() {
    use ed25519_dalek::{Signer, SigningKey};

    let (app, device_repo) =
        build_test_app_parts(AppConfig::default(), Some(TEST_AUTH_TOKEN.to_string()));
    let signing_key = SigningKey::from_bytes(&[21u8; 32]);
    let public_key = hex::encode(signing_key.verifying_key().as_bytes());

    let response = app
        .clone()
        .oneshot(
            Request::post("/devices/pair/request")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "device_name": "expired-device",
                        "public_key": public_key,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let pairing_request = body_json(response).await;
    let request_id = pairing_request["request_id"].as_str().unwrap().to_string();
    let challenge = pairing_request["challenge"].as_str().unwrap().to_string();
    let signature = signing_key.sign(&hex::decode(&challenge).unwrap());

    {
        let mut requests = device_repo.requests.lock().await;
        let request = requests
            .iter_mut()
            .find(|request| request.id.to_string() == request_id)
            .expect("pending request entry");
        request.expires_at = chrono::Utc::now() - chrono::Duration::minutes(1);
    }

    let response = app
        .oneshot(
            Request::post("/devices/pair/approve")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {TEST_AUTH_TOKEN}"))
                .body(Body::from(
                    serde_json::json!({
                        "request_id": request_id,
                        "challenge_response": hex::encode(signature.to_bytes()),
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let payload = body_json(response).await;
    assert_eq!(payload["code"], "bad_request");
    assert!(
        payload["message"]
            .as_str()
            .unwrap()
            .contains("pairing request expired")
    );
}

#[tokio::test]
async fn device_pair_request_rejects_duplicate_public_key() {
    use ed25519_dalek::{Signer, SigningKey};

    let app = build_test_app(Some(TEST_AUTH_TOKEN.to_string()));
    let signing_key = SigningKey::from_bytes(&[10u8; 32]);
    let public_key = hex::encode(signing_key.verifying_key().as_bytes());

    let response = app
        .clone()
        .oneshot(
            Request::post("/devices/pair/request")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "device_name": "dup-a",
                        "public_key": public_key,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let pairing_request = body_json(response).await;
    let request_id = pairing_request["request_id"].as_str().unwrap().to_string();
    let challenge = pairing_request["challenge"].as_str().unwrap().to_string();
    let signature = signing_key.sign(&hex::decode(&challenge).unwrap());

    let response = app
        .clone()
        .oneshot(
            Request::post("/devices/pair/approve")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {TEST_AUTH_TOKEN}"))
                .body(Body::from(
                    serde_json::json!({
                        "request_id": request_id,
                        "challenge_response": hex::encode(signature.to_bytes()),
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .oneshot(
            Request::post("/devices/pair/request")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "device_name": "dup-b",
                        "public_key": public_key,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn device_list_masks_tokens_and_includes_pending_requests() {
    use ed25519_dalek::SigningKey;

    let app = build_test_app(Some(TEST_AUTH_TOKEN.to_string()));
    let signing_key = SigningKey::from_bytes(&[11u8; 32]);
    let public_key = hex::encode(signing_key.verifying_key().as_bytes());

    let response = app
        .clone()
        .oneshot(
            Request::post("/devices/pair/request")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "device_name": "list-me",
                        "public_key": public_key,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .oneshot(
            Request::get("/devices")
                .header(header::AUTHORIZATION, format!("Bearer {TEST_AUTH_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = body_json(response).await;
    assert!(!payload["pending_requests"].as_array().unwrap().is_empty());
    assert!(payload["devices"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn device_reject_rotate_and_delete_routes_work() {
    use ed25519_dalek::{Signer, SigningKey};

    let app = build_test_app(Some(TEST_AUTH_TOKEN.to_string()));

    let reject_key = SigningKey::from_bytes(&[12u8; 32]);
    let reject_public_key = hex::encode(reject_key.verifying_key().as_bytes());
    let response = app
        .clone()
        .oneshot(
            Request::post("/devices/pair/request")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "device_name": "reject-me",
                        "public_key": reject_public_key,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let pending = body_json(response).await;
    let reject_request_id = pending["request_id"].as_str().unwrap().to_string();

    let response = app
        .clone()
        .oneshot(
            Request::post("/devices/pair/reject")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {TEST_AUTH_TOKEN}"))
                .body(Body::from(
                    serde_json::json!({"request_id": reject_request_id}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = body_json(response).await;
    assert_eq!(payload["rejected"], true);

    let rotate_key = SigningKey::from_bytes(&[13u8; 32]);
    let rotate_public_key = hex::encode(rotate_key.verifying_key().as_bytes());
    let response = app
        .clone()
        .oneshot(
            Request::post("/devices/pair/request")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "device_name": "rotate-me",
                        "public_key": rotate_public_key,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let pairing_request = body_json(response).await;
    let request_id = pairing_request["request_id"].as_str().unwrap().to_string();
    let challenge = pairing_request["challenge"].as_str().unwrap().to_string();
    let signature = rotate_key.sign(&hex::decode(&challenge).unwrap());

    let response = app
        .clone()
        .oneshot(
            Request::post("/devices/pair/approve")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {TEST_AUTH_TOKEN}"))
                .body(Body::from(
                    serde_json::json!({
                        "request_id": request_id,
                        "challenge_response": hex::encode(signature.to_bytes()),
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let approved = body_json(response).await;
    let original_token = approved["token"].as_str().unwrap().to_string();
    let device_id = approved["device_id"].as_str().unwrap().to_string();

    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/devices/{device_id}/rotate-token"))
                .header(header::AUTHORIZATION, format!("Bearer {TEST_AUTH_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let rotated = body_json(response).await;
    let rotated_token = rotated["token"].as_str().unwrap().to_string();
    assert_ne!(original_token, rotated_token);

    let response = app
        .clone()
        .oneshot(
            Request::get("/status")
                .header(header::AUTHORIZATION, format!("Bearer {original_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .clone()
        .oneshot(
            Request::delete(format!("/devices/{device_id}"))
                .header(header::AUTHORIZATION, format!("Bearer {TEST_AUTH_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = body_json(response).await;
    assert_eq!(payload["deleted"], true);

    let response = app
        .oneshot(
            Request::get("/status")
                .header(header::AUTHORIZATION, format!("Bearer {rotated_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn device_pair_pending_route_requires_gateway_token() {
    let app = build_test_app(Some(TEST_AUTH_TOKEN.to_string()));

    let response = app
        .clone()
        .oneshot(
            Request::get("/devices/pair/pending")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .oneshot(
            Request::get("/devices/pair/pending")
                .header(header::AUTHORIZATION, format!("Bearer {TEST_AUTH_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn paired_device_token_cannot_approve_or_reject_other_pairing_requests() {
    use ed25519_dalek::{Signer, SigningKey};

    let app = build_test_app(Some(TEST_AUTH_TOKEN.to_string()));

    let operator_device_key = SigningKey::from_bytes(&[31u8; 32]);
    let operator_device_public_key = hex::encode(operator_device_key.verifying_key().as_bytes());
    let response = app
        .clone()
        .oneshot(
            Request::post("/devices/pair/request")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "device_name": "existing-device",
                        "public_key": operator_device_public_key,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let pairing_request = body_json(response).await;
    let request_id = pairing_request["request_id"].as_str().unwrap().to_string();
    let challenge = pairing_request["challenge"].as_str().unwrap().to_string();
    let signature = operator_device_key.sign(&hex::decode(&challenge).unwrap());

    let response = app
        .clone()
        .oneshot(
            Request::post("/devices/pair/approve")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {TEST_AUTH_TOKEN}"))
                .body(Body::from(
                    serde_json::json!({
                        "request_id": request_id,
                        "challenge_response": hex::encode(signature.to_bytes()),
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let approved = body_json(response).await;
    let device_token = approved["token"].as_str().unwrap().to_string();

    let pending_key = SigningKey::from_bytes(&[32u8; 32]);
    let pending_public_key = hex::encode(pending_key.verifying_key().as_bytes());
    let response = app
        .clone()
        .oneshot(
            Request::post("/devices/pair/request")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "device_name": "pending-device",
                        "public_key": pending_public_key,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let pending_request = body_json(response).await;
    let pending_request_id = pending_request["request_id"].as_str().unwrap().to_string();
    let pending_challenge = pending_request["challenge"].as_str().unwrap().to_string();
    let pending_signature = pending_key.sign(&hex::decode(&pending_challenge).unwrap());

    let response = app
        .clone()
        .oneshot(
            Request::post("/devices/pair/approve")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {device_token}"))
                .body(Body::from(
                    serde_json::json!({
                        "request_id": pending_request_id,
                        "challenge_response": hex::encode(pending_signature.to_bytes()),
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .oneshot(
            Request::post("/devices/pair/reject")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {device_token}"))
                .body(Body::from(
                    serde_json::json!({ "request_id": pending_request_id }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn mem_device_repo_prunes_expired_requests() {
    let repo = MemDeviceRepo::new();
    let now = chrono::Utc::now();

    repo.create_pairing_request(NewPairingRequest {
        id: Uuid::now_v7(),
        device_name: "expired".into(),
        public_key: "deadbeef".into(),
        challenge: "beadfeed".into(),
        created_at: now - chrono::Duration::minutes(10),
        expires_at: now - chrono::Duration::minutes(5),
    })
    .await
    .unwrap();

    repo.create_pairing_request(NewPairingRequest {
        id: Uuid::now_v7(),
        device_name: "fresh".into(),
        public_key: "cafebabe".into(),
        challenge: "facefeed".into(),
        created_at: now,
        expires_at: now + chrono::Duration::minutes(5),
    })
    .await
    .unwrap();

    let pruned = repo.prune_expired_requests().await.unwrap();
    assert_eq!(pruned, 1);

    let pending = repo.list_pending_requests().await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].device_name, "fresh");
}

#[tokio::test]
async fn patch_and_delete_session_routes_work() {
    let app = build_test_app(None);

    let create_response = app
        .clone()
        .oneshot(
            Request::post("/sessions")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"kind":"direct"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::CREATED);
    let session_json = body_json(create_response).await;
    let session_id = session_json["id"].as_str().unwrap();

    let patch_response = app
        .clone()
        .oneshot(
            Request::patch(format!("/sessions/{session_id}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"label":"backend","verbose":true,"reasoning":"medium"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(patch_response.status(), StatusCode::OK);
    let patched_json = body_json(patch_response).await;
    assert_eq!(patched_json["id"], session_id);

    let status_response = app
        .clone()
        .oneshot(
            Request::get(format!("/sessions/{session_id}/status"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(status_response.status(), StatusCode::OK);
    let status_json = body_json(status_response).await;
    assert_eq!(status_json["reasoning"], "medium");
    assert_eq!(status_json["verbose"], true);

    let delete_response = app
        .clone()
        .oneshot(
            Request::delete(format!("/sessions/{session_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_response.status(), StatusCode::OK);

    let get_response = app
        .oneshot(
            Request::get(format!("/sessions/{session_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_response.status(), StatusCode::NOT_FOUND);
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
    // SessionEngine auto-transitions created → ready.
    assert_eq!(json["status"], "ready");
}

#[tokio::test]
async fn send_message_and_transcript_with_shared_state() {
    let session_repo = Arc::new(MemSessionRepo::new());
    let turn_repo = Arc::new(MemTurnRepo::new());
    let transcript_repo = Arc::new(MemTranscriptRepo::new());
    let model_provider: Arc<dyn ModelProvider> = Arc::new(FakeModelProvider);
    let scheduler = Arc::new(Scheduler::new());
    let session_engine = Arc::new(
        SessionEngine::new(session_repo.clone()).with_transcript_repo(transcript_repo.clone()),
    );

    let workspace_root = std::env::temp_dir().join(format!("rune-gw-test-{}", Uuid::now_v7()));
    std::fs::create_dir_all(workspace_root.join("memory")).unwrap();
    std::fs::write(workspace_root.join("AGENTS.md"), "# Test").unwrap();

    let context_assembler = ContextAssembler::new("You are a test assistant.");
    let compaction: Arc<dyn CompactionStrategy> = Arc::new(NoOpCompaction);
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(FakeToolExecutor);
    let tool_registry = Arc::new(ToolRegistry::new());
    let approval_repo = Arc::new(MemApprovalRepo::new());

    let turn_executor = Arc::new(
        TurnExecutor::new(
            session_repo.clone() as Arc<dyn SessionRepo>,
            turn_repo.clone() as Arc<dyn TurnRepo>,
            transcript_repo.clone() as Arc<dyn TranscriptRepo>,
            approval_repo.clone() as Arc<dyn ApprovalRepo>,
            model_provider.clone(),
            tool_executor,
            tool_registry,
            context_assembler,
            compaction,
        )
        .with_default_model("fake-model"),
    );

    let (event_tx, _) = broadcast::channel::<SessionEvent>(64);
    let skill_registry = Arc::new(SkillRegistry::new());
    let skill_loader = Arc::new(SkillLoader::new(
        std::env::temp_dir(),
        skill_registry.clone(),
    ));
    let device_repo = Arc::new(MemDeviceRepo::new());
    let device_registry = Arc::new(DeviceRegistry::new(device_repo.clone()));
    let (plugin_registry, plugin_loader, hook_registry) = test_plugins();

    let state = AppState {
        config: Arc::new(RwLock::new(AppConfig::default())),
        started_at: Arc::new(Instant::now()),
        session_engine,
        turn_executor,
        session_repo: session_repo as Arc<dyn SessionRepo>,
        transcript_repo: transcript_repo as Arc<dyn TranscriptRepo>,
        turn_repo: turn_repo as Arc<dyn TurnRepo>,
        model_provider,
        scheduler,
        heartbeat: Arc::new(HeartbeatRunner::new(std::env::temp_dir())),
        reminder_store: Arc::new(ReminderStore::new()),
        approval_repo: approval_repo as Arc<dyn ApprovalRepo>,
        tool_approval_repo: Arc::new(MemToolApprovalPolicyRepo::new())
            as Arc<dyn ToolApprovalPolicyRepo>,
        process_manager: ProcessManager::new(),
        capabilities: test_capabilities(0),
        device_repo: device_repo.clone() as Arc<dyn DeviceRepo>,
        device_registry,
        skill_registry,
        skill_loader,
        plugin_registry,
        plugin_loader,
        hook_registry,
        event_tx,
        tts_engine: None,
        stt_engine: None,
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
async fn get_session_status_returns_parity_card_fields() {
    let app = build_test_app(None);

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

    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/sessions/{session_id}/status"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["session_id"], session_id);
    // Session transitions: created → ready (auto) → running (on message).
    assert_eq!(json["status"], "running");
    assert_eq!(json["current_model"], "fake-model");
    assert_eq!(json["prompt_tokens"], 10);
    assert_eq!(json["completion_tokens"], 5);
    assert_eq!(json["total_tokens"], 15);
    assert_eq!(json["turn_count"], 1);
    assert_eq!(json["reasoning"], "off");
    assert_eq!(json["approval_mode"], "on-miss");
    assert_eq!(json["security_mode"], "allowlist");
    assert!(json["unresolved"].is_array());
    let unresolved = json["unresolved"].as_array().unwrap();
    assert!(unresolved.iter().any(|item| item.as_str() == Some("approval requests and operator-triggered resume are durable, but restart-safe continuation for mid-resume approval flows is not parity-complete yet")));
}

#[tokio::test]
async fn get_session_status_surfaces_subagent_metadata() {
    let session_repo = Arc::new(MemSessionRepo::new());
    let turn_repo = Arc::new(MemTurnRepo::new());
    let transcript_repo = Arc::new(MemTranscriptRepo::new());
    let model_provider: Arc<dyn ModelProvider> = Arc::new(FakeModelProvider);
    let scheduler = Arc::new(Scheduler::new());
    let session_engine = Arc::new(
        SessionEngine::new(session_repo.clone()).with_transcript_repo(transcript_repo.clone()),
    );
    let context_assembler = ContextAssembler::new("You are a test assistant.");
    let compaction: Arc<dyn CompactionStrategy> = Arc::new(NoOpCompaction);
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(FakeToolExecutor);
    let tool_registry = Arc::new(ToolRegistry::new());
    let approval_repo = Arc::new(MemApprovalRepo::new());
    let turn_executor = Arc::new(
        TurnExecutor::new(
            session_repo.clone() as Arc<dyn SessionRepo>,
            turn_repo.clone() as Arc<dyn TurnRepo>,
            transcript_repo.clone() as Arc<dyn TranscriptRepo>,
            approval_repo.clone() as Arc<dyn ApprovalRepo>,
            model_provider.clone(),
            tool_executor,
            tool_registry,
            context_assembler,
            compaction,
        )
        .with_default_model("fake-model"),
    );
    let (event_tx, _) = broadcast::channel::<SessionEvent>(64);
    let skill_registry = Arc::new(SkillRegistry::new());
    let skill_loader = Arc::new(SkillLoader::new(
        std::env::temp_dir(),
        skill_registry.clone(),
    ));

    let session_id = Uuid::now_v7();
    let now = chrono::Utc::now();
    session_repo
        .create(NewSession {
            id: session_id,
            kind: "subagent".into(),
            status: "running".into(),
            workspace_root: None,
            channel_ref: None,
            requester_session_id: Some(Uuid::parse_str("33333333-3333-3333-3333-333333333333").unwrap()),
            latest_turn_id: None,
            metadata: serde_json::json!({
                "selected_model": "gpt-5.4",
                "subagent_lifecycle": "steered",
                "subagent_runtime_status": "not_attached",
                "subagent_runtime_attached": false,
                "subagent_status_updated_at": "2026-03-14T12:00:00Z",
                "subagent_last_note": "Steering message queued for subagent/session: tighten the tests"
            }),
            created_at: now,
            updated_at: now,
            last_activity_at: now,
        })
        .await
        .unwrap();
    let device_repo = Arc::new(MemDeviceRepo::new());
    let device_registry = Arc::new(DeviceRegistry::new(device_repo.clone()));
    let (plugin_registry, plugin_loader, hook_registry) = test_plugins();

    let state = AppState {
        config: Arc::new(RwLock::new(AppConfig::default())),
        started_at: Arc::new(Instant::now()),
        session_engine,
        turn_executor,
        session_repo: session_repo as Arc<dyn SessionRepo>,
        transcript_repo: transcript_repo as Arc<dyn TranscriptRepo>,
        turn_repo: turn_repo as Arc<dyn TurnRepo>,
        model_provider,
        scheduler,
        heartbeat: Arc::new(HeartbeatRunner::new(std::env::temp_dir())),
        reminder_store: Arc::new(ReminderStore::new()),
        approval_repo: approval_repo as Arc<dyn ApprovalRepo>,
        tool_approval_repo: Arc::new(MemToolApprovalPolicyRepo::new())
            as Arc<dyn ToolApprovalPolicyRepo>,
        process_manager: ProcessManager::new(),
        capabilities: test_capabilities(0),
        device_repo: device_repo.clone() as Arc<dyn DeviceRepo>,
        device_registry,
        skill_registry,
        skill_loader,
        plugin_registry,
        plugin_loader,
        hook_registry,
        event_tx,
        tts_engine: None,
        stt_engine: None,
    };

    let app = build_router(state, None);
    let response = app
        .oneshot(
            Request::get(format!("/sessions/{session_id}/status"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["session_id"], session_id.to_string());
    assert_eq!(
        json["runtime"],
        "kind=subagent | channel=local | status=running"
    );
    assert_eq!(json["status"], "running");
    assert_eq!(json["current_model"], "gpt-5.4");
    assert_eq!(json["model_override"], "gpt-5.4");
    assert_eq!(json["estimated_cost"], "not available");
    assert_eq!(json["verbose"], false);
    assert_eq!(json["elevated"], false);
    assert_eq!(json["subagent_lifecycle"], "steered");
    assert_eq!(json["subagent_runtime_status"], "not_attached");
    assert_eq!(json["subagent_runtime_attached"], false);
    assert_eq!(
        json["subagent_last_note"],
        "Steering message queued for subagent/session: tighten the tests"
    );
    let unresolved = json["unresolved"].as_array().unwrap();
    assert!(unresolved.iter().any(|item| item.as_str()
        == Some("cost posture is estimate-only; provider pricing is not wired yet")));
    assert!(unresolved.iter().any(|item| item.as_str() == Some("approval requests and operator-triggered resume are durable, but restart-safe continuation for mid-resume approval flows is not parity-complete yet")));
    assert!(unresolved.iter().any(|item| item.as_str()
        == Some("host/node/sandbox parity and PTY fidelity are not yet parity-complete")));
    assert!(unresolved.iter().any(|item| item.as_str() == Some("subagent runtime execution remains conservative; durable lifecycle inspection is available but full remote/runtime attachment parity is not complete")));
}

#[tokio::test]
async fn get_session_returns_aggregate_usage_and_turn_metadata() {
    let app = build_test_app(None);

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

    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/sessions/{session_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["turn_count"], 1);
    assert_eq!(json["latest_model"], "fake-model");
    assert_eq!(json["usage_prompt_tokens"], 10);
    assert_eq!(json["usage_completion_tokens"], 5);
    assert!(json["last_turn_started_at"].is_string());
    assert!(json["last_turn_ended_at"].is_string());
}

#[tokio::test]
async fn list_sessions_includes_turn_aggregates() {
    let app = build_test_app(None);

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

    let response = app
        .oneshot(Request::get("/sessions").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    let sessions = json.as_array().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["turn_count"], 1);
    assert_eq!(sessions[0]["latest_model"], "fake-model");
    assert_eq!(sessions[0]["usage_prompt_tokens"], 10);
    assert_eq!(sessions[0]["usage_completion_tokens"], 5);
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
async fn cron_add_computes_real_next_run_for_cron_schedule() {
    let app = build_test_app(None);
    let response = app
        .clone()
        .oneshot(
            Request::post("/cron")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{
                        "name":"daily-check",
                        "schedule":{"kind":"cron","expr":"0 0 9 * * *","tz":"Europe/Sarajevo"},
                        "payload":{"kind":"system_event","text":"run daily check"},
                        "sessionTarget":"main",
                        "enabled":true
                    }"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let response = app
        .oneshot(Request::get("/cron").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let json = body_json(response).await;
    let items = json.as_array().unwrap();
    assert_eq!(items.len(), 1);
    let next_run_at = items[0]["next_run_at"].as_str().unwrap();
    let parsed = chrono::DateTime::parse_from_rfc3339(next_run_at).unwrap();
    let local = parsed.with_timezone(&"Europe/Sarajevo".parse::<chrono_tz::Tz>().unwrap());
    assert_eq!(local.hour(), 9);
    assert_eq!(local.minute(), 0);
    assert_eq!(local.second(), 0);
}

#[tokio::test]
async fn cron_add_respects_interval_anchor_for_first_run() {
    let app = build_test_app(None);
    let anchor = (chrono::Utc::now() + chrono::Duration::minutes(7)).timestamp_millis();
    let body = serde_json::json!({
        "name": "anchored-interval",
        "schedule": {"kind": "every", "every_ms": 300000_u64, "anchor_ms": anchor},
        "payload": {"kind": "system_event", "text": "anchored interval"},
        "sessionTarget": "main",
        "enabled": true
    });

    let response = app
        .clone()
        .oneshot(
            Request::post("/cron")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let response = app
        .oneshot(Request::get("/cron").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let json = body_json(response).await;
    let items = json.as_array().unwrap();
    assert_eq!(items.len(), 1);
    let next_run_at = items[0]["next_run_at"].as_str().unwrap();
    let parsed = chrono::DateTime::parse_from_rfc3339(next_run_at).unwrap();
    assert_eq!(parsed.timestamp_millis(), anchor);
}

#[tokio::test]
async fn cron_get_and_update_delivery_mode() {
    let app = build_test_app(None);
    let response = app
        .clone()
        .oneshot(
            Request::post("/cron")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{
                        "name":"daily-check",
                        "schedule":{"kind":"at","at":"2026-03-18T10:00:00Z"},
                        "payload":{"kind":"system_event","text":"run daily check"},
                        "session_target":"main",
                        "delivery_mode":"announce",
                        "enabled":true
                    }"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = body_json(response).await;
    let job_id = created["job_id"].as_str().unwrap().to_string();

    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/cron/{job_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let job = body_json(response).await;
    assert_eq!(job["delivery_mode"], "announce");
    assert_eq!(job["payload"]["kind"], "system_event");
    assert_eq!(job["schedule"]["kind"], "at");

    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/cron/{job_id}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{ "delivery_mode":"webhook" }"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .oneshot(
            Request::get(format!("/cron/{job_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let job = body_json(response).await;
    assert_eq!(job["delivery_mode"], "webhook");
}

#[tokio::test]
async fn cron_create_with_webhook_url_roundtrips() {
    let app = build_test_app(None);
    let response = app
        .clone()
        .oneshot(
            Request::post("/cron")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{
                        "name":"webhook-job",
                        "schedule":{"kind":"at","at":"2026-03-18T10:00:00Z"},
                        "payload":{"kind":"system_event","text":"fire!"},
                        "session_target":"main",
                        "delivery_mode":"webhook",
                        "webhook_url":"https://example.com/hook",
                        "enabled":true
                    }"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = body_json(response).await;
    let job_id = created["job_id"].as_str().unwrap().to_string();

    // GET the job and verify webhook_url is persisted
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/cron/{job_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let job = body_json(response).await;
    assert_eq!(job["delivery_mode"], "webhook");
    assert_eq!(job["webhook_url"], "https://example.com/hook");

    // Update the webhook_url
    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/cron/{job_id}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{ "webhook_url":"https://example.com/new-hook" }"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Verify the update stuck
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/cron/{job_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let job = body_json(response).await;
    assert_eq!(job["webhook_url"], "https://example.com/new-hook");
}

#[tokio::test]
async fn cron_update_schedule_recomputes_next_run_at() {
    let app = build_test_app(None);

    // Create a job with a 60-second interval
    let response = app
        .clone()
        .oneshot(
            Request::post("/cron")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{
                        "name":"recompute-test",
                        "schedule":{"kind":"every","every_ms":60000},
                        "payload":{"kind":"system_event","text":"tick"},
                        "sessionTarget":"main",
                        "enabled":true
                    }"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = body_json(response).await;
    let job_id = created["job_id"].as_str().unwrap().to_string();

    // Read the original next_run_at
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/cron/{job_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let original = body_json(response).await;
    let original_next = original["next_run_at"].as_str().unwrap().to_string();

    // Update the schedule to a cron expression (daily at 09:00 UTC)
    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/cron/{job_id}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{ "schedule": {"kind":"cron","expr":"0 0 9 * * *","tz":"UTC"} }"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Read the updated job and verify next_run_at changed
    let response = app
        .oneshot(
            Request::get(format!("/cron/{job_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let updated = body_json(response).await;
    let updated_next = updated["next_run_at"].as_str().unwrap().to_string();

    assert_ne!(
        original_next, updated_next,
        "next_run_at must be recomputed when schedule changes"
    );
    // The new next_run_at should be at 09:00 UTC
    let parsed = chrono::DateTime::parse_from_rfc3339(&updated_next).unwrap();
    assert_eq!(parsed.hour(), 9);
    assert_eq!(parsed.minute(), 0);
}

#[tokio::test]
async fn cron_disable_clears_next_run_at_and_reenable_recomputes() {
    let app = build_test_app(None);

    // Create an enabled job
    let response = app
        .clone()
        .oneshot(
            Request::post("/cron")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{
                        "name":"toggle-test",
                        "schedule":{"kind":"every","every_ms":60000},
                        "payload":{"kind":"system_event","text":"tick"},
                        "sessionTarget":"main",
                        "enabled":true
                    }"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = body_json(response).await;
    let job_id = created["job_id"].as_str().unwrap().to_string();

    // Disable the job
    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/cron/{job_id}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{ "enabled": false }"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Verify next_run_at is cleared
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/cron/{job_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let disabled = body_json(response).await;
    assert!(
        disabled["next_run_at"].is_null(),
        "disabled job should have null next_run_at"
    );

    // Re-enable the job
    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/cron/{job_id}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{ "enabled": true }"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Verify next_run_at is recomputed
    let response = app
        .oneshot(
            Request::get(format!("/cron/{job_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let reenabled = body_json(response).await;
    assert!(
        !reenabled["next_run_at"].is_null(),
        "re-enabled job should have a freshly computed next_run_at"
    );
}

#[tokio::test]
async fn cron_wake_accepts_snake_case_and_normalizes_mode() {
    let app = build_test_app(None);
    let response = app
        .oneshot(
            Request::post("/cron/wake")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{
                        "text":"wake up",
                        "mode":"next_heartbeat",
                        "context_messages":2
                    }"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["mode"], "next-heartbeat");
    assert_eq!(body["context_messages"], 2);
}

#[tokio::test]
async fn cron_wake_rejects_unknown_mode() {
    let app = build_test_app(None);
    let response = app
        .oneshot(
            Request::post("/cron/wake")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{
                        "text":"wake up",
                        "mode":"later"
                    }"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn cron_run_executes_and_records_run_history() {
    let app = build_test_app(None);
    let response = app
        .clone()
        .oneshot(
            Request::post("/cron")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{
                        "name":"run-now",
                        "schedule":{"kind":"every","every_ms":60000},
                        "payload":{"kind":"system_event","text":"manual trigger"},
                        "sessionTarget":"main",
                        "enabled":true
                    }"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = body_json(response).await;
    let job_id = created["job_id"].as_str().unwrap().to_string();

    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/cron/{job_id}/run"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/cron/{job_id}/runs"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let runs = body_json(response).await;
    let items = runs.as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["status"], "completed");
    assert_eq!(items[0]["job_id"], job_id);
    assert_eq!(items[0]["trigger_kind"], "manual");
    assert_eq!(items[0]["output"], "Hello from fake model!");

    let response = app
        .oneshot(Request::get("/cron").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let jobs = body_json(response).await;
    let job = jobs
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == job_id)
        .unwrap();
    assert_eq!(job["run_count"], 1);
    assert!(job["last_run_at"].is_string());
    assert!(job["next_run_at"].is_string());
}

#[tokio::test]
async fn skills_routes_list_reload_and_toggle() {
    let mut config = AppConfig::default();
    let skills_dir = std::env::temp_dir().join(format!("rune-gw-skills-{}", Uuid::now_v7()));
    std::fs::create_dir_all(skills_dir.join("alpha")).unwrap();
    std::fs::write(
        skills_dir.join("alpha/SKILL.md"),
        r#"---
name: alpha
description: Alpha skill
binary: ./run-alpha.sh
enabled: true
---

# Alpha
"#,
    )
    .unwrap();
    config.paths.skills_dir = skills_dir.clone();

    let app = build_test_app_with_config(config, None);

    let response = app
        .clone()
        .oneshot(Request::get("/skills").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json, serde_json::json!([]));

    std::fs::create_dir_all(skills_dir.join("beta")).unwrap();
    std::fs::write(
        skills_dir.join("beta/SKILL.md"),
        r#"---
name: beta
description: Beta skill
enabled: false
---

# Beta
"#,
    )
    .unwrap();

    let response = app
        .clone()
        .oneshot(Request::post("/skills/reload").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["success"], true);
    assert_eq!(json["discovered"], 2);
    assert_eq!(json["loaded"], 2);
    assert_eq!(json["removed"], 0);

    let response = app
        .clone()
        .oneshot(Request::get("/skills").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    let items = json.as_array().unwrap();
    assert_eq!(items.len(), 2);
    let alpha = items.iter().find(|item| item["name"] == "alpha").unwrap();
    assert_eq!(alpha["description"], "Alpha skill");
    assert_eq!(alpha["enabled"], true);
    assert!(
        alpha["binary_path"]
            .as_str()
            .unwrap()
            .contains("run-alpha.sh")
    );

    let response = app
        .clone()
        .oneshot(
            Request::post("/skills/alpha/disable")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(
            Request::post("/skills/beta/enable")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(Request::get("/skills").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    let items = json.as_array().unwrap();
    assert_eq!(items.len(), 2);
    let alpha = items.iter().find(|item| item["name"] == "alpha").unwrap();
    let beta = items.iter().find(|item| item["name"] == "beta").unwrap();
    assert_eq!(alpha["enabled"], false);
    assert_eq!(beta["enabled"], true);

    std::fs::remove_dir_all(skills_dir.join("beta")).unwrap();
    let response = app
        .clone()
        .oneshot(Request::post("/skills/reload").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["removed"], 1);

    let response = app
        .clone()
        .oneshot(
            Request::post("/skills/missing/enable")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let json = body_json(response).await;
    assert_eq!(json["code"], "skill_not_found");
    assert_eq!(json["message"], "skill not found: missing");

    let response = app
        .oneshot(
            Request::post("/skills/missing/disable")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let json = body_json(response).await;
    assert_eq!(json["code"], "skill_not_found");
    assert_eq!(json["message"], "skill not found: missing");
}

#[tokio::test]
async fn skills_detail_route_returns_skill_and_missing_error() {
    let mut config = AppConfig::default();
    let skills_dir =
        std::env::temp_dir().join(format!("rune-gw-skill-detail-{}", Uuid::now_v7()));
    std::fs::create_dir_all(skills_dir.join("alpha")).unwrap();
    std::fs::write(
        skills_dir.join("alpha/SKILL.md"),
        r#"---
name: alpha
description: Alpha skill
binary: ./run-alpha.sh
enabled: true
---

# Alpha
"#,
    )
    .unwrap();
    config.paths.skills_dir = skills_dir.clone();

    let app = build_test_app_with_config(config, None);

    let response = app
        .clone()
        .oneshot(Request::post("/skills/reload").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(Request::get("/skills/alpha").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["name"], "alpha");
    assert_eq!(json["description"], "Alpha skill");
    assert_eq!(json["enabled"], true);
    assert_eq!(json["source_dir"], skills_dir.join("alpha").display().to_string());
    assert!(
        json["binary_path"]
            .as_str()
            .unwrap()
            .contains("run-alpha.sh")
    );

    let response = app
        .oneshot(Request::get("/skills/missing").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let json = body_json(response).await;
    assert_eq!(json["code"], "skill_not_found");
    assert_eq!(json["message"], "skill not found: missing");
    assert_eq!(json["retriable"], false);
    assert_eq!(json["approval_required"], false);
    assert!(json["request_id"].as_str().is_some());
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
            latest_turn_id: None,
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
            latest_turn_id: None,
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
    let session_engine = Arc::new(
        SessionEngine::new(session_repo.clone()).with_transcript_repo(transcript_repo.clone()),
    );
    let context_assembler = ContextAssembler::new("You are a test assistant.");
    let compaction: Arc<dyn CompactionStrategy> = Arc::new(NoOpCompaction);
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(FakeToolExecutor);
    let tool_registry = Arc::new(ToolRegistry::new());
    let approval_repo = Arc::new(MemApprovalRepo::new());
    let turn_executor = Arc::new(
        TurnExecutor::new(
            session_repo.clone() as Arc<dyn SessionRepo>,
            turn_repo.clone() as Arc<dyn TurnRepo>,
            transcript_repo.clone() as Arc<dyn TranscriptRepo>,
            approval_repo.clone() as Arc<dyn ApprovalRepo>,
            model_provider.clone(),
            tool_executor,
            tool_registry,
            context_assembler,
            compaction,
        )
        .with_default_model("fake-model"),
    );
    let (event_tx, _) = broadcast::channel::<SessionEvent>(64);
    let skill_registry = Arc::new(SkillRegistry::new());
    let skill_loader = Arc::new(SkillLoader::new(
        std::env::temp_dir(),
        skill_registry.clone(),
    ));
    let device_repo = Arc::new(MemDeviceRepo::new());
    let device_registry = Arc::new(DeviceRegistry::new(device_repo.clone()));
    let (plugin_registry, plugin_loader, hook_registry) = test_plugins();

    let state = AppState {
        config: Arc::new(RwLock::new(AppConfig::default())),
        started_at: Arc::new(Instant::now()),
        session_engine,
        turn_executor,
        session_repo: session_repo as Arc<dyn SessionRepo>,
        transcript_repo: transcript_repo as Arc<dyn TranscriptRepo>,
        turn_repo: turn_repo as Arc<dyn TurnRepo>,
        model_provider,
        scheduler,
        heartbeat: Arc::new(HeartbeatRunner::new(std::env::temp_dir())),
        reminder_store: Arc::new(ReminderStore::new()),
        approval_repo: approval_repo as Arc<dyn ApprovalRepo>,
        tool_approval_repo: Arc::new(MemToolApprovalPolicyRepo::new())
            as Arc<dyn ToolApprovalPolicyRepo>,
        process_manager: ProcessManager::new(),
        capabilities: test_capabilities(0),
        device_repo: device_repo.clone() as Arc<dyn DeviceRepo>,
        device_registry,
        skill_registry,
        skill_loader,
        plugin_registry,
        plugin_loader,
        hook_registry,
        event_tx,
        tts_engine: None,
        stt_engine: None,
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

// ── Agents (subagent kind filter) tests ───────────────────────────────────────

#[tokio::test]
async fn list_sessions_filters_by_kind_subagent() {
    let app = build_test_app(None);

    // Create a direct session.
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
    let parent_json = body_json(response).await;
    let parent_id = parent_json["id"].as_str().unwrap().to_string();

    // Create a subagent session linked to the parent.
    let response = app
        .clone()
        .oneshot(
            Request::post("/sessions")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "kind": "subagent",
                        "requester_session_id": parent_id,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    // List all sessions (should return 2).
    let response = app
        .clone()
        .oneshot(Request::get("/sessions").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let all_sessions = body_json(response).await;
    assert_eq!(all_sessions.as_array().unwrap().len(), 2);

    // Filter by kind=subagent (should return 1).
    let response = app
        .clone()
        .oneshot(
            Request::get("/sessions?kind=subagent")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    let items = json.as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["kind"], "subagent");
    assert!(items[0]["requester_session_id"].is_string());
}

#[tokio::test]
async fn list_sessions_includes_kind_field() {
    let app = build_test_app(None);

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

    let response = app
        .oneshot(Request::get("/sessions").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    let items = json.as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["kind"], "direct");
}

// ── Reminder route tests ──────────────────────────────────────────────────────

#[tokio::test]
async fn reminders_add_returns_created_with_target_and_status() {
    let app = build_test_app(None);

    let response = app
        .oneshot(
            Request::post("/reminders")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&serde_json::json!({
                        "message": "Stand up!",
                        "fire_at": "2026-04-01T09:00:00Z",
                        "target": "isolated"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let json = body_json(response).await;
    assert_eq!(json["message"], "Stand up!");
    assert_eq!(json["target"], "isolated");
    assert_eq!(json["status"], "pending");
    assert_eq!(json["delivered"], false);
    assert!(json["outcome_at"].is_null());
    assert!(json["last_error"].is_null());
}

#[tokio::test]
async fn reminders_add_defaults_target_to_main() {
    let app = build_test_app(None);

    let response = app
        .oneshot(
            Request::post("/reminders")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&serde_json::json!({
                        "message": "Take meds",
                        "fire_at": "2026-04-01T08:00:00Z"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let json = body_json(response).await;
    assert_eq!(json["target"], "main");
}

#[tokio::test]
async fn reminders_list_includes_outcome_fields() {
    let session_repo = Arc::new(MemSessionRepo::new());
    let turn_repo = Arc::new(MemTurnRepo::new());
    let transcript_repo = Arc::new(MemTranscriptRepo::new());
    let approval_repo = Arc::new(MemApprovalRepo::new());
    let model_provider: Arc<dyn ModelProvider> = Arc::new(FakeModelProvider);
    let scheduler = Arc::new(Scheduler::new());
    let session_engine = Arc::new(
        SessionEngine::new(session_repo.clone()).with_transcript_repo(transcript_repo.clone()),
    );
    let context_assembler = ContextAssembler::new("You are a test assistant.");
    let compaction: Arc<dyn CompactionStrategy> = Arc::new(NoOpCompaction);
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(FakeToolExecutor);
    let tool_registry = Arc::new(ToolRegistry::new());
    let turn_executor = Arc::new(
        TurnExecutor::new(
            session_repo.clone() as Arc<dyn SessionRepo>,
            turn_repo.clone() as Arc<dyn TurnRepo>,
            transcript_repo.clone() as Arc<dyn TranscriptRepo>,
            approval_repo.clone() as Arc<dyn ApprovalRepo>,
            model_provider.clone(),
            tool_executor,
            tool_registry,
            context_assembler,
            compaction,
        )
        .with_default_model("fake-model"),
    );
    let (event_tx, _) = broadcast::channel::<SessionEvent>(64);
    let skill_registry = Arc::new(SkillRegistry::new());
    let skill_loader = Arc::new(SkillLoader::new(
        std::env::temp_dir(),
        skill_registry.clone(),
    ));
    let device_repo = Arc::new(MemDeviceRepo::new());
    let device_registry = Arc::new(DeviceRegistry::new(device_repo.clone()));
    let (plugin_registry, plugin_loader, hook_registry) = test_plugins();

    // Pre-populate reminder store with a cancelled reminder.
    let reminder_store = Arc::new(ReminderStore::new());
    let reminder = rune_runtime::scheduler::Reminder::new(
        "Cancelled event",
        "isolated",
        chrono::Utc::now() + chrono::Duration::hours(2),
    );
    let id = reminder_store.add(reminder).await;
    reminder_store.cancel(&id).await;

    let state = AppState {
        config: Arc::new(RwLock::new(AppConfig::default())),
        started_at: Arc::new(Instant::now()),
        session_engine,
        turn_executor,
        session_repo: session_repo as Arc<dyn SessionRepo>,
        transcript_repo: transcript_repo as Arc<dyn TranscriptRepo>,
        turn_repo: turn_repo as Arc<dyn TurnRepo>,
        model_provider,
        scheduler,
        heartbeat: Arc::new(HeartbeatRunner::new(std::env::temp_dir())),
        reminder_store,
        approval_repo: approval_repo as Arc<dyn ApprovalRepo>,
        tool_approval_repo: Arc::new(MemToolApprovalPolicyRepo::new())
            as Arc<dyn ToolApprovalPolicyRepo>,
        process_manager: ProcessManager::new(),
        capabilities: test_capabilities(0),
        device_repo: device_repo as Arc<dyn DeviceRepo>,
        device_registry,
        skill_registry,
        skill_loader,
        plugin_registry,
        plugin_loader,
        hook_registry,
        event_tx,
        tts_engine: None,
        stt_engine: None,
    };

    let app = build_router(state, None);

    // includeDelivered=true so cancelled reminder is visible
    let response = app
        .oneshot(
            Request::get("/reminders?includeDelivered=true")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    let items = json.as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["target"], "isolated");
    assert_eq!(items[0]["status"], "cancelled");
    assert!(items[0]["outcome_at"].is_string());
    assert_eq!(items[0]["delivered"], false);
}

#[tokio::test]
async fn reminders_cancel_returns_success() {
    let session_repo = Arc::new(MemSessionRepo::new());
    let turn_repo = Arc::new(MemTurnRepo::new());
    let transcript_repo = Arc::new(MemTranscriptRepo::new());
    let approval_repo = Arc::new(MemApprovalRepo::new());
    let model_provider: Arc<dyn ModelProvider> = Arc::new(FakeModelProvider);
    let scheduler = Arc::new(Scheduler::new());
    let session_engine = Arc::new(
        SessionEngine::new(session_repo.clone()).with_transcript_repo(transcript_repo.clone()),
    );
    let context_assembler = ContextAssembler::new("You are a test assistant.");
    let compaction: Arc<dyn CompactionStrategy> = Arc::new(NoOpCompaction);
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(FakeToolExecutor);
    let tool_registry = Arc::new(ToolRegistry::new());
    let turn_executor = Arc::new(
        TurnExecutor::new(
            session_repo.clone() as Arc<dyn SessionRepo>,
            turn_repo.clone() as Arc<dyn TurnRepo>,
            transcript_repo.clone() as Arc<dyn TranscriptRepo>,
            approval_repo.clone() as Arc<dyn ApprovalRepo>,
            model_provider.clone(),
            tool_executor,
            tool_registry,
            context_assembler,
            compaction,
        )
        .with_default_model("fake-model"),
    );
    let (event_tx, _) = broadcast::channel::<SessionEvent>(64);
    let skill_registry = Arc::new(SkillRegistry::new());
    let skill_loader = Arc::new(SkillLoader::new(
        std::env::temp_dir(),
        skill_registry.clone(),
    ));
    let device_repo = Arc::new(MemDeviceRepo::new());
    let device_registry = Arc::new(DeviceRegistry::new(device_repo.clone()));
    let (plugin_registry, plugin_loader, hook_registry) = test_plugins();

    // Pre-populate reminder store with a pending reminder.
    let reminder_store = Arc::new(ReminderStore::new());
    let reminder = rune_runtime::scheduler::Reminder::new(
        "Cancel me",
        "main",
        chrono::Utc::now() + chrono::Duration::hours(1),
    );
    let id = reminder_store.add(reminder).await;

    let state = AppState {
        config: Arc::new(RwLock::new(AppConfig::default())),
        started_at: Arc::new(Instant::now()),
        session_engine,
        turn_executor,
        session_repo: session_repo as Arc<dyn SessionRepo>,
        transcript_repo: transcript_repo as Arc<dyn TranscriptRepo>,
        turn_repo: turn_repo as Arc<dyn TurnRepo>,
        model_provider,
        scheduler,
        heartbeat: Arc::new(HeartbeatRunner::new(std::env::temp_dir())),
        reminder_store: reminder_store.clone(),
        approval_repo: approval_repo as Arc<dyn ApprovalRepo>,
        tool_approval_repo: Arc::new(MemToolApprovalPolicyRepo::new())
            as Arc<dyn ToolApprovalPolicyRepo>,
        process_manager: ProcessManager::new(),
        capabilities: test_capabilities(0),
        device_repo: device_repo as Arc<dyn DeviceRepo>,
        device_registry,
        skill_registry,
        skill_loader,
        plugin_registry,
        plugin_loader,
        hook_registry,
        event_tx,
        tts_engine: None,
        stt_engine: None,
    };

    let app = build_router(state, None);

    let response = app
        .oneshot(
            Request::delete(format!("/reminders/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["success"], true);

    // Verify the reminder was actually cancelled in the store.
    let r = reminder_store.get(&id).await.unwrap();
    assert_eq!(
        serde_json::to_value(&r.status).unwrap(),
        serde_json::json!("cancelled")
    );
    assert!(r.outcome_at.is_some());
}

// ── Agent (subagent) control tests ───────────────────────────────────────────

#[tokio::test]
async fn agent_steer_success() {
    let session_repo = Arc::new(MemSessionRepo::new());
    let transcript_repo = Arc::new(MemTranscriptRepo::new());

    let now = chrono::Utc::now();
    let agent_id = Uuid::now_v7();
    session_repo
        .create(rune_store::models::NewSession {
            id: agent_id,
            kind: "subagent".into(),
            status: "running".into(),
            workspace_root: None,
            channel_ref: None,
            requester_session_id: None,
            latest_turn_id: None,
            metadata: serde_json::json!({}),
            created_at: now,
            updated_at: now,
            last_activity_at: now,
        })
        .await
        .unwrap();

    let turn_repo = Arc::new(MemTurnRepo::new());
    let approval_repo = Arc::new(MemApprovalRepo::new());
    let model_provider: Arc<dyn ModelProvider> = Arc::new(FakeModelProvider);
    let scheduler = Arc::new(Scheduler::new());
    let session_engine = Arc::new(
        SessionEngine::new(session_repo.clone()).with_transcript_repo(transcript_repo.clone()),
    );
    let context_assembler = ContextAssembler::new("test");
    let compaction: Arc<dyn CompactionStrategy> = Arc::new(NoOpCompaction);
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(FakeToolExecutor);
    let tool_registry = Arc::new(ToolRegistry::new());
    let turn_executor = Arc::new(
        TurnExecutor::new(
            session_repo.clone() as Arc<dyn SessionRepo>,
            turn_repo.clone() as Arc<dyn TurnRepo>,
            transcript_repo.clone() as Arc<dyn TranscriptRepo>,
            approval_repo.clone() as Arc<dyn ApprovalRepo>,
            model_provider.clone(),
            tool_executor,
            tool_registry,
            context_assembler,
            compaction,
        )
        .with_default_model("fake-model"),
    );
    let (event_tx, _) = broadcast::channel::<SessionEvent>(64);
    let skill_registry = Arc::new(SkillRegistry::new());
    let skill_loader = Arc::new(SkillLoader::new(
        std::env::temp_dir(),
        skill_registry.clone(),
    ));
    let device_repo = Arc::new(MemDeviceRepo::new());
    let device_registry = Arc::new(DeviceRegistry::new(device_repo.clone()));
    let (plugin_registry, plugin_loader, hook_registry) = test_plugins();

    let state = AppState {
        config: Arc::new(RwLock::new(AppConfig::default())),
        started_at: Arc::new(Instant::now()),
        session_engine,
        turn_executor,
        session_repo: session_repo.clone() as Arc<dyn SessionRepo>,
        transcript_repo: transcript_repo.clone() as Arc<dyn TranscriptRepo>,
        turn_repo: turn_repo as Arc<dyn TurnRepo>,
        model_provider,
        scheduler,
        heartbeat: Arc::new(HeartbeatRunner::new(std::env::temp_dir())),
        reminder_store: Arc::new(ReminderStore::new()),
        approval_repo: approval_repo as Arc<dyn ApprovalRepo>,
        tool_approval_repo: Arc::new(MemToolApprovalPolicyRepo::new())
            as Arc<dyn ToolApprovalPolicyRepo>,
        process_manager: ProcessManager::new(),
        capabilities: test_capabilities(0),
        device_repo: device_repo.clone() as Arc<dyn DeviceRepo>,
        device_registry,
        skill_registry,
        skill_loader,
        plugin_registry,
        plugin_loader,
        hook_registry,
        event_tx,
        tts_engine: None,
        stt_engine: None,
    };

    let app = build_router(state, None);

    let response = app
        .oneshot(
            Request::post(format!("/agents/{agent_id}/steer"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"message":"focus on tests"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["session_id"], agent_id.to_string());
    assert_eq!(json["accepted"], true);
    assert!(json["detail"].as_str().unwrap().contains("steering instruction delivered"));

    // Verify transcript got a status_note.
    let items = transcript_repo.list_by_session(agent_id).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind, "status_note");
    assert!(items[0].payload["content"]
        .as_str()
        .unwrap()
        .contains("focus on tests"));

    // Verify metadata was updated.
    let session = session_repo.find_by_id(agent_id).await.unwrap();
    assert_eq!(session.metadata["subagent_lifecycle"], "steered");
}

#[tokio::test]
async fn agent_steer_not_found() {
    let app = build_test_app(None);
    let fake_id = Uuid::now_v7();

    let response = app
        .oneshot(
            Request::post(format!("/agents/{fake_id}/steer"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"message":"hello"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let json = body_json(response).await;
    assert_eq!(json["code"], "session_not_found");
}

#[tokio::test]
async fn agent_kill_success() {
    let session_repo = Arc::new(MemSessionRepo::new());
    let transcript_repo = Arc::new(MemTranscriptRepo::new());

    let now = chrono::Utc::now();
    let agent_id = Uuid::now_v7();
    session_repo
        .create(rune_store::models::NewSession {
            id: agent_id,
            kind: "subagent".into(),
            status: "running".into(),
            workspace_root: None,
            channel_ref: None,
            requester_session_id: None,
            latest_turn_id: None,
            metadata: serde_json::json!({}),
            created_at: now,
            updated_at: now,
            last_activity_at: now,
        })
        .await
        .unwrap();

    let turn_repo = Arc::new(MemTurnRepo::new());
    let approval_repo = Arc::new(MemApprovalRepo::new());
    let model_provider: Arc<dyn ModelProvider> = Arc::new(FakeModelProvider);
    let scheduler = Arc::new(Scheduler::new());
    let session_engine = Arc::new(
        SessionEngine::new(session_repo.clone()).with_transcript_repo(transcript_repo.clone()),
    );
    let context_assembler = ContextAssembler::new("test");
    let compaction: Arc<dyn CompactionStrategy> = Arc::new(NoOpCompaction);
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(FakeToolExecutor);
    let tool_registry = Arc::new(ToolRegistry::new());
    let turn_executor = Arc::new(
        TurnExecutor::new(
            session_repo.clone() as Arc<dyn SessionRepo>,
            turn_repo.clone() as Arc<dyn TurnRepo>,
            transcript_repo.clone() as Arc<dyn TranscriptRepo>,
            approval_repo.clone() as Arc<dyn ApprovalRepo>,
            model_provider.clone(),
            tool_executor,
            tool_registry,
            context_assembler,
            compaction,
        )
        .with_default_model("fake-model"),
    );
    let (event_tx, _) = broadcast::channel::<SessionEvent>(64);
    let skill_registry = Arc::new(SkillRegistry::new());
    let skill_loader = Arc::new(SkillLoader::new(
        std::env::temp_dir(),
        skill_registry.clone(),
    ));
    let device_repo = Arc::new(MemDeviceRepo::new());
    let device_registry = Arc::new(DeviceRegistry::new(device_repo.clone()));
    let (plugin_registry, plugin_loader, hook_registry) = test_plugins();

    let state = AppState {
        config: Arc::new(RwLock::new(AppConfig::default())),
        started_at: Arc::new(Instant::now()),
        session_engine,
        turn_executor,
        session_repo: session_repo.clone() as Arc<dyn SessionRepo>,
        transcript_repo: transcript_repo.clone() as Arc<dyn TranscriptRepo>,
        turn_repo: turn_repo as Arc<dyn TurnRepo>,
        model_provider,
        scheduler,
        heartbeat: Arc::new(HeartbeatRunner::new(std::env::temp_dir())),
        reminder_store: Arc::new(ReminderStore::new()),
        approval_repo: approval_repo as Arc<dyn ApprovalRepo>,
        tool_approval_repo: Arc::new(MemToolApprovalPolicyRepo::new())
            as Arc<dyn ToolApprovalPolicyRepo>,
        process_manager: ProcessManager::new(),
        capabilities: test_capabilities(0),
        device_repo: device_repo.clone() as Arc<dyn DeviceRepo>,
        device_registry,
        skill_registry,
        skill_loader,
        plugin_registry,
        plugin_loader,
        hook_registry,
        event_tx,
        tts_engine: None,
        stt_engine: None,
    };

    let app = build_router(state, None);

    let response = app
        .oneshot(
            Request::post(format!("/agents/{agent_id}/kill"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"reason":"no longer needed"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["session_id"], agent_id.to_string());
    assert_eq!(json["killed"], true);
    assert!(json["detail"].as_str().unwrap().contains("cancelled"));

    // Verify session status changed.
    let session = session_repo.find_by_id(agent_id).await.unwrap();
    assert_eq!(session.status, "cancelled");
    assert_eq!(session.metadata["subagent_lifecycle"], "cancelled");

    // Verify transcript got a status_note.
    let items = transcript_repo.list_by_session(agent_id).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind, "status_note");
    assert!(items[0].payload["content"]
        .as_str()
        .unwrap()
        .contains("no longer needed"));
}

#[tokio::test]
async fn agent_kill_not_found() {
    let app = build_test_app(None);
    let fake_id = Uuid::now_v7();

    let response = app
        .oneshot(
            Request::post(format!("/agents/{fake_id}/kill"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let json = body_json(response).await;
    assert_eq!(json["code"], "session_not_found");
}

#[tokio::test]
async fn ws_rpc_agent_steer_and_kill() {
    use rune_gateway::ws_rpc::RpcDispatcher;

    let session_repo = Arc::new(MemSessionRepo::new());
    let transcript_repo = Arc::new(MemTranscriptRepo::new());

    let now = chrono::Utc::now();
    let agent_id = Uuid::now_v7();
    session_repo
        .create(rune_store::models::NewSession {
            id: agent_id,
            kind: "subagent".into(),
            status: "running".into(),
            workspace_root: None,
            channel_ref: None,
            requester_session_id: None,
            latest_turn_id: None,
            metadata: serde_json::json!({}),
            created_at: now,
            updated_at: now,
            last_activity_at: now,
        })
        .await
        .unwrap();

    let turn_repo = Arc::new(MemTurnRepo::new());
    let approval_repo = Arc::new(MemApprovalRepo::new());
    let model_provider: Arc<dyn ModelProvider> = Arc::new(FakeModelProvider);
    let scheduler = Arc::new(Scheduler::new());
    let session_engine = Arc::new(
        SessionEngine::new(session_repo.clone()).with_transcript_repo(transcript_repo.clone()),
    );
    let context_assembler = ContextAssembler::new("test");
    let compaction: Arc<dyn CompactionStrategy> = Arc::new(NoOpCompaction);
    let tool_executor: Arc<dyn ToolExecutor> = Arc::new(FakeToolExecutor);
    let tool_registry = Arc::new(ToolRegistry::new());
    let turn_executor = Arc::new(
        TurnExecutor::new(
            session_repo.clone() as Arc<dyn SessionRepo>,
            turn_repo.clone() as Arc<dyn TurnRepo>,
            transcript_repo.clone() as Arc<dyn TranscriptRepo>,
            approval_repo.clone() as Arc<dyn ApprovalRepo>,
            model_provider.clone(),
            tool_executor,
            tool_registry,
            context_assembler,
            compaction,
        )
        .with_default_model("fake-model"),
    );
    let (event_tx, _) = broadcast::channel::<SessionEvent>(64);
    let skill_registry = Arc::new(SkillRegistry::new());
    let skill_loader = Arc::new(SkillLoader::new(
        std::env::temp_dir(),
        skill_registry.clone(),
    ));
    let device_repo = Arc::new(MemDeviceRepo::new());
    let device_registry = Arc::new(DeviceRegistry::new(device_repo.clone()));
    let (plugin_registry, plugin_loader, hook_registry) = test_plugins();

    let state = AppState {
        config: Arc::new(RwLock::new(AppConfig::default())),
        started_at: Arc::new(Instant::now()),
        session_engine,
        turn_executor,
        session_repo: session_repo as Arc<dyn SessionRepo>,
        transcript_repo: transcript_repo as Arc<dyn TranscriptRepo>,
        turn_repo: turn_repo as Arc<dyn TurnRepo>,
        model_provider,
        scheduler,
        heartbeat: Arc::new(HeartbeatRunner::new(std::env::temp_dir())),
        reminder_store: Arc::new(ReminderStore::new()),
        approval_repo: approval_repo as Arc<dyn ApprovalRepo>,
        tool_approval_repo: Arc::new(MemToolApprovalPolicyRepo::new())
            as Arc<dyn ToolApprovalPolicyRepo>,
        process_manager: ProcessManager::new(),
        capabilities: test_capabilities(0),
        device_repo: device_repo.clone() as Arc<dyn DeviceRepo>,
        device_registry,
        skill_registry,
        skill_loader,
        plugin_registry,
        plugin_loader,
        hook_registry,
        event_tx,
        tts_engine: None,
        stt_engine: None,
    };

    let dispatcher = RpcDispatcher::new(state);

    // Test agent.steer via WS-RPC.
    let steer_result = dispatcher
        .dispatch(
            "agent.steer",
            serde_json::json!({
                "session_id": agent_id.to_string(),
                "message": "prioritize security review"
            }),
        )
        .await
        .unwrap();
    assert_eq!(steer_result["accepted"], true);
    assert_eq!(steer_result["session_id"], agent_id.to_string());

    // Test agent.kill via WS-RPC.
    let kill_result = dispatcher
        .dispatch(
            "agent.kill",
            serde_json::json!({
                "session_id": agent_id.to_string(),
                "reason": "task complete"
            }),
        )
        .await
        .unwrap();
    assert_eq!(kill_result["killed"], true);
    assert_eq!(kill_result["session_id"], agent_id.to_string());

    // Test not-found via WS-RPC.
    let fake_id = Uuid::now_v7();
    let err = dispatcher
        .dispatch(
            "agent.steer",
            serde_json::json!({
                "session_id": fake_id.to_string(),
                "message": "hello"
            }),
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, "not_found");
}

// ── Configure / Setup route tests (#61) ───────────────────────────────────────

#[tokio::test]
async fn configure_returns_items_with_default_config() {
    let app = build_test_app(None);
    let response = app
        .oneshot(
            Request::post("/configure")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;

    // With default config: no providers, no auth → success should be false
    assert_eq!(json["success"], false);
    assert!(json["detail"].as_str().unwrap().contains("need configuration"));

    // Items array should exist and contain expected keys
    let items = json["items"].as_array().unwrap();
    let names: Vec<&str> = items.iter().map(|i| i["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"model_providers"));
    assert!(names.contains(&"auth"));
    assert!(names.contains(&"sessions_dir"));
    assert!(names.contains(&"memory_dir"));
    assert!(names.contains(&"tts"));
    assert!(names.contains(&"stt"));
    assert!(names.contains(&"channels"));
    assert!(names.contains(&"mcp_servers"));

    // model_providers should be "needed" with default config
    let mp = items.iter().find(|i| i["name"] == "model_providers").unwrap();
    assert_eq!(mp["status"], "needed");
}

#[tokio::test]
async fn setup_returns_same_shape_as_configure() {
    let app = build_test_app(None);
    let response = app
        .oneshot(
            Request::post("/setup")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert!(json["items"].is_array());
    assert!(json.get("success").is_some());
    assert!(json.get("detail").is_some());
}

#[tokio::test]
async fn configure_with_providers_reports_configured() {
    let mut config = AppConfig::default();
    config.gateway.auth_token = Some("test-token".to_string());
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

    let app = build_test_app_with_config(config, Some("test-token".to_string()));
    let response = app
        .oneshot(
            Request::post("/configure")
                .header("Authorization", "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;

    let items = json["items"].as_array().unwrap();

    // model_providers should be "configured"
    let mp = items.iter().find(|i| i["name"] == "model_providers").unwrap();
    assert_eq!(mp["status"], "configured");
    assert!(mp["message"].as_str().unwrap().contains("1 provider"));

    // auth should be "configured"
    let auth = items.iter().find(|i| i["name"] == "auth").unwrap();
    assert_eq!(auth["status"], "configured");

    // Optional items should be "skipped" (not "needed")
    let tts = items.iter().find(|i| i["name"] == "tts").unwrap();
    assert_eq!(tts["status"], "skipped");
}
