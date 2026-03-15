//! Thin gateway binary — loads config and starts the Rune gateway daemon.
//!
//! When `database.database_url` is configured, connects to the external
//! PostgreSQL instance. Otherwise bootstraps an embedded PostgreSQL
//! server via `postgresql_embedded` for zero-config local development.
//! Data in both cases is durable and persisted to disk.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use uuid::Uuid;

use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::signal;
use tracing::{error, info, warn};

use rune_channels::TelegramAdapter;
use rune_config::AppConfig;
use rune_core::ToolCategory;
use rune_gateway::{Services, init_logging, start};
use rune_models::{
    CompletionRequest, CompletionResponse, FinishReason, ModelError, ModelProvider,
    RoutedModelProvider, Usage,
};
use rune_runtime::{
    ContextAssembler, LaneQueue, NoOpCompaction, SessionEngine, TurnExecutor,
    heartbeat::HeartbeatRunner,
    scheduler::{ReminderStore, Scheduler},
    session_loop::SessionLoop,
};
use rune_store::EmbeddedPg;
use rune_store::models::{NewToolExecution, SessionRow, TurnRow};
use rune_store::repos::{
    ApprovalRepo, SessionRepo, ToolApprovalPolicyRepo, ToolExecutionRepo, TranscriptRepo, TurnRepo,
};
use rune_store::{JobRepo, PgApprovalRepo, PgJobRepo, PgToolExecutionRepo};
use rune_tools::ApprovalCheck;
use rune_tools::approval::{ApprovalRequest, PolicyBasedApproval};
use rune_tools::exec_tool::ExecToolExecutor;
use rune_tools::file_tools::FileToolExecutor;
use rune_tools::memory_tool::MemoryToolExecutor;
use rune_tools::process_audit::{
    CompletedProcessAudit, NewProcessAudit, ProcessAuditRecord, ProcessAuditStore,
};
use rune_tools::process_tool::{ProcessManager, ProcessToolExecutor};
use rune_tools::session_tool::{SessionQuery, SessionToolExecutor};
use rune_tools::spawn_tool::{SessionSpawner, SpawnToolExecutor};
use rune_tools::subagent_tool::{SubagentManager, SubagentToolExecutor};
use rune_tools::{ToolCall, ToolDefinition, ToolError, ToolExecutor, ToolRegistry};

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

    let (services, _embedded_pg, session_loop) = build_services(config).await?;
    let handle = start(services).await.context("failed to start gateway")?;

    // Start the session loop (channel listener) if a channel is configured.
    if let Some(loop_handle) = session_loop {
        let loop_handle = Arc::new(loop_handle);
        let lh = loop_handle.clone();
        tokio::spawn(async move {
            if let Err(e) = lh.run().await {
                error!(error = %e, "session loop exited with error");
            }
        });
        info!("session loop started for Telegram channel");
    }

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
async fn build_services(
    config: AppConfig,
) -> Result<(Services, Option<EmbeddedPg>, Option<SessionLoop>)> {
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
    let pool =
        rune_store::pool::create_pool(&database_url, config.database.max_connections as usize)?;

    let session_repo: Arc<dyn SessionRepo> =
        Arc::new(rune_store::pg::PgSessionRepo::new(pool.clone()));
    let turn_repo: Arc<dyn TurnRepo> = Arc::new(rune_store::pg::PgTurnRepo::new(pool.clone()));
    let transcript_repo: Arc<dyn TranscriptRepo> =
        Arc::new(rune_store::pg::PgTranscriptRepo::new(pool.clone()));
    let job_repo: Arc<dyn JobRepo> = Arc::new(PgJobRepo::new(pool.clone()));
    let job_run_repo: Arc<dyn rune_store::repos::JobRunRepo> =
        Arc::new(rune_store::pg::PgJobRunRepo::new(pool.clone()));
    let approval_repo: Arc<dyn ApprovalRepo> = Arc::new(PgApprovalRepo::new(pool.clone()));
    let tool_approval_repo: Arc<dyn rune_store::repos::ToolApprovalPolicyRepo> =
        Arc::new(rune_store::pg::PgToolApprovalPolicyRepo::new(pool.clone()));
    let device_repo: Arc<dyn rune_store::repos::DeviceRepo> =
        Arc::new(rune_store::pg::PgDeviceRepo::new(pool.clone()));
    let tool_execution_repo: Arc<dyn ToolExecutionRepo> = Arc::new(PgToolExecutionRepo::new(pool));

    let session_engine = Arc::new(
        SessionEngine::new(session_repo.clone()).with_transcript_repo(transcript_repo.clone()),
    );

    let model_provider: Arc<dyn ModelProvider> = build_model_provider(&config);
    let scheduler = Arc::new(Scheduler::new_with_repos(job_repo.clone(), job_run_repo));

    let process_audit_store: Arc<dyn ProcessAuditStore> =
        Arc::new(DbProcessAuditStore::new(tool_execution_repo));
    let process_manager = ProcessManager::new().with_audit_store(process_audit_store.clone());
    let lane_queue = Arc::new(LaneQueue::with_capacities(
        config.runtime.lanes.main_capacity,
        config.runtime.lanes.subagent_capacity,
        config.runtime.lanes.cron_capacity,
    ));
    let workspace_root = config
        .paths
        .config_dir
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();

    let heartbeat_state_file = config.paths.logs_dir.join("heartbeat-state.json");
    let heartbeat = Arc::new(HeartbeatRunner::with_state_file(
        workspace_root.clone(),
        heartbeat_state_file,
    ));
    let reminder_store = Arc::new(ReminderStore::new_with_repo(job_repo));
    let mut registry = ToolRegistry::new();
    register_real_tool_definitions(&mut registry);
    let tool_count = registry.len();
    let tool_registry = Arc::new(registry);
    let approval = Arc::new(PolicyBasedApproval::new(std::collections::HashSet::new()));
    let live_session_query = LiveSessionQuery::new(
        session_repo.clone(),
        transcript_repo.clone(),
        turn_repo.clone(),
        Instant::now(),
    );
    let live_session_spawner = LiveSessionSpawner::new(
        session_engine.clone(),
        session_repo.clone(),
        transcript_repo.clone(),
        workspace_root.clone(),
    );
    let live_subagent_manager =
        LiveSubagentManager::new(session_repo.clone(), transcript_repo.clone());
    let tool_executor = Arc::new(AppToolExecutor {
        file: FileToolExecutor::new(workspace_root.clone()),
        exec: ExecToolExecutor::new(workspace_root.clone(), Duration::from_secs(30))
            .with_process_manager(process_manager.clone())
            .with_audit_store(process_audit_store),
        process: ProcessToolExecutor::new(process_manager.clone()),
        memory: MemoryToolExecutor::new(workspace_root),
        session: SessionToolExecutor::new(live_session_query),
        spawn: SpawnToolExecutor::new(live_session_spawner),
        subagents: SubagentToolExecutor::new(live_subagent_manager),
        approval,
        tool_approval_repo: tool_approval_repo.clone(),
    });

    // Resolve system prompt: agent config → hardcoded default
    let system_prompt = config
        .agents
        .default_agent()
        .and_then(|a| config.agents.effective_system_prompt(a))
        .unwrap_or("You are Rune, a Rust-powered AI assistant built for speed and reliability.")
        .to_string();

    let mut turn_executor = TurnExecutor::new(
        session_repo.clone(),
        turn_repo.clone(),
        transcript_repo.clone(),
        approval_repo.clone(),
        model_provider.clone(),
        tool_executor,
        tool_registry,
        ContextAssembler::new(&system_prompt),
        Arc::new(NoOpCompaction),
    );

    // Resolve default model: agent config → models.default_model
    let default_model = config
        .agents
        .default_agent()
        .and_then(|a| config.agents.effective_model(a))
        .map(String::from)
        .or_else(|| config.models.default_model.clone());

    if let Some(ref model) = default_model {
        turn_executor = turn_executor.with_default_model(model);
        info!(model = %model, "default model configured");
    }

    turn_executor = turn_executor.with_lane_queue(lane_queue.clone());
    info!(stats = %lane_queue.stats(), "lane queue configured for turn execution");

    let turn_executor = Arc::new(turn_executor);

    // Build session loop if Telegram channel is configured.
    let session_loop = if let Some(ref tg) = config.channels.telegram_token {
        if !tg.is_empty() {
            info!(token_len = tg.len(), "configuring Telegram channel adapter");
            let adapter = TelegramAdapter::new(tg);
            Some(SessionLoop::new(
                session_engine.clone(),
                turn_executor.clone(),
                session_repo.clone(),
                Box::new(adapter),
                config.agents.clone(),
                config.models.clone(),
            ))
        } else {
            None
        }
    } else {
        info!("no Telegram bot token configured — session loop disabled");
        None
    };

    let services = Services {
        config,
        session_engine,
        turn_executor,
        session_repo,
        transcript_repo,
        turn_repo,
        model_provider,
        scheduler,
        heartbeat,
        reminder_store,
        approval_repo,
        tool_approval_repo,
        process_manager,
        tool_count,
        device_repo,
    };

    Ok((services, embedded_pg, session_loop))
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

struct AppToolExecutor {
    file: FileToolExecutor,
    exec: ExecToolExecutor,
    process: ProcessToolExecutor,
    memory: MemoryToolExecutor,
    session: SessionToolExecutor<LiveSessionQuery>,
    spawn: SpawnToolExecutor<LiveSessionSpawner>,
    subagents: SubagentToolExecutor<LiveSubagentManager>,
    approval: Arc<PolicyBasedApproval>,
    tool_approval_repo: Arc<dyn ToolApprovalPolicyRepo>,
}

#[derive(Clone)]
struct DbProcessAuditStore {
    repo: Arc<dyn ToolExecutionRepo>,
}

impl DbProcessAuditStore {
    fn new(repo: Arc<dyn ToolExecutionRepo>) -> Self {
        Self { repo }
    }
}

fn row_to_process_audit(row: rune_store::models::ToolExecutionRow) -> ProcessAuditRecord {
    let command = row
        .arguments
        .get("command")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    let workdir = row
        .arguments
        .get("workdir")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    let session_id = row
        .arguments
        .get("__session_id")
        .and_then(|value| value.as_str())
        .and_then(|value| Uuid::parse_str(value).ok())
        .or(Some(row.session_id));
    let turn_id = row
        .arguments
        .get("__turn_id")
        .and_then(|value| value.as_str())
        .and_then(|value| Uuid::parse_str(value).ok())
        .or(Some(row.turn_id));

    ProcessAuditRecord {
        process_id: row.tool_call_id.to_string(),
        tool_call_id: row.tool_call_id,
        tool_execution_id: row.id,
        session_id,
        turn_id,
        tool_name: row.tool_name,
        command,
        workdir,
        arguments: row.arguments,
        status: row.status,
        result_summary: row.result_summary,
        error_summary: row.error_summary,
        started_at: row.started_at,
        ended_at: row.ended_at,
    }
}

#[async_trait]
impl ProcessAuditStore for DbProcessAuditStore {
    async fn record_spawn(&self, spawn: NewProcessAudit) -> Result<ProcessAuditRecord, String> {
        let row = self
            .repo
            .create(NewToolExecution {
                id: Uuid::now_v7(),
                tool_call_id: spawn.tool_call_id,
                session_id: spawn.session_id.unwrap_or_else(Uuid::nil),
                turn_id: spawn.turn_id.unwrap_or_else(Uuid::nil),
                tool_name: spawn.tool_name,
                arguments: spawn.arguments,
                status: "running".to_string(),
                started_at: spawn.started_at,
            })
            .await
            .map_err(|e| e.to_string())?;
        Ok(row_to_process_audit(row))
    }

    async fn record_completion(
        &self,
        completion: CompletedProcessAudit,
    ) -> Result<ProcessAuditRecord, String> {
        let tool_call_id = Uuid::parse_str(&completion.process_id).map_err(|e| e.to_string())?;
        let recent = self
            .repo
            .list_recent(500)
            .await
            .map_err(|e| e.to_string())?;
        let row = recent
            .into_iter()
            .find(|row| row.tool_call_id == tool_call_id)
            .ok_or_else(|| {
                format!(
                    "tool execution not found for process {}",
                    completion.process_id
                )
            })?;
        let updated = self
            .repo
            .complete(
                row.id,
                &completion.status,
                completion.result_summary.as_deref(),
                completion.error_summary.as_deref(),
                completion.ended_at,
            )
            .await
            .map_err(|e| e.to_string())?;
        Ok(row_to_process_audit(updated))
    }

    async fn find(&self, process_id: &str) -> Result<Option<ProcessAuditRecord>, String> {
        let tool_call_id = match Uuid::parse_str(process_id) {
            Ok(id) => id,
            Err(_) => return Ok(None),
        };
        let recent = self
            .repo
            .list_recent(500)
            .await
            .map_err(|e| e.to_string())?;
        Ok(recent
            .into_iter()
            .find(|row| row.tool_call_id == tool_call_id)
            .map(row_to_process_audit))
    }

    async fn list_recent(&self, limit: usize) -> Result<Vec<ProcessAuditRecord>, String> {
        let rows = self
            .repo
            .list_recent(limit as i64)
            .await
            .map_err(|e| e.to_string())?;
        Ok(rows.into_iter().map(row_to_process_audit).collect())
    }
}

#[async_trait]
impl ToolExecutor for AppToolExecutor {
    async fn execute(&self, call: ToolCall) -> Result<rune_tools::ToolResult, ToolError> {
        match call.tool_name.as_str() {
            "read" | "read_file" | "write" | "write_file" | "edit" | "edit_file" | "list_files" => {
                self.file.execute(call).await
            }
            "exec" | "execute_command" => {
                let approval_request = ApprovalRequest::from_call(&call);
                let ask_mode = call
                    .arguments
                    .get("ask")
                    .and_then(|v| v.as_str())
                    .unwrap_or("on-miss");
                let security_mode = call
                    .arguments
                    .get("security")
                    .and_then(|v| v.as_str())
                    .unwrap_or("allowlist");
                let elevated = call
                    .arguments
                    .get("elevated")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let approval_token = call
                    .arguments
                    .get("__approval_resume")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                if elevated && security_mode != "full" {
                    return Err(ToolError::ApprovalDenied {
                        tool: call.tool_name.clone(),
                    });
                }

                if security_mode == "deny" {
                    return Err(ToolError::ApprovalDenied {
                        tool: call.tool_name.clone(),
                    });
                }

                let persisted_policy = self
                    .tool_approval_repo
                    .get_policy(&call.tool_name)
                    .await
                    .map_err(|e| {
                        ToolError::ExecutionFailed(format!("failed to load approval policy: {e}"))
                    })?;

                if let Some(policy) = persisted_policy {
                    match policy.decision.as_str() {
                        "deny" => {
                            return Err(ToolError::ApprovalDenied {
                                tool: call.tool_name.clone(),
                            });
                        }
                        "allow_always" if ask_mode != "always" => {
                            return self.exec.execute(call).await;
                        }
                        _ => {}
                    }
                }

                if approval_token {
                    return self.exec.execute(call).await;
                }

                let approval_required = match ask_mode {
                    "always" => true,
                    "off" => false,
                    _ => match self.approval.check(&call, true).await {
                        Ok(()) => false,
                        Err(ToolError::ApprovalRequired { .. }) => true,
                        Err(other) => return Err(other),
                    },
                };

                if approval_required {
                    let details = serde_json::to_string_pretty(&approval_request)
                        .unwrap_or_else(|_| call.arguments.to_string());
                    return Err(ToolError::ApprovalRequired {
                        tool: call.tool_name.clone(),
                        details,
                    });
                }

                self.exec.execute(call).await
            }
            "process" => self.process.execute(call).await,
            "memory_search" | "memory_get" => self.memory.execute(call).await,
            "sessions_list" | "sessions_history" | "session_status" => {
                self.session.execute(call).await
            }
            "sessions_spawn" | "sessions_send" => self.spawn.execute(call).await,
            "subagents" => self.subagents.execute(call).await,
            other => Err(ToolError::UnknownTool {
                name: other.to_string(),
            }),
        }
    }
}

fn register_real_tool_definitions(registry: &mut ToolRegistry) {
    let builtins = [
        ToolDefinition {
            name: "read".into(),
            description: "Read the contents of a file. Supports text files with offset/limit truncation semantics.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "file_path": { "type": "string" },
                    "offset": { "type": "integer" },
                    "limit": { "type": "integer" },
                    "from": { "type": "integer" },
                    "lines": { "type": "integer" }
                }
            }),
            category: ToolCategory::FileRead,
            requires_approval: false,
        },
        ToolDefinition {
            name: "write".into(),
            description: "Create or overwrite a file, creating parent directories as needed.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "file_path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["content"]
            }),
            category: ToolCategory::FileWrite,
            requires_approval: false,
        },
        ToolDefinition {
            name: "edit".into(),
            description: "Make an exact text replacement in a file.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "file_path": { "type": "string" },
                    "oldText": { "type": "string" },
                    "newText": { "type": "string" },
                    "old_string": { "type": "string" },
                    "new_string": { "type": "string" }
                }
            }),
            category: ToolCategory::FileWrite,
            requires_approval: false,
        },
        ToolDefinition {
            name: "exec".into(),
            description: "Execute a shell command in the workspace with optional timeout/background execution.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "workdir": { "type": "string" },
                    "timeout": { "type": "integer" },
                    "background": { "type": "boolean" }
                },
                "required": ["command"]
            }),
            category: ToolCategory::ProcessExec,
            requires_approval: true,
        },
        ToolDefinition {
            name: "process".into(),
            description: "Inspect and control background processes started by exec.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string" },
                    "sessionId": { "type": "string" }
                },
                "required": ["action"]
            }),
            category: ToolCategory::ProcessBackground,
            requires_approval: false,
        },
        ToolDefinition {
            name: "memory_search".into(),
            description: "Search MEMORY.md and memory/*.md for relevant snippets.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "maxResults": { "type": "integer" }
                },
                "required": ["query"]
            }),
            category: ToolCategory::MemoryAccess,
            requires_approval: false,
        },
        ToolDefinition {
            name: "memory_get".into(),
            description: "Read a bounded snippet from MEMORY.md or memory/*.md.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "from": { "type": "integer" },
                    "lines": { "type": "integer" }
                },
                "required": ["path"]
            }),
            category: ToolCategory::MemoryAccess,
            requires_approval: false,
        },
        ToolDefinition {
            name: "sessions_list".into(),
            description: "List sessions with optional limit and kind filters.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer" },
                    "kinds": {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                }
            }),
            category: ToolCategory::SessionControl,
            requires_approval: false,
        },
        ToolDefinition {
            name: "sessions_history".into(),
            description: "Fetch transcript history for a target session.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "sessionKey": { "type": "string" },
                    "limit": { "type": "integer" }
                },
                "required": ["sessionKey"]
            }),
            category: ToolCategory::SessionControl,
            requires_approval: false,
        },
        ToolDefinition {
            name: "session_status".into(),
            description: "Show the parity status card for the current or targeted session.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "sessionKey": { "type": "string" },
                    "session_id": { "type": "string" },
                    "id": { "type": "string" }
                }
            }),
            category: ToolCategory::SessionControl,
            requires_approval: false,
        },
        ToolDefinition {
            name: "sessions_spawn".into(),
            description: "Spawn a persisted subagent session linked to the requester session.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "task": { "type": "string" },
                    "model": { "type": "string" },
                    "mode": { "type": "string" },
                    "timeoutSeconds": { "type": "integer" },
                    "sessionKey": { "type": "string" },
                    "requesterSessionId": { "type": "string" }
                },
                "required": ["task"]
            }),
            category: ToolCategory::SessionControl,
            requires_approval: false,
        },
        ToolDefinition {
            name: "sessions_send".into(),
            description: "Append a steering message into another persisted session transcript.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "sessionKey": { "type": "string" },
                    "label": { "type": "string" },
                    "message": { "type": "string" }
                },
                "required": ["message"]
            }),
            category: ToolCategory::SessionControl,
            requires_approval: false,
        },
        ToolDefinition {
            name: "subagents".into(),
            description: "List, steer, or mark persisted subagent sessions as cancelled.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string" },
                    "target": { "type": "string" },
                    "message": { "type": "string" },
                    "recentMinutes": { "type": "integer" }
                },
                "required": ["action"]
            }),
            category: ToolCategory::SessionControl,
            requires_approval: false,
        },
    ];

    for tool in builtins {
        registry.register(tool);
    }
}

/// Build the model provider from config, falling back to echo if none configured.
#[derive(Clone)]
struct LiveSessionQuery {
    session_repo: Arc<dyn SessionRepo>,
    transcript_repo: Arc<dyn TranscriptRepo>,
    turn_repo: Arc<dyn TurnRepo>,
    started_at: Instant,
}

impl LiveSessionQuery {
    fn new(
        session_repo: Arc<dyn SessionRepo>,
        transcript_repo: Arc<dyn TranscriptRepo>,
        turn_repo: Arc<dyn TurnRepo>,
        started_at: Instant,
    ) -> Self {
        Self {
            session_repo,
            transcript_repo,
            turn_repo,
            started_at,
        }
    }
}

#[derive(Debug, Default)]
struct TurnAggregate {
    turn_count: u32,
    latest_model: Option<String>,
    usage_prompt_tokens: u64,
    usage_completion_tokens: u64,
    last_turn_started_at: Option<String>,
    last_turn_ended_at: Option<String>,
}

fn aggregate_turns(turns: &[TurnRow]) -> TurnAggregate {
    let mut aggregate = TurnAggregate::default();
    for turn in turns {
        aggregate.turn_count += 1;
        if let Some(model) = &turn.model_ref {
            aggregate.latest_model = Some(model.clone());
        }
        aggregate.usage_prompt_tokens += turn.usage_prompt_tokens.unwrap_or_default().max(0) as u64;
        aggregate.usage_completion_tokens +=
            turn.usage_completion_tokens.unwrap_or_default().max(0) as u64;
        aggregate.last_turn_started_at = Some(turn.started_at.to_rfc3339());
        aggregate.last_turn_ended_at = turn.ended_at.map(|ended| ended.to_rfc3339());
    }
    aggregate
}

fn metadata_string(metadata: &serde_json::Value, key: &str) -> Option<String> {
    metadata
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

fn metadata_bool(metadata: &serde_json::Value, key: &str) -> Option<bool> {
    metadata.get(key).and_then(|value| value.as_bool())
}

fn serialize_session_summary(row: &SessionRow, aggregate: &TurnAggregate) -> serde_json::Value {
    json!({
        "id": row.id,
        "kind": row.kind,
        "status": row.status,
        "requester_session_id": row.requester_session_id,
        "channel_ref": row.channel_ref,
        "created_at": row.created_at.to_rfc3339(),
        "updated_at": row.updated_at.to_rfc3339(),
        "turn_count": aggregate.turn_count,
        "latest_model": aggregate.latest_model,
        "usage_prompt_tokens": aggregate.usage_prompt_tokens,
        "usage_completion_tokens": aggregate.usage_completion_tokens,
        "last_turn_started_at": aggregate.last_turn_started_at,
        "last_turn_ended_at": aggregate.last_turn_ended_at,
    })
}

fn render_session_status_card(
    row: &SessionRow,
    aggregate: &TurnAggregate,
    started_at: Instant,
) -> serde_json::Value {
    let metadata = &row.metadata;
    let model_override = metadata_string(metadata, "selected_model");
    let current_model = aggregate
        .latest_model
        .clone()
        .or_else(|| model_override.clone());
    let approval_mode =
        metadata_string(metadata, "approval_mode").unwrap_or_else(|| "on-miss".to_string());
    let security_mode =
        metadata_string(metadata, "security_mode").unwrap_or_else(|| "allowlist".to_string());
    let reasoning = metadata_string(metadata, "reasoning").unwrap_or_else(|| "off".to_string());
    let verbose = metadata_bool(metadata, "verbose").unwrap_or(false);
    let elevated = metadata_bool(metadata, "elevated").unwrap_or(false);
    let subagent_lifecycle = metadata_string(metadata, "subagent_lifecycle");
    let subagent_runtime_status = metadata_string(metadata, "subagent_runtime_status");
    let subagent_runtime_attached = metadata_bool(metadata, "subagent_runtime_attached");
    let subagent_status_updated_at = metadata_string(metadata, "subagent_status_updated_at");
    let subagent_last_note = metadata_string(metadata, "subagent_last_note");

    let mut unresolved =
        vec!["cost posture is estimate-only; provider pricing is not wired yet".to_string()];
    if approval_mode == "on-miss" {
        unresolved.push(
            "approval requests and operator-triggered resume are durable, but restart-safe continuation for mid-resume approval flows is not parity-complete yet".to_string(),
        );
    }
    if security_mode == "allowlist" {
        unresolved.push(
            "host/node/sandbox parity and PTY fidelity are not yet parity-complete".to_string(),
        );
    }
    if row.kind == "subagent" {
        unresolved.push(
            "subagent runtime execution remains conservative; durable lifecycle inspection is available but full remote/runtime attachment parity is not complete".to_string(),
        );
    }

    json!({
        "session_id": row.id,
        "runtime": format!(
            "kind={} | channel={} | status={}",
            row.kind,
            row.channel_ref.as_deref().unwrap_or("local"),
            row.status,
        ),
        "status": row.status,
        "current_model": current_model,
        "model_override": model_override,
        "prompt_tokens": aggregate.usage_prompt_tokens,
        "completion_tokens": aggregate.usage_completion_tokens,
        "total_tokens": aggregate.usage_prompt_tokens + aggregate.usage_completion_tokens,
        "estimated_cost": "not available",
        "turn_count": aggregate.turn_count,
        "uptime_seconds": started_at.elapsed().as_secs(),
        "last_turn_started_at": aggregate.last_turn_started_at,
        "last_turn_ended_at": aggregate.last_turn_ended_at,
        "reasoning": reasoning,
        "verbose": verbose,
        "elevated": elevated,
        "approval_mode": approval_mode,
        "security_mode": security_mode,
        "subagent_lifecycle": subagent_lifecycle,
        "subagent_runtime_status": subagent_runtime_status,
        "subagent_runtime_attached": subagent_runtime_attached,
        "subagent_status_updated_at": subagent_status_updated_at,
        "subagent_last_note": subagent_last_note,
        "unresolved": unresolved,
    })
}

async fn list_sessions_payload(
    session_repo: &Arc<dyn SessionRepo>,
    turn_repo: &Arc<dyn TurnRepo>,
    limit: usize,
    kinds: Option<&[String]>,
) -> Result<String, String> {
    let rows = session_repo
        .list(limit as i64, 0)
        .await
        .map_err(|e| e.to_string())?;

    let filtered = rows.into_iter().filter(|row| {
        kinds.is_none_or(|allowed| {
            allowed
                .iter()
                .any(|kind| kind.eq_ignore_ascii_case(&row.kind))
        })
    });

    let mut items = Vec::new();
    for row in filtered.take(limit) {
        let turns = turn_repo
            .list_by_session(row.id)
            .await
            .map_err(|e| e.to_string())?;
        let aggregate = aggregate_turns(&turns);
        items.push(serialize_session_summary(&row, &aggregate));
    }

    serde_json::to_string(&items).map_err(|e| e.to_string())
}

async fn session_history_payload(
    transcript_repo: &Arc<dyn TranscriptRepo>,
    session_id: Uuid,
    limit: Option<usize>,
) -> Result<String, String> {
    let mut items = transcript_repo
        .list_by_session(session_id)
        .await
        .map_err(|e| e.to_string())?;
    if let Some(limit) = limit {
        if items.len() > limit {
            let start = items.len() - limit;
            items = items.split_off(start);
        }
    }
    serde_json::to_string(&items).map_err(|e| e.to_string())
}

#[async_trait]
impl SessionQuery for LiveSessionQuery {
    async fn list_sessions(
        &self,
        limit: Option<usize>,
        kinds: Option<Vec<String>>,
    ) -> Result<String, String> {
        let limit = limit.unwrap_or(20).clamp(1, 200);
        list_sessions_payload(&self.session_repo, &self.turn_repo, limit, kinds.as_deref()).await
    }

    async fn get_session(&self, session_id: &str) -> Result<String, String> {
        let session_id = Uuid::parse_str(session_id).map_err(|e| e.to_string())?;
        let row = self
            .session_repo
            .find_by_id(session_id)
            .await
            .map_err(|e| e.to_string())?;
        let turns = self
            .turn_repo
            .list_by_session(row.id)
            .await
            .map_err(|e| e.to_string())?;
        let aggregate = aggregate_turns(&turns);
        serde_json::to_string(&serialize_session_summary(&row, &aggregate))
            .map_err(|e| e.to_string())
    }

    async fn get_history(&self, session_id: &str, limit: Option<usize>) -> Result<String, String> {
        let session_id = Uuid::parse_str(session_id).map_err(|e| e.to_string())?;
        let _ = self
            .session_repo
            .find_by_id(session_id)
            .await
            .map_err(|e| e.to_string())?;
        session_history_payload(&self.transcript_repo, session_id, limit).await
    }

    async fn session_status(&self, session_id: Option<&str>) -> Result<String, String> {
        let session_id = session_id.ok_or_else(|| {
            "session_status requires sessionKey/session_id/id until current-session resolution is wired".to_string()
        })?;
        let session_id = Uuid::parse_str(session_id).map_err(|e| e.to_string())?;
        let row = self
            .session_repo
            .find_by_id(session_id)
            .await
            .map_err(|e| e.to_string())?;
        let turns = self
            .turn_repo
            .list_by_session(row.id)
            .await
            .map_err(|e| e.to_string())?;
        let aggregate = aggregate_turns(&turns);
        serde_json::to_string(&render_session_status_card(
            &row,
            &aggregate,
            self.started_at,
        ))
        .map_err(|e| e.to_string())
    }
}

#[derive(Clone)]
struct LiveSessionSpawner {
    session_engine: Arc<SessionEngine>,
    session_repo: Arc<dyn SessionRepo>,
    transcript_repo: Arc<dyn TranscriptRepo>,
    workspace_root: PathBuf,
}

fn set_metadata_string(metadata: &mut serde_json::Value, key: &str, value: impl Into<String>) {
    if let Some(object) = metadata.as_object_mut() {
        object.insert(key.to_string(), serde_json::Value::String(value.into()));
    }
}

fn set_metadata_u64(metadata: &mut serde_json::Value, key: &str, value: u64) {
    if let Some(object) = metadata.as_object_mut() {
        object.insert(key.to_string(), serde_json::Value::Number(value.into()));
    }
}

fn set_metadata_bool(metadata: &mut serde_json::Value, key: &str, value: bool) {
    if let Some(object) = metadata.as_object_mut() {
        object.insert(key.to_string(), serde_json::Value::Bool(value));
    }
}

impl LiveSessionSpawner {
    fn new(
        session_engine: Arc<SessionEngine>,
        session_repo: Arc<dyn SessionRepo>,
        transcript_repo: Arc<dyn TranscriptRepo>,
        workspace_root: PathBuf,
    ) -> Self {
        Self {
            session_engine,
            session_repo,
            transcript_repo,
            workspace_root,
        }
    }

    async fn append_status_note(
        &self,
        session_id: Uuid,
        status: rune_core::SessionStatus,
        note: String,
    ) -> Result<(), String> {
        let existing = self
            .transcript_repo
            .list_by_session(session_id)
            .await
            .map_err(|e| e.to_string())?;
        let item = rune_core::TranscriptItem::StatusNote { status, note };
        self.transcript_repo
            .append(rune_store::models::NewTranscriptItem {
                id: Uuid::now_v7(),
                session_id,
                turn_id: None,
                seq: existing.len() as i32,
                kind: "status_note".to_string(),
                payload: serde_json::to_value(item).map_err(|e| e.to_string())?,
                created_at: chrono::Utc::now(),
            })
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[async_trait]
impl SessionSpawner for LiveSessionSpawner {
    async fn spawn_session(
        &self,
        requester_session_id: Option<&str>,
        task: &str,
        model: Option<&str>,
        mode: Option<&str>,
        timeout_seconds: Option<u64>,
    ) -> Result<String, String> {
        let requester_session_id = requester_session_id
            .map(Uuid::parse_str)
            .transpose()
            .map_err(|e| format!("invalid requester session id: {e}"))?;
        let row = self
            .session_engine
            .create_session_full(
                rune_core::SessionKind::Subagent,
                Some(self.workspace_root.display().to_string()),
                requester_session_id,
                None,
            )
            .await
            .map_err(|e| e.to_string())?;

        let mut metadata = row.metadata.clone();
        set_metadata_string(&mut metadata, "spawn_task", task.to_string());
        if let Some(requester_session_id) = requester_session_id {
            set_metadata_string(
                &mut metadata,
                "requester_session_id",
                requester_session_id.to_string(),
            );
        }
        set_metadata_string(
            &mut metadata,
            "spawn_mode",
            mode.unwrap_or("run").to_string(),
        );
        if let Some(model) = model {
            set_metadata_string(&mut metadata, "selected_model", model.to_string());
        }
        if let Some(timeout_seconds) = timeout_seconds {
            set_metadata_u64(&mut metadata, "spawn_timeout_seconds", timeout_seconds);
        }
        set_metadata_string(&mut metadata, "subagent_lifecycle", "spawned");
        set_metadata_string(&mut metadata, "subagent_runtime_status", "not_attached");
        set_metadata_bool(&mut metadata, "subagent_runtime_attached", false);
        set_metadata_string(
            &mut metadata,
            "subagent_status_updated_at",
            chrono::Utc::now().to_rfc3339(),
        );

        self.session_engine
            .mark_ready(row.id)
            .await
            .map_err(|e| e.to_string())?;
        let row = self
            .session_repo
            .update_metadata(row.id, metadata, chrono::Utc::now())
            .await
            .map_err(|e| e.to_string())?;
        self.session_engine
            .mark_running(row.id)
            .await
            .map_err(|e| e.to_string())?;

        self.append_status_note(
            row.id,
            rune_core::SessionStatus::WaitingForSubagent,
            match requester_session_id {
                Some(requester) => format!(
                    "Subagent session spawned for task: {task}. Requester session: {requester}. Execution runtime is not yet attached; session is persisted for inspectability."
                ),
                None => format!(
                    "Subagent session spawned for task: {task}. Execution runtime is not yet attached; session is persisted for inspectability."
                ),
            },
        )
        .await?;

        serde_json::to_string(&json!({
            "sessionId": row.id,
            "status": "running",
            "kind": "subagent",
            "requester_session_id": requester_session_id,
            "task": task,
            "mode": mode.unwrap_or("run"),
            "model": model,
            "timeoutSeconds": timeout_seconds,
            "note": "Persisted subagent session created; transcript/status inspection is live, runtime execution remains conservative."
        }))
        .map_err(|e| e.to_string())
    }

    async fn send_message(
        &self,
        session_key: Option<&str>,
        label: Option<&str>,
        message: &str,
    ) -> Result<String, String> {
        let target = session_key
            .or(label)
            .ok_or_else(|| "missing target session".to_string())?;
        let session_id = Uuid::parse_str(target).map_err(|e| e.to_string())?;
        let note = format!("Steering message queued for subagent/session: {message}");

        self.append_status_note(
            session_id,
            rune_core::SessionStatus::WaitingForSubagent,
            note.clone(),
        )
        .await?;

        if let Ok(row) = self.session_repo.find_by_id(session_id).await {
            let mut metadata = row.metadata;
            set_metadata_string(&mut metadata, "subagent_lifecycle", "steered");
            set_metadata_string(&mut metadata, "subagent_runtime_status", "not_attached");
            set_metadata_bool(&mut metadata, "subagent_runtime_attached", false);
            set_metadata_string(
                &mut metadata,
                "subagent_status_updated_at",
                chrono::Utc::now().to_rfc3339(),
            );
            set_metadata_string(&mut metadata, "subagent_last_note", note.clone());
            self.session_repo
                .update_metadata(session_id, metadata, chrono::Utc::now())
                .await
                .map_err(|e| e.to_string())?;
        }

        serde_json::to_string(&json!({
            "delivered": true,
            "sessionId": session_id,
            "message": message,
            "note": "Message persisted into transcript as a steering/status note.",
            "subagentLifecycle": "steered",
            "subagentRuntimeStatus": "not_attached"
        }))
        .map_err(|e| e.to_string())
    }
}

#[derive(Clone)]
struct LiveSubagentManager {
    session_repo: Arc<dyn SessionRepo>,
    transcript_repo: Arc<dyn TranscriptRepo>,
}

impl LiveSubagentManager {
    fn new(session_repo: Arc<dyn SessionRepo>, transcript_repo: Arc<dyn TranscriptRepo>) -> Self {
        Self {
            session_repo,
            transcript_repo,
        }
    }

    async fn append_status_note(&self, session_id: Uuid, note: String) -> Result<(), String> {
        let existing = self
            .transcript_repo
            .list_by_session(session_id)
            .await
            .map_err(|e| e.to_string())?;
        self.transcript_repo
            .append(rune_store::models::NewTranscriptItem {
                id: Uuid::now_v7(),
                session_id,
                turn_id: None,
                seq: existing.len() as i32,
                kind: "status_note".to_string(),
                payload: serde_json::to_value(rune_core::TranscriptItem::StatusNote {
                    status: rune_core::SessionStatus::WaitingForSubagent,
                    note,
                })
                .map_err(|e| e.to_string())?,
                created_at: chrono::Utc::now(),
            })
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn update_subagent_metadata(
        &self,
        session_id: Uuid,
        lifecycle: &str,
        runtime_status: &str,
        last_note: Option<&str>,
    ) -> Result<(), String> {
        let row = self
            .session_repo
            .find_by_id(session_id)
            .await
            .map_err(|e| e.to_string())?;
        let mut metadata = row.metadata;
        set_metadata_string(&mut metadata, "subagent_lifecycle", lifecycle.to_string());
        set_metadata_string(
            &mut metadata,
            "subagent_runtime_status",
            runtime_status.to_string(),
        );
        set_metadata_bool(&mut metadata, "subagent_runtime_attached", false);
        set_metadata_string(
            &mut metadata,
            "subagent_status_updated_at",
            chrono::Utc::now().to_rfc3339(),
        );
        if let Some(note) = last_note {
            set_metadata_string(&mut metadata, "subagent_last_note", note.to_string());
        }
        self.session_repo
            .update_metadata(session_id, metadata, chrono::Utc::now())
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[async_trait]
impl SubagentManager for LiveSubagentManager {
    async fn list(&self, recent_minutes: Option<u64>) -> Result<String, String> {
        let rows = self
            .session_repo
            .list(200, 0)
            .await
            .map_err(|e| e.to_string())?;
        let cutoff = recent_minutes
            .map(|minutes| chrono::Utc::now() - chrono::Duration::minutes(minutes as i64));
        let items: Vec<_> = rows
            .into_iter()
            .filter(|row| row.kind == "subagent")
            .filter(|row| cutoff.is_none_or(|cutoff| row.updated_at >= cutoff))
            .map(|row| {
                json!({
                    "id": row.id,
                    "status": row.status,
                    "requester_session_id": row.requester_session_id,
                    "created_at": row.created_at.to_rfc3339(),
                    "updated_at": row.updated_at.to_rfc3339(),
                    "task": row.metadata.get("spawn_task").and_then(|v| v.as_str()),
                    "mode": row.metadata.get("spawn_mode").and_then(|v| v.as_str()),
                    "selected_model": row.metadata.get("selected_model").and_then(|v| v.as_str()),
                    "subagent_lifecycle": row.metadata.get("subagent_lifecycle").and_then(|v| v.as_str()),
                    "subagent_runtime_status": row.metadata.get("subagent_runtime_status").and_then(|v| v.as_str()),
                    "subagent_runtime_attached": row.metadata.get("subagent_runtime_attached").and_then(|v| v.as_bool()),
                    "subagent_status_updated_at": row.metadata.get("subagent_status_updated_at").and_then(|v| v.as_str()),
                    "subagent_last_note": row.metadata.get("subagent_last_note").and_then(|v| v.as_str()),
                })
            })
            .collect();
        serde_json::to_string(&items).map_err(|e| e.to_string())
    }

    async fn steer(&self, target: &str, message: &str) -> Result<String, String> {
        let session_id = Uuid::parse_str(target).map_err(|e| e.to_string())?;
        let _ = self
            .session_repo
            .find_by_id(session_id)
            .await
            .map_err(|e| e.to_string())?;
        let note = format!("Subagent steering message: {message}");
        self.append_status_note(session_id, note.clone()).await?;
        self.update_subagent_metadata(session_id, "steered", "not_attached", Some(&note))
            .await?;
        serde_json::to_string(&json!({
            "target": session_id,
            "steered": true,
            "message": message,
            "note": "Steering message persisted as a status note.",
            "subagentLifecycle": "steered",
            "subagentRuntimeStatus": "not_attached"
        }))
        .map_err(|e| e.to_string())
    }

    async fn kill(&self, target: &str) -> Result<String, String> {
        let session_id = Uuid::parse_str(target).map_err(|e| e.to_string())?;
        let row = self
            .session_repo
            .update_status(session_id, "cancelled", chrono::Utc::now())
            .await
            .map_err(|e| e.to_string())?;
        let note = "Subagent marked cancelled by operator.".to_string();
        self.append_status_note(session_id, note.clone()).await?;
        self.update_subagent_metadata(session_id, "cancelled", "not_attached", Some(&note))
            .await?;
        serde_json::to_string(&json!({
            "target": row.id,
            "killed": true,
            "status": row.status,
            "note": "Persisted subagent session marked cancelled.",
            "subagentLifecycle": "cancelled",
            "subagentRuntimeStatus": "not_attached"
        }))
        .map_err(|e| e.to_string())
    }
}

fn build_model_provider(config: &AppConfig) -> Arc<dyn ModelProvider> {
    if !config.models.providers.is_empty() {
        match RoutedModelProvider::from_models_config(&config.models) {
            Ok(provider) => Arc::new(provider),
            Err(error) => {
                warn!(error = %error, "failed to build routed model provider, falling back to echo");
                Arc::new(EchoModelProvider)
            }
        }
    } else {
        info!("no model providers configured — using echo fallback");
        Arc::new(EchoModelProvider)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tokio::sync::Mutex;

    use rune_store::StoreError;

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
        async fn create(
            &self,
            session: rune_store::models::NewSession,
        ) -> Result<SessionRow, StoreError> {
            let row = SessionRow {
                id: session.id,
                kind: session.kind,
                status: session.status,
                workspace_root: session.workspace_root,
                channel_ref: session.channel_ref,
                requester_session_id: session.requester_session_id,
                metadata: session.metadata,
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
                .find(|row| row.id == id)
                .cloned()
                .ok_or(StoreError::NotFound {
                    entity: "session",
                    id: id.to_string(),
                })
        }

        async fn list(&self, limit: i64, offset: i64) -> Result<Vec<SessionRow>, StoreError> {
            let rows = self.sessions.lock().await;
            Ok(rows
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
            let rows = self.sessions.lock().await;
            Ok(rows
                .iter()
                .rev()
                .find(|row| row.channel_ref.as_deref() == Some(channel_ref))
                .cloned())
        }

        async fn update_status(
            &self,
            id: Uuid,
            status: &str,
            updated_at: chrono::DateTime<chrono::Utc>,
        ) -> Result<SessionRow, StoreError> {
            let mut rows = self.sessions.lock().await;
            let row = rows
                .iter_mut()
                .find(|row| row.id == id)
                .ok_or(StoreError::NotFound {
                    entity: "session",
                    id: id.to_string(),
                })?;
            row.status = status.to_string();
            row.updated_at = updated_at;
            Ok(row.clone())
        }

        async fn update_metadata(
            &self,
            id: Uuid,
            metadata: Value,
            updated_at: chrono::DateTime<chrono::Utc>,
        ) -> Result<SessionRow, StoreError> {
            let mut rows = self.sessions.lock().await;
            let row = rows
                .iter_mut()
                .find(|row| row.id == id)
                .ok_or(StoreError::NotFound {
                    entity: "session",
                    id: id.to_string(),
                })?;
            row.metadata = metadata;
            row.updated_at = updated_at;
            Ok(row.clone())
        }

        async fn delete(&self, id: Uuid) -> Result<bool, StoreError> {
            let mut rows = self.sessions.lock().await;
            let before = rows.len();
            rows.retain(|row| row.id != id);
            Ok(rows.len() != before)
        }
    }

    struct MemTranscriptRepo {
        items: Mutex<Vec<rune_store::models::TranscriptItemRow>>,
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
        async fn append(
            &self,
            item: rune_store::models::NewTranscriptItem,
        ) -> Result<rune_store::models::TranscriptItemRow, StoreError> {
            let row = rune_store::models::TranscriptItemRow {
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
        ) -> Result<Vec<rune_store::models::TranscriptItemRow>, StoreError> {
            let mut rows: Vec<_> = self
                .items
                .lock()
                .await
                .iter()
                .filter(|row| row.session_id == session_id)
                .cloned()
                .collect();
            rows.sort_by_key(|row| row.seq);
            Ok(rows)
        }

        async fn delete_by_session(&self, session_id: Uuid) -> Result<usize, StoreError> {
            let mut rows = self.items.lock().await;
            let before = rows.len();
            rows.retain(|row| row.session_id != session_id);
            Ok(before - rows.len())
        }
    }

    #[tokio::test]
    async fn live_session_spawner_persists_metadata_and_status_note() {
        let session_repo: Arc<dyn SessionRepo> = Arc::new(MemSessionRepo::new());
        let transcript_repo: Arc<dyn TranscriptRepo> = Arc::new(MemTranscriptRepo::new());
        let session_engine = Arc::new(SessionEngine::new(session_repo.clone()));
        let spawner = LiveSessionSpawner::new(
            session_engine,
            session_repo.clone(),
            transcript_repo.clone(),
            PathBuf::from("/tmp/rune-tests"),
        );

        let response = spawner
            .spawn_session(
                Some("11111111-1111-1111-1111-111111111111"),
                "close parity gap",
                Some("gpt-5.4"),
                Some("run"),
                Some(120),
            )
            .await
            .expect("spawn session should succeed");
        let payload: Value = serde_json::from_str(&response).expect("valid JSON response");
        let session_id = payload
            .get("sessionId")
            .and_then(Value::as_str)
            .expect("sessionId present");
        let session_id = Uuid::parse_str(session_id).expect("sessionId is UUID");

        let row = session_repo
            .find_by_id(session_id)
            .await
            .expect("session row exists");
        assert_eq!(row.kind, "subagent");
        assert_eq!(row.status, "running");
        assert_eq!(
            row.requester_session_id,
            Some(Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap())
        );
        assert_eq!(
            row.metadata.get("spawn_task").and_then(Value::as_str),
            Some("close parity gap")
        );
        assert_eq!(
            row.metadata
                .get("requester_session_id")
                .and_then(Value::as_str),
            Some("11111111-1111-1111-1111-111111111111")
        );
        assert_eq!(
            row.metadata.get("spawn_mode").and_then(Value::as_str),
            Some("run")
        );
        assert_eq!(
            row.metadata
                .get("subagent_lifecycle")
                .and_then(Value::as_str),
            Some("spawned")
        );
        assert_eq!(
            row.metadata
                .get("subagent_runtime_status")
                .and_then(Value::as_str),
            Some("not_attached")
        );
        assert_eq!(
            row.metadata
                .get("subagent_runtime_attached")
                .and_then(Value::as_bool),
            Some(false)
        );
        assert_eq!(
            row.metadata.get("selected_model").and_then(Value::as_str),
            Some("gpt-5.4")
        );
        assert_eq!(
            row.metadata
                .get("spawn_timeout_seconds")
                .and_then(Value::as_u64),
            Some(120)
        );

        let transcript = transcript_repo
            .list_by_session(session_id)
            .await
            .expect("transcript exists");
        assert_eq!(transcript.len(), 1);
        assert_eq!(transcript[0].kind, "status_note");
        let note = transcript[0]
            .payload
            .get("note")
            .and_then(Value::as_str)
            .expect("status note payload");
        assert!(note.contains("Requester session: 11111111-1111-1111-1111-111111111111"));
        assert!(note.contains("Execution runtime is not yet attached"));
    }

    #[tokio::test]
    async fn live_subagent_manager_lists_steers_and_kills_persisted_subagents() {
        let session_repo: Arc<dyn SessionRepo> = Arc::new(MemSessionRepo::new());
        let transcript_repo: Arc<dyn TranscriptRepo> = Arc::new(MemTranscriptRepo::new());
        let session_engine = Arc::new(SessionEngine::new(session_repo.clone()));
        let spawner = LiveSessionSpawner::new(
            session_engine,
            session_repo.clone(),
            transcript_repo.clone(),
            PathBuf::from("/tmp/rune-tests"),
        );
        let manager = LiveSubagentManager::new(session_repo.clone(), transcript_repo.clone());

        let spawned: Value = serde_json::from_str(
            &spawner
                .spawn_session(
                    Some("22222222-2222-2222-2222-222222222222"),
                    "verify inspectability",
                    None,
                    Some("session"),
                    None,
                )
                .await
                .expect("spawn should succeed"),
        )
        .expect("valid JSON response");
        let session_id = spawned
            .get("sessionId")
            .and_then(Value::as_str)
            .expect("sessionId present")
            .to_string();

        let listed: Value =
            serde_json::from_str(&manager.list(Some(60)).await.expect("list works"))
                .expect("valid list JSON");
        let listed = listed.as_array().expect("list should be an array");
        assert_eq!(listed.len(), 1);
        assert_eq!(
            listed[0].get("id").and_then(Value::as_str),
            Some(session_id.as_str())
        );
        assert_eq!(
            listed[0].get("task").and_then(Value::as_str),
            Some("verify inspectability")
        );
        assert_eq!(
            listed[0]
                .get("requester_session_id")
                .and_then(Value::as_str),
            Some("22222222-2222-2222-2222-222222222222")
        );
        assert_eq!(
            listed[0].get("subagent_lifecycle").and_then(Value::as_str),
            Some("spawned")
        );
        assert_eq!(
            listed[0]
                .get("subagent_runtime_status")
                .and_then(Value::as_str),
            Some("not_attached")
        );

        let steer: Value = serde_json::from_str(
            &manager
                .steer(&session_id, "keep going")
                .await
                .expect("steer works"),
        )
        .expect("valid steer JSON");
        assert_eq!(steer.get("steered").and_then(Value::as_bool), Some(true));
        assert_eq!(
            steer.get("subagentLifecycle").and_then(Value::as_str),
            Some("steered")
        );

        let steered_row = session_repo
            .find_by_id(Uuid::parse_str(&session_id).expect("session uuid"))
            .await
            .expect("session row exists after steer");
        assert_eq!(
            steered_row
                .metadata
                .get("subagent_lifecycle")
                .and_then(Value::as_str),
            Some("steered")
        );
        assert_eq!(
            steered_row
                .metadata
                .get("subagent_last_note")
                .and_then(Value::as_str),
            Some("Subagent steering message: keep going")
        );

        let send_response: Value = serde_json::from_str(
            &spawner
                .send_message(Some(&session_id), None, "tighten the tests")
                .await
                .expect("send_message works"),
        )
        .expect("valid send JSON");
        assert_eq!(
            send_response
                .get("subagentLifecycle")
                .and_then(Value::as_str),
            Some("steered")
        );
        assert_eq!(
            send_response
                .get("subagentRuntimeStatus")
                .and_then(Value::as_str),
            Some("not_attached")
        );

        let sent_row = session_repo
            .find_by_id(Uuid::parse_str(&session_id).expect("session uuid"))
            .await
            .expect("session row exists after sessions_send");
        assert_eq!(
            sent_row
                .metadata
                .get("subagent_lifecycle")
                .and_then(Value::as_str),
            Some("steered")
        );
        assert_eq!(
            sent_row
                .metadata
                .get("subagent_last_note")
                .and_then(Value::as_str),
            Some("Steering message queued for subagent/session: tighten the tests")
        );

        let killed: Value =
            serde_json::from_str(&manager.kill(&session_id).await.expect("kill works"))
                .expect("valid kill JSON");
        assert_eq!(killed.get("killed").and_then(Value::as_bool), Some(true));
        assert_eq!(
            killed.get("status").and_then(Value::as_str),
            Some("cancelled")
        );

        let row = session_repo
            .find_by_id(Uuid::parse_str(&session_id).expect("session uuid"))
            .await
            .expect("session row exists after kill");
        assert_eq!(row.status, "cancelled");
        assert_eq!(
            row.metadata
                .get("subagent_lifecycle")
                .and_then(Value::as_str),
            Some("cancelled")
        );
        assert_eq!(
            row.metadata
                .get("subagent_runtime_status")
                .and_then(Value::as_str),
            Some("not_attached")
        );

        let transcript = transcript_repo
            .list_by_session(Uuid::parse_str(&session_id).expect("session uuid"))
            .await
            .expect("transcript exists");
        assert_eq!(transcript.len(), 4);
    }
}

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
