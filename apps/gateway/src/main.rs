//! Thin gateway binary — loads config and starts the Rune gateway daemon.
//!
//! When `database.database_url` is configured, connects to the external
//! PostgreSQL instance. Otherwise bootstraps an embedded PostgreSQL
//! server via `postgresql_embedded` for zero-config local development.
//! Data in both cases is durable and persisted to disk.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::signal;
use tracing::{error, info, warn};

use rune_channels::TelegramAdapter;
use rune_config::AppConfig;
use rune_core::ToolCategory;
use rune_gateway::{Services, init_logging, start};
use rune_models::{
    CompletionRequest, CompletionResponse, FinishReason, ModelError, ModelProvider, Usage,
    AnthropicProvider, AzureFoundryProvider, AzureOpenAiProvider, OpenAiProvider,
};
use rune_runtime::{
    ContextAssembler, NoOpCompaction, SessionEngine, TurnExecutor, scheduler::Scheduler,
    session_loop::SessionLoop,
};
use rune_store::EmbeddedPg;
use rune_store::repos::{SessionRepo, TranscriptRepo, TurnRepo};
use rune_tools::approval::ApprovalRequest;
use rune_tools::exec_tool::ExecToolExecutor;
use rune_tools::file_tools::FileToolExecutor;
use rune_tools::memory_tool::MemoryToolExecutor;
use rune_tools::process_tool::{ProcessManager, ProcessToolExecutor};
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
        Arc::new(rune_store::pg::PgTranscriptRepo::new(pool));

    let session_engine = Arc::new(SessionEngine::new(session_repo.clone()));

    let model_provider: Arc<dyn ModelProvider> = build_model_provider(&config);
    let scheduler = Arc::new(Scheduler::new());

    let process_manager = ProcessManager::new();
    let workspace_root = config
        .paths
        .config_dir
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();
    let mut registry = ToolRegistry::new();
    register_real_tool_definitions(&mut registry);
    let tool_count = registry.len();
    let tool_registry = Arc::new(registry);
    let tool_executor = Arc::new(AppToolExecutor {
        file: FileToolExecutor::new(workspace_root.clone()),
        exec: ExecToolExecutor::new(workspace_root.clone(), Duration::from_secs(30))
            .with_process_manager(process_manager.clone()),
        process: ProcessToolExecutor::new(process_manager),
        memory: MemoryToolExecutor::new(workspace_root),
    });

    let mut turn_executor = TurnExecutor::new(
        session_repo.clone(),
        turn_repo,
        transcript_repo.clone(),
        model_provider.clone(),
        tool_executor,
        tool_registry,
        ContextAssembler::new(
            "You are Rune, a Rust-powered AI assistant built for speed and reliability.",
        ),
        Arc::new(NoOpCompaction),
    );

    if let Some(ref model) = config.models.default_model {
        turn_executor = turn_executor.with_default_model(model);
        info!(model = %model, "default model configured");
    }

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
        model_provider,
        scheduler,
        tool_count,
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
                let approval_bypassed = matches!(
                    call.arguments.get("ask").and_then(|v| v.as_str()),
                    Some("off")
                );
                if !approval_bypassed {
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
    ];

    for tool in builtins {
        registry.register(tool);
    }
}

/// Build the model provider from config, falling back to echo if none configured.
fn build_model_provider(config: &AppConfig) -> Arc<dyn ModelProvider> {
    if let Some(provider_cfg) = config.models.providers.first() {
        let api_key = provider_cfg
            .api_key
            .clone()
            .or_else(|| {
                provider_cfg
                    .api_key_env
                    .as_ref()
                    .and_then(|env_var| std::env::var(env_var).ok())
            })
            .unwrap_or_default();

        match provider_cfg.kind.as_str() {
            "anthropic" => {
                info!(
                    provider = %provider_cfg.name,
                    base_url = %provider_cfg.base_url,
                    "using Anthropic provider"
                );
                let api_version = provider_cfg
                    .api_version
                    .as_deref()
                    .unwrap_or("2023-06-01");
                Arc::new(AnthropicProvider::azure(
                    &provider_cfg.base_url,
                    &api_key,
                    api_version,
                ))
            }
            "openai" => {
                let is_azure = provider_cfg.base_url.contains(".azure.com")
                    || provider_cfg.base_url.contains("azure");
                info!(
                    provider = %provider_cfg.name,
                    azure = is_azure,
                    "using OpenAI provider"
                );
                if is_azure {
                    Arc::new(OpenAiProvider::azure(&provider_cfg.base_url, &api_key))
                } else {
                    Arc::new(OpenAiProvider::new(&provider_cfg.base_url, &api_key))
                }
            }
            "azure-foundry" | "azure-ai" => {
                info!(
                    provider = %provider_cfg.name,
                    base_url = %provider_cfg.base_url,
                    "using Azure AI Foundry provider (multi-model)"
                );
                let api_version = provider_cfg
                    .api_version
                    .as_deref()
                    .unwrap_or("2023-06-01");
                Arc::new(AzureFoundryProvider::with_api_version(
                    &provider_cfg.base_url,
                    &api_key,
                    api_version,
                ))
            }
            "azure-openai" => {
                let deployment = provider_cfg
                    .deployment_name
                    .as_deref()
                    .unwrap_or("gpt-4o");
                let api_version = provider_cfg
                    .api_version
                    .as_deref()
                    .unwrap_or("2024-06-01");
                info!(
                    provider = %provider_cfg.name,
                    deployment,
                    "using Azure OpenAI provider"
                );
                Arc::new(AzureOpenAiProvider::new(
                    &provider_cfg.base_url,
                    deployment,
                    api_version,
                    &api_key,
                ))
            }
            other => {
                warn!(kind = other, "unknown provider kind, falling back to echo");
                Arc::new(EchoModelProvider)
            }
        }
    } else {
        info!("no model providers configured — using echo fallback");
        Arc::new(EchoModelProvider)
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
