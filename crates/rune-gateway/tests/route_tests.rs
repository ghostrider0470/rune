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

use rune_gateway::ms365::{
    CreateCalendarEventRequest, CreatePlannerTaskRequest, CreateTodoTaskRequest, FileContent,
    FileItem, FileMetadata, FileSearchItem, FilesList, FilesSearch, ForwardMailRequest,
    Ms365CalendarService, Ms365CalendarServiceError, Ms365FilesService, Ms365FilesServiceError,
    Ms365MailService, Ms365MailServiceError, Ms365PlannerService, Ms365PlannerServiceError,
    Ms365TodoService, Ms365TodoServiceError, Ms365UsersService, Ms365UsersServiceError,
    PlannerTask, ReplyMailRequest, RespondCalendarEventRequest, SendMailRequest, TodoTask,
    UpdatePlannerTaskRequest, UpdateTodoTaskRequest, UserProfile, UserSummary, UsersList,
};
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
            runtime_profile: None,
            policy_profile: None,
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum PlannerMutationCall {
    Create(CreatePlannerTaskRequest),
    Update {
        id: String,
        request: UpdatePlannerTaskRequest,
    },
    Complete {
        id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TodoMutationCall {
    Create {
        list_id: String,
        request: CreateTodoTaskRequest,
    },
    Update {
        list_id: String,
        id: String,
        request: UpdateTodoTaskRequest,
    },
    Complete {
        list_id: String,
        id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CalendarMutationCall {
    Create(CreateCalendarEventRequest),
    Delete {
        id: String,
    },
    Respond {
        id: String,
        request: RespondCalendarEventRequest,
    },
}

#[derive(Debug)]
struct FakeMs365CalendarService {
    create_response: Result<(), Ms365CalendarServiceError>,
    delete_response: Result<(), Ms365CalendarServiceError>,
    respond_response: Result<(), Ms365CalendarServiceError>,
    calls: Mutex<Vec<CalendarMutationCall>>,
}

#[derive(Debug)]
struct FakeMs365PlannerService {
    create_response: Result<PlannerTask, Ms365PlannerServiceError>,
    update_response: Result<PlannerTask, Ms365PlannerServiceError>,
    complete_response: Result<PlannerTask, Ms365PlannerServiceError>,
    calls: Mutex<Vec<PlannerMutationCall>>,
}

#[derive(Debug)]
struct FakeMs365TodoService {
    create_response: Result<TodoTask, Ms365TodoServiceError>,
    update_response: Result<TodoTask, Ms365TodoServiceError>,
    complete_response: Result<TodoTask, Ms365TodoServiceError>,
    calls: Mutex<Vec<TodoMutationCall>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MailMutationCall {
    Send(SendMailRequest),
    Reply {
        id: String,
        request: ReplyMailRequest,
    },
    Forward {
        id: String,
        request: ForwardMailRequest,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum UsersReadCall {
    Me,
    List { limit: u32 },
    Read { id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FilesReadCall {
    List { path: String, limit: u32 },
    Read { id: String },
    Search { query: String, limit: u32 },
    Content { id: String },
}

#[derive(Debug)]
struct FakeMs365MailService {
    send_response: Result<(), Ms365MailServiceError>,
    reply_response: Result<(), Ms365MailServiceError>,
    forward_response: Result<(), Ms365MailServiceError>,
    calls: Mutex<Vec<MailMutationCall>>,
}

#[derive(Debug)]
struct FakeMs365FilesService {
    list_response: Result<FilesList, Ms365FilesServiceError>,
    read_response: Result<FileMetadata, Ms365FilesServiceError>,
    search_response: Result<FilesSearch, Ms365FilesServiceError>,
    content_response: Result<FileContent, Ms365FilesServiceError>,
    calls: Mutex<Vec<FilesReadCall>>,
}

#[derive(Debug)]
struct FakeMs365UsersService {
    me_response: Result<UserProfile, Ms365UsersServiceError>,
    list_response: Result<UsersList, Ms365UsersServiceError>,
    read_response: Result<UserProfile, Ms365UsersServiceError>,
    calls: Mutex<Vec<UsersReadCall>>,
}

impl Default for FakeMs365CalendarService {
    fn default() -> Self {
        Self {
            create_response: Err(Ms365CalendarServiceError::NotConfigured(
                "test calendar service not configured".to_string(),
            )),
            delete_response: Err(Ms365CalendarServiceError::NotConfigured(
                "test calendar service not configured".to_string(),
            )),
            respond_response: Err(Ms365CalendarServiceError::NotConfigured(
                "test calendar service not configured".to_string(),
            )),
            calls: Mutex::new(Vec::new()),
        }
    }
}

impl Default for FakeMs365PlannerService {
    fn default() -> Self {
        Self {
            create_response: Err(Ms365PlannerServiceError::NotConfigured(
                "test planner service not configured".to_string(),
            )),
            update_response: Err(Ms365PlannerServiceError::NotConfigured(
                "test planner service not configured".to_string(),
            )),
            complete_response: Err(Ms365PlannerServiceError::NotConfigured(
                "test planner service not configured".to_string(),
            )),
            calls: Mutex::new(Vec::new()),
        }
    }
}

impl Default for FakeMs365TodoService {
    fn default() -> Self {
        Self {
            create_response: Err(Ms365TodoServiceError::NotConfigured(
                "test todo service not configured".to_string(),
            )),
            update_response: Err(Ms365TodoServiceError::NotConfigured(
                "test todo service not configured".to_string(),
            )),
            complete_response: Err(Ms365TodoServiceError::NotConfigured(
                "test todo service not configured".to_string(),
            )),
            calls: Mutex::new(Vec::new()),
        }
    }
}

impl Default for FakeMs365MailService {
    fn default() -> Self {
        Self {
            send_response: Ok(()),
            reply_response: Ok(()),
            forward_response: Ok(()),
            calls: Mutex::new(Vec::new()),
        }
    }
}

impl Default for FakeMs365FilesService {
    fn default() -> Self {
        Self {
            list_response: Err(Ms365FilesServiceError::NotConfigured(
                "test files service not configured".to_string(),
            )),
            read_response: Err(Ms365FilesServiceError::NotConfigured(
                "test files service not configured".to_string(),
            )),
            search_response: Err(Ms365FilesServiceError::NotConfigured(
                "test files service not configured".to_string(),
            )),
            content_response: Err(Ms365FilesServiceError::NotConfigured(
                "test files service not configured".to_string(),
            )),
            calls: Mutex::new(Vec::new()),
        }
    }
}

impl Default for FakeMs365UsersService {
    fn default() -> Self {
        Self {
            me_response: Err(Ms365UsersServiceError::NotConfigured(
                "test users service not configured".to_string(),
            )),
            list_response: Err(Ms365UsersServiceError::NotConfigured(
                "test users service not configured".to_string(),
            )),
            read_response: Err(Ms365UsersServiceError::NotConfigured(
                "test users service not configured".to_string(),
            )),
            calls: Mutex::new(Vec::new()),
        }
    }
}

impl FakeMs365CalendarService {
    fn with_create_response(response: Result<(), Ms365CalendarServiceError>) -> Self {
        Self {
            create_response: response,
            ..Self::default()
        }
    }

    fn with_delete_response(response: Result<(), Ms365CalendarServiceError>) -> Self {
        Self {
            delete_response: response,
            ..Self::default()
        }
    }

    fn with_respond_response(response: Result<(), Ms365CalendarServiceError>) -> Self {
        Self {
            respond_response: response,
            ..Self::default()
        }
    }

    async fn calls(&self) -> Vec<CalendarMutationCall> {
        self.calls.lock().await.clone()
    }
}

impl FakeMs365PlannerService {
    fn with_create_response(response: Result<PlannerTask, Ms365PlannerServiceError>) -> Self {
        Self {
            create_response: response,
            ..Self::default()
        }
    }

    fn with_update_response(response: Result<PlannerTask, Ms365PlannerServiceError>) -> Self {
        Self {
            update_response: response,
            ..Self::default()
        }
    }

    fn with_complete_response(response: Result<PlannerTask, Ms365PlannerServiceError>) -> Self {
        Self {
            complete_response: response,
            ..Self::default()
        }
    }

    async fn calls(&self) -> Vec<PlannerMutationCall> {
        self.calls.lock().await.clone()
    }
}

impl FakeMs365TodoService {
    fn with_create_response(response: Result<TodoTask, Ms365TodoServiceError>) -> Self {
        Self {
            create_response: response,
            ..Self::default()
        }
    }

    fn with_update_response(response: Result<TodoTask, Ms365TodoServiceError>) -> Self {
        Self {
            update_response: response,
            ..Self::default()
        }
    }

    fn with_complete_response(response: Result<TodoTask, Ms365TodoServiceError>) -> Self {
        Self {
            complete_response: response,
            ..Self::default()
        }
    }

    async fn calls(&self) -> Vec<TodoMutationCall> {
        self.calls.lock().await.clone()
    }
}

impl FakeMs365FilesService {
    fn with_list_response(response: Result<FilesList, Ms365FilesServiceError>) -> Self {
        Self {
            list_response: response,
            ..Self::default()
        }
    }

    fn with_read_response(response: Result<FileMetadata, Ms365FilesServiceError>) -> Self {
        Self {
            read_response: response,
            ..Self::default()
        }
    }

    fn with_search_response(response: Result<FilesSearch, Ms365FilesServiceError>) -> Self {
        Self {
            search_response: response,
            ..Self::default()
        }
    }

    fn with_content_response(response: Result<FileContent, Ms365FilesServiceError>) -> Self {
        Self {
            content_response: response,
            ..Self::default()
        }
    }

    async fn calls(&self) -> Vec<FilesReadCall> {
        self.calls.lock().await.clone()
    }
}

impl FakeMs365UsersService {
    fn with_me_response(response: Result<UserProfile, Ms365UsersServiceError>) -> Self {
        Self {
            me_response: response,
            ..Self::default()
        }
    }

    fn with_list_response(response: Result<UsersList, Ms365UsersServiceError>) -> Self {
        Self {
            list_response: response,
            ..Self::default()
        }
    }

    fn with_read_response(response: Result<UserProfile, Ms365UsersServiceError>) -> Self {
        Self {
            read_response: response,
            ..Self::default()
        }
    }

    async fn calls(&self) -> Vec<UsersReadCall> {
        self.calls.lock().await.clone()
    }
}

#[async_trait]
impl Ms365CalendarService for FakeMs365CalendarService {
    async fn create_event(
        &self,
        request: CreateCalendarEventRequest,
    ) -> Result<(), Ms365CalendarServiceError> {
        self.calls
            .lock()
            .await
            .push(CalendarMutationCall::Create(request));
        self.create_response.clone()
    }

    async fn delete_event(&self, id: &str) -> Result<(), Ms365CalendarServiceError> {
        self.calls
            .lock()
            .await
            .push(CalendarMutationCall::Delete { id: id.to_string() });
        self.delete_response.clone()
    }

    async fn respond_to_event(
        &self,
        id: &str,
        request: RespondCalendarEventRequest,
    ) -> Result<(), Ms365CalendarServiceError> {
        self.calls.lock().await.push(CalendarMutationCall::Respond {
            id: id.to_string(),
            request,
        });
        self.respond_response.clone()
    }
}

#[async_trait]
impl Ms365PlannerService for FakeMs365PlannerService {
    async fn create_task(
        &self,
        request: CreatePlannerTaskRequest,
    ) -> Result<PlannerTask, Ms365PlannerServiceError> {
        self.calls
            .lock()
            .await
            .push(PlannerMutationCall::Create(request));
        self.create_response.clone()
    }

    async fn update_task(
        &self,
        id: &str,
        request: UpdatePlannerTaskRequest,
    ) -> Result<PlannerTask, Ms365PlannerServiceError> {
        self.calls.lock().await.push(PlannerMutationCall::Update {
            id: id.to_string(),
            request,
        });
        self.update_response.clone()
    }

    async fn complete_task(&self, id: &str) -> Result<PlannerTask, Ms365PlannerServiceError> {
        self.calls
            .lock()
            .await
            .push(PlannerMutationCall::Complete { id: id.to_string() });
        self.complete_response.clone()
    }
}

#[async_trait]
impl Ms365TodoService for FakeMs365TodoService {
    async fn create_task(
        &self,
        list_id: &str,
        request: CreateTodoTaskRequest,
    ) -> Result<TodoTask, Ms365TodoServiceError> {
        self.calls.lock().await.push(TodoMutationCall::Create {
            list_id: list_id.to_string(),
            request,
        });
        self.create_response.clone()
    }

    async fn update_task(
        &self,
        list_id: &str,
        id: &str,
        request: UpdateTodoTaskRequest,
    ) -> Result<TodoTask, Ms365TodoServiceError> {
        self.calls.lock().await.push(TodoMutationCall::Update {
            list_id: list_id.to_string(),
            id: id.to_string(),
            request,
        });
        self.update_response.clone()
    }

    async fn complete_task(
        &self,
        list_id: &str,
        id: &str,
    ) -> Result<TodoTask, Ms365TodoServiceError> {
        self.calls.lock().await.push(TodoMutationCall::Complete {
            list_id: list_id.to_string(),
            id: id.to_string(),
        });
        self.complete_response.clone()
    }
}

#[async_trait]
impl Ms365MailService for FakeMs365MailService {
    async fn send_mail(&self, request: SendMailRequest) -> Result<(), Ms365MailServiceError> {
        self.calls
            .lock()
            .await
            .push(MailMutationCall::Send(request));
        self.send_response.clone()
    }

    async fn reply_to_message(
        &self,
        id: &str,
        request: ReplyMailRequest,
    ) -> Result<(), Ms365MailServiceError> {
        self.calls.lock().await.push(MailMutationCall::Reply {
            id: id.to_string(),
            request,
        });
        self.reply_response.clone()
    }

    async fn forward_message(
        &self,
        id: &str,
        request: ForwardMailRequest,
    ) -> Result<(), Ms365MailServiceError> {
        self.calls.lock().await.push(MailMutationCall::Forward {
            id: id.to_string(),
            request,
        });
        self.forward_response.clone()
    }
}

#[async_trait]
impl Ms365FilesService for FakeMs365FilesService {
    async fn list(&self, path: &str, limit: u32) -> Result<FilesList, Ms365FilesServiceError> {
        self.calls.lock().await.push(FilesReadCall::List {
            path: path.to_string(),
            limit,
        });
        self.list_response.clone()
    }

    async fn read(&self, id: &str) -> Result<FileMetadata, Ms365FilesServiceError> {
        self.calls
            .lock()
            .await
            .push(FilesReadCall::Read { id: id.to_string() });
        self.read_response.clone()
    }

    async fn search(&self, query: &str, limit: u32) -> Result<FilesSearch, Ms365FilesServiceError> {
        self.calls.lock().await.push(FilesReadCall::Search {
            query: query.to_string(),
            limit,
        });
        self.search_response.clone()
    }

    async fn download_content(&self, id: &str) -> Result<FileContent, Ms365FilesServiceError> {
        self.calls
            .lock()
            .await
            .push(FilesReadCall::Content { id: id.to_string() });
        self.content_response.clone()
    }
}

#[async_trait]
impl Ms365UsersService for FakeMs365UsersService {
    async fn me(&self) -> Result<UserProfile, Ms365UsersServiceError> {
        self.calls.lock().await.push(UsersReadCall::Me);
        self.me_response.clone()
    }

    async fn list(&self, limit: u32) -> Result<UsersList, Ms365UsersServiceError> {
        self.calls.lock().await.push(UsersReadCall::List { limit });
        self.list_response.clone()
    }

    async fn read(&self, id: &str) -> Result<UserProfile, Ms365UsersServiceError> {
        self.calls
            .lock()
            .await
            .push(UsersReadCall::Read { id: id.to_string() });
        self.read_response.clone()
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

fn test_ms365_calendar_service() -> Arc<dyn Ms365CalendarService> {
    Arc::new(FakeMs365CalendarService::default())
}

fn test_ms365_planner_service() -> Arc<dyn Ms365PlannerService> {
    Arc::new(FakeMs365PlannerService::default())
}

fn test_ms365_todo_service() -> Arc<dyn Ms365TodoService> {
    Arc::new(FakeMs365TodoService::default())
}

fn test_ms365_mail_service() -> Arc<dyn Ms365MailService> {
    Arc::new(FakeMs365MailService::default())
}

fn test_ms365_files_service() -> Arc<dyn Ms365FilesService> {
    Arc::new(FakeMs365FilesService::default())
}

fn test_ms365_users_service() -> Arc<dyn Ms365UsersService> {
    Arc::new(FakeMs365UsersService::default())
}

fn build_test_app_parts(
    config: AppConfig,
    auth_token: Option<String>,
) -> (axum::Router, Arc<MemDeviceRepo>) {
    build_test_app_parts_with_ms365_services(
        config,
        auth_token,
        test_ms365_calendar_service(),
        test_ms365_planner_service(),
        test_ms365_todo_service(),
        test_ms365_files_service(),
        test_ms365_users_service(),
    )
}

fn build_test_app_parts_with_calendar_service(
    config: AppConfig,
    auth_token: Option<String>,
    ms365_calendar_service: Arc<dyn Ms365CalendarService>,
) -> (axum::Router, Arc<MemDeviceRepo>) {
    build_test_app_parts_with_ms365_services(
        config,
        auth_token,
        ms365_calendar_service,
        test_ms365_planner_service(),
        test_ms365_todo_service(),
        test_ms365_files_service(),
        test_ms365_users_service(),
    )
}

fn build_test_app_parts_with_planner_service(
    config: AppConfig,
    auth_token: Option<String>,
    ms365_planner_service: Arc<dyn Ms365PlannerService>,
) -> (axum::Router, Arc<MemDeviceRepo>) {
    build_test_app_parts_with_ms365_services(
        config,
        auth_token,
        test_ms365_calendar_service(),
        ms365_planner_service,
        test_ms365_todo_service(),
        test_ms365_files_service(),
        test_ms365_users_service(),
    )
}

fn build_test_app_parts_with_todo_service(
    config: AppConfig,
    auth_token: Option<String>,
    ms365_todo_service: Arc<dyn Ms365TodoService>,
) -> (axum::Router, Arc<MemDeviceRepo>) {
    build_test_app_parts_with_ms365_services(
        config,
        auth_token,
        test_ms365_calendar_service(),
        test_ms365_planner_service(),
        ms365_todo_service,
        test_ms365_files_service(),
        test_ms365_users_service(),
    )
}

fn build_test_app_parts_with_files_service(
    config: AppConfig,
    auth_token: Option<String>,
    ms365_files_service: Arc<dyn Ms365FilesService>,
) -> (axum::Router, Arc<MemDeviceRepo>) {
    build_test_app_parts_with_ms365_services(
        config,
        auth_token,
        test_ms365_calendar_service(),
        test_ms365_planner_service(),
        test_ms365_todo_service(),
        ms365_files_service,
        test_ms365_users_service(),
    )
}

fn build_test_app_parts_with_users_service(
    config: AppConfig,
    auth_token: Option<String>,
    ms365_users_service: Arc<dyn Ms365UsersService>,
) -> (axum::Router, Arc<MemDeviceRepo>) {
    build_test_app_parts_with_ms365_services(
        config,
        auth_token,
        test_ms365_calendar_service(),
        test_ms365_planner_service(),
        test_ms365_todo_service(),
        test_ms365_files_service(),
        ms365_users_service,
    )
}

fn build_test_app_parts_with_ms365_services(
    mut config: AppConfig,
    auth_token: Option<String>,
    ms365_calendar_service: Arc<dyn Ms365CalendarService>,
    ms365_planner_service: Arc<dyn Ms365PlannerService>,
    ms365_todo_service: Arc<dyn Ms365TodoService>,
    ms365_files_service: Arc<dyn Ms365FilesService>,
    ms365_users_service: Arc<dyn Ms365UsersService>,
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
        ms365_calendar_service,
        ms365_planner_service,
        ms365_todo_service,
        ms365_mail_service: test_ms365_mail_service(),
        ms365_files_service,
        ms365_users_service,
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

fn sample_user_profile(id: &str, mail: &str) -> UserProfile {
    UserProfile {
        id: id.to_string(),
        display_name: "Test User".to_string(),
        user_principal_name: mail.to_string(),
        mail: Some(mail.to_string()),
        job_title: Some("Engineer".to_string()),
        department: Some("Platform".to_string()),
        office_location: Some("Sarajevo".to_string()),
        mobile_phone: Some("+38761111222".to_string()),
    }
}

fn sample_file_item(id: &str, name: &str, is_folder: bool) -> FileItem {
    FileItem {
        id: id.to_string(),
        name: name.to_string(),
        size: if is_folder { 0 } else { 4096 },
        is_folder,
        last_modified: "2026-03-25T16:30:00Z".to_string(),
        web_url: Some(format!("https://example.com/files/{id}")),
    }
}

fn sample_file_metadata(id: &str, name: &str) -> FileMetadata {
    FileMetadata {
        id: id.to_string(),
        name: name.to_string(),
        size: 4096,
        is_folder: false,
        mime_type: Some("application/pdf".to_string()),
        last_modified: "2026-03-25T16:30:00Z".to_string(),
        created_at: "2026-03-24T09:15:00Z".to_string(),
        web_url: Some(format!("https://example.com/files/{id}")),
        parent_path: Some("/Shared Documents".to_string()),
        download_url: Some(format!("https://download.example.com/{id}")),
    }
}

fn sample_file_search_item(id: &str, name: &str) -> FileSearchItem {
    FileSearchItem {
        id: id.to_string(),
        name: name.to_string(),
        size: 2048,
        is_folder: false,
        last_modified: "2026-03-25T16:30:00Z".to_string(),
        web_url: Some(format!("https://example.com/files/{id}")),
        parent_path: Some("/Shared Documents/Search".to_string()),
    }
}

fn sample_file_content(id: &str, filename: &str) -> FileContent {
    FileContent {
        filename: filename.to_string(),
        content_type: "application/pdf".to_string(),
        bytes: format!("file-bytes-{id}").into_bytes(),
    }
}

fn sample_planner_task(id: &str) -> PlannerTask {
    PlannerTask {
        id: id.to_string(),
        title: "Draft release notes".to_string(),
        plan_id: "plan-123".to_string(),
        bucket_id: Some("bucket-456".to_string()),
        percent_complete: 25,
        assigned_to: Some("user-789".to_string()),
        due_date: Some("2026-03-30T12:00:00Z".to_string()),
        created_at: Some("2026-03-22T10:15:00Z".to_string()),
        priority: Some(5),
        description: Some("Collect user-facing changes.".to_string()),
    }
}

fn sample_calendar_create_request() -> CreateCalendarEventRequest {
    CreateCalendarEventRequest {
        subject: "Sprint review".to_string(),
        start: "2026-03-30T09:00:00Z".to_string(),
        end: "2026-03-30T10:00:00Z".to_string(),
        attendees: vec![
            "alice@example.com".to_string(),
            "bob@example.com".to_string(),
        ],
        location: Some("Conference Room A".to_string()),
        body: Some("Walk through the shipped backend slice.".to_string()),
    }
}

fn sample_todo_task(list_id: &str, id: &str) -> TodoTask {
    TodoTask {
        id: id.to_string(),
        list_id: list_id.to_string(),
        title: "Draft operator checklist".to_string(),
        status: "inProgress".to_string(),
        importance: "high".to_string(),
        is_reminder_on: false,
        due_date: Some("2026-03-30T12:00:00Z".to_string()),
        completed_at: None,
        created_at: Some("2026-03-22T10:15:00Z".to_string()),
        body_preview: Some("Line up the backend follow-on work.".to_string()),
    }
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
        ms365_calendar_service: test_ms365_calendar_service(),
        ms365_planner_service: test_ms365_planner_service(),
        ms365_todo_service: test_ms365_todo_service(),
        ms365_mail_service: test_ms365_mail_service(),
        ms365_files_service: test_ms365_files_service(),
        ms365_users_service: test_ms365_users_service(),
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
        ms365_calendar_service: test_ms365_calendar_service(),
        ms365_planner_service: test_ms365_planner_service(),
        ms365_todo_service: test_ms365_todo_service(),
        ms365_mail_service: test_ms365_mail_service(),
        ms365_files_service: test_ms365_files_service(),
        ms365_users_service: test_ms365_users_service(),
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
        ms365_calendar_service: test_ms365_calendar_service(),
        ms365_planner_service: test_ms365_planner_service(),
        ms365_todo_service: test_ms365_todo_service(),
        ms365_mail_service: test_ms365_mail_service(),
        ms365_files_service: test_ms365_files_service(),
        ms365_users_service: test_ms365_users_service(),
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
        ms365_calendar_service: test_ms365_calendar_service(),
        ms365_planner_service: test_ms365_planner_service(),
        ms365_todo_service: test_ms365_todo_service(),
        ms365_mail_service: test_ms365_mail_service(),
        ms365_files_service: test_ms365_files_service(),
        ms365_users_service: test_ms365_users_service(),
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
            runtime_profile: None,
            policy_profile: None,
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
            runtime_profile: None,
            policy_profile: None,
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
        ms365_calendar_service: test_ms365_calendar_service(),
        ms365_planner_service: test_ms365_planner_service(),
        ms365_todo_service: test_ms365_todo_service(),
        ms365_mail_service: test_ms365_mail_service(),
        ms365_files_service: test_ms365_files_service(),
        ms365_users_service: test_ms365_users_service(),
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
        ms365_calendar_service: test_ms365_calendar_service(),
        ms365_planner_service: test_ms365_planner_service(),
        ms365_todo_service: test_ms365_todo_service(),
        ms365_mail_service: test_ms365_mail_service(),
        ms365_files_service: test_ms365_files_service(),
        ms365_users_service: test_ms365_users_service(),
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
            runtime_profile: None,
            policy_profile: None,
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
        ms365_calendar_service: test_ms365_calendar_service(),
        ms365_planner_service: test_ms365_planner_service(),
        ms365_todo_service: test_ms365_todo_service(),
        ms365_mail_service: test_ms365_mail_service(),
        ms365_files_service: test_ms365_files_service(),
        ms365_users_service: test_ms365_users_service(),
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
    assert!(unresolved.iter().any(|item| item.as_str() == Some("approval requests, operator-triggered resume, and restart-safe mid-resume continuation are durable")));
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
            runtime_profile: None,
            policy_profile: None,
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
        ms365_calendar_service: test_ms365_calendar_service(),
        ms365_planner_service: test_ms365_planner_service(),
        ms365_todo_service: test_ms365_todo_service(),
        ms365_mail_service: test_ms365_mail_service(),
        ms365_files_service: test_ms365_files_service(),
        ms365_users_service: test_ms365_users_service(),
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
        ms365_calendar_service: test_ms365_calendar_service(),
        ms365_planner_service: test_ms365_planner_service(),
        ms365_todo_service: test_ms365_todo_service(),
        ms365_mail_service: test_ms365_mail_service(),
        ms365_files_service: test_ms365_files_service(),
        ms365_users_service: test_ms365_users_service(),
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
async fn ws_rpc_turns_list_and_get_return_turn_rows() {
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
        ms365_calendar_service: test_ms365_calendar_service(),
        ms365_planner_service: test_ms365_planner_service(),
        ms365_todo_service: test_ms365_todo_service(),
        ms365_mail_service: test_ms365_mail_service(),
        ms365_files_service: test_ms365_files_service(),
        ms365_users_service: test_ms365_users_service(),
    };

    let session_id = Uuid::new_v4();
    let first_turn_id = Uuid::new_v4();
    let second_turn_id = Uuid::new_v4();
    let started_at = chrono::Utc::now();

    turn_repo
        .create(NewTurn {
            id: first_turn_id,
            session_id,
            trigger_kind: "message".to_string(),
            status: "completed".to_string(),
            model_ref: Some("gpt-4.1".to_string()),
            started_at,
            ended_at: Some(started_at),
            usage_prompt_tokens: Some(10),
            usage_completion_tokens: Some(5),
        })
        .await
        .unwrap();
    turn_repo
        .create(NewTurn {
            id: second_turn_id,
            session_id,
            trigger_kind: "cron".to_string(),
            status: "running".to_string(),
            model_ref: Some("gpt-4.1-mini".to_string()),
            started_at: started_at + chrono::TimeDelta::seconds(1),
            ended_at: None,
            usage_prompt_tokens: None,
            usage_completion_tokens: None,
        })
        .await
        .unwrap();

    let dispatcher = RpcDispatcher::new(state);

    let turns = dispatcher
        .dispatch(
            "turns.list",
            serde_json::json!({
                "session_id": session_id,
                "limit": 1,
                "offset": 1
            }),
        )
        .await
        .unwrap();
    let turns = turns.as_array().unwrap();
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0]["id"], second_turn_id.to_string());
    assert_eq!(turns[0]["trigger_kind"], "cron");
    assert_eq!(turns[0]["status"], "running");
    assert_eq!(turns[0]["ended_at"], Value::Null);

    let turn = dispatcher
        .dispatch("turns.get", serde_json::json!({ "turn_id": first_turn_id }))
        .await
        .unwrap();
    assert_eq!(turn["id"], first_turn_id.to_string());
    assert_eq!(turn["session_id"], session_id.to_string());
    assert_eq!(turn["usage_prompt_tokens"], 10);
    assert_eq!(turn["usage_completion_tokens"], 5);
    assert_eq!(turn["model_ref"], "gpt-4.1");
}

#[tokio::test]
async fn ws_rpc_tools_and_approvals_list_surface_state() {
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

    skill_registry
        .register(rune_runtime::Skill {
            name: "memory_search".to_string(),
            description: "Search memory files".to_string(),
            parameters: serde_json::json!({"type": "object"}),
            binary_path: None,
            source_dir: std::env::temp_dir().join("memory_search"),
            enabled: true,
        })
        .await;

    let approval_id = Uuid::new_v4();
    let created_at = chrono::Utc::now();
    approval_repo
        .create(NewApproval {
            id: approval_id,
            subject_type: "tool_call".to_string(),
            subject_id: Uuid::new_v4(),
            reason: "needs network access".to_string(),
            presented_payload: serde_json::json!({ "tool": "web_fetch" }),
            created_at,
            handle_ref: None,
            host_ref: None,
        })
        .await
        .unwrap();

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
        capabilities: test_capabilities(1),
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
        ms365_calendar_service: test_ms365_calendar_service(),
        ms365_planner_service: test_ms365_planner_service(),
        ms365_todo_service: test_ms365_todo_service(),
        ms365_mail_service: test_ms365_mail_service(),
        ms365_files_service: test_ms365_files_service(),
        ms365_users_service: test_ms365_users_service(),
    };

    let dispatcher = RpcDispatcher::new(state);

    let tools = dispatcher
        .dispatch("tools.list", serde_json::json!({}))
        .await
        .unwrap();
    let tools = tools.as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "memory_search");
    assert_eq!(tools[0]["description"], "Search memory files");
    assert_eq!(tools[0]["enabled"], true);

    let approvals = dispatcher
        .dispatch("approvals.list", serde_json::json!({}))
        .await
        .unwrap();
    let approvals = approvals.as_array().unwrap();
    assert_eq!(approvals.len(), 1);
    assert_eq!(approvals[0]["id"], approval_id.to_string());
    assert_eq!(approvals[0]["subject_type"], "tool_call");
    assert!(approvals[0]["subject_id"].as_str().is_some());
    assert_eq!(approvals[0]["reason"], "needs network access");
    assert_eq!(approvals[0]["decision"], Value::Null);
    assert_eq!(approvals[0]["presented_payload"]["tool"], "web_fetch");
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
        ms365_calendar_service: test_ms365_calendar_service(),
        ms365_planner_service: test_ms365_planner_service(),
        ms365_todo_service: test_ms365_todo_service(),
        ms365_mail_service: test_ms365_mail_service(),
        ms365_files_service: test_ms365_files_service(),
        ms365_users_service: test_ms365_users_service(),
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
        ms365_calendar_service: test_ms365_calendar_service(),
        ms365_planner_service: test_ms365_planner_service(),
        ms365_todo_service: test_ms365_todo_service(),
        ms365_mail_service: test_ms365_mail_service(),
        ms365_files_service: test_ms365_files_service(),
        ms365_users_service: test_ms365_users_service(),
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
        ms365_calendar_service: test_ms365_calendar_service(),
        ms365_planner_service: test_ms365_planner_service(),
        ms365_todo_service: test_ms365_todo_service(),
        ms365_mail_service: test_ms365_mail_service(),
        ms365_files_service: test_ms365_files_service(),
        ms365_users_service: test_ms365_users_service(),
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
        ms365_calendar_service: test_ms365_calendar_service(),
        ms365_planner_service: test_ms365_planner_service(),
        ms365_todo_service: test_ms365_todo_service(),
        ms365_mail_service: test_ms365_mail_service(),
        ms365_files_service: test_ms365_files_service(),
        ms365_users_service: test_ms365_users_service(),
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
async fn ms365_files_list_forwards_path_and_clamped_limit() {
    let files_service = Arc::new(FakeMs365FilesService::with_list_response(Ok(FilesList {
        items: vec![
            sample_file_item("file-1", "Quarterly Report.pdf", false),
            sample_file_item("folder-1", "Shared", true),
        ],
        path: "/Documents".to_string(),
        total: 2,
    })));
    let (app, _device_repo) =
        build_test_app_parts_with_files_service(AppConfig::default(), None, files_service.clone());

    let response = app
        .oneshot(
            Request::get("/ms365/files?path=/Documents&limit=250")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let json = body_json(response).await;
    assert_eq!(json["path"], "/Documents");
    assert_eq!(json["total"], 2);
    assert_eq!(json["items"][0]["name"], "Quarterly Report.pdf");
    assert_eq!(json["items"][1]["is_folder"], true);

    let calls = files_service.calls().await;
    assert_eq!(
        calls,
        vec![FilesReadCall::List {
            path: "/Documents".to_string(),
            limit: 100,
        }]
    );
}

#[tokio::test]
async fn ms365_files_read_forwards_id_and_returns_metadata() {
    let files_service = Arc::new(FakeMs365FilesService::with_read_response(Ok(
        sample_file_metadata("file-42", "Specs.pdf"),
    )));
    let (app, _device_repo) =
        build_test_app_parts_with_files_service(AppConfig::default(), None, files_service.clone());

    let response = app
        .oneshot(
            Request::get("/ms365/files/file-42")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let json = body_json(response).await;
    assert_eq!(json["id"], "file-42");
    assert_eq!(json["name"], "Specs.pdf");
    assert_eq!(json["mime_type"], "application/pdf");
    assert_eq!(json["parent_path"], "/Shared Documents");

    let calls = files_service.calls().await;
    assert_eq!(
        calls,
        vec![FilesReadCall::Read {
            id: "file-42".to_string(),
        }]
    );
}

#[tokio::test]
async fn ms365_files_search_forwards_query_and_returns_results() {
    let files_service = Arc::new(FakeMs365FilesService::with_search_response(Ok(
        FilesSearch {
            items: vec![sample_file_search_item("search-1", "roadmap-notes.md")],
            query: "roadmap".to_string(),
            total: 1,
        },
    )));
    let (app, _device_repo) =
        build_test_app_parts_with_files_service(AppConfig::default(), None, files_service.clone());

    let response = app
        .oneshot(
            Request::get("/ms365/files/search?query=roadmap&limit=0")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let json = body_json(response).await;
    assert_eq!(json["query"], "roadmap");
    assert_eq!(json["total"], 1);
    assert_eq!(json["items"][0]["id"], "search-1");

    let calls = files_service.calls().await;
    assert_eq!(
        calls,
        vec![FilesReadCall::Search {
            query: "roadmap".to_string(),
            limit: 1,
        }]
    );
}

#[tokio::test]
async fn ms365_files_content_returns_bytes_with_headers() {
    let files_service = Arc::new(FakeMs365FilesService::with_content_response(Ok(
        sample_file_content("file-77", "handoff.pdf"),
    )));
    let (app, _device_repo) =
        build_test_app_parts_with_files_service(AppConfig::default(), None, files_service.clone());

    let response = app
        .oneshot(
            Request::get("/ms365/files/file-77/content")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/pdf"
    );
    assert_eq!(
        response.headers().get(header::CONTENT_DISPOSITION).unwrap(),
        "attachment; filename=\"handoff.pdf\""
    );
    let body = body_text(response).await;
    assert_eq!(body, "file-bytes-file-77");

    let calls = files_service.calls().await;
    assert_eq!(
        calls,
        vec![FilesReadCall::Content {
            id: "file-77".to_string(),
        }]
    );
}

#[tokio::test]
async fn ms365_files_routes_require_auth_when_gateway_token_enabled() {
    let app = build_test_app(Some(TEST_AUTH_TOKEN.to_string()));

    let response = app
        .clone()
        .oneshot(Request::get("/ms365/files").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .oneshot(
            Request::get("/ms365/files/search?query=doc")
                .header(header::AUTHORIZATION, format!("Bearer {TEST_AUTH_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn ms365_files_read_maps_validation_errors_to_bad_request() {
    let files_service = Arc::new(FakeMs365FilesService::with_read_response(Err(
        Ms365FilesServiceError::Validation("file item id is required".to_string()),
    )));
    let (app, _device_repo) =
        build_test_app_parts_with_files_service(AppConfig::default(), None, files_service);

    let response = app
        .oneshot(
            Request::get("/ms365/files/%20")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let json = body_json(response).await;
    assert!(
        json["message"]
            .as_str()
            .unwrap()
            .contains("file item id is required")
    );
}

#[tokio::test]
async fn ms365_users_me_forwards_request_and_returns_profile() {
    let users_service = Arc::new(FakeMs365UsersService::with_me_response(Ok(
        sample_user_profile("user-me-1", "me@example.com"),
    )));
    let (app, _device_repo) =
        build_test_app_parts_with_users_service(AppConfig::default(), None, users_service.clone());

    let response = app
        .oneshot(Request::get("/ms365/users/me").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let json = body_json(response).await;
    assert_eq!(json["user"]["id"], "user-me-1");
    assert_eq!(json["user"]["user_principal_name"], "me@example.com");

    let calls = users_service.calls().await;
    assert_eq!(calls, vec![UsersReadCall::Me]);
}

#[tokio::test]
async fn ms365_users_list_forwards_limit_and_returns_directory_page() {
    let users_service = Arc::new(FakeMs365UsersService::with_list_response(Ok(UsersList {
        users: vec![
            UserSummary {
                id: "user-1".to_string(),
                display_name: "Ada Lovelace".to_string(),
                user_principal_name: "ada@example.com".to_string(),
                job_title: Some("Engineer".to_string()),
            },
            UserSummary {
                id: "user-2".to_string(),
                display_name: "Grace Hopper".to_string(),
                user_principal_name: "grace@example.com".to_string(),
                job_title: Some("Admiral".to_string()),
            },
        ],
        total: 2,
    })));
    let (app, _device_repo) =
        build_test_app_parts_with_users_service(AppConfig::default(), None, users_service.clone());

    let response = app
        .oneshot(
            Request::get("/ms365/users?limit=250")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let json = body_json(response).await;
    assert_eq!(json["total"], 2);
    assert_eq!(json["users"][0]["id"], "user-1");
    assert_eq!(json["users"][1]["user_principal_name"], "grace@example.com");

    let calls = users_service.calls().await;
    assert_eq!(calls, vec![UsersReadCall::List { limit: 100 }]);
}

#[tokio::test]
async fn ms365_users_read_forwards_id_and_returns_profile() {
    let users_service = Arc::new(FakeMs365UsersService::with_read_response(Ok(
        sample_user_profile("user-read-1", "reader@example.com"),
    )));
    let (app, _device_repo) =
        build_test_app_parts_with_users_service(AppConfig::default(), None, users_service.clone());

    let response = app
        .oneshot(
            Request::get("/ms365/users/reader@example.com")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let json = body_json(response).await;
    assert_eq!(json["user"]["id"], "user-read-1");
    assert_eq!(json["user"]["mail"], "reader@example.com");

    let calls = users_service.calls().await;
    assert_eq!(
        calls,
        vec![UsersReadCall::Read {
            id: "reader@example.com".to_string(),
        }]
    );
}

#[tokio::test]
async fn ms365_users_routes_require_auth_when_gateway_token_enabled() {
    let app = build_test_app(Some(TEST_AUTH_TOKEN.to_string()));

    let response = app
        .clone()
        .oneshot(Request::get("/ms365/users/me").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .oneshot(
            Request::get("/ms365/users")
                .header(header::AUTHORIZATION, format!("Bearer {TEST_AUTH_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn ms365_users_read_maps_validation_errors_to_bad_request() {
    let users_service = Arc::new(FakeMs365UsersService::with_read_response(Err(
        Ms365UsersServiceError::Validation("user id is required".to_string()),
    )));
    let (app, _device_repo) =
        build_test_app_parts_with_users_service(AppConfig::default(), None, users_service);

    let response = app
        .oneshot(
            Request::get("/ms365/users/%20")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let json = body_json(response).await;
    assert!(
        json["message"]
            .as_str()
            .unwrap()
            .contains("user id is required")
    );
}

#[tokio::test]
async fn ms365_calendar_create_event_forwards_request_and_returns_created_response() {
    let calendar_service = Arc::new(FakeMs365CalendarService::with_create_response(Ok(())));
    let (app, _device_repo) = build_test_app_parts_with_calendar_service(
        AppConfig::default(),
        None,
        calendar_service.clone(),
    );

    let request = sample_calendar_create_request();
    let response = app
        .oneshot(
            Request::post("/ms365/calendar/events")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_string(&request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);

    let json = body_json(response).await;
    assert_eq!(json["success"], true);
    assert_eq!(json["message"], "Calendar event created");

    let calls = calendar_service.calls().await;
    assert_eq!(calls, vec![CalendarMutationCall::Create(request)]);
}

#[tokio::test]
async fn ms365_calendar_delete_routes_forward_id_and_preserve_compat_alias() {
    let calendar_service = Arc::new(FakeMs365CalendarService::with_delete_response(Ok(())));
    let (app, _device_repo) = build_test_app_parts_with_calendar_service(
        AppConfig::default(),
        None,
        calendar_service.clone(),
    );

    let response = app
        .clone()
        .oneshot(
            Request::delete("/ms365/calendar/events/event-123")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["success"], true);
    assert_eq!(json["message"], "Calendar event deleted");

    let response = app
        .oneshot(
            Request::post("/ms365/calendar/events/event-456/delete")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let calls = calendar_service.calls().await;
    assert_eq!(
        calls,
        vec![
            CalendarMutationCall::Delete {
                id: "event-123".to_string(),
            },
            CalendarMutationCall::Delete {
                id: "event-456".to_string(),
            },
        ]
    );
}

#[tokio::test]
async fn ms365_calendar_respond_event_forwards_request_and_returns_success() {
    let calendar_service = Arc::new(FakeMs365CalendarService::with_respond_response(Ok(())));
    let (app, _device_repo) = build_test_app_parts_with_calendar_service(
        AppConfig::default(),
        None,
        calendar_service.clone(),
    );

    let request = RespondCalendarEventRequest {
        response: "tentative".to_string(),
        comment: Some("Need to confirm timing with the team.".to_string()),
    };
    let response = app
        .oneshot(
            Request::post("/ms365/calendar/events/event-789/respond")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_string(&request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let json = body_json(response).await;
    assert_eq!(json["success"], true);
    assert_eq!(json["message"], "Calendar response sent");

    let calls = calendar_service.calls().await;
    assert_eq!(
        calls,
        vec![CalendarMutationCall::Respond {
            id: "event-789".to_string(),
            request,
        }]
    );
}

#[tokio::test]
async fn ms365_calendar_mutation_routes_require_auth_when_gateway_token_enabled() {
    let app = build_test_app(Some(TEST_AUTH_TOKEN.to_string()));

    let response = app
        .clone()
        .oneshot(
            Request::post("/ms365/calendar/events")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_string(&sample_calendar_create_request()).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .oneshot(
            Request::delete("/ms365/calendar/events/event-123")
                .header(header::AUTHORIZATION, format!("Bearer {TEST_AUTH_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn ms365_planner_create_task_forwards_request_and_returns_created_task() {
    let planner_service = Arc::new(FakeMs365PlannerService::with_create_response(Ok(
        sample_planner_task("task-create-1"),
    )));
    let (app, _device_repo) = build_test_app_parts_with_planner_service(
        AppConfig::default(),
        None,
        planner_service.clone(),
    );

    let response = app
        .oneshot(
            Request::post("/ms365/planner/tasks")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "plan_id": "plan-123",
                        "title": "Draft release notes",
                        "bucket_id": "bucket-456",
                        "assigned_to": "user-789",
                        "due_date": "2026-03-30T12:00:00Z",
                        "priority": 5,
                        "description": "Collect user-facing changes."
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);

    let json = body_json(response).await;
    assert_eq!(json["task"]["id"], "task-create-1");
    assert_eq!(json["task"]["plan_id"], "plan-123");
    assert_eq!(json["task"]["title"], "Draft release notes");
    assert_eq!(json["task"]["percent_complete"], 25);

    let calls = planner_service.calls().await;
    assert_eq!(
        calls,
        vec![PlannerMutationCall::Create(CreatePlannerTaskRequest {
            plan_id: "plan-123".to_string(),
            title: "Draft release notes".to_string(),
            bucket_id: Some("bucket-456".to_string()),
            assigned_to: Some("user-789".to_string()),
            due_date: Some("2026-03-30T12:00:00Z".to_string()),
            priority: Some(5),
            description: Some("Collect user-facing changes.".to_string()),
        })]
    );
}

#[tokio::test]
async fn ms365_planner_update_task_forwards_request_and_returns_updated_task() {
    let planner_service = Arc::new(FakeMs365PlannerService::with_update_response(Ok(
        sample_planner_task("task-update-1"),
    )));
    let (app, _device_repo) = build_test_app_parts_with_planner_service(
        AppConfig::default(),
        None,
        planner_service.clone(),
    );

    let response = app
        .oneshot(
            Request::post("/ms365/planner/tasks/task-update-1")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "title": "Finalize release notes",
                        "description": "Ship the final draft."
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let json = body_json(response).await;
    assert_eq!(json["task"]["id"], "task-update-1");
    assert_eq!(json["task"]["title"], "Draft release notes");

    let calls = planner_service.calls().await;
    assert_eq!(
        calls,
        vec![PlannerMutationCall::Update {
            id: "task-update-1".to_string(),
            request: UpdatePlannerTaskRequest {
                title: Some("Finalize release notes".to_string()),
                bucket_id: None,
                assigned_to: None,
                due_date: None,
                priority: None,
                description: Some("Ship the final draft.".to_string()),
            },
        }]
    );
}

#[tokio::test]
async fn ms365_planner_update_task_rejects_empty_patch() {
    let planner_service = Arc::new(FakeMs365PlannerService::with_update_response(Err(
        Ms365PlannerServiceError::Validation(
            "planner task update requires at least one mutable field".to_string(),
        ),
    )));
    let (app, _device_repo) =
        build_test_app_parts_with_planner_service(AppConfig::default(), None, planner_service);

    let response = app
        .oneshot(
            Request::post("/ms365/planner/tasks/task-update-1")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let json = body_json(response).await;
    assert!(
        json["message"]
            .as_str()
            .unwrap()
            .contains("planner task update requires at least one mutable field")
    );
}

#[tokio::test]
async fn ms365_planner_complete_task_forwards_id_and_returns_completed_task() {
    let mut task = sample_planner_task("task-complete-1");
    task.percent_complete = 100;

    let planner_service = Arc::new(FakeMs365PlannerService::with_complete_response(Ok(task)));
    let (app, _device_repo) = build_test_app_parts_with_planner_service(
        AppConfig::default(),
        None,
        planner_service.clone(),
    );

    let response = app
        .oneshot(
            Request::post("/ms365/planner/tasks/task-complete-1/complete")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let json = body_json(response).await;
    assert_eq!(json["task"]["id"], "task-complete-1");
    assert_eq!(json["task"]["percent_complete"], 100);

    let calls = planner_service.calls().await;
    assert_eq!(
        calls,
        vec![PlannerMutationCall::Complete {
            id: "task-complete-1".to_string(),
        }]
    );
}

#[tokio::test]
async fn ms365_planner_mutation_routes_require_auth_when_gateway_token_enabled() {
    let app = build_test_app(Some(TEST_AUTH_TOKEN.to_string()));

    let response = app
        .clone()
        .oneshot(
            Request::post("/ms365/planner/tasks")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "plan_id": "plan-123",
                        "title": "Draft release notes"
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
            Request::post("/ms365/planner/tasks/task-complete-1/complete")
                .header(header::AUTHORIZATION, format!("Bearer {TEST_AUTH_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn ms365_todo_create_task_forwards_request_and_returns_created_task() {
    let todo_service = Arc::new(FakeMs365TodoService::with_create_response(Ok(
        sample_todo_task("list-123", "task-create-1"),
    )));
    let (app, _device_repo) =
        build_test_app_parts_with_todo_service(AppConfig::default(), None, todo_service.clone());

    let response = app
        .oneshot(
            Request::post("/ms365/todo/lists/list-123/tasks")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "title": "Draft operator checklist",
                        "body_preview": "Line up the backend follow-on work.",
                        "due_date": "2026-03-30T12:00:00Z",
                        "importance": "high",
                        "status": "inProgress"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);

    let json = body_json(response).await;
    assert_eq!(json["task"]["id"], "task-create-1");
    assert_eq!(json["task"]["list_id"], "list-123");
    assert_eq!(json["task"]["status"], "inProgress");

    let calls = todo_service.calls().await;
    assert_eq!(
        calls,
        vec![TodoMutationCall::Create {
            list_id: "list-123".to_string(),
            request: CreateTodoTaskRequest {
                title: "Draft operator checklist".to_string(),
                body_preview: Some("Line up the backend follow-on work.".to_string()),
                due_date: Some("2026-03-30T12:00:00Z".to_string()),
                importance: Some("high".to_string()),
                status: Some("inProgress".to_string()),
            },
        }]
    );
}

#[tokio::test]
async fn ms365_todo_update_task_forwards_request_and_returns_updated_task() {
    let todo_service = Arc::new(FakeMs365TodoService::with_update_response(Ok(
        sample_todo_task("list-123", "task-update-1"),
    )));
    let (app, _device_repo) =
        build_test_app_parts_with_todo_service(AppConfig::default(), None, todo_service.clone());

    let response = app
        .oneshot(
            Request::post("/ms365/todo/lists/list-123/tasks/task-update-1")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "title": "Finalize operator checklist",
                        "status": "completed"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let json = body_json(response).await;
    assert_eq!(json["task"]["id"], "task-update-1");
    assert_eq!(json["task"]["list_id"], "list-123");

    let calls = todo_service.calls().await;
    assert_eq!(
        calls,
        vec![TodoMutationCall::Update {
            list_id: "list-123".to_string(),
            id: "task-update-1".to_string(),
            request: UpdateTodoTaskRequest {
                title: Some("Finalize operator checklist".to_string()),
                body_preview: None,
                due_date: None,
                importance: None,
                status: Some("completed".to_string()),
            },
        }]
    );
}

#[tokio::test]
async fn ms365_todo_update_task_rejects_empty_patch() {
    let todo_service = Arc::new(FakeMs365TodoService::with_update_response(Err(
        Ms365TodoServiceError::Validation(
            "todo task update requires at least one mutable field".to_string(),
        ),
    )));
    let (app, _device_repo) =
        build_test_app_parts_with_todo_service(AppConfig::default(), None, todo_service);

    let response = app
        .oneshot(
            Request::post("/ms365/todo/lists/list-123/tasks/task-update-1")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let json = body_json(response).await;
    assert!(
        json["message"]
            .as_str()
            .unwrap()
            .contains("todo task update requires at least one mutable field")
    );
}

#[tokio::test]
async fn ms365_todo_complete_task_forwards_ids_and_returns_completed_task() {
    let mut task = sample_todo_task("list-123", "task-complete-1");
    task.status = "completed".to_string();
    task.completed_at = Some("2026-03-22T11:30:00Z".to_string());

    let todo_service = Arc::new(FakeMs365TodoService::with_complete_response(Ok(task)));
    let (app, _device_repo) =
        build_test_app_parts_with_todo_service(AppConfig::default(), None, todo_service.clone());

    let response = app
        .oneshot(
            Request::post("/ms365/todo/lists/list-123/tasks/task-complete-1/complete")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let json = body_json(response).await;
    assert_eq!(json["task"]["id"], "task-complete-1");
    assert_eq!(json["task"]["status"], "completed");

    let calls = todo_service.calls().await;
    assert_eq!(
        calls,
        vec![TodoMutationCall::Complete {
            list_id: "list-123".to_string(),
            id: "task-complete-1".to_string(),
        }]
    );
}

#[tokio::test]
async fn ms365_todo_mutation_routes_require_auth_when_gateway_token_enabled() {
    let app = build_test_app(Some(TEST_AUTH_TOKEN.to_string()));

    let response = app
        .clone()
        .oneshot(
            Request::post("/ms365/todo/lists/list-123/tasks")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "title": "Draft operator checklist"
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
            Request::post("/ms365/todo/lists/list-123/tasks/task-complete-1/complete")
                .header(header::AUTHORIZATION, format!("Bearer {TEST_AUTH_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
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
        ms365_calendar_service: test_ms365_calendar_service(),
        ms365_planner_service: test_ms365_planner_service(),
        ms365_todo_service: test_ms365_todo_service(),
        ms365_mail_service: test_ms365_mail_service(),
        ms365_files_service: test_ms365_files_service(),
        ms365_users_service: test_ms365_users_service(),
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
    assert!(unresolved.iter().any(|item| item.as_str() == Some("approval requests, operator-triggered resume, and restart-safe mid-resume continuation are durable")));
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
            runtime_profile: None,
            policy_profile: None,
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
        ms365_calendar_service: test_ms365_calendar_service(),
        ms365_planner_service: test_ms365_planner_service(),
        ms365_todo_service: test_ms365_todo_service(),
        ms365_mail_service: test_ms365_mail_service(),
        ms365_files_service: test_ms365_files_service(),
        ms365_users_service: test_ms365_users_service(),
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
    assert!(unresolved.iter().any(|item| item.as_str() == Some("approval requests, operator-triggered resume, and restart-safe mid-resume continuation are durable")));
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
    let skills_dir = std::env::temp_dir().join(format!("rune-gw-skill-detail-{}", Uuid::now_v7()));
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
    assert_eq!(
        json["source_dir"],
        skills_dir.join("alpha").display().to_string()
    );
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
            runtime_profile: None,
            policy_profile: None,
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
            runtime_profile: None,
            policy_profile: None,
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
        ms365_calendar_service: test_ms365_calendar_service(),
        ms365_planner_service: test_ms365_planner_service(),
        ms365_todo_service: test_ms365_todo_service(),
        ms365_mail_service: test_ms365_mail_service(),
        ms365_files_service: test_ms365_files_service(),
        ms365_users_service: test_ms365_users_service(),
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
        ms365_calendar_service: test_ms365_calendar_service(),
        ms365_planner_service: test_ms365_planner_service(),
        ms365_todo_service: test_ms365_todo_service(),
        ms365_mail_service: test_ms365_mail_service(),
        ms365_files_service: test_ms365_files_service(),
        ms365_users_service: test_ms365_users_service(),
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
        ms365_calendar_service: test_ms365_calendar_service(),
        ms365_planner_service: test_ms365_planner_service(),
        ms365_todo_service: test_ms365_todo_service(),
        ms365_mail_service: test_ms365_mail_service(),
        ms365_files_service: test_ms365_files_service(),
        ms365_users_service: test_ms365_users_service(),
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
            runtime_profile: None,
            policy_profile: None,
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
        ms365_calendar_service: test_ms365_calendar_service(),
        ms365_planner_service: test_ms365_planner_service(),
        ms365_todo_service: test_ms365_todo_service(),
        ms365_mail_service: test_ms365_mail_service(),
        ms365_files_service: test_ms365_files_service(),
        ms365_users_service: test_ms365_users_service(),
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
    assert!(
        json["detail"]
            .as_str()
            .unwrap()
            .contains("steering instruction delivered")
    );

    // Verify transcript got a status_note.
    let items = transcript_repo.list_by_session(agent_id).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind, "status_note");
    assert!(
        items[0].payload["content"]
            .as_str()
            .unwrap()
            .contains("focus on tests")
    );

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
            runtime_profile: None,
            policy_profile: None,
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
        ms365_calendar_service: test_ms365_calendar_service(),
        ms365_planner_service: test_ms365_planner_service(),
        ms365_todo_service: test_ms365_todo_service(),
        ms365_mail_service: test_ms365_mail_service(),
        ms365_files_service: test_ms365_files_service(),
        ms365_users_service: test_ms365_users_service(),
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
    assert!(
        items[0].payload["content"]
            .as_str()
            .unwrap()
            .contains("no longer needed")
    );
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
            runtime_profile: None,
            policy_profile: None,
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
        ms365_calendar_service: test_ms365_calendar_service(),
        ms365_planner_service: test_ms365_planner_service(),
        ms365_todo_service: test_ms365_todo_service(),
        ms365_mail_service: test_ms365_mail_service(),
        ms365_files_service: test_ms365_files_service(),
        ms365_users_service: test_ms365_users_service(),
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
        .oneshot(Request::post("/configure").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;

    // With default config: no providers, no auth → success should be false
    assert_eq!(json["success"], false);
    assert!(
        json["detail"]
            .as_str()
            .unwrap()
            .contains("need configuration")
    );

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
    let mp = items
        .iter()
        .find(|i| i["name"] == "model_providers")
        .unwrap();
    assert_eq!(mp["status"], "needed");
}

#[tokio::test]
async fn setup_returns_same_shape_as_configure() {
    let app = build_test_app(None);
    let response = app
        .oneshot(Request::post("/setup").body(Body::empty()).unwrap())
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
    let mp = items
        .iter()
        .find(|i| i["name"] == "model_providers")
        .unwrap();
    assert_eq!(mp["status"], "configured");
    assert!(mp["message"].as_str().unwrap().contains("1 provider"));

    // auth should be "configured"
    let auth = items.iter().find(|i| i["name"] == "auth").unwrap();
    assert_eq!(auth["status"], "configured");

    // Optional items should be "skipped" (not "needed")
    let tts = items.iter().find(|i| i["name"] == "tts").unwrap();
    assert_eq!(tts["status"], "skipped");
}

#[tokio::test]
async fn set_get_list_and_clear_approval_policy_routes() {
    let app = build_test_app(None);

    let response = app
        .clone()
        .oneshot(
            Request::put("/approvals/policies/exec")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"decision":"allow-always"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["tool_name"], "exec");
    assert_eq!(json["decision"], "allow_always");

    let response = app
        .clone()
        .oneshot(
            Request::get("/approvals/policies/exec")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["tool_name"], "exec");
    assert_eq!(json["decision"], "allow_always");

    let response = app
        .clone()
        .oneshot(
            Request::get("/approvals/policies")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    let policies = json.as_array().unwrap();
    assert!(
        policies
            .iter()
            .any(|item| item["tool_name"] == "exec" && item["decision"] == "allow_always")
    );

    let response = app
        .clone()
        .oneshot(
            Request::delete("/approvals/policies/exec")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response).await;
    assert_eq!(json["success"], true);

    let response = app
        .oneshot(
            Request::get("/approvals/policies/exec")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn set_approval_policy_rejects_invalid_decision() {
    let app = build_test_app(None);

    let response = app
        .oneshot(
            Request::put("/approvals/policies/exec")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"decision":"allow-once"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let json = body_json(response).await;
    assert_eq!(json["code"], "bad_request");
}

#[tokio::test]
async fn chat_route_redirects_to_webchat() {
    let app = build_test_app(None);

    let response = app
        .oneshot(Request::get("/chat").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        response
            .headers()
            .get(axum::http::header::LOCATION)
            .unwrap(),
        "/webchat"
    );
}

#[tokio::test]
async fn webchat_route_serves_embedded_chat_ui() {
    let app = build_test_app(None);

    let response = app
        .oneshot(Request::get("/webchat").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers().clone();
    let body = body_text(response).await;
    assert!(
        headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .contains("text/html")
    );
    assert!(body.contains("Rune WebChat"));
    assert!(body.contains("new WebSocket"));
    assert!(body.contains("session.create"));
    assert!(body.contains("session.send"));
    assert!(body.contains("assistant_reply"));
}

#[tokio::test]
async fn webchat_route_documents_multi_user_browser_sessions() {
    let app = build_test_app(None);

    let response = app
        .oneshot(Request::get("/webchat").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_text(response).await;
    assert!(body.contains("const sessionStorageKey = 'rune.webchat.session_id'"));
    assert!(body.contains("window.sessionStorage.getItem(sessionStorageKey)"));
    assert!(body.contains("window.sessionStorage.setItem(sessionStorageKey, sessionId)"));
    assert!(body.contains("history.replaceState"));
    assert!(body.contains("sessionToken"));
}
#[tokio::test]
async fn webchat_route_preserves_session_and_auth_query_params() {
    let app = build_test_app(None);

    let response = app
        .oneshot(
            Request::get(
                "/webchat?session_id=sess-123&api_key=test-key&session_token=browser-token",
            )
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_text(response).await;
    assert!(body.contains("query.get('session_id')"));
    assert!(body.contains("query.get('api_key')"));
    assert!(body.contains("query.get('session_token')"));
    assert!(body.contains("buildWsUrl"));
    assert!(body.contains("wsQuery.set('api_key', authToken)"));
    assert!(body.contains("wsQuery.set('session_token', sessionToken)"));
    assert!(body.contains("next.set('session_id', sessionId)"));
    assert!(body.contains("next.set('api_key', authToken)"));
    assert!(body.contains("next.set('session_token', sessionToken)"));
}

#[tokio::test]
async fn webchat_route_mentions_authorization_header_auth() {
    let app = build_test_app(None);

    let response = app
        .oneshot(Request::get("/webchat").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_text(response).await;
    assert!(body.contains("authHeaderToken"));
    assert!(body.contains("Bearer"));
}

#[tokio::test]
async fn webchat_allows_query_api_key_when_gateway_auth_is_enabled() {
    let app = build_test_app(Some("test-token".to_string()));

    let response = app
        .oneshot(
            Request::get("/webchat?api_key=test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
